//! File descriptor limit detection for capping concurrent operations (Unix).

/// Estimated number of file descriptors used per walk worker (dir handles, files, etc.).
pub const FDS_PER_WORKER: usize = 10;

/// Fraction of the process FD limit to use (leave headroom for other code).
const FD_LIMIT_FRACTION: f64 = 0.8;

/// Returns the soft limit for max open file descriptors, or `None` if unavailable (e.g. Windows).
/// Used to cap walk parallelism so we don't hit EMFILE.
#[cfg(unix)]
pub fn max_open_fds() -> Option<u64> {
    use std::mem::MaybeUninit;
    let mut rlim = MaybeUninit::<libc::rlimit>::uninit();
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, rlim.as_mut_ptr()) } != 0 {
        return None;
    }
    let rlim = unsafe { rlim.assume_init() };
    let cur = rlim.rlim_cur;
    // RLIM_INFINITY is typically !0 or u64::MAX; treat as "no practical limit"
    if cur == libc::RLIM_INFINITY || cur > i64::MAX as u64 {
        return None;
    }
    Some(cur)
}

#[cfg(not(unix))]
pub fn max_open_fds() -> Option<u64> {
    None
}

/// Suggested max parallelism (thread count) so we stay under ~80% of the FD limit.
/// Returns `None` if no limit is available (use caller's default).
pub fn max_workers_by_fd_limit() -> Option<usize> {
    let limit = max_open_fds()?;
    let usable = (limit as f64 * FD_LIMIT_FRACTION) as usize;
    if usable < FDS_PER_WORKER {
        return Some(1);
    }
    Some(usable / FDS_PER_WORKER)
}
