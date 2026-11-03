//! Live load monitoring of the share's encoder (FR-7.57): the encode thread reports every frame,
//! and once the encoder has been behind for [`BEHIND_FOR`] in a row the share falls back.

use std::time::{Duration, Instant};

/// How long the encoder must stay behind before the share falls back (FR-7.57).
pub(crate) const BEHIND_FOR: Duration = Duration::from_secs(3);

/// The numbers behind a load fallback, for the "Details" of the notice (mockup 07).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LoadDetails {
    /// Average encode time per frame while the encoder was behind.
    pub encode_ms: f64,
    /// The time one frame may take at the share's frame rate.
    pub budget_ms: f64,
    /// Frames the share skipped while the encoder was behind.
    pub dropped: u32,
}

/// One frame as the encode thread saw it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FrameLoad {
    /// How long the encoder took for the frame.
    pub encode: Duration,
    /// The time one frame may take at the current frame rate.
    pub budget: Duration,
    /// The smoothed encode time is over what leaves room for capture and sending.
    pub busy: bool,
    /// The share skipped frames before this one because it was late.
    pub dropped: u32,
}

#[derive(Debug, Default)]
pub(crate) struct LoadMonitor {
    behind: Option<Behind>,
}

#[derive(Debug)]
struct Behind {
    since: Instant,
    encode: Duration,
    frames: u32,
    budget: Duration,
    dropped: u32,
}

impl LoadMonitor {
    /// Counts one frame; returns the details once the encoder has been behind for
    /// [`BEHIND_FOR`], and starts over after that.
    pub fn frame(&mut self, frame: FrameLoad, now: Instant) -> Option<LoadDetails> {
        if !frame.busy && frame.dropped == 0 {
            self.behind = None;
            return None;
        }
        let behind = self.behind.get_or_insert(Behind {
            since: now,
            encode: Duration::ZERO,
            frames: 0,
            budget: frame.budget,
            dropped: 0,
        });
        behind.encode += frame.encode;
        behind.frames += 1;
        behind.budget = frame.budget;
        behind.dropped = behind.dropped.saturating_add(frame.dropped);
        if now.duration_since(behind.since) < BEHIND_FOR {
            return None;
        }
        let details = LoadDetails {
            encode_ms: behind.encode.as_secs_f64() * 1000.0 / f64::from(behind.frames),
            budget_ms: behind.budget.as_secs_f64() * 1000.0,
            dropped: behind.dropped,
        };
        self.behind = None;
        Some(details)
    }

    /// Forgets a running count, e.g. after the encoder changed.
    pub fn reset(&mut self) {
        self.behind = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUDGET: Duration = Duration::from_millis(33);

    fn load(encode_ms: u64, busy: bool, dropped: u32) -> FrameLoad {
        FrameLoad {
            encode: Duration::from_millis(encode_ms),
            budget: BUDGET,
            busy,
            dropped,
        }
    }

    /// Feeds `frame` every 33 ms from `from` for `seconds`; the first fallback and when.
    fn run(
        monitor: &mut LoadMonitor,
        from: Instant,
        seconds: u64,
        frame: impl Fn(u32) -> FrameLoad,
    ) -> Option<(Duration, LoadDetails)> {
        let frames = u32::try_from(seconds * 30).unwrap_or_default();
        (0..frames).find_map(|n| {
            let at = BUDGET * n;
            monitor.frame(frame(n), from + at).map(|d| (at, d))
        })
    }

    #[test]
    fn falls_back_after_three_seconds_behind() {
        let mut monitor = LoadMonitor::default();
        let start = Instant::now();
        let (at, details) = run(&mut monitor, start, 5, |_| load(40, true, 0)).expect("fallback");
        assert!(
            at >= BEHIND_FOR && at < BEHIND_FOR + BUDGET * 2,
            "at {at:?}"
        );
        assert!((details.encode_ms - 40.0).abs() < 0.01);
        assert!((details.budget_ms - 33.0).abs() < 0.01);
        assert_eq!(details.dropped, 0);
    }

    #[test]
    fn dropped_frames_count_as_behind() {
        let mut monitor = LoadMonitor::default();
        let (_, details) =
            run(&mut monitor, Instant::now(), 5, |_| load(20, false, 1)).expect("fallback");
        assert!(details.dropped >= 90);
    }

    #[test]
    fn one_good_frame_starts_the_count_over() {
        let mut monitor = LoadMonitor::default();
        // Every second a frame keeps up: never three seconds in a row.
        let fallback = run(&mut monitor, Instant::now(), 10, |n| {
            load(40, n % 30 != 29, 0)
        });
        assert_eq!(fallback, None);
    }

    #[test]
    fn reset_forgets_the_count() {
        let mut monitor = LoadMonitor::default();
        let start = Instant::now();
        assert_eq!(monitor.frame(load(40, true, 0), start), None);
        monitor.reset();
        assert_eq!(monitor.frame(load(40, true, 0), start + BEHIND_FOR), None);
        assert!(
            monitor
                .frame(load(40, true, 0), start + BEHIND_FOR * 2)
                .is_some()
        );
    }
}
