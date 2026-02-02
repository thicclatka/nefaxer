//! Nefaxer: High-performance directory indexer with content-aware diffing

pub mod check;
pub mod disk_detect;
pub mod engine;
pub mod index;
pub mod pipeline;
pub mod utils;

use std::collections::HashMap;
use std::path::PathBuf;

// Re-export main API (lib only; check_dir is CLI-internal)
pub use index::nefax_dir;

/// Metadata for a single path (file or dir). Dirs have size 0 and no hash.
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub mtime_ns: i64,
    pub size: u64,
    pub hash: Option<[u8; 32]>,
}

/// Metadata for one path in the index (same shape as a row in the `.nefaxer` DB).
#[derive(Clone, Debug)]
pub struct PathMeta {
    pub mtime_ns: i64,
    pub size: u64,
    pub hash: Option<[u8; 32]>,
}

/// Result of comparing a directory to an existing index.
#[derive(Default)]
pub struct Diff {
    pub added: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
    pub modified: Vec<PathBuf>,
}

/// The nefax map: path → metadata. Same shape as the `.nefaxer` DB. Returned by [`nefax_dir`].
pub type Nefax = HashMap<PathBuf, PathMeta>;

/// Lib-only options for [`nefax_dir`]. Only the fields that apply when using the crate (no DB).
#[derive(Clone, Debug, Default)]
pub struct NefaxOpts {
    /// Override worker thread count. When None, derived from drive type and FD limit.
    pub num_threads: Option<usize>,
    /// Compute blake3 hash for files (slower but accurate change detection).
    pub with_hash: bool,
    /// Follow symbolic links.
    pub follow_links: bool,
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
            with_hash: o.with_hash,
            follow_links: o.follow_links,
            exclude: vec![],
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
