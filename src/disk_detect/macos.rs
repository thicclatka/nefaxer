//! macOS-specific disk type detection using statfs

use super::DriveType;
use super::network::is_network_fs;
use log::debug;
use std::ffi::CString;
use std::mem::MaybeUninit;
use std::path::Path;
use sysinfo::Disks;

pub fn detect(path: &Path) -> DriveType {
    // Primary: Use statfs to get filesystem type directly
    // This catches SMB/NFS/AFP mounts that don't show up in sysinfo
    if let Ok(path_cstr) = CString::new(path.to_string_lossy().as_bytes()) {
        unsafe {
            let mut stat: MaybeUninit<libc::statfs> = MaybeUninit::uninit();
            if libc::statfs(path_cstr.as_ptr(), stat.as_mut_ptr()) == 0 {
                let stat = stat.assume_init();
                let fs_type =
                    std::ffi::CStr::from_ptr(stat.f_fstypename.as_ptr()).to_string_lossy();

                debug!("macOS statfs: path={}, fs_type={}", path.display(), fs_type);

                if is_network_fs(&fs_type) {
                    debug!("Detected network filesystem via statfs");
                    return DriveType::Network;
                }
            }
        }
    }

    // Fallback: Use sysinfo for disk type detection
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
            path_str.starts_with(mount.as_ref())
        })
        .max_by_key(|d| d.mount_point().to_string_lossy().len());

    match disk {
        Some(disk) => {
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

            // Use sysinfo's disk type detection
            match disk.kind() {
                sysinfo::DiskKind::HDD => DriveType::HDD,
                sysinfo::DiskKind::SSD => DriveType::SSD,
                sysinfo::DiskKind::Unknown(_) => DriveType::SSD, // Default to SSD
            }
        }
        None => {
            debug!("No disk found for path: {}", path.display());
            DriveType::Unknown
        }
    }
}
