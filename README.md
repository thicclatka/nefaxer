# Nefaxer

![Build](https://github.com/thicclatka/nefaxer/workflows/Build/badge.svg)
![Rust](https://img.shields.io/badge/rust-1.93.0-orange.svg)

**Nefaxer** is a high-performance directory indexing and change detection tool written in Rust. It walks directory trees in parallel, computes cryptographic hashes (Blake3) of file contents, and stores metadata in a fast SQLite database. You can then quickly compare the current state of a directory against a previous snapshot to detect additions, deletions, and modifications.

## Usage

### CLI

```bash
# Index a directory
nefaxer index -h
Walk directory and write/update the index database

Usage: nefaxer index [OPTIONS] [DIR]

Arguments:
  [DIR]  Directory to index. Default: current directory [default: .]

Options:
  -d, --db <DB>       Path to nefaxer index file. Default: `.nefaxer` in the current directory [default: .nefaxer]
  -v, --verbose       Verbose output. Default: false
  -n, --no-hash       Skip hashing files (faster, less accurate diff). Default: false
  -l, --follow-links  Follow symbolic links. Default: false
  -h, --help          Print help

# Check directory post-indexing for diffs
nefaxer check -h
Compare directory to existing index; report added/removed/modified paths

Usage: nefaxer check [OPTIONS] [DIR]

Arguments:
  [DIR]  Directory to index. Default: current directory [default: .]

Options:
  -d, --db <DB>       Path to nefaxer index file. Default: `.nefaxer` in the current directory [default: .nefaxer]
  -v, --verbose       Verbose output. Default: false
  -n, --no-hash       Skip hashing files (faster, less accurate diff). Default: false
  -l, --follow-links  Follow symbolic links. Default: false
  -h, --help          Print help
```

### Database Schema

Schema for `.nefaxer`:

```sql
CREATE TABLE paths (
    path TEXT PRIMARY KEY,     -- relative path from index root
    mtime_ns INTEGER NOT NULL, -- modification time (nanoseconds since epoch)
    size INTEGER NOT NULL,     -- file size in bytes
    hash BLOB                  -- Blake3 hash (32 bytes, NULL for directories)
);

CREATE TABLE diskinfo (
    root_path TEXT PRIMARY KEY,  -- canonical path of indexed directory
    data TEXT NOT NULL           -- JSON: disk/network probe cache for worker tuning
);
```

## License

Dual-licensed under MIT or Apache 2.0 (your choice).

## To Do

[ ] - Add usage for `lib.rs`
