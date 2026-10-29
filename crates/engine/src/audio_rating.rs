//! Overall audio rating from the health counters (issue #152).
//!
//! The raw counters run since voice started, so one old hiccup would keep a rating on totals
//! "poor" forever. [`HealthHistory`] keeps a snapshot of the totals every
//! [`BUCKET_SECS`] seconds; the rating looks at what changed over the last [`WINDOW_SECS`]
//! seconds, as rates per minute (or as a share of received voice), and names the most likely
//! cause so the user knows where to look: the network, the CPU or the audio device.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How far back the rating looks.
pub const WINDOW_SECS: u64 = 120;
/// Spacing of the history snapshots; also one point of the sparklines.
pub const BUCKET_SECS: u64 = 10;
/// Snapshots in one window (and points per sparkline).
pub const BUCKETS: usize = (WINDOW_SECS / BUCKET_SECS) as usize;

// Thresholds of the bands, all in one place. A value at or above the "fair" threshold makes the
// rating at least Fair, at or above the "poor" one Poor. Tuned so that a healthy session reads
// Good: healthy systems show no dropouts, an occasional late frame and well under 1 % concealment.

/// Rates are taken over at least a minute, so a single glitch right after connecting reads as
/// one per minute rather than as a burst.
const MIN_RATE_SPAN: Duration = Duration::from_secs(60);
/// Device: dropouts (xruns, stream restarts, over- and underruns) per minute. Each one is an
/// audible click or gap, so a single one a minute is already noticeable.
const DROPOUTS_FAIR_PER_MIN: f64 = 1.0;
const DROPOUTS_POOR_PER_MIN: f64 = 6.0;
/// CPU: frames handled late per minute (the capture or playback thread was not scheduled in
/// time). A few a minute go unnoticed; many mean the system is overloaded.
const LATE_FAIR_PER_MIN: f64 = 5.0;
const LATE_POOR_PER_MIN: f64 = 30.0;
/// CPU: worst processing time of one 10 ms frame. Above one frame (10 ms) the thread falls
/// behind and lives on the device buffer; above one 20 ms packet audio is cut.
const PROCESSING_FAIR_US: u64 = 10_000;
const PROCESSING_POOR_US: u64 = 20_000;
/// Network: share of received voice frames that had to be rebuilt by concealment, in percent.
/// Opus hides about 1 % well; at 5 % voices sound choppy.
const CONCEALED_FAIR_PERCENT: f64 = 1.0;
const CONCEALED_POOR_PERCENT: f64 = 5.0;
/// Below this much received voice (3 s) the concealed share is too noisy to rate.
const MIN_RECEIVED_FRAMES: u64 = 300;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum AudioRating {
    #[default]
    Good,
    Fair,
    Poor,
}

/// The most likely cause of a Fair or Poor rating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AudioIssue {
    /// Voice from others was concealed: packets lost or too late on the way.
    Network,
    /// The local audio threads ran late or processing took too long: high CPU load.
    Cpu,
    /// The audio device reported dropouts.
    Device,
}

/// Cumulative counters since voice started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HealthTotals {
    pub dropouts: u64,
    pub late_frames: u64,
    pub concealed_frames: u64,
    pub received_frames: u64,
}

/// The rating over the recent window, with the window's counts behind it and a per-bucket
/// history (oldest first) for sparklines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct AudioRatingView {
    pub rating: AudioRating,
    /// `None` while the rating is Good.
    pub issue: Option<AudioIssue>,
    /// Length of the window the counts below cover, in seconds (shorter right after connecting).
    pub window_secs: u32,
    pub dropouts: u64,
    pub late_frames: u64,
    pub concealed_frames: u64,
    pub received_frames: u64,
    /// Worst processing time of one 10 ms frame in the window (capture or mixing), in µs.
    pub processing_max_us: u64,
    pub dropouts_history: [u32; BUCKETS],
    pub late_frames_history: [u32; BUCKETS],
    pub concealed_history: [u32; BUCKETS],
    pub processing_max_history_us: [u32; BUCKETS],
}

#[derive(Clone, Copy)]
struct Snapshot {
    at: Instant,
    totals: HealthTotals,
    /// Worst frame time during the bucket that ends with this snapshot.
    processing_max_us: u64,
}

/// Snapshots of the totals over the last window.
pub struct HealthHistory {
    snapshots: VecDeque<Snapshot>,
    /// Worst frame time since the last snapshot.
    pending_max_us: u64,
}

impl HealthHistory {
    /// Starts at zero totals, as the counters do when voice starts.
    pub fn new(now: Instant) -> Self {
        let mut snapshots = VecDeque::with_capacity(BUCKETS + 1);
        snapshots.push_back(Snapshot {
            at: now,
            totals: HealthTotals::default(),
            processing_max_us: 0,
        });
        Self {
            snapshots,
            pending_max_us: 0,
        }
    }

    /// Records the current totals and the worst frame time since the previous call; called
    /// about once a second. Keeps a snapshot every [`BUCKET_SECS`].
    pub fn sample(&mut self, now: Instant, totals: HealthTotals, processing_max_us: u64) {
        self.pending_max_us = self.pending_max_us.max(processing_max_us);
        let last = self.snapshots.back().map_or(now, |s| s.at);
        if now.saturating_duration_since(last) < Duration::from_secs(BUCKET_SECS) {
            return;
        }
        if self.snapshots.len() > BUCKETS {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(Snapshot {
            at: now,
            totals,
            processing_max_us: std::mem::take(&mut self.pending_max_us),
        });
    }

    /// The rating over the window ending `now` at the current `totals`.
    pub fn rate(&self, now: Instant, totals: HealthTotals) -> AudioRatingView {
        let Some(oldest) = self.snapshots.front() else {
            return AudioRatingView::default();
        };
        let span = now.saturating_duration_since(oldest.at);
        let delta = |f: fn(&HealthTotals) -> u64| f(&totals).saturating_sub(f(&oldest.totals));
        let processing_max_us = self
            .snapshots
            .iter()
            .skip(1)
            .map(|s| s.processing_max_us)
            .fold(self.pending_max_us, u64::max);
        let mut view = AudioRatingView {
            window_secs: u32::try_from(span.as_secs()).unwrap_or(u32::MAX),
            dropouts: delta(|t| t.dropouts),
            late_frames: delta(|t| t.late_frames),
            concealed_frames: delta(|t| t.concealed_frames),
            received_frames: delta(|t| t.received_frames),
            processing_max_us,
            ..AudioRatingView::default()
        };
        // Sparklines end with the latest full bucket; earlier slots stay 0 right after connecting.
        let offset = BUCKETS + 1 - self.snapshots.len();
        let count = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
        let pairs = self.snapshots.iter().zip(self.snapshots.iter().skip(1));
        for (slot, (older, newer)) in (offset..).zip(pairs) {
            let (a, b) = (older.totals, newer.totals);
            view.dropouts_history[slot] = count(b.dropouts.saturating_sub(a.dropouts));
            view.late_frames_history[slot] = count(b.late_frames.saturating_sub(a.late_frames));
            view.concealed_history[slot] =
                count(b.concealed_frames.saturating_sub(a.concealed_frames));
            view.processing_max_history_us[slot] = count(newer.processing_max_us);
        }
        (view.rating, view.issue) = judge(&view, span);
        view
    }
}

/// The band of each cause and the overall rating: the worst band wins. On a tie the local
/// causes go first (device, then CPU), since they also explain symptoms elsewhere.
fn judge(view: &AudioRatingView, span: Duration) -> (AudioRating, Option<AudioIssue>) {
    let minutes = span.max(MIN_RATE_SPAN).as_secs_f64() / 60.0;
    let band = |value: f64, fair: f64, poor: f64| {
        if value >= poor {
            AudioRating::Poor
        } else if value >= fair {
            AudioRating::Fair
        } else {
            AudioRating::Good
        }
    };
    let device = band(
        view.dropouts as f64 / minutes,
        DROPOUTS_FAIR_PER_MIN,
        DROPOUTS_POOR_PER_MIN,
    );
    let cpu = band(
        view.late_frames as f64 / minutes,
        LATE_FAIR_PER_MIN,
        LATE_POOR_PER_MIN,
    )
    .max(band(
        view.processing_max_us as f64,
        PROCESSING_FAIR_US as f64,
        PROCESSING_POOR_US as f64,
    ));
    let network = if view.received_frames >= MIN_RECEIVED_FRAMES {
        let total = view.received_frames + view.concealed_frames;
        band(
            view.concealed_frames as f64 * 100.0 / total as f64,
            CONCEALED_FAIR_PERCENT,
            CONCEALED_POOR_PERCENT,
        )
    } else {
        AudioRating::Good
    };
    [
        (device, AudioIssue::Device),
        (cpu, AudioIssue::Cpu),
        (network, AudioIssue::Network),
    ]
    .into_iter()
    .filter(|(rating, _)| *rating > AudioRating::Good)
    .reduce(|best, next| if next.0 > best.0 { next } else { best })
    .map_or((AudioRating::Good, None), |(rating, issue)| {
        (rating, Some(issue))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn totals(dropouts: u64, late: u64, concealed: u64, received: u64) -> HealthTotals {
        HealthTotals {
            dropouts,
            late_frames: late,
            concealed_frames: concealed,
            received_frames: received,
        }
    }

    /// Rates `totals` reached after `secs` of voice with no snapshots in between.
    fn rate_after(secs: u64, t: HealthTotals, processing_max_us: u64) -> AudioRatingView {
        let start = Instant::now();
        let mut history = HealthHistory::new(start);
        let now = start + Duration::from_secs(secs);
        history.sample(now, t, processing_max_us);
        history.rate(now, t)
    }

    fn verdict(view: &AudioRatingView) -> (AudioRating, Option<AudioIssue>) {
        (view.rating, view.issue)
    }

    #[test]
    fn silence_and_a_clean_session_are_good() {
        assert_eq!(
            verdict(&rate_after(120, HealthTotals::default(), 300)),
            (AudioRating::Good, None)
        );
        assert_eq!(
            verdict(&rate_after(120, totals(0, 2, 10, 6_000), 3_000)),
            (AudioRating::Good, None)
        );
    }

    #[test]
    fn device_dropouts_set_the_band() {
        // 2 minutes: 2 dropouts = 1/min (Fair), 12 = 6/min (Poor), 1 = 0.5/min (Good).
        assert_eq!(
            rate_after(120, totals(1, 0, 0, 0), 0).rating,
            AudioRating::Good
        );
        assert_eq!(
            verdict(&rate_after(120, totals(2, 0, 0, 0), 0)),
            (AudioRating::Fair, Some(AudioIssue::Device))
        );
        assert_eq!(
            verdict(&rate_after(120, totals(12, 0, 0, 0), 0)),
            (AudioRating::Poor, Some(AudioIssue::Device))
        );
    }

    #[test]
    fn late_frames_and_slow_processing_point_to_the_cpu() {
        assert_eq!(
            rate_after(120, totals(0, 9, 0, 0), 0).rating,
            AudioRating::Good
        );
        assert_eq!(
            verdict(&rate_after(120, totals(0, 10, 0, 0), 0)),
            (AudioRating::Fair, Some(AudioIssue::Cpu))
        );
        assert_eq!(
            verdict(&rate_after(120, totals(0, 60, 0, 0), 0)),
            (AudioRating::Poor, Some(AudioIssue::Cpu))
        );
        assert_eq!(
            verdict(&rate_after(120, HealthTotals::default(), 12_000)),
            (AudioRating::Fair, Some(AudioIssue::Cpu))
        );
        assert_eq!(
            verdict(&rate_after(120, HealthTotals::default(), 25_000)),
            (AudioRating::Poor, Some(AudioIssue::Cpu))
        );
    }

    #[test]
    fn concealed_share_points_to_the_network() {
        // 0.5 % Good, 2 % Fair, 6 % Poor.
        assert_eq!(
            rate_after(120, totals(0, 0, 5, 995), 0).rating,
            AudioRating::Good
        );
        assert_eq!(
            verdict(&rate_after(120, totals(0, 0, 20, 980), 0)),
            (AudioRating::Fair, Some(AudioIssue::Network))
        );
        assert_eq!(
            verdict(&rate_after(120, totals(0, 0, 60, 940), 0)),
            (AudioRating::Poor, Some(AudioIssue::Network))
        );
        // Too little voice to judge: a single lost packet of a short word.
        assert_eq!(
            rate_after(120, totals(0, 0, 4, 40), 0).rating,
            AudioRating::Good
        );
    }

    #[test]
    fn the_worst_cause_is_named_and_local_causes_win_ties() {
        assert_eq!(
            verdict(&rate_after(120, totals(2, 0, 60, 940), 0)),
            (AudioRating::Poor, Some(AudioIssue::Network)),
            "network is worse than the device"
        );
        assert_eq!(
            verdict(&rate_after(120, totals(2, 10, 20, 980), 0)),
            (AudioRating::Fair, Some(AudioIssue::Device)),
            "on a tie the device goes first"
        );
        assert_eq!(
            verdict(&rate_after(120, totals(0, 10, 20, 980), 0)),
            (AudioRating::Fair, Some(AudioIssue::Cpu)),
            "then the CPU"
        );
    }

    #[test]
    fn one_glitch_right_after_connecting_is_not_a_burst() {
        // 1 dropout in 5 s would be 12/min; it counts over at least a minute.
        assert_eq!(
            rate_after(5, totals(1, 0, 0, 0), 0).issue,
            Some(AudioIssue::Device)
        );
        assert_eq!(
            rate_after(5, totals(1, 0, 0, 0), 0).rating,
            AudioRating::Fair
        );
    }

    #[test]
    fn old_problems_leave_the_window() {
        let start = Instant::now();
        let mut history = HealthHistory::new(start);
        let bad = totals(20, 0, 0, 0);
        let mut now = start;
        for _ in 0..10 {
            now += Duration::from_secs(1);
            history.sample(now, bad, 25_000);
        }
        assert_eq!(history.rate(now, bad).rating, AudioRating::Poor);
        // Three quiet minutes later the dropouts and the slow frame are out of the window.
        for _ in 0..180 {
            now += Duration::from_secs(1);
            history.sample(now, bad, 1_000);
        }
        let view = history.rate(now, bad);
        assert_eq!(verdict(&view), (AudioRating::Good, None));
        assert_eq!(view.dropouts, 0);
        assert_eq!(view.window_secs, WINDOW_SECS as u32);
        assert_eq!(view.processing_max_us, 1_000);
    }

    #[test]
    fn history_fills_from_the_right() {
        let start = Instant::now();
        let mut history = HealthHistory::new(start);
        let now = start + Duration::from_secs(BUCKET_SECS);
        history.sample(now, totals(3, 1, 7, 100), 4_000);
        let view = history.rate(now, totals(3, 1, 7, 100));
        assert_eq!(view.dropouts_history[BUCKETS - 1], 3);
        assert_eq!(view.concealed_history[BUCKETS - 1], 7);
        assert_eq!(view.processing_max_history_us[BUCKETS - 1], 4_000);
        assert_eq!(view.dropouts_history[..BUCKETS - 1], [0; BUCKETS - 1]);
    }
}
