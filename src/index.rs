//! Directory indexing operations

use anyhow::Result;
use kdam::Animation;
use std::path::Path;
use std::time::Instant;

use crate::Opts;
use crate::engine;
use crate::engine::progress::{ProgressBarConfig, create_progress_bar, update_progress_bar};
use crate::utils::config::IN_MEMORY_INDEX_THRESHOLD;
use log::debug;

/// Index directory at `root` into the database at `db_path`.
pub fn index_dir(root: &Path, db_path: &Path, opts: &Opts) -> Result<()> {
    let mut conn = engine::open_db(db_path)?;
    let (entries, writer_pool_size) = engine::collect_entries(root, opts, db_path, &conn)?;
    let count = entries.len();

    let start = Instant::now();

    // Load existing index to determine what changed
    let existing = engine::load_index(&conn)?;

    let pb = if opts.verbose && !entries.is_empty() {
        let config = ProgressBarConfig::new(entries.len(), "Writing index", Animation::Classic);
        Some(create_progress_bar(config))
    } else {
        None
    };

    let on_batch = pb.as_ref().map(|bar| {
        let bar = std::sync::Arc::clone(bar);
        Box::new(move |n: usize| update_progress_bar(&bar, n)) as Box<dyn Fn(usize) + Send>
    });

    if count < IN_MEMORY_INDEX_THRESHOLD {
        // Small dir: index in memory then backup to disk (avoids WAL contention, single writer)
        drop(conn);
        let mut mem_conn = engine::open_db_in_memory()?;
        engine::apply_index_diff(
            &mut mem_conn,
            &entries,
            &existing,
            opts.mtime_window_ns,
            on_batch,
        )?;
        engine::backup_to_file(&mem_conn, db_path)?;
    } else {
        engine::apply_index_diff_pooled(
            db_path,
            &mut conn,
            &entries,
            &existing,
            opts.mtime_window_ns,
            on_batch,
            writer_pool_size,
        )?;
    }

    debug!("Indexed {} paths into {}", count, db_path.display());
    debug!("Indexing took {:.2}s", start.elapsed().as_secs_f64());
    Ok(())
}
