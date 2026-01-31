//! Cross-platform disk type detection for performance tuning

use std::path::Path;

use rusqlite::Connection;

use crate::utils::config::WorkerThreadLimits;

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

/// Returns (num_threads, drive_type). Use drive_type for writer pool size (e.g. 1 if network).
pub fn determine_threads_for_drive(
    path: &Path,
    conn: &Connection,
    available_threads: usize,
) -> (usize, DriveType) {
    let limits = WorkerThreadLimits::default();
    let drive_type = drive_type_for_path(path);
    let num_threads = match drive_type {
        DriveType::SSD => available_threads,
        DriveType::HDD => available_threads.min(limits.hdd_max),
        DriveType::Network => {
            probe::detect_optimal_workers(path, drive_type, conn).unwrap_or(available_threads)
        }
        DriveType::Unknown => available_threads.min(limits.floor),
    };
    (num_threads, drive_type)
}
