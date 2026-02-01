//! Index diff: apply_index_diff, apply_index_diff_streaming, apply_index_diff_pooled.

use anyhow::{Context, Result};
use crossbeam_channel::Receiver;
use rayon::prelude::*;
use rusqlite::{Connection, Statement};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::Entry;
use crate::engine::hashing::{hash_equals, hash_file};
use crate::engine::tools::mtime_changed;
use crate::utils::config::{DB_INSERT_BATCH_SIZE, SMALL_FILE_THRESHOLD};

use super::open::open_db;
use super::{INSERT_PATH_SQL, StoredMeta};

/// True if the entry is new or its mtime/size/hash differ from existing (within mtime_window_ns).
pub fn entry_needs_update(
    entry: &Entry,
    existing: &HashMap<PathBuf, StoredMeta>,
    mtime_window_ns: i64,
) -> bool {
    match existing.get(&entry.path) {
        None => true,
        Some((old_mtime, old_size, old_hash)) => {
            mtime_changed(entry.mtime_ns, *old_mtime, mtime_window_ns)
                || *old_size != entry.size
                || !hash_equals(&entry.hash, old_hash)
        }
    }
}

/// Delete from the paths table every key in `existing` that is not in `current_paths`.
fn delete_removed_paths(
    conn: &Connection,
    existing: &HashMap<PathBuf, StoredMeta>,
    current_paths: &HashSet<PathBuf>,
) -> Result<()> {
    let mut stmt = conn
        .prepare("DELETE FROM paths WHERE path = ?1")
        .context("prepare delete")?;
    for old_path in existing.keys() {
        if !current_paths.contains(old_path) {
            stmt.execute([old_path.to_string_lossy().as_ref()])
                .context("delete path")?;
        }
    }
    Ok(())
}

/// Execute one path insert for an entry (shared by flush_batch, apply_index_diff, apply_index_diff_pooled).
fn execute_insert_entry(stmt: &mut Statement<'_>, e: &Entry) -> Result<()> {
    stmt.execute((
        e.path.to_string_lossy().as_ref(),
        e.mtime_ns,
        e.size as i64,
        e.hash.as_ref().map(|h| h.as_slice()),
    ))
    .context("insert path")?;
    Ok(())
}

/// Insert a batch of entries in a single transaction and optionally call on_batch_progress(batch.len()). Returns batch length.
fn flush_batch(
    conn: &mut Connection,
    batch: &[Entry],
    on_batch_progress: Option<&(dyn Fn(usize) + Send)>,
) -> Result<usize> {
    let tx = conn.transaction().context("begin transaction")?;
    let mut stmt = tx.prepare(INSERT_PATH_SQL).context("prepare insert")?;
    for e in batch {
        execute_insert_entry(&mut stmt, e)?;
    }
    drop(stmt);
    tx.commit().context("commit transaction")?;
    let n = batch.len();
    if let Some(cb) = on_batch_progress {
        cb(n);
    }
    Ok(n)
}

/// Assign path to a writer bucket by hash (reduces lock contention when using multiple writers).
fn path_bucket(path: &Path, n_buckets: usize) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    (hasher.finish() as usize) % n_buckets
}

/// Apply index diff: insert/update changed entries, delete removed paths.
/// Calls `on_batch_progress(n)` after each batch of inserts (if provided).
pub fn apply_index_diff(
    conn: &mut Connection,
    entries: &[Entry],
    existing: &HashMap<PathBuf, StoredMeta>,
    mtime_window_ns: i64,
    on_batch_progress: Option<Box<dyn Fn(usize) + Send>>,
) -> Result<()> {
    let current_paths: HashSet<_> = entries.iter().map(|e| e.path.clone()).collect();

    let tx = conn.transaction().context("begin transaction")?;

    let mut stmt = tx.prepare(INSERT_PATH_SQL).context("prepare insert")?;

    for chunk in entries.chunks(DB_INSERT_BATCH_SIZE) {
        for e in chunk {
            if entry_needs_update(e, existing, mtime_window_ns) {
                execute_insert_entry(&mut stmt, e)?;
            }
        }
        if let Some(ref cb) = on_batch_progress {
            cb(chunk.len());
        }
    }
    drop(stmt);

    delete_removed_paths(&tx, existing, &current_paths)?;

    tx.commit().context("commit transaction")?;

    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .context("WAL checkpoint")?;

    Ok(())
}

/// Partition entries (that need update) and deletes by path hash into `n_buckets` for writer pool.
pub fn partition_index_diff(
    entries: &[Entry],
    existing: &HashMap<PathBuf, StoredMeta>,
    current_paths: &HashSet<PathBuf>,
    n_buckets: usize,
    mtime_window_ns: i64,
) -> Vec<(Vec<Entry>, Vec<PathBuf>)> {
    let mut buckets: Vec<(Vec<Entry>, Vec<PathBuf>)> =
        (0..n_buckets).map(|_| (Vec::new(), Vec::new())).collect();

    for e in entries {
        if entry_needs_update(e, existing, mtime_window_ns) {
            let b = path_bucket(&e.path, n_buckets);
            buckets[b].0.push(e.clone());
        }
    }

    for old_path in existing.keys() {
        if !current_paths.contains(old_path) {
            let b = path_bucket(old_path, n_buckets);
            buckets[b].1.push(old_path.clone());
        }
    }

    buckets
}

/// Parameters for [`apply_index_diff_pooled`].
pub struct ApplyIndexDiffPooledParams<'a> {
    pub db_path: &'a Path,
    pub mtime_window_ns: i64,
    pub on_batch_progress: Option<Box<dyn Fn(usize) + Send>>,
    pub pool_size: usize,
    pub passphrase: Option<&'a str>,
}

/// Parameters for [`apply_index_diff_streaming`].
pub struct ApplyIndexDiffStreamingParams<'a> {
    pub existing: &'a HashMap<PathBuf, StoredMeta>,
    pub mtime_window_ns: i64,
    pub on_batch_progress: Option<Box<dyn Fn(usize) + Send>>,
    pub on_received_progress: Option<Box<dyn Fn(usize) + Send>>,
    pub root: Option<&'a Path>,
    pub with_hash: bool,
    /// When set, streaming checks this on each recv; if true, stops receiving, flushes batch, and returns (partial index).
    pub cancel_check: Option<Arc<AtomicBool>>,
}

/// Write entries to DB as they are received (streaming). Tracks current paths for deletes at end.
pub fn apply_index_diff_streaming(
    conn: &mut Connection,
    entry_rx: Receiver<Entry>,
    params: ApplyIndexDiffStreamingParams<'_>,
) -> Result<usize> {
    let mut current_paths = HashSet::new();
    let mut batch = Vec::with_capacity(DB_INSERT_BATCH_SIZE);
    let mut written = 0_usize;
    let mut received = 0_usize;

    let recv_timeout = params
        .cancel_check
        .as_ref()
        .map(|_| Duration::from_millis(200));

    loop {
        let mut entry = match recv_timeout {
            Some(ref timeout) => match entry_rx.recv_timeout(*timeout) {
                Ok(entry) => entry,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    if params
                        .cancel_check
                        .as_ref()
                        .is_some_and(|c| c.load(Ordering::Relaxed))
                    {
                        log::info!("Indexing cancelled (Ctrl+C); flushing partial index...");
                        break;
                    }
                    continue;
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            },
            None => match entry_rx.recv() {
                Ok(entry) => entry,
                Err(_) => break,
            },
        };
        received += 1;
        if let Some(ref cb) = params.on_received_progress
            && received.is_multiple_of(DB_INSERT_BATCH_SIZE)
        {
            cb(DB_INSERT_BATCH_SIZE);
        }
        if params.with_hash
            && entry.size >= SMALL_FILE_THRESHOLD
            && let Some(r) = params.root
        {
            let abs = r.join(&entry.path);
            if let Ok(Some(h)) = hash_file(&abs, entry.size) {
                entry.hash = Some(h);
            }
        }
        current_paths.insert(entry.path.clone());
        if entry_needs_update(&entry, params.existing, params.mtime_window_ns) {
            batch.push(entry);
        }
        if batch.len() >= DB_INSERT_BATCH_SIZE {
            written += flush_batch(conn, &batch, params.on_batch_progress.as_deref())?;
            batch.clear();
        }
    }

    if let Some(ref cb) = params.on_received_progress {
        let remainder = received % DB_INSERT_BATCH_SIZE;
        if remainder > 0 {
            cb(remainder);
        }
    }

    if !batch.is_empty() {
        written += flush_batch(conn, &batch, params.on_batch_progress.as_deref())?;
    }

    delete_removed_paths(conn, params.existing, &current_paths)?;

    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .context("WAL checkpoint")?;

    Ok(written)
}

/// Apply index diff using a pool of writer connections.
///
/// When `params.pool_size > 1`, each writer opens its own connection; `conn` is only used for
/// the final `PRAGMA wal_checkpoint(TRUNCATE)` after all writers finish.
pub fn apply_index_diff_pooled(
    conn: &mut Connection,
    entries: &[Entry],
    existing: &HashMap<PathBuf, StoredMeta>,
    params: ApplyIndexDiffPooledParams<'_>,
) -> Result<()> {
    if params.pool_size <= 1 {
        return apply_index_diff(
            conn,
            entries,
            existing,
            params.mtime_window_ns,
            params.on_batch_progress,
        );
    }

    let current_paths: HashSet<_> = entries.iter().map(|e| e.path.clone()).collect();
    let buckets = partition_index_diff(
        entries,
        existing,
        &current_paths,
        params.pool_size,
        params.mtime_window_ns,
    );

    let db_path = params.db_path.to_path_buf();
    let on_batch_arc = params.on_batch_progress.map(|cb| Arc::new(Mutex::new(cb)));
    let passphrase = params.passphrase.map(String::from);

    let results: Vec<Result<()>> = buckets
        .into_par_iter()
        .map(|(bucket_entries, bucket_deletes)| {
            let n_written = bucket_entries.len();
            let mut conn = open_db(&db_path, passphrase.as_deref())?;
            let tx = conn.transaction().context("begin transaction")?;

            let mut stmt = tx.prepare(INSERT_PATH_SQL).context("prepare insert")?;

            for e in bucket_entries {
                execute_insert_entry(&mut stmt, &e)?;
            }
            drop(stmt);

            let mut delete_stmt = tx
                .prepare("DELETE FROM paths WHERE path = ?1")
                .context("prepare delete")?;
            for p in bucket_deletes {
                delete_stmt
                    .execute([p.to_string_lossy().as_ref()])
                    .context("delete path")?;
            }
            drop(delete_stmt);

            tx.commit().context("commit transaction")?;
            if let Some(ref arc) = on_batch_arc
                && let Ok(guard) = arc.as_ref().lock()
            {
                (*guard)(n_written);
            }
            Ok(())
        })
        .collect();

    for r in results {
        r?;
    }

    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .context("WAL checkpoint")?;

    Ok(())
}
