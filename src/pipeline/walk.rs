//! Common walk loop: consumes an iterator of Ok(path) / Err and sends to path_tx, handles strict/skipped.

use crossbeam_channel::Sender;
use std::path::PathBuf;
use std::thread::{self, JoinHandle};

use crate::engine::tools::should_include_in_walk;

use super::context::PipelineContext;

/// One result from a directory walk: either a path to consider or an error with optional path.
pub enum WalkOutcome {
    Ok(PathBuf),
    Err { msg: String, path: Option<PathBuf> },
}

/// Convert a jwalk result into [`WalkOutcome`].
pub fn to_outcome_jwalk(r: Result<jwalk::DirEntry<((), ())>, jwalk::Error>) -> WalkOutcome {
    match r {
        Ok(entry) => WalkOutcome::Ok(entry.path().to_path_buf()),
        Err(err) => WalkOutcome::Err {
            msg: format!("{}", err),
            path: err.path().map(PathBuf::from),
        },
    }
}

/// Convert a walkdir result into [`WalkOutcome`].
pub fn to_outcome_walkdir(r: Result<walkdir::DirEntry, walkdir::Error>) -> WalkOutcome {
    match r {
        Ok(entry) => WalkOutcome::Ok(entry.into_path()),
        Err(err) => WalkOutcome::Err {
            msg: format!("{}", err),
            path: err.path().map(PathBuf::from),
        },
    }
}

fn jwalk_iter(ctx: &PipelineContext) -> Box<dyn Iterator<Item = WalkOutcome>> {
    use jwalk::Parallelism;
    use std::time::Duration;
    Box::new(
        jwalk::WalkDir::new(&ctx.root)
            .follow_links(ctx.follow_links)
            .parallelism(Parallelism::RayonDefaultPool {
                busy_timeout: Duration::from_secs(60),
            })
            .into_iter()
            .map(to_outcome_jwalk),
    )
}

fn walkdir_iter(ctx: &PipelineContext) -> Box<dyn Iterator<Item = WalkOutcome>> {
    use walkdir::WalkDir;
    Box::new(
        WalkDir::new(&ctx.root)
            .follow_links(ctx.follow_links)
            .into_iter()
            .map(to_outcome_walkdir),
    )
}
pub fn spawn_walk_thread(
    path_tx: Sender<PathBuf>,
    path_count_tx: Sender<usize>,
    ctx: PipelineContext,
    parallel_walk: bool,
) -> JoinHandle<usize> {
    thread::spawn(move || {
        let iter: Box<dyn Iterator<Item = WalkOutcome>> = match parallel_walk {
            true => jwalk_iter(&ctx),
            false => walkdir_iter(&ctx),
        };
        run_walk_loop(path_tx, path_count_tx, ctx, iter, !parallel_walk)
    })
}

/// Run the common walk loop: consume `iter` of [`WalkOutcome`], filter with `should_include_in_walk`,
/// send included paths to `path_tx`, handle errors (strict → set first_error and break; else log and push to skipped_paths).
/// Sends total count on `path_count_tx` and drops `path_tx` when done. Returns the count of paths sent.
/// When `track_last_path` is true (walkdir/serial), we record the last path seen and use it when an error has no path.
/// When false (jwalk/parallel), we don't track—avoids cloning on every Ok and "last path" would be nondeterministic anyway.
pub fn run_walk_loop<I>(
    path_tx: Sender<PathBuf>,
    path_count_tx: Sender<usize>,
    ctx: PipelineContext,
    iter: I,
    track_last_path: bool,
) -> usize
where
    I: Iterator<Item = WalkOutcome>,
{
    let mut count = 0_usize;
    let mut last_path: Option<PathBuf> = None;
    for outcome in iter {
        match outcome {
            WalkOutcome::Ok(path) => {
                if track_last_path {
                    last_path = Some(path.clone());
                }
                if should_include_in_walk(
                    &path,
                    &ctx.root,
                    &ctx.db_canonical,
                    &ctx.temp_canonical,
                    &ctx.exclude,
                ) {
                    if path_tx.send(path).is_err() {
                        break;
                    }
                    count += 1;
                }
            }
            WalkOutcome::Err { msg, path } => {
                if ctx.strict {
                    let _ = ctx.first_error.lock().unwrap().get_or_insert_with(|| msg);
                    break;
                }
                // Record every error (path or synthetic line so timeouts/errors with no path are counted).
                let to_push = path.unwrap_or_else(|| {
                    PathBuf::from(format!(
                        "<no-path, last was {}>",
                        last_path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "<none>".to_string())
                    ))
                });
                ctx.skipped_paths.lock().unwrap().push((to_push, msg));
            }
        }
    }
    let _ = path_count_tx.send(count);
    drop(path_tx);
    count
}
