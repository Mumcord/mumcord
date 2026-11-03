//! Quality steps of the own share (FR-7.37, FR-7.38): what the sharer picked is a ceiling, and
//! the share steps down below it when the upload or the encoder can't keep up. "Optimize for"
//! decides the order of the steps, the upload limit caps the total upload.
//!
//! Everything here is plain logic without I/O; the encode thread drives it once per frame.

use std::time::{Duration, Instant};

use mumcord_screenshare::Codec;
use mumcord_store::{ScreenFps, ScreenOptimize, ScreenResolution, ScreenUploadLimit, Settings};

/// One quality step, with the bitrate for the codec in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Quality {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

impl Quality {
    /// How the bar names the step, e.g. "1080p30".
    pub fn label(&self) -> String {
        format!("{}p{}", self.height, self.fps)
    }
}

/// Target bitrate of one resolution and frame rate.
struct Rate {
    height: u32,
    fps: u32,
    av1_kbps: u32,
    vp9_kbps: u32,
}

/// Target bitrates of every step a ladder can use. The FR-7.37 rows are as specified; the rows
/// marked "derived" are not in the spec and only appear on the "Sharp text" and "Smooth motion"
/// ladders, estimated from their neighbours. Tune here.
const RATES: [Rate; 12] = [
    rate(1440, 60, 9000, 12000),
    rate(1440, 30, 6000, 8000),
    rate(1440, 15, 3500, 4500), // derived
    rate(1080, 60, 6000, 8000),
    rate(1080, 30, 4000, 4000),
    rate(1080, 15, 2200, 2200), // derived
    rate(720, 60, 3000, 3500),  // derived
    rate(720, 30, 2000, 2000),
    rate(720, 15, 1200, 1200),
    rate(480, 60, 1500, 1500), // derived
    rate(480, 30, 1000, 1000), // derived
    rate(480, 15, 700, 700),
];

const fn rate(height: u32, fps: u32, av1_kbps: u32, vp9_kbps: u32) -> Rate {
    Rate {
        height,
        fps,
        av1_kbps,
        vp9_kbps,
    }
}

/// Resolutions best first, as (width, height) of a 16:9 picture; smaller sources are never
/// enlarged, and each step keeps the source's aspect ratio (`convert::fit`).
const SIZES: [(u32, u32); 4] = [(2560, 1440), (1920, 1080), (1280, 720), (854, 480)];
/// Frame rates best first.
const RATES_FPS: [u32; 3] = [60, 30, 15];

/// "Auto" resolution tops out at today's best step; 1440p only on a manual pick.
pub(super) const AUTO_HEIGHT: u32 = 1080;

/// Share of the upload limit the video may plan with; the rest is for sound and overhead.
const LIMIT_SHARE_PERCENT: u64 = 90;

/// Pressure has to last this long before quality goes down.
const DOWN_AFTER: Duration = Duration::from_secs(1);
/// At least this long between two steps down, so the viewers' queues can settle in between.
const DOWN_PAUSE: Duration = Duration::from_secs(2);
/// Everything has to be fine this long before quality goes up again.
const UP_AFTER: Duration = Duration::from_secs(5);
/// A step up that goes down again within this time did not fit the upload limit.
const UP_FAILED_WITHIN: Duration = Duration::from_secs(10);
/// Longest wait before retrying a step up that failed before; with the 5 s wait for the step
/// below, a lifted limit is back at full quality well within 45 s (AC-2.29).
const UP_AFTER_MAX: Duration = Duration::from_secs(40);
/// A step up that held this long counts as fitting, and its wait starts from `UP_AFTER` again.
const UP_STABLE: Duration = Duration::from_secs(60);

/// What the sharer picked for this share (start dialog, Quality menu or Settings defaults).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QualityPrefs {
    pub resolution: ScreenResolution,
    pub fps: ScreenFps,
    pub optimize: ScreenOptimize,
    pub upload_limit: ScreenUploadLimit,
}

impl QualityPrefs {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            resolution: settings.screen_resolution,
            fps: settings.screen_fps,
            optimize: settings.screen_optimize,
            upload_limit: settings.screen_upload_limit,
        }
    }

    fn ceiling(&self) -> (usize, usize) {
        let height = match self.resolution {
            ScreenResolution::Auto => AUTO_HEIGHT,
            ScreenResolution::P720 => 720,
            ScreenResolution::P1080 => 1080,
            ScreenResolution::P1440 => 1440,
        };
        let fps = match self.fps {
            ScreenFps::Fps15 => 15,
            ScreenFps::Fps30 => 30,
            ScreenFps::Fps60 => 60,
        };
        (
            SIZES.iter().position(|s| s.1 == height).unwrap_or(1),
            RATES_FPS.iter().position(|f| *f == fps).unwrap_or(1),
        )
    }

    pub fn upload_limit_kbps(&self) -> Option<u32> {
        match self.upload_limit {
            ScreenUploadLimit::Auto => None,
            ScreenUploadLimit::Mbit2 => Some(2_000),
            ScreenUploadLimit::Mbit5 => Some(5_000),
            ScreenUploadLimit::Mbit10 => Some(10_000),
            ScreenUploadLimit::Mbit20 => Some(20_000),
        }
    }
}

/// Why the share runs below what the sharer picked ("Lowered to {step}: {reason}").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoweredReason {
    UploadTooSlow,
    EncoderTooSlow,
    /// The upload limit, split over one copy per viewer, no longer fits the picked step.
    ManyViewers,
}

/// The steps from the ceiling down to the floor (480p15), as indices into `SIZES` and
/// `RATES_FPS`. Sharp text keeps the resolution and drops the frame rate first, Smooth motion
/// the other way round, Balanced alternates, starting with the resolution like before.
fn ladder(prefs: QualityPrefs) -> Vec<(usize, usize)> {
    let (mut size, mut fps) = prefs.ceiling();
    let (last_size, last_fps) = (SIZES.len() - 1, RATES_FPS.len() - 1);
    let mut out = vec![(size, fps)];
    let mut resolution_next = true;
    while size < last_size || fps < last_fps {
        let drop_resolution = match prefs.optimize {
            ScreenOptimize::Text => fps == last_fps,
            ScreenOptimize::Motion => size < last_size,
            ScreenOptimize::Balanced => (resolution_next && size < last_size) || fps == last_fps,
        };
        if drop_resolution {
            size += 1;
        } else {
            fps += 1;
        }
        resolution_next = !drop_resolution;
        out.push((size, fps));
    }
    out
}

/// One look at how sending goes.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Load {
    /// Viewers that lost at least one temporal layer.
    pub lagging: usize,
    /// Viewers that lost every layer and still do not keep up.
    pub struggling: usize,
    /// Encoding takes too much of the frame time.
    pub encoder_busy: bool,
    /// Copies of the stream that go out: one per viewer.
    pub copies: usize,
}

/// Picks the quality step from the sharer's picks and what the viewers and the encoder report.
pub(crate) struct Steps {
    prefs: QualityPrefs,
    codec: Codec,
    ladder: Vec<(usize, usize)>,
    step: usize,
    lowered: Option<LoweredReason>,
    pressure_since: Option<Instant>,
    fine_since: Option<Instant>,
    last_change: Option<Instant>,
    /// How long everything has to be fine before stepping up to each step; doubles when that
    /// step failed right after being reached.
    up_after: Vec<Duration>,
    /// The latest step up, until it is judged failed or stable.
    up_at: Option<Instant>,
}

impl Steps {
    pub fn new(prefs: QualityPrefs, codec: Codec) -> Self {
        let ladder = ladder(prefs);
        Self {
            prefs,
            codec,
            up_after: vec![UP_AFTER; ladder.len()],
            ladder,
            step: 0,
            lowered: None,
            pressure_since: None,
            fine_since: None,
            last_change: None,
            up_at: None,
        }
    }

    pub fn prefs(&self) -> QualityPrefs {
        self.prefs
    }

    /// New picks start over at the new ceiling; the next [`Self::observe`] applies the limit.
    pub fn set_prefs(&mut self, prefs: QualityPrefs) {
        *self = Self::new(prefs, self.codec);
    }

    /// The codec only changes the bitrates; the step stays.
    pub fn set_codec(&mut self, codec: Codec) {
        self.codec = codec;
    }

    pub fn quality(&self) -> Quality {
        self.at(self.step)
    }

    pub fn at(&self, step: usize) -> Quality {
        let (size, fps) = self.ladder[step];
        let (width, height) = SIZES[size];
        let fps = RATES_FPS[fps];
        let rate = RATES
            .iter()
            .find(|r| r.height == height && r.fps == fps)
            .expect("every ladder step has a rate");
        let bitrate_kbps = match self.codec {
            Codec::Av1 => rate.av1_kbps,
            Codec::Vp9 => rate.vp9_kbps,
        };
        Quality {
            width,
            height,
            fps,
            bitrate_kbps,
        }
    }

    pub fn step(&self) -> usize {
        self.step
    }

    pub fn floor(&self) -> usize {
        self.ladder.len() - 1
    }

    /// Why the share is below the picked ceiling; `None` at the top.
    pub fn lowered(&self) -> Option<LoweredReason> {
        self.lowered
    }

    /// The best step whose upload, one copy per viewer, fits the limit; the floor when none does.
    fn cap(&self, copies: usize) -> usize {
        let Some(limit) = self.prefs.upload_limit_kbps() else {
            return 0;
        };
        let budget = u64::from(limit) * LIMIT_SHARE_PERCENT / 100;
        let copies = copies.max(1) as u64;
        (0..self.ladder.len())
            .find(|&s| u64::from(self.at(s).bitrate_kbps) * copies <= budget)
            .unwrap_or(self.floor())
    }

    /// Steps down right away when the upload limit no longer fits `copies`; true when the step
    /// changed.
    pub fn limit(&mut self, copies: usize, now: Instant) -> bool {
        let cap = self.cap(copies);
        if self.step >= cap {
            return false;
        }
        self.lowered = Some(if self.cap(1) < cap {
            LoweredReason::ManyViewers
        } else {
            LoweredReason::UploadTooSlow
        });
        self.step = cap;
        self.up_at = None;
        self.changed(now);
        true
    }

    /// Takes in the current load; true when the step changed.
    pub fn observe(&mut self, load: Load, now: Instant) -> bool {
        if self.limit(load.copies, now) {
            return true;
        }
        let cap = self.cap(load.copies);
        let pressure = load.lagging >= 2 || load.struggling > 0 || load.encoder_busy;
        let fine = load.lagging == 0 && !load.encoder_busy;
        if let Some(at) = self.up_at
            && now.duration_since(at) >= UP_STABLE
        {
            self.up_after[self.step] = UP_AFTER;
            self.up_at = None;
        }
        if pressure {
            self.fine_since = None;
            let since = *self.pressure_since.get_or_insert(now);
            let rested = self
                .last_change
                .is_none_or(|at| now.duration_since(at) >= DOWN_PAUSE);
            if self.step < self.floor() && now.duration_since(since) >= DOWN_AFTER && rested {
                if self
                    .up_at
                    .take()
                    .is_some_and(|at| now.duration_since(at) < UP_FAILED_WITHIN)
                {
                    self.up_after[self.step] = (self.up_after[self.step] * 2).min(UP_AFTER_MAX);
                }
                self.step += 1;
                self.lowered = Some(if load.encoder_busy {
                    LoweredReason::EncoderTooSlow
                } else {
                    LoweredReason::UploadTooSlow
                });
                self.changed(now);
                return true;
            }
        } else {
            self.pressure_since = None;
            if fine {
                let since = *self.fine_since.get_or_insert(now);
                if self.step > cap && now.duration_since(since) >= self.up_after[self.step - 1] {
                    self.step -= 1;
                    if self.step == 0 {
                        self.lowered = None;
                    }
                    self.up_at = Some(now);
                    self.changed(now);
                    return true;
                }
            } else {
                self.fine_since = None;
            }
        }
        false
    }

    fn changed(&mut self, now: Instant) {
        self.last_change = Some(now);
        self.pressure_since = None;
        self.fine_since = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps() -> Steps {
        Steps::new(QualityPrefs::default(), Codec::Vp9)
    }

    fn labels(prefs: QualityPrefs) -> Vec<String> {
        let mut steps = Steps::new(prefs, Codec::Vp9);
        (0..=steps.floor())
            .map(|s| {
                steps.step = s;
                steps.quality().label()
            })
            .collect()
    }

    fn prefs(
        resolution: ScreenResolution,
        fps: ScreenFps,
        optimize: ScreenOptimize,
    ) -> QualityPrefs {
        QualityPrefs {
            resolution,
            fps,
            optimize,
            upload_limit: ScreenUploadLimit::Auto,
        }
    }

    fn load(lagging: usize, struggling: usize) -> Load {
        Load {
            lagging,
            struggling,
            encoder_busy: false,
            copies: 1,
        }
    }

    #[test]
    fn auto_keeps_the_steps_of_before() {
        assert_eq!(
            labels(QualityPrefs::default()),
            ["1080p30", "720p30", "720p15", "480p15"]
        );
    }

    #[test]
    fn optimize_for_orders_the_steps() {
        use ScreenOptimize::*;
        let top = |o| prefs(ScreenResolution::P1440, ScreenFps::Fps60, o);
        assert_eq!(
            labels(top(Balanced)),
            [
                "1440p60", "1080p60", "1080p30", "720p30", "720p15", "480p15"
            ]
        );
        assert_eq!(
            labels(top(Text)),
            [
                "1440p60", "1440p30", "1440p15", "1080p15", "720p15", "480p15"
            ]
        );
        assert_eq!(
            labels(top(Motion)),
            ["1440p60", "1080p60", "720p60", "480p60", "480p30", "480p15"]
        );
    }

    #[test]
    fn a_manual_pick_is_the_ceiling() {
        let low = prefs(
            ScreenResolution::P720,
            ScreenFps::Fps15,
            ScreenOptimize::Balanced,
        );
        assert_eq!(labels(low), ["720p15", "480p15"]);
    }

    #[test]
    fn the_bitrate_follows_the_codec() {
        let top = prefs(
            ScreenResolution::P1440,
            ScreenFps::Fps60,
            ScreenOptimize::Balanced,
        );
        let mut steps = Steps::new(top, Codec::Vp9);
        assert_eq!(steps.quality().bitrate_kbps, 12_000);
        steps.set_codec(Codec::Av1);
        assert_eq!(steps.quality().bitrate_kbps, 9_000);
    }

    #[test]
    fn the_upload_limit_caps_the_total() {
        let now = Instant::now();
        let mut steps = Steps::new(
            QualityPrefs {
                upload_limit: ScreenUploadLimit::Mbit5,
                ..QualityPrefs::default()
            },
            Codec::Vp9,
        );
        let copies = |copies| Load {
            copies,
            ..load(0, 0)
        };
        assert!(!steps.observe(copies(1), now), "1080p30 fits 5 Mbit/s");
        assert!(steps.observe(copies(2), now));
        assert_eq!(steps.quality().label(), "720p30");
        assert_eq!(steps.lowered(), Some(LoweredReason::ManyViewers));
        assert!(steps.observe(copies(3), now));
        assert_eq!(steps.quality().label(), "720p15");
        // A viewer leaves: back up once all is fine, but never above what the limit allows.
        let later = now + Duration::from_secs(30);
        assert!(!steps.observe(copies(2), later));
        assert!(steps.observe(copies(2), later + UP_AFTER));
        assert_eq!(steps.quality().label(), "720p30");
        assert!(!steps.observe(copies(2), later + UP_AFTER * 4));
    }

    #[test]
    fn a_limit_below_the_top_is_upload_too_slow() {
        let mut steps = Steps::new(
            QualityPrefs {
                upload_limit: ScreenUploadLimit::Mbit2,
                ..QualityPrefs::default()
            },
            Codec::Vp9,
        );
        assert!(steps.observe(load(0, 0), Instant::now()));
        assert_eq!(steps.quality().label(), "720p15");
        assert_eq!(steps.lowered(), Some(LoweredReason::UploadTooSlow));
    }

    #[test]
    fn new_prefs_start_over_at_the_new_ceiling() {
        let now = Instant::now();
        let mut steps = steps();
        steps.observe(load(0, 1), now);
        steps.observe(load(0, 1), now + DOWN_AFTER);
        assert_eq!(steps.step(), 1);
        steps.set_prefs(prefs(
            ScreenResolution::P1440,
            ScreenFps::Fps30,
            ScreenOptimize::Text,
        ));
        assert_eq!((steps.step(), steps.lowered()), (0, None));
        assert_eq!(steps.quality().label(), "1440p30");
    }

    #[test]
    fn one_lagging_viewer_keeps_the_quality() {
        let now = Instant::now();
        let mut steps = steps();
        assert!(!steps.observe(load(1, 0), now));
        assert!(!steps.observe(load(1, 0), now + Duration::from_secs(10)));
        assert_eq!(steps.step(), 0);
    }

    #[test]
    fn quality_steps_down_to_the_floor_and_back_up() {
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let mut steps = steps();
        let floor = steps.floor();
        assert!(
            !steps.observe(load(2, 0), at(0)),
            "pressure has to last a second"
        );
        assert!(steps.observe(load(2, 0), at(1)));
        assert_eq!(steps.step(), 1);
        assert_eq!(steps.lowered(), Some(LoweredReason::UploadTooSlow));
        assert!(
            !steps.observe(load(2, 0), at(2)),
            "a pause between two steps down"
        );
        assert!(
            steps.observe(load(0, 1), at(3)),
            "one struggling viewer is enough"
        );
        assert_eq!(steps.quality().fps, 15);
        assert!(!steps.observe(load(0, 1), at(5)));
        assert!(steps.observe(load(0, 1), at(6)));
        assert_eq!(steps.step(), floor);
        assert!(!steps.observe(load(3, 1), at(10)), "the floor holds");
        assert!(!steps.observe(load(3, 1), at(20)));
        assert!(
            !steps.observe(load(1, 0), at(21)),
            "one lagging viewer is no pressure, but not fine either"
        );
        assert!(!steps.observe(load(0, 0), at(22)));
        assert!(!steps.observe(load(1, 0), at(26)), "lag resets the wait");
        assert!(!steps.observe(load(0, 0), at(27)));
        assert!(steps.observe(load(0, 0), at(32)));
        assert_eq!(steps.step(), floor - 1);
        assert!(
            !steps.observe(load(0, 0), at(33)),
            "each step up waits again"
        );
        assert!(steps.observe(load(0, 0), at(38)));
        assert!(!steps.observe(load(0, 0), at(39)));
        assert!(steps.observe(load(0, 0), at(44)));
        assert_eq!((steps.step(), steps.lowered()), (0, None));
    }

    /// Runs the steps once a second against an upload limit that only fits `fits_from` and
    /// below; returns the number of step-ups and the mean fps over `secs`.
    fn simulate(
        steps: &mut Steps,
        start: Instant,
        from: u64,
        secs: u64,
        fits_from: usize,
    ) -> (usize, f64) {
        let (mut ups, mut fps) = (0, 0.0);
        for s in from..from + secs {
            let before = steps.step();
            let pressure = steps.step() < fits_from;
            let l = if pressure { load(2, 0) } else { load(0, 0) };
            steps.observe(l, start + Duration::from_secs(s));
            if steps.step() < before {
                ups += 1;
            }
            fps += f64::from(steps.quality().fps);
        }
        (ups, fps / secs as f64)
    }

    #[test]
    fn a_step_up_that_keeps_failing_is_tried_less_often() {
        let start = Instant::now();
        let mut steps = steps();
        // Full HD and 720p30 never fit, 720p15 does.
        simulate(&mut steps, start, 0, 20, 2);
        let (ups, fps) = simulate(&mut steps, start, 20, 60, 2);
        assert!(ups <= 2, "{ups} tries in 60 s");
        assert!(fps >= 14.0, "{fps} fps");
    }

    #[test]
    fn recovery_after_a_lifted_limit_stays_under_45_seconds() {
        let start = Instant::now();
        let mut steps = steps();
        simulate(&mut steps, start, 0, 200, 1);
        let mut t = 200;
        while steps.step() > 0 {
            simulate(&mut steps, start, t, 1, 0);
            t += 1;
            assert!(t - 200 <= 45, "still at step {} after 45 s", steps.step());
        }
    }

    #[test]
    fn a_step_up_that_held_resets_its_wait() {
        let start = Instant::now();
        let mut steps = steps();
        simulate(&mut steps, start, 0, 200, 1);
        simulate(&mut steps, start, 200, 100, 0);
        assert_eq!(steps.up_after[0], UP_AFTER);
    }

    #[test]
    fn a_busy_encoder_lowers_quality() {
        let now = Instant::now();
        let mut steps = steps();
        let busy = Load {
            encoder_busy: true,
            ..load(0, 0)
        };
        steps.observe(busy, now);
        assert!(steps.observe(busy, now + DOWN_AFTER));
        assert_eq!(steps.lowered(), Some(LoweredReason::EncoderTooSlow));
    }
}
