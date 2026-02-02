use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::config::PackagePaths;

/// Get the temporary path for the index database.
pub fn temp_path_for(db_path: &Path) -> PathBuf {
    let name = db_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_else(|| PackagePaths::get().output_filename());
    db_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{name}.tmp"))
}

/// Remove SQLite WAL and SHM files for a temp path after rename (they are left behind).
pub fn remove_temp_wal_and_shm(temp_path: &Path) {
    let file_name = temp_path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let parent = temp_path.parent().unwrap_or(Path::new("."));
    let _ = std::fs::remove_file(parent.join(format!("{file_name}-wal")));
    let _ = std::fs::remove_file(parent.join(format!("{file_name}-shm")));
}

/// Prepare work path for indexing: temp file and whether to use it (atomic rename).
/// Removes stale temp and WAL/SHM; copies existing DB to temp when possible. On permission denied, falls back to writing directly to db_path.
pub fn prepare_index_work_path(db_path: &Path) -> Result<(PathBuf, bool)> {
    let temp_path = temp_path_for(db_path);
    let mut use_temp = true;

    if temp_path.exists() {
        remove_temp_wal_and_shm(&temp_path);
        if let Err(e) = std::fs::remove_file(&temp_path) {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                use_temp = false;
            } else {
                return Err(e).with_context(|| {
                    format!("remove stale temp index at {}", temp_path.display())
                });
            }
        }
    }
    if use_temp
        && db_path.exists()
        && let Err(e) = std::fs::copy(db_path, &temp_path)
    {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            use_temp = false;
        } else {
            return Err(e).context(format!(
                "copy existing index to temp ({} -> {})",
                db_path.display(),
                temp_path.display()
            ));
        }
    }
    Ok((temp_path, use_temp))
}

pub fn rename_temp_to_final(temp_path: &Path, final_path: &Path) -> Result<()> {
    fs::rename(temp_path, final_path).with_context(|| {
        format!(
            "atomic rename temp index to final path ({} -> {})",
            temp_path.display(),
            final_path.display()
        )
    })?;
    remove_temp_wal_and_shm(temp_path);
    Ok(())
}
