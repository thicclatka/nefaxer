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
    #[arg(long, short = 'l', num_args = 0..=1, default_missing_value = "true", value_parser = clap::value_parser!(bool))]
    pub list: Option<bool>,

    /// Verbose output.
    #[arg(long, short = 'v', num_args = 0..=1, default_missing_value = "true", value_parser = clap::value_parser!(bool))]
    pub verbose: Option<bool>,

    /// Compute blake3 hash for files (slower but more accurate).
    #[arg(long, short = 'c', num_args = 0..=1, default_missing_value = "true", value_parser = clap::value_parser!(bool))]
    pub check_hash: Option<bool>,

    /// Follow symbolic links.
    #[arg(long, short = 'f', num_args = 0..=1, default_missing_value = "true", value_parser = clap::value_parser!(bool))]
    pub follow_links: Option<bool>,

    /// Mtime tolerance window in seconds. Files within this window are considered unchanged.
    #[arg(long, short = 'm', value_parser = clap::value_parser!(i64))]
    pub mtime_window: Option<i64>,

    /// Exclude patterns (glob syntax). Can specify multiple: -e pattern1 pattern2 pattern3
    #[arg(long, short = 'e', num_args = 1..)]
    pub exclude: Vec<String>,

    /// Strict mode: fail on first permission error instead of skipping.
    #[arg(long, num_args = 0..=1, default_missing_value = "true", value_parser = clap::value_parser!(bool))]
    pub strict: Option<bool>,

    /// Paranoid mode: re-hash files when hash matches but mtime/size differ (detect collisions).
    #[arg(long, num_args = 0..=1, default_missing_value = "true", value_parser = clap::value_parser!(bool))]
    pub paranoid: Option<bool>,

    /// Encrypt the index database with SQLCipher. Prompts for passphrase (or use NEFAXER_DB_KEY / .env).
    #[arg(long, short = 'x', num_args = 0..=1, default_missing_value = "true", value_parser = clap::value_parser!(bool))]
    pub encrypt: Option<bool>,
}

impl Cli {
    /// Get the database path, defaulting to package db filename in the target directory.
    pub fn db_path(&self) -> PathBuf {
        self.db
            .clone()
            .unwrap_or_else(|| self.dir.join(PackagePaths::get().output_filename()))
    }
}
