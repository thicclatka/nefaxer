//! Core collection and processing logic

use anyhow::Result;
use crossbeam_channel::{Receiver, bounded};
use log::debug;
use rayon::prelude::*;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use walkdir::WalkDir;

use crate::disk_detect::determine_threads_for_drive;
use crate::utils::config::{SMALL_FILE_THRESHOLD, WRITER_POOL_SIZE};
use crate::utils::fd_limit::max_workers_by_fd_limit;
use crate::{Entry, Opts};

use super::hashing::hash_file;
use super::tools::{check_root_and_canonicalize, path_relative_to, should_include_in_walk};

/// Path and entry channel capacity. Must be >= max path count so the walk never blocks on send
/// and can drop path_tx promptly (lets workers see channel close and exit).
const STREAMING_CHANNEL_CAP: usize = 50_000;

/// Process a single path into an Entry (metadata + optional hash).
fn path_to_entry(abs_path: &Path, root: &Path, with_hash: bool) -> Result<Entry> {
    let meta = std::fs::metadata(abs_path)?;
    let mtime_ns = meta
        .modified()
        .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as i64)
        .unwrap_or(0);
    let size = meta.len();
    let is_file = meta.is_file();
    let rel = path_relative_to(abs_path, root).unwrap_or_else(|| abs_path.to_path_buf());
    let hash = if with_hash && is_file && size >= SMALL_FILE_THRESHOLD {
        hash_file(abs_path, size)?
    } else {
        None
    };
    Ok(Entry {
        path: rel,
        mtime_ns,
        size,
        hash,
    })
}

/// Result of [`collect_entries`]: (entries, writer_pool_size, path_count).
type CollectEntriesResult = (Vec<Entry>, usize, usize);

/// Handles returned by [`run_pipeline`] for streaming: receive entries and join when done.
/// `path_count_rx`: receives the walk's path count when the walk finishes (use to set progress bar total).
/// `is_network_drive`: true when indexing a network path (use counter-style progress, no total).
pub struct PipelineHandles {
    pub entry_rx: Receiver<Entry>,
    pub path_count_rx: Receiver<usize>,
    pub walk_handle: JoinHandle<usize>,
    pub worker_handles: Vec<JoinHandle<()>>,
    pub writer_pool_size: usize,
    pub is_network_drive: bool,
    pub first_error: Arc<Mutex<Option<String>>>,
    pub skipped_paths: Arc<Mutex<Vec<PathBuf>>>,
}

/// Start the walk + metadata pipeline. Returns receiver and handles; caller receives from
/// `entry_rx` and must join `walk_handle` and `worker_handles` when done.
pub fn run_pipeline(
    root: &Path,
    opts: &Opts,
    db_path: &Path,
    temp_path: Option<&Path>,
    _conn: &Connection,
) -> Result<PipelineHandles> {
    let root = check_root_and_canonicalize(root)?;
    let db_canonical = db_path.canonicalize().ok();
    let temp_canonical = temp_path.and_then(|p| p.canonicalize().ok());

    let (num_threads, drive_type, parallel_walk) =
        determine_threads_for_drive(&root, _conn, rayon::current_num_threads());
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
    debug!("Writer pool size: {}", writer_pool_size);
    if parallel_walk {
        debug!("Walking in parallel");
    } else {
        debug!("Walking serially");
    }

    let (path_tx, path_rx) = bounded::<PathBuf>(STREAMING_CHANNEL_CAP);
    let (entry_tx, entry_rx) = bounded::<Entry>(STREAMING_CHANNEL_CAP);
    let (path_count_tx, path_count_rx) = bounded::<usize>(1);
    let first_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let skipped_paths: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));

    let root_w = root.clone();
    let db_canonical_w = db_canonical.clone();
    let temp_canonical_w = temp_canonical.clone();
    let opts_w = opts.clone();
    let first_error_w = Arc::clone(&first_error);
    let skipped_paths_w = Arc::clone(&skipped_paths);

    let walk_handle = thread::spawn(move || {
        let mut count = 0_usize;
        if parallel_walk {
            use jwalk::Parallelism;
            use std::time::Duration;
            for entry_result in jwalk::WalkDir::new(&root_w)
                .follow_links(opts_w.follow_links)
                .parallelism(Parallelism::RayonDefaultPool {
                    busy_timeout: Duration::from_secs(60),
                })
                .into_iter()
            {
                match entry_result {
                    Ok(entry) => {
                        let path = entry.path().to_path_buf();
                        if should_include_in_walk(
                            &path,
                            &root_w,
                            &db_canonical_w,
                            &temp_canonical_w,
                            &opts_w.exclude,
                        ) {
                            if path_tx.send(path).is_err() {
                                break;
                            }
                            count += 1;
                        }
                    }
                    Err(err) => {
                        if opts_w.strict {
                            let _ = first_error_w.lock().unwrap().get_or_insert_with(|| {
                                format!("strict mode: {} (path: {:?})", err, err.path())
                            });
                            break;
                        }
                        log::warn!("Permission denied or error accessing path: {}", err);
                        if let Some(p) = err.path() {
                            skipped_paths_w.lock().unwrap().push(p.to_path_buf());
                        }
                    }
                }
            }
        } else {
            for entry_result in WalkDir::new(&root_w)
                .follow_links(opts_w.follow_links)
                .into_iter()
            {
                match entry_result {
                    Ok(entry) => {
                        let path = entry.into_path();
                        if should_include_in_walk(
                            &path,
                            &root_w,
                            &db_canonical_w,
                            &temp_canonical_w,
                            &opts_w.exclude,
                        ) {
                            if path_tx.send(path).is_err() {
                                break;
                            }
                            count += 1;
                        }
                    }
                    Err(err) => {
                        if opts_w.strict {
                            let _ = first_error_w.lock().unwrap().get_or_insert_with(|| {
                                format!("strict mode: {} (path: {:?})", err, err.path())
                            });
                            break;
                        }
                        log::warn!("Permission denied or error accessing path: {}", err);
                        if let Some(p) = err.path() {
                            skipped_paths_w.lock().unwrap().push(p.to_path_buf());
                        }
                    }
                }
            }
        }
        let _ = path_count_tx.send(count);
        drop(path_tx);
        count
    });

    let root_c = root.clone();
    let worker_handles: Vec<_> = (0..num_threads)
        .map(|_worker_id| {
            let path_rx = path_rx.clone();
            let entry_tx = entry_tx.clone();
            let root = root_c.clone();
            thread::spawn(move || {
                while let Ok(abs_path) = path_rx.recv() {
                    if let Ok(entry) = path_to_entry(&abs_path, &root, false) {
                        let _ = entry_tx.send(entry);
                    }
                }
                drop(entry_tx);
            })
        })
        .collect();

    drop(entry_tx);

    Ok(PipelineHandles {
        entry_rx,
        path_count_rx,
        walk_handle,
        worker_handles,
        writer_pool_size,
        is_network_drive: drive_type.is_network(),
        first_error,
        skipped_paths,
    })
}

/// Fill hashes for entries that need them (size >= SMALL_FILE_THRESHOLD). Call after collect_entries when opts.with_hash.
pub fn fill_hashes(entries: &mut [Entry], root: &Path) {
    entries.par_iter_mut().for_each(|entry| {
        if entry.size >= SMALL_FILE_THRESHOLD {
            let abs = root.join(&entry.path);
            if let Ok(Some(h)) = hash_file(&abs, entry.size) {
                entry.hash = Some(h);
            }
        }
    });
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
) -> Result<CollectEntriesResult> {
    let PipelineHandles {
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

    Ok((entries, writer_pool_size, path_count))
}
