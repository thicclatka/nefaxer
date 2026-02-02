use clap::Parser;
use std::path::PathBuf;

use crate::utils::config::PackagePaths;

struct DefaultArgs;

impl DefaultArgs {
    pub const DIR: &'static str = ".";
}

/// High-performance directory indexer with content-aware diffing.
#[derive(Clone, Parser)]
#[command(name = "nefaxer")]
#[command(about = "Index a directory; use --dry-run to compare without writing.")]
pub struct Cli {
    /// Directory to index. Default: current directory.
    #[arg(value_name = "DIR", default_value = DefaultArgs::DIR)]
    pub dir: PathBuf,

    /// Path to nefaxer index file. Default: `.nefaxer` in DIR.
    #[arg(long, short)]
    pub db: Option<PathBuf>,

    /// Compare to index and report added/removed/modified; do not write to the index.
    #[arg(long)]
    pub dry_run: bool,

    /// List each changed path. If total changes exceed threshold, write to nefaxer.results instead of stdout.
    #[arg(long, short = 'l')]
    pub list: bool,

    /// Verbose output. Default: false.
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Compute blake3 hash for files (slower but more accurate). Default: false.
    #[arg(long, short = 'c')]
    pub check_hash: bool,

    /// Follow symbolic links. Default: false.
    #[arg(long, short = 'f')]
    pub follow_links: bool,

    /// Mtime tolerance window in seconds. Files within this window are considered unchanged. Default: 0 (exact match).
    #[arg(long, short = 'm', default_value = "0")]
    pub mtime_window: i64,

    /// Exclude patterns (glob syntax). Can specify multiple: -e pattern1 pattern2 pattern3
    #[arg(long, short = 'e', num_args = 1..)]
    pub exclude: Vec<String>,

    /// Strict mode: fail on first permission error instead of skipping. Default: false.
    #[arg(long)]
    pub strict: bool,

    /// Paranoid mode: re-hash files when hash matches but mtime/size differ (detect collisions). Default: false.
    #[arg(long)]
    pub paranoid: bool,

    /// Encrypt the index database with SQLCipher. Prompts for passphrase (or use NEFAXER_DB_KEY / .env).
    #[arg(long, short = 'x')]
    pub encrypt: bool,
}

impl Cli {
    /// Get the database path, defaulting to package db filename in the target directory.
    pub fn db_path(&self) -> PathBuf {
        self.db
            .clone()
            .unwrap_or_else(|| self.dir.join(PackagePaths::get().output_filename()))
    }
}
