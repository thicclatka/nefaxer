//! Directory indexing operations

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::thread;

use kdam::Animation;

use crate::Opts;
use crate::engine;
use crate::engine::progress::{
    ProgressBarConfig, create_counter, create_progress_bar, refresh_bar, set_bar_total,
    update_progress_bar,
};
use crate::utils::config::PackagePaths;
use crate::utils::get_passphrase;

fn temp_path_for(db_path: &Path) -> PathBuf {
    let name = db_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_else(|| PackagePaths::get().output_filename());
    db_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{name}.tmp"))
}

/// Remove SQLite WAL and SHM files for a temp path after rename (they are left behind).
fn remove_temp_wal_and_shm(temp_path: &Path) {
    let file_name = temp_path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let parent = temp_path.parent().unwrap_or(Path::new("."));
    let _ = std::fs::remove_file(parent.join(format!("{file_name}-wal")));
    let _ = std::fs::remove_file(parent.join(format!("{file_name}-shm")));
}

/// Index directory at `root` into the database at `db_path`.
/// Writes to a temp file then renames on success (atomic update).
/// If the directory is read-only or copy fails with permission denied, works directly on `db_path` (no atomic rename).
pub fn index_dir(root: &Path, db_path: &Path, opts: &Opts) -> Result<()> {
    let temp_path = temp_path_for(db_path);
    let mut use_temp = true;

    if temp_path.exists() {
        remove_temp_wal_and_shm(&temp_path);
        if let Err(e) = std::fs::remove_file(&temp_path) {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                use_temp = false;
            } else {
                return Err(e).context("remove stale temp index");
            }
        }
    }
    if use_temp
        && db_path.exists()
        && let Err(e) = std::fs::copy(db_path, &temp_path)
    {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            use_temp = false;
        } else {
            return Err(e).context(format!(
                "copy existing index to temp ({} -> {})",
                db_path.display(),
                temp_path.display()
            ));
        }
    }

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
    let handles = engine::run_pipeline(
        root,
        opts,
        db_path,
        if use_temp { Some(work_path) } else { None },
        &conn,
    )?;
    // On network drives use a counter (no total); walk/metadata/write proceed at similar rate.
    let nefaxing_bar = if opts.verbose {
        let bar = if handles.is_network_drive {
            create_counter("Nefaxing")
        } else {
            create_progress_bar(ProgressBarConfig::new(1, "Nefaxing", Animation::Classic))
        };
        if handles.is_network_drive {
            refresh_bar(&bar);
        }
        Some(bar)
    } else {
        None
    };
    // Set progress bar total when walk finishes (local drives only; counter has no total).
    if let Some(ref bar) = nefaxing_bar
        && !handles.is_network_drive
    {
        let path_count_rx = handles.path_count_rx.clone();
        let bar_clone = std::sync::Arc::clone(bar);
        thread::spawn(move || {
            if let Ok(total) = path_count_rx.recv() {
                set_bar_total(&bar_clone, total);
            }
        });
    }
    let on_batch = if handles.is_network_drive {
        None
    } else {
        nefaxing_bar.as_ref().map(|bar| {
            let bar = std::sync::Arc::clone(bar);
            Box::new(move |n: usize| update_progress_bar(&bar, n)) as Box<dyn Fn(usize) + Send>
        })
    };
    let on_received = if handles.is_network_drive {
        nefaxing_bar.as_ref().map(|bar| {
            let bar = std::sync::Arc::clone(bar);
            Box::new(move |n: usize| update_progress_bar(&bar, n)) as Box<dyn Fn(usize) + Send>
        })
    } else {
        None
    };
    let written = engine::apply_index_diff_streaming(
        &mut conn,
        handles.entry_rx,
        engine::ApplyIndexDiffStreamingParams {
            existing: &existing,
            mtime_window_ns: opts.mtime_window_ns,
            on_batch_progress: on_batch,
            on_received_progress: on_received,
            root: Some(root),
            with_hash: opts.with_hash,
        },
    )?;
    let path_count = handles
        .walk_handle
        .join()
        .map_err(|_| anyhow::anyhow!("walk thread panicked"))?;
    for h in handles.worker_handles {
        let _ = h.join();
    }
    // When index was up to date, almost nothing was written so the bar stayed at 0%; push to 100% (local only).
    if let Some(ref bar) = nefaxing_bar
        && !handles.is_network_drive
        && path_count > written
    {
        update_progress_bar(bar, path_count - written);
    }
    if opts.strict
        && let Some(msg) = handles.first_error.lock().unwrap().take()
    {
        return Err(anyhow::anyhow!("{}", msg));
    }
    let skipped = handles.skipped_paths.lock().unwrap().len();
    if skipped > 0 && !opts.strict {
        log::warn!(
            "Skipped {} paths due to permission errors or access issues",
            skipped
        );
        if opts.verbose {
            for p in handles.skipped_paths.lock().unwrap().iter() {
                eprintln!("  skipped: {}", p.display());
            }
        }
    }

    if do_rename {
        std::fs::rename(&temp_path, db_path).context("atomic rename temp index to final path")?;
        remove_temp_wal_and_shm(&temp_path);
    }
    Ok(())
}
