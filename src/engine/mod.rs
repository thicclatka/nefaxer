//! Engine module for core indexing operations

pub mod arg_parser;
pub mod db_ops;
pub mod handlers;
pub mod hashing;
pub mod parallel;
pub mod progress;
pub mod tools;

// Re-export commonly used functions
pub use arg_parser::{Cli, Commands, CommonArgs};
pub use db_ops::{
    ApplyIndexDiffPooledParams, ApplyIndexDiffStreamingParams, StoredMeta, apply_index_diff,
    apply_index_diff_pooled, apply_index_diff_streaming, backup_to_file, load_index, open_db,
    open_db_in_memory, open_db_or_detect_encrypted,
};
pub use handlers::{handle_check, handle_index};
pub use hashing::{fill_hashes, hash_equals, hash_file};
pub use parallel::{parallel_walk_handler, setup_writer_pool_size};
pub use tools::{mtime_changed, path_relative_to, running_as_root};
