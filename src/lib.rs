//! Nefaxer: High-performance directory indexer with content-aware diffing

pub mod check;
pub mod disk_detect;
pub mod engine;
pub mod index;
pub mod utils;

use std::path::PathBuf;

// Re-export main API
pub use check::check_dir;
pub use index::index_dir;

/// Metadata for a single path (file or dir). Dirs have size 0 and no hash.
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
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

/// Options for indexing and checking.
#[derive(Clone, Default)]
pub struct Opts {
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
}
