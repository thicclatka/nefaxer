//! Engine module for core indexing operations

pub mod arg_parser;
pub mod cli;
pub mod db_ops;
pub mod hashing;
pub mod parallel;
pub mod progress;
pub mod tools;

// Re-export commonly used functions
pub use arg_parser::Cli;
pub use cli::handle_run;
pub use db_ops::*;
pub use hashing::*;
pub use parallel::*;
pub use progress::*;
pub use tools::*;
