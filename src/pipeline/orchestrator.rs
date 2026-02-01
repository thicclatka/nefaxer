use anyhow::Result;
use log::debug;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

use crate::Opts;
use crate::disk_detect::determine_threads_for_drive;
use crate::engine::parallel::{parallel_walk_handler, setup_writer_pool_size};
use crate::engine::tools::canonicalize_paths;
use crate::pipeline;
use crate::utils::config::WorkerThreadLimits;

/// Start the walk + metadata pipeline. Returns receiver and handles; caller receives from
/// `entry_rx` and must join `walk_handle` and `worker_handles` when done.
pub fn run_pipeline(
    root: &Path,
    opts: &Opts,
    db_path: &Path,
    temp_path: Option<&Path>,
    conn: &Connection,
) -> Result<pipeline::PipelineHandles> {
    let (root, db_canonical, temp_canonical, tuning) =
        setup_pipeline_root_and_tuning(root, db_path, temp_path, conn)?;

    let channels = pipeline::create_pipeline_channels(&root, &db_canonical, &temp_canonical, opts);

    let walk_handle = pipeline::spawn_walk_thread(
        channels.path_tx,
        channels.path_count_tx,
        channels.ctx,
        tuning.parallel_walk,
    );

    let worker_handles = pipeline::spawn_metadata_workers(
        channels.path_rx,
        &channels.entry_tx,
        &root,
        tuning.num_threads,
    );

    // Dropping the last sender closes the channel so workers exit.
    drop(channels.entry_tx);

    Ok(pipeline::PipelineHandles {
        entry_rx: channels.entry_rx,
        path_count_rx: channels.path_count_rx,
        walk_handle,
        worker_handles,
        writer_pool_size: tuning.writer_pool_size,
        is_network_drive: tuning.is_network_drive,
        first_error: channels.first_error,
        skipped_paths: channels.skipped_paths,
    })
}

/// Canonicalize root and paths, detect drive type, compute thread count and writer pool size.
pub fn setup_pipeline_root_and_tuning(
    root: &Path,
    db_path: &Path,
    temp_path: Option<&Path>,
    conn: &Connection,
) -> Result<(
    PathBuf,
    Option<PathBuf>,
    Option<PathBuf>,
    pipeline::PipelineTuning,
)> {
    let (root, db_canonical, temp_canonical) = canonicalize_paths(root, db_path, temp_path)?;

    let (num_threads, drive_type, parallel_walk) =
        determine_threads_for_drive(&root, conn, WorkerThreadLimits::current().all_threads);

    let writer_pool_size = setup_writer_pool_size(drive_type.is_network());

    parallel_walk_handler(parallel_walk);

    let tuning = pipeline::PipelineTuning {
        num_threads,
        writer_pool_size,
        parallel_walk,
        is_network_drive: drive_type.is_network(),
    };
    Ok((root, db_canonical, temp_canonical, tuning))
}

/// Main orchestrator: Collect all entries under `root` via streaming pipeline.
/// Returns (entries, writer_pool_size, path_count). No progress bar here so kdam never blocks the pipeline; caller may create one for Phase 3 using path_count.
/// Walk → path channel → workers (metadata) → entry channel → Vec.
pub fn collect_entries(
    root: &Path,
    opts: &Opts,
    db_path: &Path,
    temp_path: Option<&Path>,
    conn: &Connection,
) -> Result<pipeline::CollectEntriesResult> {
    let pipeline::PipelineHandles {
        entry_rx,
        path_count_rx: _path_count_rx,
        walk_handle,
        worker_handles,
        writer_pool_size,
        is_network_drive: _,
        first_error,
        skipped_paths,
    } = run_pipeline(root, opts, db_path, temp_path, conn)?;

    let mut entries = Vec::new();
    while let Ok(entry) = entry_rx.recv() {
        entries.push(entry);
    }
    debug!(
        "main: channel closed, total {} entries (metadata phase done)",
        entries.len()
    );

    let path_count = walk_handle
        .join()
        .map_err(|_| anyhow::anyhow!("walk thread panicked"))?;
    for h in worker_handles {
        let _ = h.join();
    }

    pipeline::check_for_initial_error_or_skipped_paths(opts, &first_error, &skipped_paths)?;

    Ok((entries, writer_pool_size, path_count))
}
