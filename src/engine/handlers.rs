//! Command handlers for index and check operations

use anyhow::Result;

use crate::Opts;
use crate::check::check_dir;
use crate::engine::{CommonArgs, running_as_root};
use crate::index::index_dir;
use crate::utils::setup_logging;

/// Setup logging and create Opts from CommonArgs
fn setup_operation(common: &CommonArgs) -> Opts {
    setup_logging(common.verbose);
    Opts {
        with_hash: common.check_hash,
        follow_links: common.follow_links,
        exclude: common.exclude.clone(),
        verbose: common.verbose,
        mtime_window_ns: common.mtime_window * 1_000_000_000, // Convert seconds to nanoseconds
        strict: common.strict,
        paranoid: common.paranoid,
        encrypt: common.encrypt,
    }
}

/// Handle index command
pub fn handle_index(common: &CommonArgs) -> Result<()> {
    let opts = setup_operation(common);
    if running_as_root() && !opts.encrypt {
        log::info!("Running as root. Consider using -x or --encrypt to protect the index.");
    }
    let db_path = common.db_path();
    index_dir(&common.dir, &db_path, &opts)?;
    Ok(())
}

/// Handle check command
pub fn handle_check(common: &CommonArgs) -> Result<()> {
    let opts = setup_operation(common);
    let db_path = common.db_path();
    check_dir(&common.dir, &db_path, &opts)?;
    Ok(())
}
