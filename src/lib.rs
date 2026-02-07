//! Nefaxer: High-performance directory indexer with content-aware diffing

pub mod check;
pub mod disk_detect;
pub mod engine;
pub mod index;
pub mod pipeline;
pub mod types;
pub mod utils;

/// Re-export types for API
pub use types::*;

use log::debug;
use std::path::Path;

/// Result alias used by public nefaxer API
pub use anyhow::Error;
pub type Result<T> = std::result::Result<T, Error>;

/// Single entry point: index `root` with `opts`, optionally diff against `existing`, and return `(nefax, diff)`.
///
/// - **`on_entry: None`** → non-callback path ([`nefax_dir_with_opts`](crate::index::nefax_dir_with_opts)). Used by CLI and by lib when you don't need streaming.
/// - **`on_entry: Some(f)`** → callback path (streaming). Lib-only; `f` is invoked for each entry as it's ready. Keep it fast or send to a channel.
///
/// Pass `existing: None` for a fresh index (diff will be all added); `Some(&nefax)` to diff against a previous snapshot (e.g. loaded from your own DB).
pub fn nefax_dir<F>(
    root: &Path,
    opts: &NefaxOpts,
    existing: Option<&Nefax>,
    on_entry: Option<F>,
) -> Result<(Nefax, Diff)>
where
    F: FnMut(&Entry),
{
    let opts = Opts::from(opts);
    let config_str = format!(
        "{} CONFIG:{:#?}",
        env!("CARGO_PKG_NAME").to_string().to_uppercase(),
        opts
    );
    debug!("{}", config_str);

    match on_entry {
        None => index::nefax_dir_with_opts(root, &opts, existing),
        Some(mut f) => index::nefax_dir_callback(root, &opts, existing, |e| f(e)),
    }
}

/// Returns `(num_threads, drive_type, use_parallel_walk)` for use in [`NefaxOpts`] when you have no DB.
///
/// Calls [`determine_threads_for_drive`](determine_threads_for_drive) with `conn: None` (network probe runs but is not cached).
/// Set all three on `NefaxOpts` so [`nefax_dir`] skips disk detection:
///
/// ```ignore
/// let (n, dt, pw) = nefaxer::tuning_for_path(path, None);
/// let opts = NefaxOpts { num_threads: Some(n), drive_type: Some(dt), use_parallel_walk: Some(pw), ..Default::default() };
/// let (nefax, _diff) = nefaxer::nefax_dir(path, &opts, None, None)?;
/// ```
pub fn tuning_for_path(
    path: &std::path::Path,
    available_threads: Option<usize>,
) -> (usize, disk_detect::DriveType, bool) {
    let avail = available_threads.unwrap_or_else(rayon::current_num_threads);
    disk_detect::determine_threads_for_drive(path, None, avail, None)
}
