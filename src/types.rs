//! Public and internal types for the nefaxer API and pipeline.

use std::collections::HashMap;
use std::path::PathBuf;

/// Metadata for a single path (file or dir). Dirs have size 0 and no hash.
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub mtime_ns: i64,
    pub size: u64,
    pub hash: Option<[u8; 32]>,
}

/// Metadata for one path in the index (same shape as a row in the `.nefaxer` DB).
///
/// Used as the value type of [`Nefax`]. For a table-backed snapshot, use columns `mtime_ns`, `size`, `hash` (32 bytes, or NULL).
#[derive(Clone, Debug)]
pub struct PathMeta {
    /// Modification time in nanoseconds since epoch.
    pub mtime_ns: i64,
    /// File size in bytes (0 for directories).
    pub size: u64,
    /// Blake3 hash (32 bytes), or `None` if not computed.
    pub hash: Option<[u8; 32]>,
}

/// Result of comparing a directory to an existing index.
#[derive(Default)]
pub struct Diff {
    pub added: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
    pub modified: Vec<PathBuf>,
}

/// Map of path → metadata for the indexed tree. Same shape as the `.nefaxer` DB.
///
/// **Shape:** `HashMap<PathBuf, PathMeta>` where each key is a path (relative to the indexed root)
/// and each value is [`PathMeta`] (`mtime_ns`, `size`, `hash`). Returned by [`nefax_dir`](crate::nefax_dir); you can also build one from your own table and
/// pass it as `existing`.
pub type Nefax = HashMap<PathBuf, PathMeta>;

/// Lib-only options for [`nefax_dir`](crate::nefax_dir). Only the fields that apply when using the crate (no DB).
#[derive(Clone, Debug, Default)]
pub struct NefaxOpts {
    /// Override worker thread count. When None, derived from drive type and FD limit.
    pub num_threads: Option<usize>,
    /// When set together with [`Self::num_threads`] and [`Self::use_parallel_walk`], skip disk detection and use these values (e.g. from [`tuning_for_path`](crate::tuning_for_path) or [`determine_threads_for_drive`](crate::disk_detect::determine_threads_for_drive) with `conn: None`).
    pub drive_type: Option<crate::disk_detect::DriveType>,
    /// Use parallel walk (jwalk). When set with `num_threads` and `drive_type`, skip disk detection.
    pub use_parallel_walk: Option<bool>,
    /// Compute blake3 hash for files (slower but accurate change detection).
    pub with_hash: bool,
    /// Follow symbolic links.
    pub follow_links: bool,
    /// Exclude patterns (glob syntax, e.g. `node_modules`, `*.log`).
    pub exclude: Vec<String>,
    /// Mtime tolerance window in nanoseconds.
    pub mtime_window_ns: i64,
    /// Strict mode: fail on first permission/access error instead of skipping.
    pub strict: bool,
    /// Paranoid mode: re-hash when hash matches but mtime/size differ.
    pub paranoid: bool,
}

impl From<&NefaxOpts> for Opts {
    fn from(o: &NefaxOpts) -> Self {
        Opts {
            db_path: None,
            num_threads: o.num_threads,
            drive_type: o.drive_type,
            use_parallel_walk: o.use_parallel_walk,
            with_hash: o.with_hash,
            follow_links: o.follow_links,
            exclude: o.exclude.clone(),
            verbose: false,
            mtime_window_ns: o.mtime_window_ns,
            strict: o.strict,
            paranoid: o.paranoid,
            encrypt: false,
            list_paths: false,
            write_to_db: false,
        }
    }
}

/// Full options (CLI and check). Use [`NefaxOpts`] for lib.
#[derive(Clone, Default)]
pub struct Opts {
    /// Index database path. When None, uses `root.join(<package index filename>)` (e.g. `.nefaxer`).
    pub db_path: Option<PathBuf>,
    /// Override worker thread count. When None, derived from drive type and FD limit.
    pub num_threads: Option<usize>,
    /// When set with num_threads and use_parallel_walk, skip disk detection (e.g. lib caller passed result of [`tuning_for_path`](crate::tuning_for_path) or [`determine_threads_for_drive`](crate::disk_detect::determine_threads_for_drive)).
    pub drive_type: Option<crate::disk_detect::DriveType>,
    /// Use parallel walk (jwalk). When set with num_threads and drive_type, skip disk detection.
    pub use_parallel_walk: Option<bool>,
    /// Compute blake3 hash for files (slower but accurate change detection).
    pub with_hash: bool,
    /// Follow symbolic links.
    pub follow_links: bool,
    /// Exclude patterns (glob syntax).
    pub exclude: Vec<String>,
    /// Show progress bar (verbose mode).
    pub verbose: bool,
    /// Mtime tolerance window in nanoseconds.
    pub mtime_window_ns: i64,
    /// Strict mode: fail on first permission/access error instead of skipping.
    pub strict: bool,
    /// Paranoid mode (check): re-hash when hash matches but mtime/size differ.
    pub paranoid: bool,
    /// Encrypt the index database with SQLCipher.
    pub encrypt: bool,
    /// List each changed path (added/removed/modified). If total > LIST_THRESHOLD, write to nefaxer.results instead of stdout.
    pub list_paths: bool,
    /// When true, write index to DB (CLI). When false, run pipeline and return diff only (lib).
    pub write_to_db: bool,
}
