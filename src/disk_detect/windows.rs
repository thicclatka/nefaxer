//! Windows-specific disk type detection using sysinfo WMI

use super::DriveType;
use super::network::{is_network_fs, is_network_mount};
use log::debug;
use std::path::Path;
use sysinfo::Disks;

pub fn detect(path: &Path) -> DriveType {
    let disks = Disks::new_with_refreshed_list();

    debug!("Available disks:");
    for d in disks.iter() {
        debug!(
            "  mount={}, fs={}, kind={:?}",
            d.mount_point().display(),
            d.file_system().to_string_lossy(),
            d.kind()
        );
    }

    // Find disk containing this path
    let path_str = path.to_string_lossy();
    let disk = disks
        .iter()
        .filter(|d| {
            let mount = d.mount_point().to_string_lossy();
            let mount_str = mount.as_ref();
            // Windows paths: check both forward and backslashes
            path_str.starts_with(mount_str)
                || path_str
                    .replace('/', "\\")
                    .starts_with(&mount.replace('/', "\\"))
        })
        .max_by_key(|d| d.mount_point().to_string_lossy().len());

    match disk {
        Some(disk) => {
            let fs_type = disk.file_system().to_string_lossy();
            let mount_point = disk.mount_point().to_string_lossy();

            debug!(
                "Disk detection: path={}, mount={}, fs_type={}, kind={:?}",
                path.display(),
                mount_point,
                fs_type,
                disk.kind()
            );

            // Check for network filesystems or UNC paths
            if is_network_fs(&fs_type) || is_network_mount(&mount_point) {
                debug!("Detected network filesystem");
                return DriveType::Network;
            }

            // Use sysinfo's disk type detection (queries WMI)
            match disk.kind() {
                sysinfo::DiskKind::HDD => DriveType::HDD,
                sysinfo::DiskKind::SSD => DriveType::SSD,
                // WMI can fail or report Unknown for removable/virtual/NVMe; use conservative parallelism
                sysinfo::DiskKind::Unknown(_) => DriveType::Unknown,
            }
        }
        None => {
            debug!("No disk found for path: {}", path.display());
            DriveType::Unknown
        }
    }
}
