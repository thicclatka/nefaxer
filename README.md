# Nefaxer

[![Crates.io](https://img.shields.io/crates/v/nefaxer.svg)](https://crates.io/crates/nefaxer)
[![docs.rs](https://img.shields.io/docsrs/nefaxer)](https://docs.rs/nefaxer)
![Build](https://github.com/thicclatka/nefaxer/workflows/Build/badge.svg)
![Rust](https://img.shields.io/badge/rust-1.93-orange.svg)

> The Demon could sit in a box among air molecules that were moving at all different random speeds, and sort out the fast molecules from the slow ones.  
> — Koteks on the Nefastis Machine, _[The Crying of Lot 49](https://bookshop.org/p/books/the-crying-of-lot-49-thomas-pynchon/e6265a50e173d7ec?ean=9780060913076&next=t)_

**Nefaxer** is a high-performance directory indexing and change detection tool written in Rust. It walks directory trees in parallel, computes hashes of file contents, and stores metadata in a fast indexed database. Compare the current state of a directory against a previous snapshot to detect additions, deletions, and modifications.

## Install

```bash
# library
cargo add nefaxer

# CLI (from crates.io)
cargo install nefaxer

# Source archive
# Download from: https://github.com/thicclatka/nefaxer/releases
```

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
| `--list`                | `-l`  | List each changed path. If total changes > 100, write to `nefaxer.results` instead of stdout     |
| `--verbose`             | `-v`  | Verbose output and progress bar                                                                  |
| `--check-hash`          | `-c`  | Compute Blake3 hash for files (slower, more accurate diff)                                       |
| `--follow-links`        | `-f`  | Follow symbolic links                                                                            |
| `--mtime-window <SECS>` | `-m`  | Mtime tolerance in seconds (default: 0)                                                          |
| `--exclude <PATTERN>`   | `-e`  | Exclude glob patterns (repeatable)                                                               |
| `--encrypt`             | `-x`  | Encrypt the index database with SQLCipher. Prompts for passphrase (or use NEFAXER_DB_KEY / .env) |
| `--strict`              |       | Fail on first permission/access error                                                            |
| `--paranoid`            |       | (with -c) Re-hash when hash matches but mtime/size differ                                        |

### Configuration file (CLI only)

When running the binary, you can put a `.nefaxer.toml` in the directory you index. Options from the file are used as defaults; command-line options override them.

```toml
[settings]
db_path = ".nefaxer"
hash = true
follow_links = false
exclude = ["node_modules", ".git"]
list = false
verbose = false
mtime_window = 0
strict = false
paranoid = false
encrypt = false
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

## Library

Use the crate for programmatic indexing and diffing. Single entry point; returns the current index and a diff. The API supports tuning (`tuning_for_path`) and streaming via callback.

### Entry point

- **`nefax_dir(root, opts, existing, on_entry)`** — Index `root` with `opts`. Returns **`Result<(Nefax, Diff)>`**.
  - **`existing`** — `None` for a fresh run (diff = all added); `Some(&nefax)` to diff against a previous snapshot (e.g. a `Nefax` you built from your own DB/table).
  - **`on_entry`** — `None` for batch (non-streaming); `Some(|entry| { ... })` to get each entry as it’s ready (streaming, e.g. for progress or forwarding to another pipeline). Callback runs on the consumer thread; keep it fast or send to a channel.

- **`tuning_for_path(path, available_threads)`** — Returns `(num_threads, drive_type, use_parallel_walk)` so you can set `NefaxOpts` and skip drive detection.

### Types

```rust
pub type Nefax = HashMap<PathBuf, PathMeta>;

pub struct PathMeta {
    pub mtime_ns: i64,
    pub size: u64,
    pub hash: Option<[u8; 32]>,
}

pub struct Entry {  // per-path in callback
    pub path: PathBuf,
    pub mtime_ns: i64,
    pub size: u64,
    pub hash: Option<[u8; 32]>,
}

pub struct Diff {
    pub added: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
    pub modified: Vec<PathBuf>,
}
```

Same shape as the `.nefaxer` DB. When you pass `existing: Some(&nefax)` from your own table, **`nefax_dir` validates it internally** (paths relative and non-empty, `mtime_ns`/`size` in valid ranges) and returns an error if invalid. You can call **`validate_nefax(&nefax)`** yourself for fail-early (e.g. right after loading from your DB).

### NefaxOpts

Use `NefaxOpts::default()` and override as needed:

- `num_threads`, `drive_type`, `use_parallel_walk` — set all three (e.g. from `tuning_for_path`) to skip drive detection
- `with_hash` — compute Blake3 for files
- `follow_links` — follow symlinks
- `exclude` — glob patterns to skip (e.g. `node_modules`, `*.log`)
- `mtime_window_ns` — mtime tolerance (nanoseconds)
- `strict` — fail on first permission/access error
- `paranoid` — re-hash when hash matches but mtime/size differ

### Examples

```rust
use nefaxer::{nefax_dir, validate_nefax, NefaxOpts, tuning_for_path};
use std::path::Path;

// Fresh index, batch: get (nefax, diff) with diff = all added
let (nefax, diff) = nefax_dir(Path::new("/some/dir"), &NefaxOpts::default(), None, None)?;

// Diff against a previous snapshot: build Nefax from your table (path → PathMeta), pass as existing.
// Validation runs inside nefax_dir; optional: validate_nefax(&prior)? to fail early after loading.
// let prior: Nefax = /* your table → HashMap<PathBuf, PathMeta> */;
// let (nefax, diff) = nefax_dir(Path::new("/some/dir"), &opts, Some(&prior), None)?;

// Streaming: process each entry as it’s ready
let (nefax, diff) = nefax_dir(
    Path::new("/some/dir"),
    &opts,
    None,
    Some(|e: &nefaxer::Entry| { /* stream to zahir, update progress, etc. */ }),
)?;

// Skip drive detection using tuning
let (n, dt, pw) = tuning_for_path(Path::new("/some/dir"), None);
let opts = NefaxOpts {
    num_threads: Some(n),
    drive_type: Some(dt),
    use_parallel_walk: Some(pw),
    ..Default::default()
};
let (nefax, _) = nefax_dir(Path::new("/some/dir"), &opts, None, None)?;
```

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
