//! Pipeline components: context, walk loop, error handling.

pub mod context;
pub mod error_handler;
pub mod metadata;
pub mod orchestrator;
pub mod walk;

pub use context::{
    CollectEntriesResult, PipelineChannels, PipelineContext, PipelineHandles, PipelineTuning,
    create_pipeline_channels,
};
pub use error_handler::check_for_initial_error_or_skipped_paths;
pub use metadata::spawn_metadata_workers;
pub use orchestrator::{
    collect_entries, run_pipeline, setup_pipeline_root_and_tuning, shutdown_pipeline_handles,
};
pub use walk::{
    WalkOutcome, run_walk_loop, spawn_walk_thread, to_outcome_jwalk, to_outcome_walkdir,
};

/// Path and entry channel capacity. Must be ≥ max path count so the walk never blocks on send
/// and can drop `path_tx` promptly (lets workers see channel close and exit). 50_000 is enough
/// for typical large trees; for very large trees this could be made configurable (e.g. via opts or env).
pub const STREAMING_CHANNEL_CAP: usize = 50_000;
