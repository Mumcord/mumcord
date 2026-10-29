//! A voice thread at real-time priority that runs past its time budget is returned to normal
//! priority instead of the kernel ending the app with SIGXCPU (issue #139).
//!
//! Needs real-time scheduling: rtkit (desktop Linux), or else a non-zero `RLIMIT_RTPRIO`
//! (`ulimit -r`) for a direct SCHED_FIFO request; skipped when neither is available.
#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

/// Longer than rtkit's default hard limit (200 ms), past which the kernel sends SIGKILL.
const OVERRUN: Duration = Duration::from_millis(400);

/// The calling thread's scheduling policy, without the reset-on-fork flag rtkit adds.
fn policy() -> libc::c_int {
    // SAFETY: queries the calling thread's policy; no pointers involved.
    unsafe { libc::sched_getscheduler(0) & !libc::SCHED_RESET_ON_FORK }
}

fn is_realtime() -> bool {
    matches!(policy(), libc::SCHED_FIFO | libc::SCHED_RR)
}

/// Real-time scheduling without rtkit, with limits like rtkit's (soft below hard).
fn promote_directly() -> bool {
    let limit = libc::rlimit {
        rlim_cur: 50_000,
        rlim_max: 200_000,
    };
    let param = libc::sched_param { sched_priority: 1 };
    // SAFETY: both calls only read the structs passed by reference.
    unsafe {
        libc::setrlimit(libc::RLIMIT_RTTIME, &limit) == 0
            && libc::sched_setscheduler(0, libc::SCHED_FIFO, &param) == 0
    }
}

#[test]
fn overrunning_realtime_thread_is_demoted_not_killed() {
    let ran = std::thread::spawn(|| {
        let guard = mumcord_audio::rt::promote_current_thread();
        if guard.is_err() && !promote_directly() {
            return false;
        }
        assert!(is_realtime(), "thread runs at real-time priority");

        // A DSP stage that never blocks, like noise suppression in an unoptimised build.
        let start = Instant::now();
        let mut spin = 0u64;
        while start.elapsed() < OVERRUN {
            spin = std::hint::black_box(spin.wrapping_add(1));
        }

        assert_eq!(policy(), libc::SCHED_OTHER, "overrunning thread is demoted");
        drop(guard);
        true
    })
    .join()
    .expect("thread");

    if ran {
        assert_eq!(mumcord_audio::rt::take_demotions(), 1);
    } else {
        eprintln!("skipped: real-time scheduling unavailable (no rtkit, RLIMIT_RTPRIO is 0)");
    }
}
