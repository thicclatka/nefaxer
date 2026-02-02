//! CLI command handler: index by default; --dry-run runs compare-only (no index write).

use anyhow::Result;
use log::{debug, warn};

use crate::Opts;
use crate::check::check_dir;
use crate::engine::arg_parser::Cli;
use crate::engine::running_as_root;
use crate::index::nefax_dir_with_opts;
use crate::utils::nefaxer_toml::{apply_file_to_opts, load_nefaxer_toml};
use crate::utils::setup_logging;

/// Overwrite opts field with CLI value only when user passed the flag.
macro_rules! apply_cli_opt {
    ($cli:expr, $opts:expr, $cli_field:ident => $opts_field:ident) => {
        if let Some(v) = $cli.$cli_field {
            $opts.$opts_field = v;
        }
    };
}

/// Setup options: load .nefaxer.toml into opts, then overwrite with CLI only when user passed a flag.
fn setup_opts(cli: &Cli) -> Opts {
    let mut opts = Opts::default();
    if let Some(file) = load_nefaxer_toml(&cli.dir) {
        apply_file_to_opts(&file, &mut opts);
    }
    opts.db_path = cli.db.clone().or(opts.db_path);
    opts.num_threads = None;
    apply_cli_opt!(cli, opts, check_hash => with_hash);
    apply_cli_opt!(cli, opts, follow_links => follow_links);
    if !cli.exclude.is_empty() {
        opts.exclude = cli.exclude.clone();
    }
    apply_cli_opt!(cli, opts, verbose => verbose);
    if let Some(secs) = cli.mtime_window {
        opts.mtime_window_ns = secs * 1_000_000_000;
    }
    apply_cli_opt!(cli, opts, strict => strict);
    apply_cli_opt!(cli, opts, paranoid => paranoid);
    apply_cli_opt!(cli, opts, encrypt => encrypt);
    apply_cli_opt!(cli, opts, list => list_paths);
    opts.write_to_db = !cli.dry_run;
    setup_logging(opts.verbose);
    opts
}

/// Run index (default) or compare-only when --dry-run. Does not write to index when dry_run.
pub fn handle_run(cli: &Cli) -> Result<()> {
    let opts = setup_opts(cli);
    if running_as_root() && !opts.encrypt {
        log::info!("Running as root. Consider using -x or --encrypt to protect the index.");
    }
    if cli.dry_run {
        warn!("RUNNING IN DRY-RUN MODE. NO CHANGES WILL BE APPLIED TO THE INDEX.");
        check_dir(&cli.dir, &opts)?;
    } else {
        debug!("Nefaxing directory...");
        nefax_dir_with_opts(&cli.dir, &opts)?;
    }
    Ok(())
}
