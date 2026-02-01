//! Path and filter utilities

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::utils::config::PackagePaths;

/// Convert absolute path to relative path from base
pub fn path_relative_to(path: &Path, base: &Path) -> Option<PathBuf> {
    path.strip_prefix(base).ok().map(|p| p.to_path_buf())
}

/// Check if a file should be excluded based on OS-specific hidden files
pub fn is_os_hidden_file(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        match name {
            // macOS
            ".DS_Store" | ".AppleDouble" | ".LSOverride" | "._*" => true,
            // Windows
            "Thumbs.db" | "ehthumbs.db" | "Desktop.ini" | "$RECYCLE.BIN" => true,
            // Linux
            ".directory" | ".Trash-*" => true,
            _ => {
                // macOS resource fork files start with ._
                name.starts_with("._")
            }
        }
    } else {
        false
    }
}

/// Returns true if the path should be included in the walk (not excluded).
pub fn should_include_in_walk(
    path: &Path,
    root: &Path,
    db_canonical: &Option<PathBuf>,
    temp_canonical: &Option<PathBuf>,
    exclude_patterns: &[String],
) -> bool {
    if path == root {
        return false;
    }
    if let Some(db) = db_canonical
        && path == db.as_path()
    {
        return false;
    }
    if let Some(temp) = temp_canonical
        && path == temp.as_path()
    {
        return false;
    }
    if is_os_hidden_file(path) {
        return false;
    }
    if exclude_patterns.is_empty() {
        return true;
    }
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return true,
    };
    let path_str = path.to_str().unwrap_or("");
    for pattern in exclude_patterns {
        if glob_match(pattern, name) || glob_match(pattern, path_str) {
            return false;
        }
    }
    true
}

/// Simple glob pattern matching (supports * and ?)
pub fn glob_match(pattern: &str, text: &str) -> bool {
    // Remove leading '!' if present (negation handled by caller)
    let pattern = pattern.strip_prefix('!').unwrap_or(pattern);

    // Simple implementation: convert to regex-like matching
    let mut pattern_chars = pattern.chars().peekable();
    let mut text_chars = text.chars().peekable();

    while let Some(&p) = pattern_chars.peek() {
        match p {
            '*' => {
                pattern_chars.next();
                if pattern_chars.peek().is_none() {
                    return true; // trailing * matches everything
                }
                // Try to match rest of pattern
                while text_chars.peek().is_some() {
                    if glob_match(
                        &pattern_chars.clone().collect::<String>(),
                        &text_chars.clone().collect::<String>(),
                    ) {
                        return true;
                    }
                    text_chars.next();
                }
                return false;
            }
            '?' => {
                pattern_chars.next();
                if text_chars.next().is_none() {
                    return false;
                }
            }
            _ => {
                pattern_chars.next();
                if text_chars.next() != Some(p) {
                    return false;
                }
            }
        }
    }

    text_chars.peek().is_none()
}

/// Check if mtime has changed beyond tolerance window
pub fn mtime_changed(new_mtime: i64, old_mtime: i64, tolerance_ns: i64) -> bool {
    let mtime_diff = (new_mtime - old_mtime).abs();
    mtime_diff > tolerance_ns
}

#[cfg(unix)]
fn check_for_root(path: &Path) -> Result<(), anyhow::Error> {
    use std::os::unix::fs::MetadataExt;
    let root_meta = std::fs::metadata(path).context("read root metadata")?;
    if root_meta.uid() == 0 {
        anyhow::bail!(
            "Cannot index root-owned directory: {}. \
            Use sudo with caution.",
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_for_root(_path: &Path) -> Result<(), anyhow::Error> {
    Ok(())
}

/// True if the process is running with effective uid 0 (e.g. via sudo).
#[cfg(unix)]
pub fn running_as_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(unix))]
pub fn running_as_root() -> bool {
    false
}

pub fn check_root_and_canonicalize(path: &Path) -> Result<PathBuf> {
    let path = path.canonicalize().context("canonicalize path")?;
    check_for_root(&path)?;
    Ok(path)
}

pub fn canonicalize_paths(
    root: &Path,
    db_path: &Path,
    temp_path: Option<&Path>,
) -> Result<(PathBuf, Option<PathBuf>, Option<PathBuf>)> {
    let root = check_root_and_canonicalize(root)?;
    let db_canonical = db_path.canonicalize().ok();
    let temp_canonical = temp_path.and_then(|p| p.canonicalize().ok());
    Ok((root, db_canonical, temp_canonical))
}

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
