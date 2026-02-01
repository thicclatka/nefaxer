//! Open, backup, and load index database.

use anyhow::{Context, Result};
use rusqlite::Connection;
use rusqlite::backup::Backup;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::utils::get_passphrase;

use super::{SCHEMA, StoredMeta, WAL_PRAGMAS};

/// Enable WAL and apply schema to an open connection (idempotent).
fn apply_wal_and_schema(conn: &Connection) -> Result<()> {
    conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))
        .context("enable WAL")?;
    conn.execute_batch(WAL_PRAGMAS).context("set WAL pragmas")?;
    conn.execute_batch(SCHEMA).context("create schema")?;
    Ok(())
}

/// Open or create the index DB and ensure schema + WAL with optimizations.
/// If `passphrase` is Some, set SQLCipher PRAGMA key before any other operations.
pub fn open_db(path: &Path, passphrase: Option<&str>) -> Result<Connection> {
    let conn = Connection::open(path).context("open database")?;

    if let Some(key) = passphrase {
        conn.pragma_update(None, "key", key)
            .context("set SQLCipher key")?;
    }

    apply_wal_and_schema(&conn)?;
    Ok(conn)
}

/// Open existing DB, detecting if it is encrypted: try without key first; if read fails, load
/// passphrase (env → .env in dir → prompt) and open with key. Returns (connection, passphrase_used).
pub fn open_db_or_detect_encrypted(
    path: &Path,
    dir: &Path,
) -> Result<(Connection, Option<String>)> {
    let conn = Connection::open(path).context("open database")?;
    if conn.query_row("SELECT 1", [], |_| Ok(())).is_ok() {
        apply_wal_and_schema(&conn)?;
        return Ok((conn, None));
    }
    drop(conn);
    let pass = get_passphrase(dir, false)?;
    let conn = open_db(path, Some(pass.as_str()))?;
    Ok((conn, Some(pass)))
}

/// Open an in-memory DB with the same schema (for small-index path; no WAL pragmas needed).
pub fn open_db_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory().context("open in-memory database")?;
    conn.execute_batch(SCHEMA).context("create schema")?;
    Ok(conn)
}

/// Copy the source database (e.g. in-memory) to a file. Destination is overwritten.
/// If `passphrase` is Some, the backup file is encrypted with SQLCipher using that key.
pub fn backup_to_file(source: &Connection, path: &Path, passphrase: Option<&str>) -> Result<()> {
    let mut dest = Connection::open(path).context("open destination database for backup")?;
    if let Some(key) = passphrase {
        dest.pragma_update(None, "key", key)
            .context("set SQLCipher key on destination")?;
    }
    {
        let backup = Backup::new(source, &mut dest).context("create backup")?;
        backup
            .run_to_completion(100, Duration::from_millis(0), None)
            .context("run backup to completion")?;
    }
    let wal_with_mode = format!("PRAGMA journal_mode = WAL;{}", WAL_PRAGMAS);
    dest.execute_batch(&wal_with_mode)
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
