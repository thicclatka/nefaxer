//! Index diff: apply_index_diff_streaming (stream entries to DB with one writer).

use anyhow::{Context, Result};
use crossbeam_channel::Receiver;
use rusqlite::{Connection, Statement};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::engine::hashing::{hash_equals, hash_file};
use crate::engine::tools::{mtime_changed, path_to_db_string};
use crate::utils::config::{DB_INSERT_BATCH_SIZE, SMALL_FILE_THRESHOLD};
use crate::{Diff, Entry};

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
            stmt.execute([path_to_db_string(old_path).as_str()])
                .context("delete path")?;
        }
    }
    Ok(())
}

/// Execute one path insert for an entry (used by flush_batch).
fn execute_insert_entry(stmt: &mut Statement<'_>, e: &Entry) -> Result<()> {
    stmt.execute((
        path_to_db_string(&e.path).as_str(),
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
    /// When set, accumulate added/removed/modified for a summary after indexing (index must have existed).
    pub diff: Option<&'a mut Diff>,
    /// When set, build the current index map incrementally (path → StoredMeta) so caller gets it without a second load_index.
    pub result_map: Option<&'a mut HashMap<PathBuf, StoredMeta>>,
}

/// Write entries to DB as they are received (streaming). Tracks current paths for deletes at end.
pub fn apply_index_diff_streaming(
    conn: &mut Connection,
    entry_rx: Receiver<Entry>,
    params: &mut ApplyIndexDiffStreamingParams<'_>,
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
            let existing_meta = params.existing.get(&entry.path);
            let reuse_hash = existing_meta.is_some_and(|(old_mtime, old_size, old_hash)| {
                !mtime_changed(entry.mtime_ns, *old_mtime, params.mtime_window_ns)
                    && entry.size == *old_size
                    && old_hash.as_ref().is_some_and(|v| v.len() == 32)
            });
            if reuse_hash {
                if let Some((_, _, Some(v))) = existing_meta {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(v);
                    entry.hash = Some(arr);
                }
            } else {
                let abs = r.join(&entry.path);
                if let Ok(Some(h)) = hash_file(&abs, entry.size) {
                    entry.hash = Some(h);
                }
            }
        }
        current_paths.insert(entry.path.clone());
        if let Some(ref mut map) = params.result_map {
            let hash = entry.hash.map(|a| a.to_vec()).or_else(|| {
                params
                    .existing
                    .get(&entry.path)
                    .and_then(|(_, _, h)| h.clone())
            });
            map.insert(entry.path.clone(), (entry.mtime_ns, entry.size, hash));
        }
        if entry_needs_update(&entry, params.existing, params.mtime_window_ns) {
            if let Some(diff) = params.diff.as_deref_mut() {
                if params.existing.contains_key(&entry.path) {
                    diff.modified.push(entry.path.clone());
                } else {
                    diff.added.push(entry.path.clone());
                }
            }
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

    if let Some(diff) = params.diff.as_deref_mut() {
        for path in params.existing.keys() {
            if !current_paths.contains(path) {
                diff.removed.push(path.clone());
            }
        }
    }

    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .context("WAL checkpoint")?;

    Ok(written)
}
