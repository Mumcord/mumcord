//! The soft `RLIMIT_RTTIME` is brought back to half the hard one after something else in the
//! process lowers it, as the audio host does whenever it promotes its own device threads.
//!
//! In its own test binary: the limit is process-wide, and `rt_overrun` depends on it too.
#![cfg(target_os = "linux")]

/// rtkit's default hard limit, used when the process has none yet.
const HARD_US: libc::rlim_t = 200_000;

fn rttime() -> libc::rlimit {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit only writes `limit`.
    let result = unsafe { libc::getrlimit(libc::RLIMIT_RTTIME, &raw mut limit) };
    assert_eq!(result, 0, "reading RLIMIT_RTTIME");
    limit
}

#[test]
fn lowered_soft_limit_is_widened_again() {
    // An unprivileged process may lower the hard limit but not raise it, so keep an existing one.
    let hard = match rttime().rlim_max {
        libc::RLIM_INFINITY => HARD_US,
        max => max,
    };
    // What the audio host leaves behind: one device buffer, far below half the hard limit.
    let lowered = libc::rlimit {
        rlim_cur: hard / 10,
        rlim_max: hard,
    };
    // SAFETY: setrlimit only reads `lowered`.
    let result = unsafe { libc::setrlimit(libc::RLIMIT_RTTIME, &raw const lowered) };
    assert_eq!(result, 0, "lowering RLIMIT_RTTIME");

    mumcord_audio::rt::keep_overrun_headroom();

    let limit = rttime();
    assert_eq!(limit.rlim_max, hard, "hard limit is left alone");
    assert_eq!(limit.rlim_cur, hard / 2, "soft limit is back at half");

    // Already wide: nothing changes.
    mumcord_audio::rt::keep_overrun_headroom();
    assert_eq!(rttime().rlim_cur, hard / 2);
}
