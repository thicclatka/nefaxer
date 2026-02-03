//! Load `.nefaxer.toml` from a directory (CLI only). Lib does not use this; the consuming program injects config via NefaxOpts.

use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::Opts;

#[derive(Debug, Deserialize)]
pub(crate) struct NefaxerToml {
    #[serde(default)]
    settings: IndexSection,
}

#[derive(Debug, Default, Deserialize)]
struct IndexSection {
    db_path: Option<String>,
    hash: Option<bool>,
    follow_links: Option<bool>,
    exclude: Option<Vec<String>>,
    list: Option<bool>,
    verbose: Option<bool>,
    mtime_window: Option<i64>,
    strict: Option<bool>,
    paranoid: Option<bool>,
    encrypt: Option<bool>,
}

/// Load `.nefaxer.toml` from `dir` if present. Returns None if file missing or unreadable. CLI only.
pub(crate) fn load_nefaxer_toml(dir: &Path) -> Option<NefaxerToml> {
    let path = dir.join(".nefaxer.toml");
    let s = std::fs::read_to_string(&path).ok()?;
    toml::from_str(&s)
        .map_err(|e| log::warn!("{}: {}", path.display(), e))
        .ok()
}

/// Overwrite opts field from file when present.
macro_rules! apply_file_opt {
    ($idx:expr, $opts:expr, $idx_field:ident => $opts_field:ident) => {
        if let Some(v) = $idx.$idx_field {
            $opts.$opts_field = v;
        }
    };
}

/// Apply file config to opts (only set fields present in the file). Call before applying CLI. dry_run is never in the file.
pub(crate) fn apply_file_to_opts(file: &NefaxerToml, opts: &mut Opts) {
    let idx = &file.settings;
    if let Some(ref p) = idx.db_path {
        opts.db_path = Some(PathBuf::from(p));
    }
    apply_file_opt!(idx, opts, hash => with_hash);
    apply_file_opt!(idx, opts, follow_links => follow_links);
    if let Some(ref v) = idx.exclude {
        opts.exclude = v.clone();
    }
    apply_file_opt!(idx, opts, list => list_paths);
    apply_file_opt!(idx, opts, verbose => verbose);
    if let Some(secs) = idx.mtime_window {
        opts.mtime_window_ns = secs * 1_000_000_000;
    }
    apply_file_opt!(idx, opts, strict => strict);
    apply_file_opt!(idx, opts, paranoid => paranoid);
    apply_file_opt!(idx, opts, encrypt => encrypt);
}
