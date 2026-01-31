//! File hashing utilities

use anyhow::Result;
use blake3::Hasher;
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;

use crate::utils::config::HashingConsts;

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
