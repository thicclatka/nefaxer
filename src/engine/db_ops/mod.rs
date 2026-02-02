//! Database operations: schema, open/backup/load, index diff (streaming).

mod connection;
mod indexer;

pub use connection::{load_index, open_db, open_db_in_memory, open_db_or_detect_encrypted};
pub use indexer::{ApplyIndexDiffStreamingParams, apply_index_diff_streaming};

/// Stored row: (mtime_ns, size, hash).
pub type StoredMeta = (i64, u64, Option<Vec<u8>>);

/// WAL tuning pragmas (synchronous, autocheckpoint, size limit). Use after PRAGMA journal_mode = WAL.
pub(crate) const WAL_PRAGMAS: &str = r#"
        PRAGMA synchronous = NORMAL;
        PRAGMA wal_autocheckpoint = 10000;
        PRAGMA journal_size_limit = 67108864;
        "#;

/// Insert statement for paths table.
pub(crate) const INSERT_PATH_SQL: &str =
    "INSERT OR REPLACE INTO paths (path, mtime_ns, size, hash) VALUES (?1, ?2, ?3, ?4)";

/// Schema for paths and diskinfo tables.
pub(crate) const SCHEMA: &str = r#"
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
