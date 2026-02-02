//! Shared helpers for parallel processing.

use log::debug;

pub fn parallel_walk_handler(parallel_walk: bool) {
    if parallel_walk {
        debug!("Walking in parallel");
    } else {
        debug!("Walking serially");
    }
}
