//! Application configuration constants.
//! Tuning and thresholds in one place.

use std::sync::OnceLock;

// ---- Package / paths (from CARGO_PKG_NAME, cached) ----

/// Package-derived paths: built once from `CARGO_PKG_NAME`, then cached.
pub struct PackagePaths {
    pkg_name: &'static str,
    db_filename: String,
    probe_dir_name: String,
    results_filename: String,
}

static PACKAGE_PATHS: OnceLock<PackagePaths> = OnceLock::new();

impl PackagePaths {
    /// Build and cache paths from `CARGO_PKG_NAME`. Called once on first use.
    pub fn get() -> &'static PackagePaths {
        PACKAGE_PATHS.get_or_init(|| {
            let pkg = env!("CARGO_PKG_NAME");
            PackagePaths {
                pkg_name: pkg,
                db_filename: format!(".{pkg}"),
                probe_dir_name: format!(".{pkg}_probe"),
                results_filename: format!("{pkg}.results"),
            }
        })
    }

    pub fn pkg_name(&self) -> &str {
        self.pkg_name
    }

    pub fn output_filename(&self) -> &str {
        &self.db_filename
    }

    pub fn probe_dir_name(&self) -> &str {
        &self.probe_dir_name
    }

    pub fn results_filename(&self) -> &str {
        &self.results_filename
    }

    /// Filenames / dir names excluded from the walk by default. Does not include the index file
    /// (output_filename); that is excluded via db_canonical in the walk.
    pub fn default_exclude_patterns(&self) -> Vec<String> {
        vec![
            self.probe_dir_name().to_string(),
            self.results_filename().to_string(),
        ]
    }
}

// ---- Worker threads ----

/// Thread limits for drive-type-based tuning.
/// Use [`WorkerThreadLimits::current()`] to fill `all_threads` from rayon; the rest are const.
#[derive(Clone, Copy, Debug)]
pub struct WorkerThreadLimits {
    /// Available threads (from rayon); set by [`WorkerThreadLimits::current()`].
    pub all_threads: usize,
    /// Max threads for HDD (spinning disk).
    pub hdd_max: usize,
    /// Floor / minimum for network or unknown (conservative).
    pub floor: usize,
    /// Max threads when drive type is unknown.
    pub unknown_max: usize,
    /// Max threads when drive type is network.
    pub network_max: usize,
}

impl Default for WorkerThreadLimits {
    fn default() -> Self {
        Self {
            all_threads: 0, // use current() to set from rayon
            hdd_max: Self::HDD_THREADS,
            floor: Self::FLOOR_THREADS,
            unknown_max: Self::UNKNOWN_MAX_THREADS,
            network_max: Self::NETWORK_MAX_THREADS,
        }
    }
}

impl WorkerThreadLimits {
    pub const HDD_THREADS: usize = 4;
    pub const FLOOR_THREADS: usize = 2;
    pub const UNKNOWN_MAX_THREADS: usize = 8;
    pub const NETWORK_MAX_THREADS: usize = 12;

    /// Build limits with `all_threads` set from `rayon::current_num_threads()`.
    /// Call this at runtime when you need the effective available thread count.
    pub fn current() -> Self {
        Self {
            all_threads: rayon::current_num_threads(),
            ..Self::default()
        }
    }
}

// ---- Progress / chunking ----

/// Progress bar and adaptive chunk tuning.
pub struct ProgressConsts;

impl ProgressConsts {
    /// Batch size for progress bar updates during directory walk (reduce lock contention).
    pub const PROGRESS_UPDATE_BATCH_SIZE: usize = 100;
    /// Target number of progress updates across all workers in read_metadata (~100 total).
    pub const ADAPTIVE_PROGRESS_TARGET_UPDATES: usize = 100;
    /// Minimum chunk size for adaptive progress (avoid too-frequent updates).
    pub const ADAPTIVE_CHUNK_MIN: usize = 10;
}

// ---- Hashing ----

/// Hashing I/O thresholds and buffer sizes.
pub struct HashingConsts;

impl HashingConsts {
    /// File size above which hashing uses memory-mapped I/O (bytes). 100 MB.
    pub const HASH_MMAP_THRESHOLD: u64 = 100 * 1024 * 1024;
    /// Chunk size for reading files below mmap threshold (bytes). 1 MB.
    pub const HASH_READ_CHUNK_SIZE: usize = 1024 * 1024;
}

// ---- Indexing ----

/// Files smaller than this are not hashed; mtime/size only (bytes).
pub const SMALL_FILE_THRESHOLD: u64 = 4 * 1024; // 4 KB

// ---- Database ----

/// Batch size for DB insert/update chunks (balance transaction size vs round-trips).
pub const DB_INSERT_BATCH_SIZE: usize = 1000;

// ---- Streaming channel cap ----

/// Channel cap (path + entry) tuned by drive type; after first run, finetuned from stored path count in diskinfo.
pub struct StreamingChannelCap;

impl StreamingChannelCap {
    /// Default cap when no stored path count (SSD: walk fast, let it run ahead).
    pub const DEFAULT_SSD: usize = 500_000;
    /// HDD: walk is serial and slower; smaller buffer is enough.
    pub const DEFAULT_HDD: usize = 100_000;
    /// Network: conservative.
    pub const DEFAULT_NETWORK: usize = 200_000;
    /// Unknown drive: conservative (matches previous fixed default).
    pub const DEFAULT_UNKNOWN: usize = 50_000;
    /// Upper bound when using stored path count (avoid huge allocation).
    pub const MAX: usize = 1_000_000;
    /// Margin added to stored path count when finetuning (e.g. tree grew slightly).
    pub const MARGIN: usize = 10_000;
}

// ---- Diff / list output ----

/// When --list is set, if total changes (added+removed+modified) exceed this, write paths to RESULTS_FILENAME instead of stdout.
pub const LIST_THRESHOLD: usize = 100;
