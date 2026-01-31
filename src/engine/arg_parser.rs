use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

use crate::utils::config::PackagePaths;

struct DefaultArgs;

impl DefaultArgs {
    pub const DIR: &'static str = ".";
}

#[derive(Parser)]
#[command(name = "nefaxer")]
#[command(about = "High-performance directory indexer with content-aware diffing")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Shared options for index and check.
#[derive(Clone, Args)]
pub struct CommonArgs {
    /// Directory to index. Default: current directory.
    #[arg(value_name = "DIR", default_value = DefaultArgs::DIR)]
    pub dir: PathBuf,

    /// Path to nefaxer index file. Default: `.nefaxer` in DIR.
    #[arg(long, short)]
    pub db: Option<PathBuf>,

    /// Verbose output. Default: false.
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Compute blake3 hash for files (slower but more accurate). Default: false.
    #[arg(long, short = 'c')]
    pub check_hash: bool,

    /// Follow symbolic links. Default: false.
    #[arg(long, short = 'l')]
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

    /// Paranoid mode (check): re-hash files when hash matches but mtime/size differ (detect collisions). Default: false.
    #[arg(long)]
    pub paranoid: bool,
}

impl CommonArgs {
    /// Get the database path, defaulting to package db filename in the target directory.
    pub fn db_path(&self) -> PathBuf {
        self.db
            .clone()
            .unwrap_or_else(|| self.dir.join(PackagePaths::get().output_filename()))
    }
}

#[derive(Subcommand)]
pub enum Commands {
    /// Walk directory and write/update the index database.
    Index {
        #[command(flatten)]
        common: CommonArgs,
    },

    /// Compare directory to existing index; report added/removed/modified paths.
    Check {
        #[command(flatten)]
        common: CommonArgs,
    },
}
