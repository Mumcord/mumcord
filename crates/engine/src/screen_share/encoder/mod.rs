//! Encoder detection and selection for screen sharing (FR-7.53 – FR-7.56, FR-7.60).
//!
//! At engine start one background worker probes the hardware encoders (with smoke test) and
//! self-tests software AV1 for the Settings defaults; the results are cached in the store per
//! machine and measured again after a driver or app update, or on "Check again". Nothing here
//! ever blocks a share start: a missing result simply counts as "not usable yet", and the
//! share starts with the next encoder in the order while the test runs.

mod battery;
mod driver;
pub(crate) mod monitor;
mod probe;
pub(crate) mod select;
mod selftest;

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use mumcord_screenshare::{
    Av1Encoder, Av1Options, Codec, Encoder, EncoderConfig, I420Frame, VideoEncoder,
    open_hardware_encoder,
};
use mumcord_store::{
    EncoderCache, EncoderEntry, ScreenCodec, ScreenOptimize, Settings, Store,
    SvtSelftest as CachedSelftest,
};

use self::probe::Hardware;
use self::select::{Candidate, Inputs, Kind, Selection};
use super::media::EncoderFactory;
use super::quality::QualityPrefs;
use crate::session::{Engine, EngineEvent, Events, lock};
use crate::test_hooks::{self, SvtSelftest as ForcedSelftest};

pub use self::select::Av1Skipped;

/// Who made the graphics card an encoder runs on ("AV1 (NVIDIA graphics card)").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Unknown,
}

/// The encoder a share runs on, for the codec badge ("AV1 · GPU", "VP9").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveEncoder {
    pub codec: Codec,
    /// `None` when it runs on the processor.
    pub gpu: Option<GpuVendor>,
}

/// Highest size and rate software AV1 keeps up with ("SVT-AV1, keeps up to 1080p30").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoftwareLimit {
    pub height: u32,
    pub fps: u32,
}

/// What Settings → Screen sharing shows about the encoders (FR-7.60, mockup 05).
#[derive(Debug, Clone, PartialEq)]
pub struct EncoderStatus {
    /// The probe or a self-test is running ("Checking…").
    pub checking: bool,
    /// The working hardware AV1 encoder, if any (regardless of "Use graphics card").
    pub av1_hardware: Option<GpuVendor>,
    pub vp9_hardware: Option<GpuVendor>,
    /// `None` when software AV1 passed no self-test for the default Optimize for.
    pub av1_software_limit: Option<SoftwareLimit>,
    /// The encoder a share with the Settings defaults starts with ("Active encoder").
    pub active: ActiveEncoder,
    /// Why that is not AV1 under Automatic ("VP9 (AV1 too heavy for this PC)").
    pub av1_skipped: Option<Av1Skipped>,
    /// Manual AV1 runs in software without a passed self-test (FR-7.54 warning).
    pub software_warning: bool,
}

/// One software AV1 self-test: height, frame rate, Optimize for.
type TestKey = (u32, u32, ScreenOptimize);

enum Job {
    /// Probe the hardware encoders; `fresh` ignores and replaces the cache ("Check again").
    Probe {
        fresh: bool,
    },
    Selftest(TestKey),
}

#[derive(Default)]
struct State {
    /// Jobs queued or running.
    busy: usize,
    hardware: Hardware,
    driver_id: String,
    entries: Vec<EncoderEntry>,
    selftests: Vec<CachedSelftest>,
    /// Self-tests asked for and not finished yet, so a key is queued once.
    queued: Vec<TestKey>,
    /// A "Check again" probe is queued or running; more clicks until it ends add nothing.
    rechecking: bool,
}

/// The engine-wide encoder knowledge; one per engine.
#[derive(Default)]
pub(crate) struct Encoders {
    state: Mutex<State>,
    jobs: Mutex<Option<mpsc::Sender<Job>>>,
}

impl Encoders {
    /// Starts the background worker and the startup probe.
    pub(crate) fn start(self: &Arc<Self>, store: Arc<Mutex<Store>>, events: Events) {
        let (tx, rx) = mpsc::channel();
        *lock(&self.jobs) = Some(tx);
        let encoders = self.clone();
        let spawned = thread::Builder::new()
            .name("screen-encoder-probe".into())
            .spawn(move || {
                let worker = Worker {
                    encoders: &encoders,
                    store: &store,
                    events: &events,
                };
                for job in rx {
                    worker.run(job);
                }
            });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "encoder probe thread did not start");
            *lock(&self.jobs) = None;
            return;
        }
        self.send(Job::Probe { fresh: false });
    }

    fn send(&self, job: Job) {
        let jobs = lock(&self.jobs);
        let Some(tx) = jobs.as_ref() else {
            return;
        };
        if let Job::Probe { fresh: true } = job {
            let mut state = lock(&self.state);
            if state.rechecking {
                return;
            }
            state.rechecking = true;
        }
        if let Job::Selftest(key) = job {
            let mut state = lock(&self.state);
            if state.queued.contains(&key) {
                return;
            }
            state.queued.push(key);
        }
        lock(&self.state).busy += 1;
        if tx.send(job).is_err() {
            tracing::warn!("encoder probe thread is gone");
            let mut state = lock(&self.state);
            state.busy -= 1;
            state.rechecking = false;
        }
    }

    /// Picks the encoders for a share. Never waits: a missing self-test result counts as
    /// failed and is measured in the background for the next share.
    pub(crate) fn select(&self, settings: &Settings, prefs: QualityPrefs) -> Selection {
        let (selection, missing) = self.select_quietly(settings, prefs);
        if let Some(key) = missing {
            self.send(Job::Selftest(key));
        }
        selection
    }

    fn select_quietly(
        &self,
        settings: &Settings,
        prefs: QualityPrefs,
    ) -> (Selection, Option<TestKey>) {
        let state = lock(&self.state);
        // Software AV1 can't come into question: manual VP9, a usable AV1 graphics card, or
        // Automatic above the software cap.
        let no_software_av1 = match settings.screen_codec {
            ScreenCodec::Vp9 => true,
            _ if settings.screen_use_gpu && !state.hardware.av1.is_empty() => true,
            ScreenCodec::Auto => select::above_software_cap(prefs),
            ScreenCodec::Av1 => false,
        };
        let (key, passed) = if no_software_av1 {
            (None, false)
        } else {
            let (height, fps) = select::ceiling(select::cap_for_software_av1(prefs));
            let key = (height, fps, prefs.optimize);
            match selftest_passed(&state.selftests, key) {
                Some(passed) => (None, passed),
                None => (Some(key), false),
            }
        };
        let inputs = Inputs {
            codec: settings.screen_codec,
            use_gpu: settings.screen_use_gpu,
            av1_hardware: &state.hardware.av1,
            vp9_hardware: &state.hardware.vp9,
            selftest_passed: passed,
            on_battery: battery::on_battery(),
        };
        (select::select(&inputs, prefs), key)
    }

    /// Runs the self-test for these picks in the background if it hasn't run yet (the start
    /// dialog calls this while the user picks).
    /// The selection a share with these picks would start with, without starting a self-test
    /// (start dialog, FR-7.36).
    pub(crate) fn preview(&self, settings: &Settings, prefs: QualityPrefs) -> Selection {
        self.select_quietly(settings, prefs).0
    }

    pub(crate) fn prepare(&self, settings: &Settings, prefs: QualityPrefs) {
        if let (_, Some(key)) = self.select_quietly(settings, prefs) {
            self.send(Job::Selftest(key));
        }
    }

    pub(crate) fn check_again(&self) {
        self.send(Job::Probe { fresh: true });
    }

    pub(crate) fn status(&self, settings: &Settings) -> EncoderStatus {
        let prefs = QualityPrefs::from_settings(settings);
        let (selection, _) = self.select_quietly(settings, prefs);
        let state = lock(&self.state);
        let first = selection.candidates[0];
        EncoderStatus {
            checking: state.busy > 0,
            av1_hardware: state.hardware.av1.first().and_then(Candidate::gpu),
            vp9_hardware: state.hardware.vp9.first().and_then(Candidate::gpu),
            av1_software_limit: software_limit(&state.selftests, prefs.optimize),
            active: ActiveEncoder {
                codec: first.codec,
                gpu: first.gpu(),
            },
            av1_skipped: selection.av1_skipped,
            software_warning: selection.software_warning,
        }
    }
}

/// `None` when the test hasn't run for this key.
fn selftest_passed(cached: &[CachedSelftest], key: TestKey) -> Option<bool> {
    if let Some(forced) = test_hooks::svt_selftest() {
        return Some(forced == ForcedSelftest::Pass);
    }
    let (height, fps, optimize) = key;
    cached
        .iter()
        .find(|t| t.resolution == height && t.fps == fps && t.optimize == optimize)
        .map(|t| selftest::passes(t.svt_selftest_ms, t.fps))
}

fn software_limit(cached: &[CachedSelftest], optimize: ScreenOptimize) -> Option<SoftwareLimit> {
    if let Some(forced) = test_hooks::svt_selftest() {
        let (height, fps) = select::SOFTWARE_AV1_CAP;
        return (forced == ForcedSelftest::Pass).then_some(SoftwareLimit { height, fps });
    }
    cached
        .iter()
        .filter(|t| t.optimize == optimize && selftest::passes(t.svt_selftest_ms, t.fps))
        .max_by_key(|t| (t.resolution, t.fps))
        .map(|t| SoftwareLimit {
            height: t.resolution,
            fps: t.fps,
        })
}

/// The self-tests run right after the probe: the Settings defaults, and 720p30 as the usual
/// step below, both with the default Optimize for.
fn startup_tests(settings: &Settings) -> Vec<TestKey> {
    let prefs = QualityPrefs::from_settings(settings);
    let (height, fps) = select::ceiling(select::cap_for_software_av1(prefs));
    let mut keys = vec![(height, fps, prefs.optimize)];
    if (height, fps) != (720, 30) {
        keys.push((720, 30, prefs.optimize));
    }
    keys
}

struct Worker<'a> {
    encoders: &'a Encoders,
    store: &'a Mutex<Store>,
    events: &'a Events,
}

impl Worker<'_> {
    fn run(&self, job: Job) {
        self.emit();
        match job {
            Job::Probe { fresh } => self.probe(fresh),
            Job::Selftest(key) => self.selftest(key),
        }
        lock(&self.encoders.state).busy -= 1;
        self.emit();
    }

    fn settings(&self) -> Settings {
        lock(self.store).settings().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "settings not read for the encoder probe");
            Settings::default()
        })
    }

    fn probe(&self, fresh: bool) {
        if fresh {
            lock(&self.encoders.state).queued.clear();
        }
        if let Some(overrides) = test_hooks::encoder_probe() {
            // Forced results never touch the cache of the real profile.
            let mut state = lock(&self.encoders.state);
            state.hardware = probe::from_overrides(&overrides);
            state.entries.clear();
        } else {
            let driver_id = driver::driver_id();
            let cached = if fresh {
                None
            } else {
                lock(self.store).encoder_cache().unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "encoder cache not read");
                    None
                })
            };
            let (entries, selftests) = match cached {
                Some(cache) if cache.is_current(&driver_id, APP_VERSION) => {
                    (cache.encoders, cache.selftests)
                }
                _ => {
                    tracing::info!("probing the screen share encoders");
                    (probe::probe(), Vec::new())
                }
            };
            let mut state = lock(&self.encoders.state);
            state.hardware = probe::from_entries(&entries);
            state.driver_id = driver_id;
            state.entries = entries;
            state.selftests = selftests;
            drop(state);
            self.save();
        }
        for key in startup_tests(&self.settings()) {
            self.selftest(key);
        }
        if fresh {
            lock(&self.encoders.state).rechecking = false;
        }
    }

    fn selftest(&self, key: TestKey) {
        let known = {
            let state = lock(&self.encoders.state);
            selftest_passed(&state.selftests, key).is_some()
        };
        if !known {
            let (height, fps, optimize) = key;
            // A test that could not run counts as far too slow.
            let ms = selftest::measure(height, fps, optimize).unwrap_or(u32::MAX);
            tracing::info!(height, fps, ?optimize, ms, "AV1 self-test measured");
            lock(&self.encoders.state).selftests.push(CachedSelftest {
                resolution: height,
                fps,
                optimize,
                svt_selftest_ms: ms,
            });
            self.save();
        }
        lock(&self.encoders.state).queued.retain(|k| *k != key);
    }

    fn save(&self) {
        if test_hooks::encoder_probe().is_some() {
            return;
        }
        let cache = {
            let state = lock(&self.encoders.state);
            EncoderCache {
                driver_id: state.driver_id.clone(),
                app_version: APP_VERSION.to_owned(),
                encoders: state.entries.clone(),
                selftests: state.selftests.clone(),
            }
        };
        if let Err(e) = lock(self.store).save_encoder_cache(&cache) {
            tracing::warn!(error = %e, "encoder cache not saved");
        }
    }

    fn emit(&self) {
        let status = self.encoders.status(&self.settings());
        self.events
            .emit(EngineEvent::ScreenEncoderStatusChanged(status));
    }
}

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Makes the share's encoder from the selection: tries each candidate in order, so a
/// hardware encoder that fails to open falls through to the next one instead of ending the
/// share. Without `av1` (a viewer cannot play it, or the load fallback) AV1 candidates are
/// skipped. The one that opened is recorded in `active`.
pub(crate) fn factory(
    candidates: Vec<Candidate>,
    active: Arc<Mutex<Option<ActiveEncoder>>>,
) -> EncoderFactory {
    let offers_av1 = candidates.iter().any(|c| c.codec == Codec::Av1);
    let open = Box::new(move |config, optimize, av1: bool| {
        let mut last = None;
        for candidate in candidates.iter().filter(|c| av1 || c.codec != Codec::Av1) {
            match open(*candidate, config, optimize) {
                Ok(encoder) => {
                    let opened = ActiveEncoder {
                        codec: candidate.codec,
                        gpu: candidate.gpu(),
                    };
                    *lock(&active) = Some(opened);
                    return Ok((encoder, opened));
                }
                Err(e) => {
                    tracing::warn!(?candidate, error = %e, "screen share encoder did not open; trying the next");
                    last = Some(e);
                }
            }
        }
        Err(last.unwrap_or_else(|| {
            mumcord_screenshare::Error::InvalidConfig("no encoder to try".into())
        }))
    });
    EncoderFactory { offers_av1, open }
}

fn open(
    candidate: Candidate,
    config: EncoderConfig,
    optimize: ScreenOptimize,
) -> mumcord_screenshare::Result<Box<dyn VideoEncoder>> {
    match (candidate.kind, candidate.codec) {
        (Kind::Hardware(encoder, _), codec) => open_hardware_encoder(encoder, codec, config),
        (Kind::FakeHardware | Kind::Software, Codec::Av1) => Ok(Box::new(Av1Encoder::new(
            config,
            Av1Options {
                screen_content: optimize == ScreenOptimize::Text,
            },
        )?)),
        (Kind::FakeHardware | Kind::Software, Codec::Vp9) => Ok(Box::new(Encoder::new(config)?)),
    }
}

/// A screen-like test picture: a soft background with rows of sharp "text" blocks; `n`
/// shifts the blocks so consecutive frames differ.
fn test_frame(width: u32, height: u32, n: u32) -> I420Frame {
    let (w, h) = (width as usize, height as usize);
    let mut data = vec![128u8; I420Frame::buffer_len(width, height)];
    for (y, row) in data[..w * h].chunks_exact_mut(w).enumerate() {
        for (x, px) in row.iter_mut().enumerate() {
            let text = !(y / 4).is_multiple_of(5)
                && !((x + n as usize * 3) / 6).is_multiple_of(3)
                && (y / 20).is_multiple_of(2);
            *px = if text {
                ((x * 7 + y * 13) % 64) as u8
            } else {
                (180 + (x + y) / 32 % 60) as u8
            };
        }
    }
    I420Frame::new(width, height, data).expect("buffer_len fits the size")
}

impl Engine {
    /// What Settings → Screen sharing shows about the encoders (FR-7.60).
    pub fn screen_encoder_status(&self) -> EncoderStatus {
        self.screen_encoders().status(&self.encoder_settings())
    }

    /// "Check again": probe and self-test from scratch in the background; the result comes as
    /// [`EngineEvent::ScreenEncoderStatusChanged`].
    pub fn screen_encoder_check_again(&self) {
        self.screen_encoders().check_again();
    }

    /// Runs the software AV1 self-test for these picks in the background, so the share started
    /// with them can use software AV1 (start dialog, FR-7.56).
    pub fn screen_encoder_prepare(&self, prefs: QualityPrefs) {
        self.screen_encoders()
            .prepare(&self.encoder_settings(), prefs);
    }

    pub(crate) fn encoder_settings(&self) -> Settings {
        lock(&self.store_handle()).settings().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "settings not read for the encoder selection");
            Settings::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use mumcord_screenshare::HardwareEncoder;

    use super::*;

    const CONFIG: EncoderConfig = EncoderConfig {
        width: 320,
        height: 180,
        fps: 30,
        bitrate_kbps: 500,
    };

    #[test]
    fn a_hardware_encoder_that_fails_to_open_falls_through_to_software() {
        let active = Arc::new(Mutex::new(None));
        let make = factory(
            vec![
                Candidate {
                    codec: Codec::Av1,
                    kind: Kind::Hardware(HardwareEncoder::Nvenc, GpuVendor::Nvidia),
                },
                Candidate {
                    codec: Codec::Vp9,
                    kind: Kind::Software,
                },
            ],
            active.clone(),
        );
        assert!(make.offers_av1);
        let (encoder, _) = (make.open)(CONFIG, ScreenOptimize::Balanced, true).expect("falls back");
        assert_eq!(encoder.codec(), Codec::Vp9);
        assert_eq!(
            *lock(&active),
            Some(ActiveEncoder {
                codec: Codec::Vp9,
                gpu: None
            })
        );
    }

    #[test]
    fn forced_hardware_runs_the_software_encoder_but_reports_the_gpu() {
        let active = Arc::new(Mutex::new(None));
        let make = factory(
            vec![Candidate {
                codec: Codec::Av1,
                kind: Kind::FakeHardware,
            }],
            active.clone(),
        );
        let (encoder, _) = (make.open)(CONFIG, ScreenOptimize::Text, true).expect("opens");
        assert_eq!(encoder.codec(), Codec::Av1);
        assert_eq!(
            *lock(&active),
            Some(ActiveEncoder {
                codec: Codec::Av1,
                gpu: Some(GpuVendor::Unknown)
            })
        );
    }

    #[test]
    fn without_av1_the_factory_opens_the_first_vp9_encoder() {
        let active = Arc::new(Mutex::new(None));
        let make = factory(
            vec![
                Candidate {
                    codec: Codec::Av1,
                    kind: Kind::Software,
                },
                Candidate {
                    codec: Codec::Vp9,
                    kind: Kind::FakeHardware,
                },
                Candidate {
                    codec: Codec::Vp9,
                    kind: Kind::Software,
                },
            ],
            active.clone(),
        );
        let (encoder, opened) =
            (make.open)(CONFIG, ScreenOptimize::Balanced, false).expect("opens");
        assert_eq!(encoder.codec(), Codec::Vp9);
        let vp9_gpu = ActiveEncoder {
            codec: Codec::Vp9,
            gpu: Some(GpuVendor::Unknown),
        };
        assert_eq!((opened, *lock(&active)), (vp9_gpu, Some(vp9_gpu)));
        let vp9_only = factory(
            vec![Candidate {
                codec: Codec::Vp9,
                kind: Kind::Software,
            }],
            active,
        );
        assert!(!vp9_only.offers_av1);
    }

    fn cached(resolution: u32, fps: u32, optimize: ScreenOptimize, ms: u32) -> CachedSelftest {
        CachedSelftest {
            resolution,
            fps,
            optimize,
            svt_selftest_ms: ms,
        }
    }

    #[test]
    fn selftest_results_count_per_optimize_for() {
        let tests = [
            cached(1080, 30, ScreenOptimize::Balanced, 5),
            cached(1080, 30, ScreenOptimize::Text, 23),
            cached(720, 30, ScreenOptimize::Text, 10),
        ];
        assert_eq!(
            selftest_passed(&tests, (1080, 30, ScreenOptimize::Balanced)),
            Some(true)
        );
        assert_eq!(
            selftest_passed(&tests, (1080, 30, ScreenOptimize::Text)),
            Some(false)
        );
        assert_eq!(
            selftest_passed(&tests, (720, 15, ScreenOptimize::Text)),
            None
        );
        assert_eq!(
            software_limit(&tests, ScreenOptimize::Text),
            Some(SoftwareLimit {
                height: 720,
                fps: 30
            })
        );
        assert_eq!(software_limit(&tests, ScreenOptimize::Motion), None);
    }

    #[test]
    fn selection_never_waits_for_a_missing_selftest() {
        let encoders = Encoders::default();
        let settings = Settings::default();
        let (selection, missing) =
            encoders.select_quietly(&settings, QualityPrefs::from_settings(&settings));
        assert_eq!(
            missing,
            Some((1080, 30, QualityPrefs::from_settings(&settings).optimize))
        );
        // No probe yet and no self-test: the share starts on VP9 at once.
        assert_eq!(
            selection.candidates.last().map(|c| c.codec),
            Some(Codec::Vp9)
        );
        assert!(
            !selection
                .candidates
                .iter()
                .any(|c| c.codec == Codec::Av1 && c.kind == Kind::Software)
        );
    }

    #[test]
    fn check_again_while_a_recheck_is_pending_queues_no_second_probe() {
        let encoders = Encoders::default();
        let (tx, rx) = mpsc::channel();
        *lock(&encoders.jobs) = Some(tx);
        encoders.check_again();
        encoders.check_again();
        encoders.check_again();
        let probes = rx
            .try_iter()
            .filter(|job| matches!(job, Job::Probe { fresh: true }))
            .count();
        assert_eq!(probes, 1);
        assert_eq!(lock(&encoders.state).busy, 1);

        // Once that probe has finished, "Check again" measures anew.
        lock(&encoders.state).rechecking = false;
        encoders.check_again();
        assert!(matches!(rx.try_recv(), Ok(Job::Probe { fresh: true })));
    }

    #[test]
    fn the_startup_tests_cover_the_defaults_and_720p30() {
        let settings = Settings::default();
        let optimize = settings.screen_optimize;
        assert_eq!(
            startup_tests(&settings),
            [(1080, 30, optimize), (720, 30, optimize)]
        );
    }

    #[test]
    fn test_frames_have_the_right_size_and_move() {
        let a = test_frame(64, 48, 0);
        let b = test_frame(64, 48, 1);
        assert_eq!(a.data().len(), I420Frame::buffer_len(64, 48));
        assert_ne!(a.data(), b.data());
    }
}
