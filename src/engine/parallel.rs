//! Shared constants and helpers for chunking and parallel processing.

/// Re-export progress/chunk config from utils for engine callers.
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
