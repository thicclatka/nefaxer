//! Database operations

use super::hashing::hash_equals;
use super::tools::mtime_changed;
use crate::Entry;
use crate::utils::config::DB_INSERT_BATCH_SIZE;
use anyhow::{Context, Result};
use rayon::prelude::*;
use rusqlite::backup::Backup;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS paths (
    path TEXT PRIMARY KEY,
    mtime_ns INTEGER NOT NULL,
    size INTEGER NOT NULL,
    hash BLOB
);
CREATE INDEX IF NOT EXISTS idx_paths_path ON paths(path);

CREATE TABLE IF NOT EXISTS diskinfo (
    root_path TEXT PRIMARY KEY,
    data TEXT NOT NULL
);
"#;

/// Stored row: (mtime_ns, size, hash).
pub type StoredMeta = (i64, u64, Option<Vec<u8>>);

/// Open or create the index DB and ensure schema + WAL with optimizations.
pub fn open_db(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).context("open database")?;

    // Enable WAL mode (persistent)
    conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))
        .context("enable WAL")?;

    // WAL optimizations for bulk inserts
    conn.execute_batch(
        r#"
        PRAGMA synchronous = NORMAL;
        PRAGMA wal_autocheckpoint = 10000;
        PRAGMA journal_size_limit = 67108864;
        "#,
    )
    .context("set WAL pragmas")?;

    conn.execute_batch(SCHEMA).context("create schema")?;
    Ok(conn)
}

/// Open an in-memory DB with the same schema (for small-index path; no WAL pragmas needed).
pub fn open_db_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory().context("open in-memory database")?;
    conn.execute_batch(SCHEMA).context("create schema")?;
    Ok(conn)
}

/// Copy the source database (e.g. in-memory) to a file. Destination is overwritten.
/// Applies WAL and pragmas to the file connection so the result is ready for normal use.
pub fn backup_to_file(source: &Connection, path: &Path) -> Result<()> {
    let mut dest = Connection::open(path).context("open destination database for backup")?;
    {
        let backup = Backup::new(source, &mut dest).context("create backup")?;
        backup
            .run_to_completion(100, Duration::from_millis(0), None)
            .context("run backup to completion")?;
    }
    dest.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA wal_autocheckpoint = 10000;
        PRAGMA journal_size_limit = 67108864;
        "#,
    )
    .context("set WAL pragmas on destination")?;
    Ok(())
}

/// Load existing index from DB into a map: path -> (mtime_ns, size, hash).
pub fn load_index(conn: &Connection) -> Result<HashMap<PathBuf, StoredMeta>> {
    let mut stmt = conn.prepare("SELECT path, mtime_ns, size, hash FROM paths")?;
    let rows = stmt.query_map([], |row| {
        let path: String = row.get(0)?;
        let mtime_ns: i64 = row.get(1)?;
        let size: i64 = row.get(2)?;
        let hash: Option<Vec<u8>> = row.get(3)?;
        Ok((PathBuf::from(path), (mtime_ns, size.max(0) as u64, hash)))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (path, rest) = row?;
        map.insert(path, rest);
    }
    Ok(map)
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

    let mut stmt = tx
        .prepare(
            "INSERT OR REPLACE INTO paths (path, mtime_ns, size, hash) VALUES (?1, ?2, ?3, ?4)",
        )
        .context("prepare insert")?;

    for chunk in entries.chunks(DB_INSERT_BATCH_SIZE) {
        for e in chunk {
            let needs_update = match existing.get(&e.path) {
                None => true,
                Some((old_mtime, old_size, old_hash)) => {
                    mtime_changed(e.mtime_ns, *old_mtime, mtime_window_ns)
                        || *old_size != e.size
                        || !hash_equals(&e.hash, old_hash)
                }
            };
            if needs_update {
                let path_str = e.path.to_string_lossy();
                stmt.execute((
                    path_str.as_ref(),
                    e.mtime_ns,
                    e.size as i64,
                    e.hash.as_ref().map(|h| h.as_slice()),
                ))
                .context("insert path")?;
            }
        }
        if let Some(ref cb) = on_batch_progress {
            cb(chunk.len());
        }
    }
    drop(stmt);

    let mut delete_stmt = tx
        .prepare("DELETE FROM paths WHERE path = ?1")
        .context("prepare delete")?;
    for old_path in existing.keys() {
        if !current_paths.contains(old_path) {
            delete_stmt
                .execute([old_path.to_string_lossy().as_ref()])
                .context("delete path")?;
        }
    }
    drop(delete_stmt);

    tx.commit().context("commit transaction")?;

    // Reclaim WAL space after bulk insert (checkpoint and truncate WAL file)
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .context("WAL checkpoint")?;

    Ok(())
}

/// Assign path to a writer bucket by hash (reduces lock contention when using multiple writers).
fn path_bucket(path: &Path, n_buckets: usize) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    (hasher.finish() as usize) % n_buckets
}

/// Partition entries (that need update) and deletes by path hash into `n_buckets` for writer pool.
fn partition_index_diff(
    entries: &[Entry],
    existing: &HashMap<PathBuf, StoredMeta>,
    current_paths: &HashSet<PathBuf>,
    n_buckets: usize,
    mtime_window_ns: i64,
) -> Vec<(Vec<Entry>, Vec<PathBuf>)> {
    let mut buckets: Vec<(Vec<Entry>, Vec<PathBuf>)> =
        (0..n_buckets).map(|_| (Vec::new(), Vec::new())).collect();

    for e in entries {
        let needs_update = match existing.get(&e.path) {
            None => true,
            Some((old_mtime, old_size, old_hash)) => {
                mtime_changed(e.mtime_ns, *old_mtime, mtime_window_ns)
                    || *old_size != e.size
                    || !hash_equals(&e.hash, old_hash)
            }
        };
        if needs_update {
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

/// Apply index diff using a pool of writer connections. Partitions by path hash so each
/// writer handles a subset; WAL serializes writers but lock hold time per transaction is reduced.
/// Uses single connection if `pool_size <= 1`.
pub fn apply_index_diff_pooled(
    db_path: &Path,
    conn: &mut Connection,
    entries: &[Entry],
    existing: &HashMap<PathBuf, StoredMeta>,
    mtime_window_ns: i64,
    on_batch_progress: Option<Box<dyn Fn(usize) + Send>>,
    pool_size: usize,
) -> Result<()> {
    if pool_size <= 1 {
        return apply_index_diff(conn, entries, existing, mtime_window_ns, on_batch_progress);
    }

    let current_paths: HashSet<_> = entries.iter().map(|e| e.path.clone()).collect();
    let buckets = partition_index_diff(
        entries,
        existing,
        &current_paths,
        pool_size,
        mtime_window_ns,
    );

    let db_path = db_path.to_path_buf();
    let on_batch = on_batch_progress;

    let results: Vec<Result<()>> = buckets
        .into_par_iter()
        .map(|(bucket_entries, bucket_deletes)| {
            let mut conn = open_db(&db_path)?;
            let tx = conn.transaction().context("begin transaction")?;

            let mut stmt = tx
                .prepare(
                    "INSERT OR REPLACE INTO paths (path, mtime_ns, size, hash) VALUES (?1, ?2, ?3, ?4)",
                )
                .context("prepare insert")?;

            for e in bucket_entries {
                let path_str = e.path.to_string_lossy();
                stmt.execute((
                    path_str.as_ref(),
                    e.mtime_ns,
                    e.size as i64,
                    e.hash.as_ref().map(|h| h.as_slice()),
                ))
                .context("insert path")?;
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
            Ok(())
        })
        .collect();

    for r in results {
        r?;
    }

    if let Some(ref cb) = on_batch {
        cb(entries.len());
    }

    // Single checkpoint after all writers finish
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .context("WAL checkpoint")?;

    Ok(())
}
