//! Shared constants and helpers for chunking and parallel processing.

use log::debug;

pub use crate::utils::config::ProgressConsts;
pub use crate::utils::config::WRITER_POOL_SIZE;

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

pub fn setup_writer_pool_size(is_network_drive: bool) -> usize {
    let writer_pool_size = if is_network_drive {
        1
    } else {
        WRITER_POOL_SIZE
    };
    debug!("Writer pool size: {}", writer_pool_size);
    writer_pool_size
}

pub fn parallel_walk_handler(parallel_walk: bool) {
    if parallel_walk {
        debug!("Walking in parallel");
    } else {
        debug!("Walking serially");
    }
}
