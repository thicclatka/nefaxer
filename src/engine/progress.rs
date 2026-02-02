//! Progress bar utilities for displaying processing status

use kdam::{Animation, Bar, BarExt};
use std::sync::{Arc, Mutex};

/// Update the bar's total (e.g. during streaming when total grows). Refreshes the display.
pub fn set_bar_total(pb: &Arc<Mutex<Bar>>, total: usize) {
    if let Ok(mut bar) = pb.try_lock() {
        bar.total = total;
        let _ = bar.refresh();
    }
}

/// Force a refresh of the bar (e.g. so counter shows "0 files" immediately).
pub fn refresh_bar(pb: &Arc<Mutex<Bar>>) {
    if let Ok(mut bar) = pb.try_lock() {
        let _ = bar.refresh();
    }
}

/// Configuration for creating a progress bar
pub struct ProgressBarConfig {
    pub total: usize,
    pub desc: &'static str,
    pub animation: Animation,
}

impl ProgressBarConfig {
    /// Create a new progress bar configuration
    pub fn new(total: usize, desc: &'static str, animation: Animation) -> Self {
        Self {
            total,
            desc,
            animation,
        }
    }
}

/// Create a progress bar with the given configuration
pub fn create_progress_bar(config: ProgressBarConfig) -> Arc<Mutex<Bar>> {
    Arc::new(Mutex::new(kdam::tqdm!(
        total = config.total,
        desc = config.desc,
        animation = config.animation
    )))
}

/// Create a counter for unknown total (shows count without percentage)
pub fn create_counter(desc: &'static str) -> Arc<Mutex<Bar>> {
    Arc::new(Mutex::new(kdam::tqdm!(
        total = 0,
        desc = desc,
        animation = Animation::Classic,
        position = 0,
        unit = " files"
    )))
}

/// Update progress bar if available
/// Uses try_lock to avoid blocking if mutex is contended (non-blocking)
pub fn update_progress_bar(pb: &Arc<Mutex<Bar>>, n: usize) {
    // Use try_lock to avoid blocking parallel workers
    // If lock is contended, skip update (progress bar will catch up on next update)
    if let Ok(mut pb) = pb.try_lock() {
        let _ = pb.update(n);
    }
}

// Progress bar type alias
pub type ProgressBar = Arc<std::sync::Mutex<Bar>>;

/// (bar, on_batch, on_received) from setup_progress for streaming index.
pub type ProgressSetup = (
    Option<ProgressBar>,
    Option<Box<dyn Fn(usize) + Send>>,
    Option<Box<dyn Fn(usize) + Send>>,
);

/// Create a progress callback function that updates the progress bar.
pub fn progress_callback(bar: &Option<ProgressBar>) -> Option<Box<dyn Fn(usize) + Send>> {
    bar.as_ref().map(|bar| {
        let bar = Arc::clone(bar);
        Box::new(move |n: usize| update_progress_bar(&bar, n)) as Box<dyn Fn(usize) + Send>
    })
}

/// Create a callback function that updates the progress bar on batch completion.
pub fn on_batch_callback(
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
pub fn on_received_callback(
    is_network_drive: bool,
    bar: &Option<ProgressBar>,
) -> Option<Box<dyn Fn(usize) + Send>> {
    if is_network_drive {
        progress_callback(bar)
    } else {
        None
    }
}

/// Macro to execute a function and update progress bar
/// Usage: `with_progress!(pb, function_call(...))`
/// Optimized: only calls update_progress_bar if pb is Some
#[macro_export]
macro_rules! with_progress {
    ($pb:expr, $func:expr) => {{
        let result = $func;
        if $pb.is_some() {
            $crate::engine::progress::update_progress_bar($pb);
        }
        result
    }};
}
