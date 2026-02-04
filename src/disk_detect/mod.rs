//! Cross-platform disk type detection for pipeline tuning.
//!
//! Used by the CLI/pipeline (with a DB connection for caching on network drives) and by library
//! callers (with no DB). See [`determine_threads_for_drive`] for the main API.

use log::debug;
use std::path::Path;

use rusqlite::Connection;

use crate::utils::config::WorkerThreadLimits;
use crate::utils::fd_limit::determine_threads_given_fd_limit;

// Platform-specific modules
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

// Probe module for network performance testing
pub mod network;
pub mod probe;

/// Drive type for performance tuning
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DriveType {
    SSD,
    HDD,
    Network,
    Unknown,
}

impl DriveType {
    /// Get optimal worker thread count for this drive type
    pub fn worker_threads(&self, available_threads: usize) -> usize {
        let limits = WorkerThreadLimits::default();
        match self {
            DriveType::SSD => available_threads,
            DriveType::HDD => available_threads.min(limits.hdd_max),
            DriveType::Network => limits.floor,
            DriveType::Unknown => available_threads.min(limits.unknown_max),
        }
    }

    pub fn is_hdd(&self) -> bool {
        matches!(self, DriveType::HDD)
    }

    #[allow(dead_code)]
    pub fn is_ssd(&self) -> bool {
        matches!(self, DriveType::SSD)
    }

    pub fn is_network(&self) -> bool {
        matches!(self, DriveType::Network)
    }

    /// Parse cached disk-type string (e.g. "Network+HDD", "Network+SSD") for probe results.
    pub fn from_disk_type_str(s: &str) -> Self {
        if s.contains("HDD") {
            DriveType::HDD
        } else if s.contains("SSD") {
            DriveType::SSD
        } else {
            DriveType::Unknown
        }
    }
}

/// Detect drive type for the given path (public for callers that need drive type only).
pub fn drive_type_for_path(path: &Path) -> DriveType {
    detect_drive_type(path)
}

fn detect_drive_type(path: &Path) -> DriveType {
    #[cfg(target_os = "macos")]
    {
        macos::detect(path)
    }

    #[cfg(target_os = "linux")]
    {
        linux::detect(path)
    }

    #[cfg(target_os = "windows")]
    {
        windows::detect(path)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        debug!("Unsupported platform for drive detection");
        DriveType::Unknown
    }
}

/// Channel cap for path/entry channels by drive type (used when no stored path count in diskinfo).
pub fn channel_cap_for_drive(drive_type: DriveType) -> usize {
    use crate::utils::config::StreamingChannelCap;
    match drive_type {
        DriveType::SSD => StreamingChannelCap::DEFAULT_SSD,
        DriveType::HDD => StreamingChannelCap::DEFAULT_HDD,
        DriveType::Network => StreamingChannelCap::DEFAULT_NETWORK,
        DriveType::Unknown => StreamingChannelCap::DEFAULT_UNKNOWN,
    }
}

/// Returns `(num_threads, drive_type, use_parallel_walk)` for pipeline tuning.
///
/// - **CLI / pipeline:** pass `Some(conn)` so network probe results can be cached in the DB.
/// - **Library / no DB:** pass `conn: None`; network probe still runs but is not cached. Use
///   [`crate::tuning_for_path`] for a convenience wrapper that fills [`crate::NefaxOpts`].
///
/// Return value: worker count (FD limit applied), drive type (SSD/HDD/Network/Unknown), and
/// `use_parallel_walk` (`true` for jwalk, `false` for walkdir). `thread_override` forces the
/// thread count (still capped by FD limit).
pub fn determine_threads_for_drive(
    path: &Path,
    conn: Option<&Connection>,
    available_threads: usize,
    thread_override: Option<usize>,
) -> (usize, DriveType, bool) {
    let limits = WorkerThreadLimits::default();
    let drive_type = drive_type_for_path(path);
    let (num_threads, use_parallel_walk) = match drive_type {
        DriveType::SSD => (available_threads, true),
        DriveType::HDD => (available_threads.min(limits.hdd_max), false),
        DriveType::Network => probe::detect_optimal_workers(path, drive_type, conn)
            .unwrap_or((available_threads, false)),
        DriveType::Unknown => (available_threads.min(limits.floor), false),
    };

    let num_threads_to_use =
        determine_threads_given_fd_limit(thread_override.unwrap_or(num_threads));

    if drive_type != DriveType::Network {
        debug!(
            "Drive type: {:?}, using {} threads",
            drive_type, num_threads_to_use
        );
    }

    (num_threads_to_use, drive_type, use_parallel_walk)
}
