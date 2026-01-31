//! Nefaxer CLI: index directories and diff against an existing index.

use anyhow::Result;
use clap::Parser;
use nefaxer::engine::arg_parser::{Cli, Commands};
use nefaxer::engine::{handle_check, handle_index};
use std::time::Instant;

fn main() -> Result<()> {
    let start_time = Instant::now();
    let cli = Cli::parse();

    match cli.command {
        Commands::Index { common } => handle_index(&common)?,
        Commands::Check { common } => handle_check(&common)?,
    }

    log::debug!("Total time: {:?}", start_time.elapsed());
    Ok(())
}
