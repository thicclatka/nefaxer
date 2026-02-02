//! CLI command handler: index by default; --dry-run runs compare-only (no index write).

use anyhow::Result;
use log::{debug, warn};

use crate::Opts;
use crate::check::check_dir;
use crate::engine::arg_parser::Cli;
use crate::engine::running_as_root;
use crate::index::nefax_dir_with_opts;
use crate::utils::setup_logging;

fn setup_opts(cli: &Cli) -> Opts {
    setup_logging(cli.verbose);
    Opts {
        db_path: cli.db.clone(),
        num_threads: None,
        with_hash: cli.check_hash,
        follow_links: cli.follow_links,
        exclude: cli.exclude.clone(),
        verbose: cli.verbose,
        mtime_window_ns: cli.mtime_window * 1_000_000_000, // seconds -> nanoseconds
        strict: cli.strict,
        paranoid: cli.paranoid,
        encrypt: cli.encrypt,
        list_paths: cli.list,
        write_to_db: !cli.dry_run,
    }
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
