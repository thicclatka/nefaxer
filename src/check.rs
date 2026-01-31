//! Directory change detection operations

use anyhow::Result;
use log::debug;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::engine::{collect_entries, hash_equals, hash_file, load_index, mtime_changed, open_db};
use crate::utils::Colors;
use crate::{Diff, Opts};

/// Compare directory at `root` to the index in `db_path`. Returns added/removed/modified paths (relative).
pub fn check_dir(root: &Path, db_path: &Path, opts: &Opts) -> Result<Diff> {
    let conn = open_db(db_path)?;
    let index = load_index(&conn)?;
    let (current, _) = collect_entries(root, opts, db_path, &conn)?;

    let current_map: HashMap<PathBuf, (i64, u64, Option<[u8; 32]>)> = current
        .into_iter()
        .map(|e| (e.path, (e.mtime_ns, e.size, e.hash)))
        .collect();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut modified = Vec::new();

    for (path, (mtime_ns, size, hash)) in &current_map {
        match index.get(path) {
            None => added.push(path.clone()),
            Some((old_mtime, old_size, old_hash)) => {
                let same = !mtime_changed(*mtime_ns, *old_mtime, opts.mtime_window_ns)
                    && *old_size == *size
                    && hash_equals(hash, old_hash);

                if !same {
                    // Paranoid: if hashes match but mtime/size differ, re-hash to rule out collision
                    let still_modified = if opts.paranoid
                        && hash.is_some()
                        && old_hash.is_some()
                        && hash_equals(hash, old_hash)
                    {
                        let abs = root.join(path);
                        if let Ok(meta) = std::fs::metadata(&abs)
                            && meta.is_file()
                        {
                            if let Ok(Some(rehash)) = hash_file(&abs, meta.len()) {
                                rehash.as_slice() != old_hash.as_deref().unwrap_or(&[])
                            } else {
                                true
                            }
                        } else {
                            true
                        }
                    } else {
                        true
                    };
                    if still_modified {
                        modified.push(path.clone());
                    }
                }
            }
        }
    }
    for path in index.keys() {
        if !current_map.contains_key(path) {
            removed.push(path.clone());
        }
    }

    let diff = Diff {
        added,
        removed,
        modified,
    };

    print_diff(&diff);
    Ok(diff)
}

/// Print diff summary
fn print_diff(diff: &Diff) {
    let added_count = diff.added.len();
    let removed_count = diff.removed.len();
    let modified_count = diff.modified.len();
    let total = added_count + removed_count + modified_count;

    if total == 0 {
        debug!("No changes detected.");
        return;
    }

    // Summary
    debug!(
        "{} | {} | {}",
        Colors::colorize(Colors::ADDED, &format!("Added: {}", added_count)),
        Colors::colorize(Colors::REMOVED, &format!("Removed: {}", removed_count)),
        Colors::colorize(Colors::MODIFIED, &format!("Modified: {}", modified_count))
    );
}
