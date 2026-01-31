//! Core collection and processing logic

use anyhow::{Context, Result};
use jwalk::WalkDir;
use kdam::{Animation, BarExt};
use log::debug;
use rayon::prelude::*;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use crate::disk_detect::determine_threads_for_drive;
use crate::engine::parallel::{self, adaptive_progress_chunk_size};
use crate::engine::progress;
use crate::utils::config::{SMALL_FILE_THRESHOLD, WRITER_POOL_SIZE};
use crate::utils::fd_limit::max_workers_by_fd_limit;
use crate::{Entry, Opts};

use super::hashing::hash_file;
use super::tools::{check_root_and_canonicalize, path_relative_to, should_include_in_walk};

/// Phase 1: Walk directories in parallel and filter paths
fn walk_and_filter(
    root: &Path,
    db_canonical: &Option<PathBuf>,
    opts: &Opts,
    num_threads: usize,
) -> Result<Vec<PathBuf>> {
    let start = Instant::now();
    debug!("Phase 1: Walking directories and filtering...");

    // Create collection counter if verbose
    let collection_counter = if opts.verbose {
        Some(progress::create_counter("Collecting files"))
    } else {
        None
    };

    let collection_count = AtomicUsize::new(0);

    let error_count = AtomicUsize::new(0);
    let first_error: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    let skipped_paths: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

    let entries: Vec<_> = WalkDir::new(root)
        .follow_links(opts.follow_links)
        .skip_hidden(false)
        .parallelism(jwalk::Parallelism::RayonNewPool(num_threads))
        .into_iter()
        .filter_map(|entry_result| match entry_result {
            Ok(entry) => {
                let path = entry.path();
                if should_include_in_walk(&path, root, db_canonical, &opts.exclude) {
                    progress::report_progress_batched(
                        collection_counter.as_ref(),
                        &collection_count,
                        parallel::ProgressConsts::PROGRESS_UPDATE_BATCH_SIZE,
                    );
                    Some(path)
                } else {
                    None
                }
            }
            Err(err) => {
                error_count.fetch_add(1, Ordering::Relaxed);
                if opts.strict {
                    let _ = first_error.lock().unwrap().get_or_insert_with(|| {
                        format!("strict mode: {} (path: {:?})", err, err.path())
                    });
                } else {
                    log::warn!("Permission denied or error accessing path: {}", err);
                }
                if let Some(p) = err.path() {
                    skipped_paths.lock().unwrap().push(p.to_path_buf());
                }
                None
            }
        })
        .collect();

    progress::flush_progress_remainder(
        collection_counter.as_ref(),
        collection_count.load(Ordering::Relaxed),
        parallel::ProgressConsts::PROGRESS_UPDATE_BATCH_SIZE,
    );

    // Clear collection counter if it exists
    if let Some(counter) = collection_counter
        && let Ok(mut bar) = counter.try_lock()
    {
        let _ = bar.clear();
    }

    if opts.strict
        && let Some(msg) = first_error.lock().unwrap().take()
    {
        return Err(anyhow::anyhow!("{}", msg));
    }

    let error_count_final = error_count.load(Ordering::Relaxed);
    if error_count_final > 0 && !opts.strict {
        log::warn!(
            "Skipped {} paths due to permission errors or access issues",
            error_count_final
        );
        if opts.verbose {
            let paths = skipped_paths.lock().unwrap();
            for p in paths.iter() {
                eprintln!("  skipped: {}", p.display());
            }
        }
    }

    debug!(
        "Found {} paths in {:.2}s",
        entries.len(),
        start.elapsed().as_secs_f64()
    );
    Ok(entries)
}

/// Phase 2: Read metadata and hash files in parallel
fn read_metadata(
    paths: Vec<PathBuf>,
    root: &Path,
    opts: &Opts,
    num_threads: usize,
) -> Result<Vec<Entry>> {
    let start = Instant::now();
    let entry_count = paths.len();
    debug!("Reading metadata for {} entries...", entry_count);

    let with_hash = opts.with_hash;

    // Create progress bar if verbose
    let pb = if opts.verbose {
        let config = progress::ProgressBarConfig::new(entry_count, "Indexing", Animation::Classic);
        Some(progress::create_progress_bar(config))
    } else {
        None
    };

    let chunk_size = adaptive_progress_chunk_size(
        entry_count,
        num_threads,
        parallel::ProgressConsts::ADAPTIVE_PROGRESS_TARGET_UPDATES,
    );
    let counter = AtomicUsize::new(0);

    // Build custom thread pool with adjusted thread count
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build()
        .context("failed to build thread pool")?;

    let collected: Vec<Result<Entry>> = pool.install(|| {
        paths
            .into_par_iter()
            .map(|abs_path| {
                let meta = std::fs::metadata(&abs_path)?;
                let mtime_ns = meta
                    .modified()
                    .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as i64)
                    .unwrap_or(0);
                let size = meta.len();
                let is_file = meta.is_file();

                // Convert to relative path
                let rel = path_relative_to(&abs_path, root).unwrap_or_else(|| abs_path.clone());

                // Skip hashing for small files (use mtime/size only; threshold in config)
                let hash = if with_hash && is_file && size >= SMALL_FILE_THRESHOLD {
                    hash_file(&abs_path, size)?
                } else {
                    None
                };

                progress::report_progress_batched(pb.as_ref(), &counter, chunk_size);

                Ok(Entry {
                    path: rel,
                    mtime_ns,
                    size,
                    hash,
                })
            })
            .collect()
    });

    progress::flush_progress_remainder(pb.as_ref(), entry_count, chunk_size);

    debug!(
        "Processed {} entries in {:.2}s",
        entry_count,
        start.elapsed().as_secs_f64()
    );
    if opts.strict {
        collected
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .context("strict mode: metadata or hash error")
    } else {
        Ok(collected.into_iter().filter_map(Result::ok).collect())
    }
}

/// Main orchestrator: Collect all entries under `root`.
/// Returns (entries, writer_pool_size). Writer pool size is derived from drive type (1 if network, else WRITER_POOL_SIZE).
///
/// Phase 1: Walk directories in parallel and filter
/// Phase 2: Read metadata and hash files in parallel
pub fn collect_entries(
    root: &Path,
    opts: &Opts,
    db_path: &Path,
    conn: &Connection,
) -> Result<(Vec<Entry>, usize)> {
    // Canonicalize paths (only once)
    // Also check if root is owned by root user (UID 0) - security risk
    let root = check_root_and_canonicalize(root)?;
    let db_canonical = db_path.canonicalize().ok();

    // Detect drive type once; use for both worker threads and writer pool size
    let (num_threads, drive_type) =
        determine_threads_for_drive(&root, conn, rayon::current_num_threads());

    // Cap threads by FD limit (avoid EMFILE during parallel walk)
    let num_threads = match max_workers_by_fd_limit() {
        Some(fd_cap) if fd_cap < num_threads => {
            debug!(
                "Capping threads {} -> {} (FD limit ~80%)",
                num_threads, fd_cap
            );
            fd_cap
        }
        _ => num_threads,
    };

    let writer_pool_size = if drive_type.is_network() {
        1
    } else {
        WRITER_POOL_SIZE
    };
    debug!(
        "Drive type: {:?} | Active threads: {} | Writer pool size: {}",
        drive_type, num_threads, writer_pool_size
    );

    // Phase 1: Walk and filter
    let paths = walk_and_filter(&root, &db_canonical, opts, num_threads)?;

    // Phase 2: Read metadata
    let entries = read_metadata(paths, &root, opts, num_threads)?;

    Ok((entries, writer_pool_size))
}
