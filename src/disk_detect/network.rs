/// Check if filesystem type indicates network storage
#[inline]
pub(crate) fn is_network_fs(fs_type: &str) -> bool {
    let fs = fs_type.to_lowercase();
    fs.contains("nfs")
        || fs.contains("smb")
        || fs.contains("cifs")
        || fs.contains("smbfs")
        || fs.contains("afp")
        || fs.contains("afpfs")
        || fs.contains("webdav")
}

/// Check if mount point indicates network path
#[inline]
#[allow(dead_code)]
pub(crate) fn is_network_mount(mount: &str) -> bool {
    mount.starts_with("\\\\") || mount.starts_with("//")
}
