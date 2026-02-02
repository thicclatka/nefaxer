use crate::Entry;
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};

use crate::engine::hashing::hash_file;
use crate::engine::tools::{path_relative_to, path_to_db_string};
use crate::utils::config::SMALL_FILE_THRESHOLD;

/// Single metadata worker: read paths from path_rx, turn into entries, send on entry_tx.
/// Hashing is done in the streaming receiver when with_hash is set, not here.
fn metadata_worker_loop(path_rx: Receiver<PathBuf>, entry_tx: Sender<Entry>, root: PathBuf) {
    while let Ok(abs_path) = path_rx.recv() {
        if let Ok(entry) = path_to_entry(&abs_path, &root, false) {
            let _ = entry_tx.send(entry);
        }
    }
    drop(entry_tx);
}

/// Spawn metadata workers: read paths from path_rx, turn into entries, send on entry_tx. Caller must drop its sender after this so workers exit.
pub fn spawn_metadata_workers(
    path_rx: Receiver<PathBuf>,
    entry_tx: &Sender<Entry>,
    root: &Path,
    num_threads: usize,
) -> Vec<JoinHandle<()>> {
    let root = root.to_path_buf();
    (0..num_threads)
        .map(|_| {
            let path_rx = path_rx.clone();
            let entry_tx = entry_tx.clone();
            let root = root.clone();
            thread::spawn(move || metadata_worker_loop(path_rx, entry_tx, root))
        })
        .collect()
}

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
    let path = PathBuf::from(path_to_db_string(&rel));
    let hash = if with_hash && is_file && size >= SMALL_FILE_THRESHOLD {
        hash_file(abs_path, size)?
    } else {
        None
    };
    Ok(Entry {
        path,
        mtime_ns,
        size,
        hash,
    })
}
