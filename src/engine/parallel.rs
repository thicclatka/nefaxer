//! Shared constants and helpers for chunking and parallel processing.

use log::debug;

pub use crate::utils::config::ProgressConsts;

/// Compute chunk size for batched progress updates in a parallel loop.
///
/// Aims for ~`target_updates` progress bar updates total, distributed across workers,
/// with a minimum of `ProgressConsts::ADAPTIVE_CHUNK_MIN` to avoid contention.
pub fn adaptive_progress_chunk_size(
    total_items: usize,
    num_workers: usize,
    target_updates: usize,
) -> usize {
    (total_items / (num_workers * target_updates)).max(ProgressConsts::ADAPTIVE_CHUNK_MIN)
}

pub fn parallel_walk_handler(parallel_walk: bool) {
    if parallel_walk {
        debug!("Walking in parallel");
    } else {
        debug!("Walking serially");
    }
}
