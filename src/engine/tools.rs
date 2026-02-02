//! Path and filter utilities

use anyhow::{Context, Result};
use log::{info, warn};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Diff;
use crate::utils::Colors;
use crate::utils::config::PackagePaths;

/// Convert absolute path to relative path from base
pub fn path_relative_to(path: &Path, base: &Path) -> Option<PathBuf> {
    path.strip_prefix(base).ok().map(|p| p.to_path_buf())
}

/// Normalize path for DB storage: forward slashes only. Makes DB portable across Windows/Unix.
pub fn path_to_db_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
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
    let path = path
        .canonicalize()
        .with_context(|| format!("canonicalize path {}", path.display()))?;
    check_for_root(&path)?;
    Ok(path)
}

pub fn canonicalize_paths(
    root: &Path,
    db_path: Option<&Path>,
    temp_path: Option<&Path>,
) -> Result<(PathBuf, Option<PathBuf>, Option<PathBuf>)> {
    let root = check_root_and_canonicalize(root)?;
    let db_canonical = db_path.and_then(|p| p.canonicalize().ok());
    let temp_canonical = temp_path.and_then(|p| p.canonicalize().ok());
    Ok((root, db_canonical, temp_canonical))
}

macro_rules! write_diff_section {
    ($out:expr, $paths:expr, $fmt:expr, $color:expr, $colorize:expr) => {
        for p in $paths {
            let line = format!($fmt, p.display());
            let _ = writeln!(
                $out,
                "{}",
                if $colorize {
                    Colors::colorize($color, &line)
                } else {
                    line
                }
            );
        }
    };
}

/// Write diff path list to `out`. If `colorize` is true, prefix/lines use ANSI colors (for stdout).
fn write_diff_paths<W: std::io::Write>(out: &mut W, diff: &Diff, colorize: bool) {
    write_diff_section!(out, &diff.added, "+ {}", Colors::ADDED, colorize);
    write_diff_section!(out, &diff.removed, "- {}", Colors::REMOVED, colorize);
    write_diff_section!(out, &diff.modified, "M {}", Colors::MODIFIED, colorize);
}

/// Print diff summary (counts: Added / Removed / Modified). When list_paths is true, list each path
/// to stdout if total <= LIST_THRESHOLD, otherwise write to output_dir / PackagePaths::results_filename().
pub fn print_diff(diff: &Diff, dry_run: bool, list_paths: bool, output_dir: &Path) {
    let msg = format!(
        "Nefaxing {} results:",
        if dry_run { "dry-run" } else { "index" }
    );
    info!("{}", msg);

    let added_count = diff.added.len();
    let removed_count = diff.removed.len();
    let modified_count = diff.modified.len();
    let total = added_count + removed_count + modified_count;

    if total == 0 {
        warn!("No changes detected.");
        return;
    }

    info!(
        "{} | {} | {}",
        Colors::colorize(Colors::ADDED, &format!("Added: {}", added_count)),
        Colors::colorize(Colors::REMOVED, &format!("Removed: {}", removed_count)),
        Colors::colorize(Colors::MODIFIED, &format!("Modified: {}", modified_count))
    );

    if !list_paths {
        return;
    }

    let threshold = crate::utils::config::LIST_THRESHOLD;
    if total <= threshold {
        let mut out = std::io::stdout().lock();
        write_diff_paths(&mut out, diff, true);
    } else {
        let out_path = output_dir.join(PackagePaths::get().results_filename());
        match std::fs::File::create(&out_path) {
            Ok(mut f) => {
                write_diff_paths(&mut f, diff, false);
                info!("Listed {} changes to {}", total, out_path.display());
            }
            Err(e) => {
                warn!("Could not write list to {}: {}", out_path.display(), e);
            }
        }
    }
}

/// Create the database path from the root and db_path options.
/// If db_path is None, use `root.join(<package index filename>)` (e.g. `.nefaxer`).
pub fn create_db_path(root: &Path, db_path: Option<&Path>) -> PathBuf {
    db_path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| root.join(PackagePaths::get().output_filename()))
}

/// Setup Ctrl+C handler and return a shared boolean indicating if the user has requested cancellation.
pub fn setup_ctrlc_handler() -> Result<Arc<AtomicBool>> {
    let cancel_requested = Arc::new(AtomicBool::new(false));
    let cancel_requested_handler = Arc::clone(&cancel_requested);

    ctrlc::set_handler(move || {
        cancel_requested_handler.store(true, Ordering::Relaxed);
    })
    .context("set Ctrl+C handler")?;
    Ok(cancel_requested)
}

/// Return an error if the user requested cancellation (e.g. after indexing; partial index may have been flushed).
pub fn check_for_cancel(cancel_requested: &Arc<AtomicBool>) -> Result<()> {
    if cancel_requested.load(Ordering::Relaxed) {
        anyhow::bail!("Nefaxing cancelled by user; partial index was flushed");
    }
    Ok(())
}
