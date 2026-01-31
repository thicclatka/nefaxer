//! Performance probing for network mounts and remote disk type detection

use anyhow::{Context, Result};
use log::{debug, info};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write as IoWrite;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::utils::config::{PackagePaths, WorkerThreadLimits};

use super::DriveType;

/// Probe tuning constants (used only in this module).
struct ProbeConsts;

impl ProbeConsts {
    pub const NUM_FILES: usize = 50;
    pub const FILE_SIZE: usize = 1024; // 1KB per file
    pub const IOPS_HDD_THRESHOLD: f64 = 150.0; // below = HDD, else SSD
    pub const NUM_LATENCY_SAMPLES: usize = 20;
    pub const LATENCY_HIGH_MS: f64 = 10.0;
    pub const LATENCY_MED_MS: f64 = 5.0;
}

/// Cached disk performance information
#[derive(Debug, Serialize, Deserialize)]
pub struct DiskInfo {
    /// Detected disk type (HDD/SSD) - cached permanently
    pub disk_type: DiskTypeInfo,
    /// Network performance metrics - measured every run
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkInfo>,
    /// Recommended worker thread count
    pub recommended_workers: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskTypeInfo {
    /// Type: HDD or SSD
    pub drive_type: String,
    /// Random I/O operations per second
    pub random_iops: f64,
    /// When this was tested
    pub tested_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkInfo {
    /// Average latency in milliseconds
    pub latency_ms: f64,
    /// When this was measured
    pub measured_at: u64,
}

/// Detect optimal worker count with caching (stored in the nefaxer DB diskinfo table).
/// For network drives also returns use_parallel_walk (true when cached disk type contains "SSD").
pub fn detect_optimal_workers(
    path: &Path,
    base_drive_type: DriveType,
    conn: &Connection,
) -> Result<(usize, bool)> {
    // Only probe if it's a network mount
    if !base_drive_type.is_network() {
        let workers = base_drive_type.worker_threads(rayon::current_num_threads());
        return Ok((workers, false)); // use_parallel_walk ignored for non-network
    }

    let root_key = path.to_string_lossy();

    // Try to load cached disk type from DB
    let disk_info = match load_cache_from_db(conn, &root_key) {
        Ok(Some(info)) => {
            debug!(
                "Loaded cached disk type: {} (tested: {})",
                info.disk_type.drive_type, info.disk_type.tested_at
            );
            Some(info)
        }
        Ok(None) => None,
        Err(e) => {
            debug!("Failed to load cache: {}, will re-probe", e);
            None
        }
    };

    // Get or probe disk type
    let disk_type_info = if let Some(ref info) = disk_info {
        info.disk_type.clone()
    } else {
        info!("Probing remote disk type (first run)...");
        probe_disk_type(path)?
    };

    // Always do quick network latency check
    debug!("Measuring current network latency...");
    let network_info = measure_network_latency(path)?;

    // Calculate optimal workers
    let workers = calculate_workers(&disk_type_info, &network_info);
    let use_parallel_walk = disk_type_info.drive_type.contains("SSD");

    // Save cache to DB (update network info)
    let cache_data = DiskInfo {
        disk_type: disk_type_info,
        network: Some(network_info),
        recommended_workers: workers,
    };
    save_cache_to_db(conn, &root_key, &cache_data)?;

    debug!(
        "Drive: {}, Network latency: {:.1}ms, Workers: {}",
        cache_data.disk_type.drive_type,
        cache_data.network.as_ref().unwrap().latency_ms,
        workers
    );

    Ok((workers, use_parallel_walk))
}

/// Probe remote disk type using random I/O test
fn probe_disk_type(base_path: &Path) -> Result<DiskTypeInfo> {
    let probe_dir = base_path.join(PackagePaths::get().probe_dir_name());
    fs::create_dir_all(&probe_dir).context("create probe directory")?;

    let data = vec![0u8; ProbeConsts::FILE_SIZE];
    let mut files = Vec::new();

    // Create test files and measure time
    let start = Instant::now();
    for i in 0..ProbeConsts::NUM_FILES {
        let file_path = probe_dir.join(format!("test_{}.dat", i));
        let mut file = File::create(&file_path)?;
        file.write_all(&data)?;
        // Try to sync but don't fail if not supported (SMB on macOS doesn't support fsync)
        let _ = file.sync_all();
        files.push(file_path);
    }
    let create_time = start.elapsed();

    // Read test files
    let start = Instant::now();
    for file_path in &files {
        let _ = fs::read(file_path)?;
    }
    let read_time = start.elapsed();

    // Cleanup
    fs::remove_dir_all(&probe_dir).ok();

    // Calculate IOPS
    let total_ops = (ProbeConsts::NUM_FILES * 2) as f64; // Create + read
    let total_time_secs = (create_time + read_time).as_secs_f64();
    let iops = total_ops / total_time_secs;

    let drive_type = if iops < ProbeConsts::IOPS_HDD_THRESHOLD {
        "Network+HDD"
    } else {
        "Network+SSD"
    };

    debug!(
        "Disk probe: {} files in {:.2}s = {:.0} IOPS → {}",
        ProbeConsts::NUM_FILES * 2,
        total_time_secs,
        iops,
        drive_type
    );

    Ok(DiskTypeInfo {
        drive_type: drive_type.to_string(),
        random_iops: iops,
        tested_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    })
}

/// Quick network latency measurement using stat calls
fn measure_network_latency(path: &Path) -> Result<NetworkInfo> {
    let start = Instant::now();
    for _ in 0..ProbeConsts::NUM_LATENCY_SAMPLES {
        // Just stat the directory itself (lightweight operation)
        let _ = fs::metadata(path)?;
    }
    let elapsed = start.elapsed();

    let avg_latency_ms = elapsed.as_secs_f64() * 1000.0 / ProbeConsts::NUM_LATENCY_SAMPLES as f64;

    debug!("Network latency: {:.2}ms avg", avg_latency_ms);

    Ok(NetworkInfo {
        latency_ms: avg_latency_ms,
        measured_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    })
}

/// Calculate optimal worker count based on disk type and network conditions.
/// Decision matrix: HDD+high latency → floor; HDD+low → hdd_max; SSD+high → hdd_max; SSD+low → network_max.
fn calculate_workers(disk_type: &DiskTypeInfo, network: &NetworkInfo) -> usize {
    let limits = WorkerThreadLimits::current();
    let is_hdd = DriveType::from_disk_type_str(&disk_type.drive_type).is_hdd();
    let latency = network.latency_ms;

    match (is_hdd, latency) {
        (true, l) if l > ProbeConsts::LATENCY_HIGH_MS => limits.floor,
        (true, l) if l > ProbeConsts::LATENCY_MED_MS => limits.hdd_max.saturating_sub(1),
        (true, _) => limits.hdd_max,
        (false, l) if l > ProbeConsts::LATENCY_HIGH_MS => limits.hdd_max,
        (false, l) if l > ProbeConsts::LATENCY_MED_MS => limits.unknown_max,
        (false, _) => limits.network_max,
    }
}

/// Load cached disk info from the diskinfo table.
fn load_cache_from_db(conn: &Connection, root_path: &str) -> Result<Option<DiskInfo>> {
    let s: String = match conn.query_row(
        "SELECT data FROM diskinfo WHERE root_path = ?1",
        [root_path],
        |row| row.get(0),
    ) {
        Ok(data) => data,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let info: DiskInfo = serde_json::from_str(&s).context("parse cache JSON")?;
    Ok(Some(info))
}

/// Save disk info to the diskinfo table.
fn save_cache_to_db(conn: &Connection, root_path: &str, info: &DiskInfo) -> Result<()> {
    let json = serde_json::to_string(info).context("serialize cache")?;
    conn.execute(
        "INSERT OR REPLACE INTO diskinfo (root_path, data) VALUES (?1, ?2)",
        [root_path, &json],
    )?;
    Ok(())
}
