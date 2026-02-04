//! Directory indexing operations

use anyhow::Result;
use crossbeam_channel::Receiver;
use kdam::{Animation, Bar};
use log::info;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use crate::Opts;
use crate::engine;
use crate::engine::StoredMeta;
use crate::engine::progress;
use crate::pipeline::{
    PipelineHandles, check_for_initial_error_or_skipped_paths, run_pipeline,
    shutdown_pipeline_handles,
};
use crate::utils::{get_passphrase, prepare_index_work_path, rename_temp_to_final};

/// Build progress bar and callbacks for streaming index. Returns (bar, on_batch, on_received).
/// For local drives: percentage bar + on_batch; path_count_rx is consumed in a background thread to set total.
/// For network: counter bar + on_received; no total.
fn setup_progress(
    verbose: bool,
    is_network_drive: bool,
    path_count_rx: Receiver<usize>,
) -> progress::ProgressSetup {
    let bar = verbose.then(|| {
        let b = if is_network_drive {
            progress::create_counter("Nefaxing")
        } else {
            progress::create_progress_bar(progress::ProgressBarConfig::new(
                1,
                "Nefaxing",
                Animation::Classic,
            ))
        };
        if is_network_drive {
            progress::refresh_bar(&b);
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
                progress::set_bar_total(&bar_clone, total);
            }
        });
    }

    let on_batch = progress::on_batch_callback(is_network_drive, &bar);
    let on_received = progress::on_received_callback(is_network_drive, &bar);
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
        progress::update_progress_bar(bar, path_count - written);
    }
    Ok(path_count)
}

/// Convert public Nefax (path → PathMeta) to internal StoredMeta map for diff.
fn nefax_to_stored(existing: &crate::Nefax) -> HashMap<PathBuf, StoredMeta> {
    existing
        .iter()
        .map(|(p, m)| (p.clone(), (m.mtime_ns, m.size, m.hash.map(|h| h.to_vec()))))
        .collect()
}

/// Lib path: run pipeline against in-memory conn, diff against existing (StoredMeta map). No DB file.
/// Pass a no-op (e.g. `|_| {}`) when not using the callback.
fn run_lib_pipeline_with_callback<F>(
    root: &Path,
    opts: &Opts,
    existing: &HashMap<PathBuf, StoredMeta>,
    on_entry: F,
) -> Result<(crate::Nefax, crate::Diff)>
where
    F: FnMut(&crate::Entry),
{
    let conn = engine::open_db_in_memory()?;
    let PipelineHandles {
        entry_rx,
        walk_handle,
        worker_handles,
        first_error,
        skipped_paths,
        ..
    } = run_pipeline(root, opts, None, None, &conn)?;
    let (diff, index_map) =
        crate::check::diff_from_stream_with_callback(entry_rx, existing, root, opts, on_entry);
    shutdown_pipeline_handles(walk_handle, worker_handles)?;
    check_for_initial_error_or_skipped_paths(opts, &first_error, &skipped_paths)?;
    Ok((index_map, diff))
}

/// Internal: run pipeline with callback; diff against `existing` (when `None`, use empty map). Returns `(nefax, diff)`. Lib-only (streaming).
pub(crate) fn nefax_dir_callback<F>(
    root: &Path,
    opts: &Opts,
    existing: Option<&crate::Nefax>,
    on_entry: F,
) -> Result<(crate::Nefax, crate::Diff)>
where
    F: FnMut(&crate::Entry),
{
    let existing_stored = match existing {
        Some(ex) => nefax_to_stored(ex),
        None => {
            let conn = engine::open_db_in_memory()?;
            engine::load_index(&conn)?
        }
    };
    run_lib_pipeline_with_callback(root, opts, &existing_stored, on_entry)
}

/// Internal: full opts (CLI or lib). Non-callback path: handles both CLI (write_to_db) and lib (no DB). Returns `(nefax, diff)`.
///
/// # Arguments
/// * `root` - Directory to index (walk root).
/// * `opts` - Full options (from CLI or converted from [`NefaxOpts`](crate::NefaxOpts)). When `write_to_db` is false, lib path; when true, CLI path (writes to DB).
/// * `existing` - Used only on lib path (`write_to_db` false). When `None`, use empty prior state (in-memory DB, **no** `.nefaxer` file read; diff will be all added). When `Some`, diff against that snapshot. On CLI path this is ignored and the previous index is loaded from the `.nefaxer` file on disk if it exists.
pub(crate) fn nefax_dir_with_opts(
    root: &Path,
    opts: &Opts,
    existing: Option<&crate::Nefax>,
) -> Result<(crate::Nefax, crate::Diff)> {
    if !opts.write_to_db {
        let existing_stored = match existing {
            Some(ex) => nefax_to_stored(ex),
            None => {
                let conn = engine::open_db_in_memory()?;
                engine::load_index(&conn)?
            }
        };
        // Pass a no-op (e.g. `|_| {}`) when not using the callback.
        return run_lib_pipeline_with_callback(root, opts, &existing_stored, |_| {});
    }

    // CLI path: write to DB (temp then rename).
    let db_path = engine::create_db_path(root, opts.db_path.as_deref());
    let (temp_path, use_temp) = prepare_index_work_path(db_path.as_path())?;
    let (active_path, do_rename) = if use_temp {
        (temp_path.as_path(), true)
    } else {
        (db_path.as_path(), false)
    };

    let (mut conn, _) = if opts.encrypt && !db_path.as_path().exists() {
        let pass = get_passphrase(root, true)?;
        let c = engine::open_db(active_path, Some(pass.as_str()))?;
        (c, Some(pass))
    } else {
        engine::open_db_or_detect_encrypted(active_path, root)?
    };

    let existing = engine::load_index(&conn)?;
    let cancel_requested = engine::setup_ctrlc_handler()?;

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
        Some(db_path.as_path()),
        if use_temp { Some(active_path) } else { None },
        &conn,
    )?;

    let (nefaxing_bar, on_batch, on_received) =
        setup_progress(opts.verbose, is_network_drive, path_count_rx);

    let mut index_diff = crate::Diff::default();
    let mut stream_params = engine::ApplyIndexDiffStreamingParams {
        existing: &existing,
        mtime_window_ns: opts.mtime_window_ns,
        on_batch_progress: on_batch,
        on_received_progress: on_received,
        root: Some(root),
        with_hash: opts.with_hash,
        cancel_check: Some(Arc::clone(&cancel_requested)),
        diff: (!existing.is_empty()).then_some(&mut index_diff),
        result_map: None, // CLI does not need the full index; lib uses write_to_db=false and diff_from_stream.
    };

    let written = engine::apply_index_diff_streaming(&mut conn, entry_rx, &mut stream_params)?;
    let _path_count = collect_pipeline_results(
        walk_handle,
        worker_handles,
        is_network_drive,
        written,
        &nefaxing_bar,
    )?;
    check_for_initial_error_or_skipped_paths(opts, &first_error, &skipped_paths)?;

    if do_rename {
        rename_temp_to_final(&temp_path, db_path.as_path())?;
    }

    engine::check_for_cancel(&cancel_requested)?;

    if !existing.is_empty() {
        engine::print_diff(&index_diff, false, opts.list_paths, root);
    } else {
        info!("New nefaxer index created.");
    }

    // CLI does not need the full index as return value; diff was built during streaming.
    Ok((std::collections::HashMap::new(), index_diff))
}
