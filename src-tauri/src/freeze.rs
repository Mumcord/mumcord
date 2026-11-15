//! The freeze detector: notices when the main (UI) thread stops running and has a report
//! written that shows where every thread stands, so a hang can be found afterwards. Tauri's
//! commands and the window's events all run on the main thread; when it blocks, the window is
//! frozen and, once the app is killed, nothing else tells where it hung.
//!
//! A thread posts a no-op to the main loop every [`TICK`] and waits for it to run. If one has
//! not run for [`THRESHOLD`], the engine writes a crash report of kind "freeze" with the stacks
//! of all threads (`mumcord_engine::crash::report_freeze`). Nothing is recovered or restarted:
//! if the main thread runs again, the app simply goes on.
//!
//! Linux only so far: reading the other threads' stacks is platform work in the engine. This is
//! not the plugin watchdog (`plugins::watchdog`), which pings the web page and restarts it.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tauri::AppHandle;

/// How often the main thread is asked. Each ping is one closure on the main loop.
const TICK: Duration = Duration::from_secs(2);
/// No answer for this long means the window is frozen. A hang-up may block the main thread for
/// up to 2 s, so this leaves a wide margin.
const THRESHOLD: Duration = Duration::from_secs(8);
/// Two ticks further apart than this mean the detector itself stood still (system suspend, a
/// stopped process, a starved machine), so the time in between says nothing about the main
/// thread. `Instant` does not count suspended time on Linux today; this guard does not rely
/// on it.
const STALL: Duration = Duration::from_secs(6);
/// Reports per run, so a window that freezes again and again does not push out other reports.
const MAX_REPORTS: u32 = 3;

/// What the detector does after a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The main thread answered the last ping: send ping number `n`.
    Ping(u64),
    /// A ping is still out: keep waiting, without queueing more on the main loop.
    Wait,
    /// The main thread has not answered for this long: write the report.
    Report(Duration),
}

/// The detector's decisions, free of threads and clocks so tests can drive them.
#[derive(Debug)]
struct Detector {
    /// The number of the last ping sent; 0 before the first.
    sent: u64,
    /// When the ping that is still out was sent, or when counting started anew.
    waiting_since: Instant,
    last_tick: Instant,
    /// The freeze that is going on was already reported.
    reported: bool,
    /// Reports written in this run.
    reports: u32,
}

impl Detector {
    fn new(now: Instant) -> Self {
        Self {
            sent: 0,
            waiting_since: now,
            last_tick: now,
            reported: false,
            reports: 0,
        }
    }

    /// `answered` is the number of the last ping the main thread ran.
    fn tick(&mut self, now: Instant, answered: u64) -> Step {
        let stalled = now.saturating_duration_since(self.last_tick) >= STALL;
        self.last_tick = now;
        if answered == self.sent {
            self.reported = false;
            self.sent += 1;
            self.waiting_since = now;
            return Step::Ping(self.sent);
        }
        if stalled {
            // The main thread may have stood still with the detector: count from here.
            self.waiting_since = now;
            return Step::Wait;
        }
        let waited = now.saturating_duration_since(self.waiting_since);
        if waited >= THRESHOLD && !self.reported && self.reports < MAX_REPORTS {
            self.reported = true;
            self.reports += 1;
            return Step::Report(waited);
        }
        Step::Wait
    }
}

/// Starts the detector thread. Without it the app runs normally, only a freeze leaves no trace.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("freeze-detector".into())
        .spawn(move || run(&app));
    if let Err(error) = spawned {
        tracing::error!(%error, "the freeze detector could not start");
    }
}

fn run(app: &AppHandle) {
    let answered = Arc::new(AtomicU64::new(0));
    if let Some(block) = test_hook_block_main_thread() {
        // Let the window come up first, then freeze it once.
        std::thread::sleep(Duration::from_secs(5));
        let _ = app.run_on_main_thread(move || std::thread::sleep(block));
    }
    let mut detector = Detector::new(Instant::now());
    loop {
        std::thread::sleep(TICK);
        match detector.tick(Instant::now(), answered.load(Ordering::Acquire)) {
            Step::Ping(ping) => {
                let answered = Arc::clone(&answered);
                // Only queues the closure; it never waits for the main thread.
                let posted =
                    app.run_on_main_thread(move || answered.store(ping, Ordering::Release));
                if posted.is_err() {
                    // The main loop is gone: the app is closing.
                    return;
                }
            }
            Step::Wait => {}
            Step::Report(waited) => report(waited),
        }
    }
}

fn report(waited: Duration) {
    match mumcord_engine::crash::report_freeze(waited) {
        Ok(_) => tracing::warn!(
            seconds = waited.as_secs(),
            "the window stopped answering; a freeze report was written"
        ),
        Err(error) => tracing::error!(
            %error,
            seconds = waited.as_secs(),
            "the window stopped answering, but the freeze report could not be written"
        ),
    }
}

/// Debug builds only: `MUMCORD_TEST_MAIN_FREEZE=<seconds>` blocks the main thread for that long,
/// once, 5 s after the start, so the detector can be checked in the running client: with `12`
/// the window freezes, one report of kind "freeze" appears in `crash-reports/` (and under
/// Settings → Advanced & Developer), and the window answers again afterwards.
#[cfg(debug_assertions)]
fn test_hook_block_main_thread() -> Option<Duration> {
    let seconds = std::env::var("MUMCORD_TEST_MAIN_FREEZE").ok()?;
    seconds.trim().parse().ok().map(Duration::from_secs)
}

#[cfg(not(debug_assertions))]
fn test_hook_block_main_thread() -> Option<Duration> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ticks every [`TICK`] from `now` on while the main thread stays silent, until a step
    /// other than `Wait` comes or `limit` has passed.
    fn silent_until(detector: &mut Detector, now: &mut Instant, limit: Duration) -> Step {
        let answered = detector.sent - 1;
        let end = *now + limit;
        while *now < end {
            *now += TICK;
            let step = detector.tick(*now, answered);
            if step != Step::Wait {
                return step;
            }
        }
        Step::Wait
    }

    #[test]
    fn an_answering_main_thread_is_pinged_every_tick_and_never_reported() {
        let mut now = Instant::now();
        let mut detector = Detector::new(now);
        for ping in 1..=100 {
            now += TICK;
            assert_eq!(detector.tick(now, ping - 1), Step::Ping(ping));
        }
    }

    #[test]
    fn a_slow_hang_up_is_not_a_freeze() {
        let mut now = Instant::now();
        let mut detector = Detector::new(now);
        now += TICK;
        assert_eq!(detector.tick(now, 0), Step::Ping(1));
        // The main thread is busy for 6 s, then answers.
        assert_eq!(
            silent_until(&mut detector, &mut now, Duration::from_secs(6)),
            Step::Wait
        );
        now += TICK;
        assert_eq!(detector.tick(now, 1), Step::Ping(2));
    }

    #[test]
    fn a_silent_main_thread_is_reported_once_after_the_threshold() {
        let mut now = Instant::now();
        let mut detector = Detector::new(now);
        now += TICK;
        assert_eq!(detector.tick(now, 0), Step::Ping(1));
        let sent = now;
        assert_eq!(
            silent_until(&mut detector, &mut now, Duration::from_secs(60)),
            Step::Report(THRESHOLD)
        );
        assert_eq!(now - sent, THRESHOLD);
        // It stays frozen: no second report and no pings piling up on the main loop.
        assert_eq!(
            silent_until(&mut detector, &mut now, Duration::from_secs(3600)),
            Step::Wait
        );
    }

    #[test]
    fn a_new_freeze_after_a_recovery_is_reported_until_the_cap() {
        let mut now = Instant::now();
        let mut detector = Detector::new(now);
        for round in 1..=5 {
            // The main thread answers again, then freezes.
            now += TICK;
            assert_eq!(detector.tick(now, detector.sent), Step::Ping(round));
            let step = silent_until(&mut detector, &mut now, Duration::from_secs(60));
            if u32::try_from(round).is_ok_and(|round| round <= MAX_REPORTS) {
                assert_eq!(step, Step::Report(THRESHOLD), "freeze {round}");
            } else {
                assert_eq!(step, Step::Wait, "freeze {round}");
            }
        }
    }

    #[test]
    fn time_the_detector_did_not_run_does_not_count() {
        let mut now = Instant::now();
        let mut detector = Detector::new(now);
        now += TICK;
        assert_eq!(detector.tick(now, 0), Step::Ping(1));
        // The lid closes right after the ping and opens an hour later, on a clock that counts
        // the sleep: no freeze.
        now += Duration::from_secs(3600);
        assert_eq!(detector.tick(now, 0), Step::Wait);
        // The main thread wakes up and answers.
        now += TICK;
        assert_eq!(detector.tick(now, 1), Step::Ping(2));
        // Another gap, but this time the main thread stays silent after it: the full threshold
        // counts from the wake-up.
        now += Duration::from_secs(3600);
        assert_eq!(detector.tick(now, 1), Step::Wait);
        let woke = now;
        assert_eq!(
            silent_until(&mut detector, &mut now, Duration::from_secs(60)),
            Step::Report(THRESHOLD)
        );
        assert_eq!(now - woke, THRESHOLD);
    }
}
