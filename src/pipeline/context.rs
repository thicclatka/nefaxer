//! Pipeline context and tuning: shared data passed into the walk thread and drive-derived settings.

use crate::Entry;
use crossbeam_channel::{Receiver, Sender, bounded};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::Opts;
use crate::utils::config::PackagePaths;

/// Tuning derived from drive type and FD limit: worker count, walk mode, channel cap.
/// Channel cap is drive-type default on first run; finetuned from stored path count in diskinfo on subsequent runs.
#[derive(Clone, Debug)]
pub struct PipelineTuning {
    pub num_threads: usize,
    pub parallel_walk: bool,
    pub is_network_drive: bool,
    /// Capacity for path and entry channels (drive-type default or from diskinfo path count).
    pub channel_cap: usize,
}

/// Shared context for the walk + metadata pipeline. Built in `run_pipeline` and passed
/// into the walk thread so the common walk loop has root, exclude, strict, and error/skip state.
pub struct PipelineContext {
    pub root: PathBuf,
    pub db_canonical: Option<PathBuf>,
    pub temp_canonical: Option<PathBuf>,
    pub exclude: Vec<String>,
    pub strict: bool,
    pub follow_links: bool,
    pub first_error: Arc<Mutex<Option<String>>>,
    pub skipped_paths: Arc<Mutex<Vec<(PathBuf, String)>>>,
}

/// Result of [`collect_entries`]: (entries, path_count).
pub type CollectEntriesResult = (Vec<Entry>, usize);

/// Handles returned by [`run_pipeline`] for streaming: receive entries and join when done.
/// `path_count_rx`: receives the walk's path count when the walk finishes (use to set progress bar total).
/// `is_network_drive`: true when indexing a network path (use counter-style progress, no total).
pub struct PipelineHandles {
    pub entry_rx: Receiver<Entry>,
    pub path_count_rx: Receiver<usize>,
    pub walk_handle: JoinHandle<usize>,
    pub worker_handles: Vec<JoinHandle<()>>,
    pub is_network_drive: bool,
    pub first_error: Arc<Mutex<Option<String>>>,
    pub skipped_paths: Arc<Mutex<Vec<(PathBuf, String)>>>,
}

/// Channels and shared state for the pipeline. Walk thread gets path_tx, path_count_tx, ctx; workers get path_rx, entry_tx.
pub struct PipelineChannels {
    pub path_tx: Sender<PathBuf>,
    pub path_rx: Receiver<PathBuf>,
    pub entry_tx: Sender<Entry>,
    pub entry_rx: Receiver<Entry>,
    pub path_count_tx: Sender<usize>,
    pub path_count_rx: Receiver<usize>,
    pub first_error: Arc<Mutex<Option<String>>>,
    pub skipped_paths: Arc<Mutex<Vec<(PathBuf, String)>>>,
    pub ctx: PipelineContext,
}

pub fn create_pipeline_channels(
    root: &Path,
    db_canonical: &Option<PathBuf>,
    temp_canonical: &Option<PathBuf>,
    opts: &Opts,
    channel_cap: usize,
) -> PipelineChannels {
    let (path_tx, path_rx) = bounded::<PathBuf>(channel_cap);
    let (entry_tx, entry_rx) = bounded::<Entry>(channel_cap);
    let (path_count_tx, path_count_rx) = bounded::<usize>(1);
    let first_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let skipped_paths: Arc<Mutex<Vec<(PathBuf, String)>>> = Arc::new(Mutex::new(Vec::new()));

    let mut exclude = PackagePaths::get().default_exclude_patterns();
    exclude.extend(opts.exclude.iter().cloned());

    let ctx = PipelineContext {
        root: root.to_path_buf(),
        db_canonical: db_canonical.clone(),
        temp_canonical: temp_canonical.clone(),
        exclude,
        strict: opts.strict,
        follow_links: opts.follow_links,
        first_error: Arc::clone(&first_error),
        skipped_paths: Arc::clone(&skipped_paths),
    };

    PipelineChannels {
        path_tx,
        path_rx,
        entry_tx,
        entry_rx,
        path_count_tx,
        path_count_rx,
        first_error,
        skipped_paths,
        ctx,
    }
}
