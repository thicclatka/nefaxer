//! Directory change detection operations (streaming: same pipeline as index, memory-efficient diff).

use anyhow::Result;
use crossbeam_channel::Receiver;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::engine;
use crate::pipeline::{
    PipelineHandles, check_for_initial_error_or_skipped_paths, run_pipeline,
    shutdown_pipeline_handles,
};
use crate::utils::config::DB_INSERT_BATCH_SIZE;
use crate::{Diff, Entry, Opts, PathMeta};

/// CLI dry-run: compare directory to existing index, print diff, no index write. Returns nothing.
pub fn check_dir(root: &Path, opts: &Opts) -> Result<()> {
    let db_path = engine::create_db_path(root, opts.db_path.as_deref());

    let (conn, _) = engine::open_db_or_detect_encrypted(db_path.as_path(), root)?;
    let index = engine::load_index(&conn)?;

    let PipelineHandles {
        entry_rx,
        path_count_rx: _,
        walk_handle,
        worker_handles,
        first_error,
        skipped_paths,
        ..
    } = run_pipeline(root, opts, Some(db_path.as_path()), None, &conn)?;

    let diff = diff_from_stream_diff_only(entry_rx, &index, root, opts);

    shutdown_pipeline_handles(walk_handle, worker_handles)?;

    check_for_initial_error_or_skipped_paths(opts, &first_error, &skipped_paths)?;

    engine::print_diff(&diff, true, opts.list_paths, root);
    Ok(())
}

/// Consume stream and build only the Diff (no map). Used by CLI dry-run.
fn diff_from_stream_diff_only(
    entry_rx: Receiver<Entry>,
    index: &HashMap<PathBuf, engine::StoredMeta>,
    root: &Path,
    opts: &Opts,
) -> Diff {
    let mut index_keys_not_seen: HashSet<PathBuf> = index.keys().cloned().collect();
    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut chunk = Vec::with_capacity(DB_INSERT_BATCH_SIZE);

    loop {
        chunk.clear();
        while let Ok(entry) = entry_rx.try_recv() {
            chunk.push(entry);
            if chunk.len() >= DB_INSERT_BATCH_SIZE {
                break;
            }
        }
        if chunk.is_empty() {
            match entry_rx.recv() {
                Ok(entry) => chunk.push(entry),
                Err(_) => break,
            }
        }

        for mut entry in chunk.drain(..) {
            engine::fill_entry_hash_if_needed(&mut entry, index, root, opts);
            index_keys_not_seen.remove(&entry.path);
            collect_entry_into_diff(entry, index, &mut added, &mut modified, root, opts);
        }
    }

    let removed: Vec<PathBuf> = index_keys_not_seen.into_iter().collect();
    Diff {
        added,
        removed,
        modified,
    }
}

/// Consume entries from the pipeline and build Diff and current index incrementally.
/// Returns (Diff, current index as path → PathMeta). Same shape as the DB; available whether or not we write to DB.
pub fn diff_from_stream(
    entry_rx: Receiver<Entry>,
    index: &HashMap<PathBuf, engine::StoredMeta>,
    root: &Path,
    opts: &Opts,
) -> (Diff, HashMap<PathBuf, PathMeta>) {
    let mut index_keys_not_seen: HashSet<PathBuf> = index.keys().cloned().collect();
    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut current_index = HashMap::new();
    let mut chunk = Vec::with_capacity(DB_INSERT_BATCH_SIZE);

    loop {
        chunk.clear();
        while let Ok(entry) = entry_rx.try_recv() {
            chunk.push(entry);
            if chunk.len() >= DB_INSERT_BATCH_SIZE {
                break;
            }
        }
        if chunk.is_empty() {
            match entry_rx.recv() {
                Ok(entry) => chunk.push(entry),
                Err(_) => break,
            }
        }

        for mut entry in chunk.drain(..) {
            engine::fill_entry_hash_if_needed(&mut entry, index, root, opts);
            current_index.insert(
                entry.path.clone(),
                PathMeta {
                    mtime_ns: entry.mtime_ns,
                    size: entry.size,
                    hash: entry.hash,
                },
            );
            index_keys_not_seen.remove(&entry.path);
            collect_entry_into_diff(entry, index, &mut added, &mut modified, root, opts);
        }
    }

    let removed: Vec<PathBuf> = index_keys_not_seen.into_iter().collect();
    let diff = Diff {
        added,
        removed,
        modified,
    };
    (diff, current_index)
}

/// Classify entry as added or modified and push into the diff lists.
fn collect_entry_into_diff(
    entry: Entry,
    index: &HashMap<PathBuf, engine::StoredMeta>,
    added: &mut Vec<PathBuf>,
    modified: &mut Vec<PathBuf>,
    root: &Path,
    opts: &Opts,
) {
    match index.get(&entry.path) {
        None => added.push(entry.path),
        Some((old_mtime, old_size, old_hash)) => {
            let same = !engine::mtime_changed(entry.mtime_ns, *old_mtime, opts.mtime_window_ns)
                && entry.size == *old_size
                && engine::hash_equals(&entry.hash, old_hash);
            if same {
                return;
            }
            let still_modified = if opts.paranoid
                && entry.hash.is_some()
                && old_hash.as_ref().map(|v| v.len() == 32).unwrap_or(false)
                && engine::hash_equals(&entry.hash, old_hash)
            {
                let abs = root.join(&entry.path);
                match std::fs::metadata(&abs) {
                    Ok(meta) if meta.is_file() => engine::hash_file(&abs, meta.len())
                        .ok()
                        .flatten()
                        .map(|rehash: [u8; 32]| {
                            rehash.as_slice() != old_hash.as_deref().unwrap_or(&[0u8; 32])
                        })
                        .unwrap_or(true),
                    _ => true,
                }
            } else {
                true
            };
            if still_modified {
                modified.push(entry.path);
            }
        }
    }
}
