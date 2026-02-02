//! Nefaxer CLI: index directories; use --dry-run to compare without writing.

use anyhow::Result;
use clap::Parser;
use nefaxer::engine::arg_parser::Cli;
use nefaxer::engine::handle_run;
use std::time::Instant;

fn main() -> Result<()> {
    let start_time = Instant::now();
    let cli = Cli::parse();
    handle_run(&cli)?;
    log::debug!("Total time: {:?}", start_time.elapsed());
    Ok(())
}
