# Nefaxer

![Build](https://github.com/thicclatka/nefaxer/workflows/Build/badge.svg)
![Rust](https://img.shields.io/badge/rust-1.93-orange.svg)

**Nefaxer** is a high-performance directory indexing and change detection tool written in Rust. It walks directory trees in parallel, computes hashes of file contents, and stores metadata in a fast indexed database. Compare the current state of a directory against a previous snapshot to detect additions, deletions, and modifications.

## Features

- **Streaming pipeline** — Walk thread sends paths over a bounded channel; metadata workers turn paths into entries; the main thread receives entries, optionally hashes large files (when `--check-hash`), and writes batches to file. No full-tree buffering: walk, metadata, and write run concurrently.
- **Drive-adaptive walk** — Serial walk (`walkdir`) where the disk is the bottleneck, otherwise parallel (`jwalk`). Parallel metadata (worker threads); hashing when enabled is sequential in the receiver.
- **Drive-type detection** (SSD / HDD / network) for automatic thread and writer-pool tuning
- **WAL** SQLite with batch inserts, optional in-memory index for small dirs (<10K files), writer pool
- **Exclude patterns** (`-e`) for gitignore-like filtering
- **Strict mode** (`--strict`): fail on first permission/access error instead of skipping
- **Paranoid mode** (`--paranoid`, with `-c`): re-hash when hash matches but mtime/size differ (collision check)
- **FD limit capping** (Unix): cap worker threads by `ulimit -n` to avoid EMFILE

## Usage

```bash
# Index a directory (creates/updates .nefaxer in DIR). Default.
nefaxer [OPTIONS] [DIR]

# Compare to index and report added/removed/modified; do not write to the index
nefaxer --dry-run [OPTIONS] [DIR]
```

### Options

| Option                  | Short | Description                                                                                      |
| ----------------------- | ----- | ------------------------------------------------------------------------------------------------ |
| `--db <DB>`             | `-d`  | Path to index file. Default: `.nefaxer` in DIR                                                   |
| `--dry-run`             |       | Compare only; report diff, do not update index                                                   |
| `--list`                |       | List each changed path. If total changes > 100, write to `nefaxer.results` instead of stdout     |
| `--verbose`             | `-v`  | Verbose output and progress bar                                                                  |
| `--check-hash`          | `-c`  | Compute Blake3 hash for files (slower, more accurate diff)                                       |
| `--follow-links`        | `-l`  | Follow symbolic links                                                                            |
| `--mtime-window <SECS>` | `-m`  | Mtime tolerance in seconds (default: 0)                                                          |
| `--exclude <PATTERN>`   | `-e`  | Exclude glob patterns (repeatable)                                                               |
| `--encrypt`             | `-x`  | Encrypt the index database with SQLCipher. Prompts for passphrase (or use NEFAXER_DB_KEY / .env) |
| `--strict`              |       | Fail on first permission/access error                                                            |
| `--paranoid`            |       | (with -c) Re-hash when hash matches but mtime/size differ                                        |

### Examples

```bash
# Index current dir, verbose
nefaxer -v

# Index with content hashing and exclude node_modules
nefaxer -c -e 'node_modules' -e '*.log'

# Compare only (no index write), strict
nefaxer --dry-run --strict

# Compare with paranoid re-hash for collision detection
nefaxer --dry-run -c --paranoid
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

Use the crate for programmatic indexing and diffing. The API returns a full current index (same shape as the `.nefaxer` DB).

### Entry point

- **`nefax_dir(root, opts)`** — Walk `root`, build and return the nefax map. Returns **`Result<Nefax>`** (path → metadata).

### Result type

```rust
pub type Nefax = HashMap<PathBuf, PathMeta>;

pub struct PathMeta {
    pub mtime_ns: i64,
    pub size: u64,
    pub hash: Option<[u8; 32]>,
}
```

Same shape as the `.nefaxer` DB. Diff against a previous nefax if you need added/removed/modified.

### NefaxOpts

Use `NefaxOpts::default()` and override as needed:

- `num_threads` — override worker count (default: derived from drive)
- `with_hash` — compute Blake3 for files
- `follow_links` — follow symlinks
- `mtime_window_ns` — mtime tolerance (nanoseconds)
- `strict` — fail on first permission/access error
- `paranoid` — re-hash when hash matches but mtime/size differ

### Example

```rust
use nefaxer::{nefax_dir, NefaxOpts};
use std::path::Path;

let nefax = nefax_dir(Path::new("/some/dir"), &NefaxOpts::default())?;
// nefax: HashMap<PathBuf, PathMeta>
```

## License

Dual-licensed under MIT or Apache-2.0 (your choice).
