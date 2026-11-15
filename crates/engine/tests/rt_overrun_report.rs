//! A real-time thread that runs past its time budget leaves a report saying where it was busy,
//! and the app keeps running.
//!
//! Needs real-time scheduling: rtkit (desktop Linux), or else a non-zero `RLIMIT_RTPRIO`
//! (`ulimit -r`) for a direct SCHED_FIFO request; skipped when neither is available.
#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

use mumcord_engine::crash::{self, CrashKind};

/// Longer than rtkit's default hard limit (200 ms), past which the kernel sends SIGKILL.
const OVERRUN: Duration = Duration::from_millis(400);

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

/// A DSP stage that never blocks; its name must show up in the report.
#[inline(never)]
fn stuck_dsp_stage() {
    let start = Instant::now();
    let mut spin = 0u64;
    while start.elapsed() < OVERRUN {
        spin = std::hint::black_box(spin.wrapping_add(1));
    }
}

#[test]
fn an_overrun_is_reported_with_the_stack_of_the_overrunning_thread() {
    let root = tempfile::tempdir().expect("temp dir");
    let dir = root.path().join("crash-reports");
    crash::install(&dir, "0.0.0-test").expect("crash reporting");

    let ran = std::thread::Builder::new()
        .name("stuck-voice".to_owned())
        .spawn(|| {
            let guard = mumcord_audio::rt::promote_current_thread();
            if guard.is_err() && !promote_directly() {
                return false;
            }
            stuck_dsp_stage();
            drop(guard);
            true
        })
        .expect("spawn")
        .join()
        .expect("thread");
    if !ran {
        eprintln!("skipped: real-time scheduling unavailable (no rtkit, RLIMIT_RTPRIO is 0)");
        return;
    }
    assert_eq!(mumcord_audio::rt::take_demotions(), 1, "still demoted");

    // What the voice poll does; the report is written on its own thread.
    crash::report_overrun();
    let deadline = Instant::now() + Duration::from_secs(20);
    while crash::list(&dir).is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    // A second poll finds nothing new: one report per run.
    crash::report_overrun();
    std::thread::sleep(Duration::from_millis(200));

    let reports = crash::list(&dir);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].kind, CrashKind::Overrun);
    assert!(reports[0].cause.contains("kept running"), "{reports:?}");
    let text = crash::read(&dir, &reports[0].id).expect("report");
    assert!(text.contains("kind: real-time overrun\n"), "{text}");
    assert!(text.contains("thread: stuck-voice\n"), "{text}");
    let stack = text.split("backtrace:\n").nth(1).expect("a backtrace");
    assert!(stack.contains("stuck_dsp_stage"), "{text}");
    assert!(crash::take_unseen(&dir).is_empty(), "no crash notice");
}
