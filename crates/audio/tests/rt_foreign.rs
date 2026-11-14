//! A real-time thread that is not one of Mumcord's must not get the whole app killed (issue
//! #844). WebKitGTK up to 2.52 makes some of its threads SCHED_RR inside Mumcord's process,
//! where they fall under the `RLIMIT_RTTIME` that the audio promotion sets for the whole
//! process. WebKit's own SIGXCPU handler only wakes its main loop, so a thread that keeps
//! running reaches the hard limit and the kernel ends the process with SIGKILL.
//!
//! In its own test binary, and each case in a child process: the limit and the signal handler
//! are process-wide, and the failure being tested is the death of the process.
//!
//! Needs a non-zero `RLIMIT_RTPRIO` (`ulimit -r`) for a direct SCHED_RR request, as WebKit makes
//! it; skipped otherwise.
#![cfg(target_os = "linux")]

use std::os::unix::process::ExitStatusExt;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

/// Set in the child process to the case it runs.
const CHILD: &str = "MUMCORD_RT_FOREIGN_CHILD";
/// Child exit code: real-time scheduling is not available to this user.
const SKIPPED: i32 = 77;
/// Child exit code: the foreign thread was still real-time at its end.
const STILL_REALTIME: i32 = 3;

/// The limits rtkit and the audio host leave on Mumcord's process: one device buffer soft,
/// 200 ms hard.
const SOFT_US: libc::rlim_t = 20_000;
const HARD_US: libc::rlim_t = 200_000;
/// Longer than the hard limit, so a thread that stays real-time gets the process killed.
const BUSY: Duration = Duration::from_millis(400);
/// Longer than the pause between two sweeps (about a second), with room for a loaded machine.
const IDLE: Duration = Duration::from_millis(2500);

/// What WebKit's handler writes to; nobody reads it here, as if its main loop never got to run.
static WAKE_FD: AtomicI32 = AtomicI32::new(-1);

/// Like WebKit's handler: it only asks another thread to act and leaves the overrunning thread
/// real-time.
extern "C" fn webkit_style_handler(_signal: libc::c_int) {
    let one = 1u64;
    // SAFETY: write is async-signal-safe; `one` outlives the call.
    unsafe {
        libc::write(WAKE_FD.load(Ordering::Relaxed), (&raw const one).cast(), 8);
    }
}

fn install_webkit_style_handler() {
    // SAFETY: eventfd takes no pointers; a zeroed `sigaction` is valid and only read.
    unsafe {
        WAKE_FD.store(libc::eventfd(0, libc::EFD_NONBLOCK), Ordering::Relaxed);
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction =
            webkit_style_handler as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::sigemptyset(&raw mut action.sa_mask);
        assert_eq!(
            libc::sigaction(libc::SIGXCPU, &raw const action, std::ptr::null_mut()),
            0,
            "installing the WebKit-style handler"
        );
    }
}

fn limit_realtime_cpu_time() {
    let limit = libc::rlimit {
        rlim_cur: SOFT_US,
        rlim_max: HARD_US,
    };
    // SAFETY: setrlimit only reads `limit`.
    let result = unsafe { libc::setrlimit(libc::RLIMIT_RTTIME, &raw const limit) };
    assert_eq!(result, 0, "setting RLIMIT_RTTIME");
}

/// Makes the calling thread real-time the way WebKit does: SCHED_RR priority 5, set directly.
fn become_realtime_like_webkit() -> bool {
    let param = libc::sched_param { sched_priority: 5 };
    // SAFETY: only reads `param`; pid 0 is the calling thread.
    unsafe { libc::sched_setscheduler(0, libc::SCHED_RR, &raw const param) == 0 }
}

/// The calling thread's scheduling policy, without the reset-on-fork flag.
fn policy() -> libc::c_int {
    // SAFETY: queries the calling thread's policy; no pointers involved.
    unsafe { libc::sched_getscheduler(0) & !libc::SCHED_RESET_ON_FORK }
}

/// The child process: what Mumcord's main process looks like to the kernel, with a foreign
/// real-time thread that idles for `idle` and then never blocks for [`BUSY`].
fn child(idle: Duration) -> ! {
    limit_realtime_cpu_time();
    install_webkit_style_handler();
    // What the app does to protect itself.
    mumcord_audio::rt::keep_overrun_headroom();

    let policy = std::thread::Builder::new()
        .name("ReceiveQueue".to_owned())
        .spawn(move || {
            if !become_realtime_like_webkit() {
                return None;
            }
            std::thread::sleep(idle);
            let start = Instant::now();
            let mut spin = 0u64;
            while start.elapsed() < BUSY {
                spin = std::hint::black_box(spin.wrapping_add(1));
            }
            Some(policy())
        })
        .expect("spawn")
        .join()
        .expect("thread");

    std::process::exit(match policy {
        None => SKIPPED,
        Some(libc::SCHED_OTHER) => 0,
        Some(_) => STILL_REALTIME,
    });
}

/// Runs `test` again in a child process as case `case` and checks that it survived.
fn survives(test: &str, case: &str) {
    let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([test, "--exact", "--nocapture"])
        .env(CHILD, case)
        .status()
        .expect("child process");
    assert_ne!(
        status.signal(),
        Some(libc::SIGKILL),
        "the kernel killed the process at the hard real-time limit"
    );
    match status.code() {
        Some(0) => {}
        Some(SKIPPED) => eprintln!("skipped: real-time scheduling unavailable (RLIMIT_RTPRIO)"),
        Some(STILL_REALTIME) => panic!("the foreign thread was still real-time at its end"),
        _ => panic!("child process failed: {status}"),
    }
}

/// The usual case: the foreign thread has been real-time for a while before it gets busy, and
/// is back at normal priority by then.
#[test]
fn idle_foreign_realtime_thread_is_demoted_before_it_gets_busy() {
    if std::env::var_os(CHILD).is_some() {
        child(IDLE);
    }
    survives(
        "idle_foreign_realtime_thread_is_demoted_before_it_gets_busy",
        "idle",
    );
}

/// The exposed case: the foreign thread gets busy right after it became real-time, before any
/// sweep can see it. Mumcord's SIGXCPU handler has to demote it, although another handler was
/// installed.
#[test]
fn busy_foreign_realtime_thread_is_demoted_at_the_soft_limit() {
    if std::env::var_os(CHILD).is_some() {
        child(Duration::ZERO);
    }
    survives(
        "busy_foreign_realtime_thread_is_demoted_at_the_soft_limit",
        "busy",
    );
}
