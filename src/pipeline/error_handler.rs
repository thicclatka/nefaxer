use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::Opts;

/// Check pipeline result: if strict and a first error was recorded, return it; otherwise log skipped paths.
/// Call after joining walk and workers.
pub fn check_for_initial_error_or_skipped_paths(
    opts: &Opts,
    first_error: &Arc<Mutex<Option<String>>>,
    skipped_paths: &Arc<Mutex<Vec<PathBuf>>>,
) -> Result<()> {
    if opts.strict
        && let Some(msg) = first_error.lock().unwrap().take()
    {
        return Err(anyhow::anyhow!("{}", msg));
    }
    let skipped = skipped_paths.lock().unwrap().len();
    if skipped > 0 && !opts.strict {
        log::warn!(
            "Skipped {} paths due to permission errors or access issues",
            skipped
        );
        if opts.verbose {
            for p in skipped_paths.lock().unwrap().iter() {
                eprintln!("  skipped: {}", p.display());
            }
        }
    }
    Ok(())
}
