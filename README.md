# Nefaxer

![Build](https://github.com/thicclatka/nefaxer/workflows/Build/badge.svg)
![Rust](https://img.shields.io/badge/rust-1.93-orange.svg)

**Nefaxer** is a high-performance directory indexing and change detection tool written in Rust. It walks directory trees in parallel, computes cryptographic hashes (Blake3) of file contents, and stores metadata in a fast SQLite database. Compare the current state of a directory against a previous snapshot to detect additions, deletions, and modifications.

## Features

- **Parallel walk** (jwalk) and **parallel metadata/hashing** (rayon)
- **Drive-type detection** (SSD / HDD / network) for automatic thread and writer-pool tuning
- **WAL** SQLite with batch inserts, optional in-memory index for small dirs (<10K files), writer pool
- **Exclude patterns** (`-e`) for gitignore-like filtering
- **Strict mode** (`--strict`): fail on first permission/access error instead of skipping
- **Paranoid mode** (`--paranoid`, check only): re-hash when hash matches but mtime/size differ (collision check)
- **FD limit capping** (Unix): cap worker threads by `ulimit -n` to avoid EMFILE

## Usage

### Commands

```bash
# Index a directory (creates/updates .nefaxer in DIR)
nefaxer index [OPTIONS] [DIR]

# Compare directory to existing index; report added/removed/modified
nefaxer check [OPTIONS] [DIR]
```

### Options (shared by `index` and `check`)

| Option                  | Short | Description                                                  |
| ----------------------- | ----- | ------------------------------------------------------------ |
| `--db <DB>`             | `-d`  | Path to index file. Default: `.nefaxer` in DIR               |
| `--verbose`             | `-v`  | Verbose output and progress bar                              |
| `--check-hash`          | `-c`  | Compute Blake3 hash for files (slower, more accurate diff)   |
| `--follow-links`        | `-l`  | Follow symbolic links                                        |
| `--mtime-window <SECS>` | `-m`  | Mtime tolerance in seconds (default: 0)                      |
| `--exclude <PATTERN>`   | `-e`  | Exclude glob patterns (repeatable)                           |
| `--strict`              |       | Fail on first permission/access error                        |
| `--paranoid`            |       | (check only) Re-hash when hash matches but mtime/size differ |

### Examples

```bash
# Index current dir, verbose
nefaxer index -v

# Index with content hashing and exclude node_modules
nefaxer index -c -e 'node_modules' -e '*.log'

# Check for changes, strict (no silent skips)
nefaxer check --strict

# Check with paranoid re-hash for collision detection
nefaxer check -c --paranoid
```

## Database schema

Index file (default `.nefaxer`, WAL mode):

```sql
CREATE TABLE paths (
    path TEXT PRIMARY KEY,
    mtime_ns INTEGER NOT NULL,
    size INTEGER NOT NULL,
    hash BLOB
);

CREATE TABLE diskinfo (
    root_path TEXT PRIMARY KEY,
    data TEXT NOT NULL
);
```

## Build

```bash
cargo build --release
```

## Library

Use the crate for programmatic indexing and diffing:

[ ] - Create better API for library access

## License

Dual-licensed under MIT or Apache-2.0 (your choice).
