use anyhow::Result;
use log::warn;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::Opts;

/// Check pipeline result: if strict and a first error was recorded, return it; otherwise log skipped paths.
/// Call after joining walk and workers. Counts by error type and prints one warn with breakdown.
///
/// # Errors
///
/// Returns [`anyhow::Error`] when `opts.strict` is true and a walk/metadata error was recorded.
///
/// # Panics
///
/// Panics if `first_error` or `skipped_paths` mutexes are poisoned.
pub fn check_for_initial_error_or_skipped_paths(
    opts: &Opts,
    first_error: &Arc<Mutex<Option<String>>>,
    skipped_paths: &Arc<Mutex<Vec<(PathBuf, String)>>>,
) -> Result<()> {
    if opts.strict
        && let Some(msg) = first_error.lock().unwrap().take()
    {
        return Err(anyhow::anyhow!("{msg}"));
    }
    let skipped = skipped_paths.lock().unwrap();
    let total = skipped.len();
    if total > 0 && !opts.strict {
        let mut by_msg: HashMap<&str, usize> = HashMap::new();
        for (_, msg) in skipped.iter() {
            *by_msg.entry(msg.as_str()).or_insert(0) += 1;
        }
        warn!("Skipped/issue breakdown:");
        warn!("  - Total: {total}");
        for (msg, count) in by_msg {
            // Log-only percentage; usize→f64 is acceptable for display.
            #[allow(clippy::cast_precision_loss)]
            let pct = (count as f64 / total as f64) * 100.0;
            warn!("  - {}: {} paths ({pct:.2}%)", msg, count);
        }
    }
    Ok(())
}
