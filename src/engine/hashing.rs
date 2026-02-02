//! File hashing utilities

use anyhow::Result;
use blake3::Hasher;
use memmap2::Mmap;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::Entry;
use crate::Opts;
use crate::engine::{StoredMeta, mtime_changed};
use crate::utils::config::HashingConsts;
use crate::utils::config::SMALL_FILE_THRESHOLD;

/// Hash a file with blake3. Uses memory-mapped I/O for files above threshold, chunked reading otherwise.
pub fn hash_file(path: &Path, size: u64) -> Result<Option<[u8; 32]>> {
    let file = File::open(path)?;
    let mut hasher = Hasher::new();

    if size > HashingConsts::HASH_MMAP_THRESHOLD {
        // Memory-mapped I/O for large files (Blake3 already uses SIMD internally)
        let mmap = unsafe { Mmap::map(&file)? };
        hasher.update(&mmap);
    } else {
        // Chunked reading for smaller files
        use std::io::Read;
        let mut reader =
            std::io::BufReader::with_capacity(HashingConsts::HASH_READ_CHUNK_SIZE, file);
        let mut buffer = vec![0u8; HashingConsts::HASH_READ_CHUNK_SIZE];
        loop {
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
    }

    Ok(Some(*hasher.finalize().as_bytes()))
}

/// Compare two hash options for equality
pub fn hash_equals(hash1: &Option<[u8; 32]>, hash2: &Option<Vec<u8>>) -> bool {
    match (hash1, hash2) {
        (None, None) => true,
        (Some(a), Some(b)) => a.as_slice() == b.as_slice(),
        _ => false,
    }
}

/// When opts.with_hash and size >= threshold: reuse index hash if mtime+size match, else hash file.
pub fn fill_entry_hash_if_needed(
    entry: &mut Entry,
    index: &HashMap<PathBuf, StoredMeta>,
    root: &Path,
    opts: &Opts,
) {
    if !opts.with_hash || entry.size < SMALL_FILE_THRESHOLD {
        return;
    }
    let existing = index.get(&entry.path);
    let reuse = existing.is_some_and(|(old_mtime, old_size, old_hash)| {
        !mtime_changed(entry.mtime_ns, *old_mtime, opts.mtime_window_ns)
            && entry.size == *old_size
            && old_hash.as_ref().is_some_and(|v| v.len() == 32)
    });
    if reuse {
        if let Some((_, _, Some(v))) = existing {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(v);
            entry.hash = Some(arr);
        }
    } else {
        let abs = root.join(&entry.path);
        if let Ok(Some(h)) = hash_file(&abs, entry.size) {
            entry.hash = Some(h);
        }
    }
}
