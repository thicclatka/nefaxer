//! Linux-specific disk type detection using sysinfo and /sys/block

use super::DriveType;
use super::network::is_network_fs;
use log::debug;
use std::path::Path;
use sysinfo::{Disk, Disks};

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

    let path_str = path.to_string_lossy();
    let disk = disks
        .iter()
        .filter(|d| path_str.starts_with(d.mount_point().to_string_lossy().as_ref()))
        .max_by_key(|d| d.mount_point().to_string_lossy().len());

    match disk {
        Some(disk) => resolve_drive_type(path, disk),
        None => {
            debug!("No disk found for path: {}", path.display());
            DriveType::Unknown
        }
    }
}

fn resolve_drive_type(path: &Path, disk: &Disk) -> DriveType {
    let fs_type = disk.file_system().to_string_lossy();
    debug!(
        "Disk detection: path={}, mount={}, fs_type={}, kind={:?}",
        path.display(),
        disk.mount_point().display(),
        fs_type,
        disk.kind()
    );

    if is_network_fs(&fs_type) {
        debug!("Detected network filesystem");
        return DriveType::Network;
    }

    match disk.kind() {
        sysinfo::DiskKind::HDD => DriveType::HDD,
        sysinfo::DiskKind::SSD => DriveType::SSD,
        sysinfo::DiskKind::Unknown(_) => read_rotational_from_sys(disk).unwrap_or(DriveType::SSD),
    }
}

/// Read /sys/block/{device}/queue/rotational to distinguish HDD (1) vs SSD (0).
fn read_rotational_from_sys(disk: &Disk) -> Option<DriveType> {
    let name = disk.name().to_str()?;
    let dev_name = name.strip_prefix("/dev/")?;
    // Strip partition: sda1 -> sda, nvme0n1p1 -> nvme0n1
    let base_dev = if dev_name.starts_with("nvme") {
        dev_name.split('p').next().unwrap_or(dev_name)
    } else {
        dev_name.trim_end_matches(char::is_numeric)
    };

    let sys_path = format!("/sys/block/{base_dev}/queue/rotational");
    let rotational = std::fs::read_to_string(&sys_path).ok()?;
    Some(if rotational.trim() == "1" {
        DriveType::HDD
    } else {
        DriveType::SSD
    })
}
