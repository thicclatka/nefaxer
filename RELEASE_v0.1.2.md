# Release 0.1.2

## Library API: single entry point, tuning, and streaming

- **Single entry point** — `nefax_dir(root, opts, existing, on_entry)` returns `Result<(Nefax, Diff)>`. One function for both batch and streaming; optional previous snapshot and optional callback.
- **Tuning** — Use `tuning_for_path(path, available_threads)` to get `(num_threads, drive_type, use_parallel_walk)` and set `NefaxOpts` to skip drive detection.
- **Streaming** — Pass `on_entry: Some(|entry| { ... })` to receive each entry as it’s ready (e.g. for progress or forwarding to another pipeline). Batch use: `on_entry: None`.
- **Existing snapshot** — Pass `existing: Some(&nefax)` to diff against a previous index (e.g. a `Nefax` you built from your own DB/table). Pass `existing: None` for a fresh run.

## Validation of provided `existing` map

- When you pass `existing: Some(&nefax)` from your own table, nefaxer **validates it internally**: paths must be relative and non-empty; `mtime_ns` and `size` must be in plausible ranges. Invalid data returns an error.
- **`validate_nefax(&nefax)`** is public so you can optionally fail early (e.g. right after loading from your DB).

## Internal and docs

- Types (`Entry`, `PathMeta`, `Diff`, `Nefax`, `NefaxOpts`, `Opts`) moved to `src/types.rs`; re-exported from the crate root so the public API is unchanged.
- README Library section updated: entry point, types, validation, `NefaxOpts`, and examples for fresh index, diff-with-prior, streaming, and tuning.

---

**Install:** `cargo install nefaxer`  
**Lib:** `cargo add nefaxer`
