//! Directory indexing operations

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use crossbeam_channel::Receiver;
use kdam::{Animation, Bar};

use crate::Opts;
use crate::engine;
use crate::engine::progress::{
    ProgressBarConfig, create_counter, create_progress_bar, refresh_bar, set_bar_total,
    update_progress_bar,
};
use crate::pipeline::{PipelineHandles, check_for_initial_error_or_skipped_paths, run_pipeline};
use crate::utils::{get_passphrase, prepare_index_work_path, remove_temp_wal_and_shm};

// Progress bar type alias
type ProgressBar = Arc<std::sync::Mutex<Bar>>;

/// (bar, on_batch, on_received) from setup_progress for streaming index.
type ProgressSetup = (
    Option<ProgressBar>,
    Option<Box<dyn Fn(usize) + Send>>,
    Option<Box<dyn Fn(usize) + Send>>,
);

/// Create a progress callback function that updates the progress bar.
fn progress_callback(bar: &Option<ProgressBar>) -> Option<Box<dyn Fn(usize) + Send>> {
    bar.as_ref().map(|bar| {
        let bar = Arc::clone(bar);
        Box::new(move |n: usize| update_progress_bar(&bar, n)) as Box<dyn Fn(usize) + Send>
    })
}

/// Create a callback function that updates the progress bar on batch completion.
fn on_batch_callback(
    is_network_drive: bool,
    bar: &Option<ProgressBar>,
) -> Option<Box<dyn Fn(usize) + Send>> {
    if is_network_drive {
        None
    } else {
        progress_callback(bar)
    }
}

/// Create a callback function that updates the progress bar on received completion.
fn on_received_callback(
    is_network_drive: bool,
    bar: &Option<ProgressBar>,
) -> Option<Box<dyn Fn(usize) + Send>> {
    if is_network_drive {
        progress_callback(bar)
    } else {
        None
    }
}

/// Build progress bar and callbacks for streaming index. Returns (bar, on_batch, on_received).
/// For local drives: percentage bar + on_batch; path_count_rx is consumed in a background thread to set total.
/// For network: counter bar + on_received; no total.
fn setup_progress(
    verbose: bool,
    is_network_drive: bool,
    path_count_rx: Receiver<usize>,
) -> ProgressSetup {
    let bar = verbose.then(|| {
        let b = if is_network_drive {
            create_counter("Nefaxing")
        } else {
            create_progress_bar(ProgressBarConfig::new(1, "Nefaxing", Animation::Classic))
        };
        if is_network_drive {
            refresh_bar(&b);
        }
        b
    });

    // Receive path_count in a separate thread: the main thread will block on entry_rx in
    // apply_index_diff_streaming, so it can't also block on path_count_rx. The walk sends total
    // when it finishes; this thread sets the bar total when that arrives (local percentage bar).
    if let (Some(bar), false) = (bar.as_ref(), is_network_drive) {
        let bar_clone = Arc::clone(bar);
        thread::spawn(move || {
            if let Ok(total) = path_count_rx.recv() {
                set_bar_total(&bar_clone, total);
            }
        });
    }

    let on_batch = on_batch_callback(is_network_drive, &bar);
    let on_received = on_received_callback(is_network_drive, &bar);
    (bar, on_batch, on_received)
}

/// Join walk and workers, then push progress bar to 100% when index was up to date (local only). Returns path_count.
fn collect_pipeline_results(
    walk_handle: std::thread::JoinHandle<usize>,
    worker_handles: Vec<std::thread::JoinHandle<()>>,
    is_network_drive: bool,
    written: usize,
    nefaxing_bar: &Option<Arc<std::sync::Mutex<Bar>>>,
) -> Result<usize> {
    let path_count = walk_handle
        .join()
        .map_err(|_| anyhow::anyhow!("walk thread panicked"))?;
    for h in worker_handles {
        let _: std::thread::Result<()> = h.join();
    }
    if let Some(bar) = nefaxing_bar
        && !is_network_drive
        && path_count > written
    {
        update_progress_bar(bar, path_count - written);
    }
    Ok(path_count)
}

/// Index directory at `root` into the database at `db_path`.
/// Writes to a temp file then renames on success (atomic update).
/// If the directory is read-only or copy fails with permission denied, works directly on `db_path` (no atomic rename).
pub fn index_dir(root: &Path, db_path: &Path, opts: &Opts) -> Result<()> {
    let (temp_path, use_temp) = prepare_index_work_path(db_path)?;
    let (work_path, do_rename) = if use_temp {
        (temp_path.as_path(), true)
    } else {
        (db_path, false)
    };
    let (mut conn, _passphrase_used) = if opts.encrypt && !db_path.exists() {
        let pass = get_passphrase(root, true)?;
        let c = engine::open_db(work_path, Some(pass.as_str()))?;
        (c, Some(pass))
    } else {
        engine::open_db_or_detect_encrypted(work_path, root)?
    };
    // Streaming: walk + metadata + write to DB (and optional hashing in receiver) all at once.
    let existing = engine::load_index(&conn)?;
    let cancel_requested = Arc::new(AtomicBool::new(false));
    let cancel_requested_handler = Arc::clone(&cancel_requested);
    ctrlc::set_handler(move || {
        cancel_requested_handler.store(true, Ordering::Relaxed);
    })
    .context("set Ctrl+C handler")?;

    let PipelineHandles {
        entry_rx,
        path_count_rx,
        walk_handle,
        worker_handles,
        is_network_drive,
        first_error,
        skipped_paths,
        ..
    } = run_pipeline(
        root,
        opts,
        db_path,
        if use_temp { Some(work_path) } else { None },
        &conn,
    )?;
    let (nefaxing_bar, on_batch, on_received) =
        setup_progress(opts.verbose, is_network_drive, path_count_rx);

    let written = engine::apply_index_diff_streaming(
        &mut conn,
        entry_rx,
        engine::ApplyIndexDiffStreamingParams {
            existing: &existing,
            mtime_window_ns: opts.mtime_window_ns,
            on_batch_progress: on_batch,
            on_received_progress: on_received,
            root: Some(root),
            with_hash: opts.with_hash,
            cancel_check: Some(Arc::clone(&cancel_requested)),
        },
    )?;
    let _path_count = collect_pipeline_results(
        walk_handle,
        worker_handles,
        is_network_drive,
        written,
        &nefaxing_bar,
    )?;
    check_for_initial_error_or_skipped_paths(opts, &first_error, &skipped_paths)?;

    if do_rename {
        std::fs::rename(&temp_path, db_path).context("atomic rename temp index to final path")?;
        remove_temp_wal_and_shm(&temp_path);
    }
    if cancel_requested.load(Ordering::Relaxed) {
        return Err(anyhow::anyhow!(
            "Indexing cancelled by user; partial index was flushed"
        ));
    }
    Ok(())
}
