//! The dedicated effects thread (FR-8.38): sits between the camera and the encoder.
//!
//! The camera hands frames to [`Effects::start`]'s input sink; the thread applies the
//! background effect in place and passes each frame on to the output sink (the encoder).
//! When the effects fall behind, new frames are dropped at the input (their buffers go straight
//! back to the camera's pool), so the camera runs at the rate the effects can deliver.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mumcord_camera::{CameraError, CameraFrame, FrameSink};
use mumcord_store::CameraEffectQuality;

use crate::background::Background;
use crate::level::{Level, LevelController, Load};
use crate::pipeline::Pipeline;
use crate::segmenter::{FixtureSegmenter, ModelShape, OnnxSegmenter, Segmenter};
use crate::{EffectsError, hooks};

/// How often the battery state is read (it is a file or system call).
const BATTERY_POLL: Duration = Duration::from_secs(5);
/// How long the thread waits for a frame before it checks for commands again.
const IDLE_WAIT: Duration = Duration::from_millis(100);

/// Reports whether the machine runs on battery (the engine passes its battery probe).
pub type BatteryProbe = Arc<dyn Fn() -> bool + Send + Sync>;

/// Settings for one camera session.
#[derive(Clone)]
pub struct EffectsConfig {
    /// Capture frame rate; sets the frame budget.
    pub fps: u32,
    pub quality: CameraEffectQuality,
    pub background: Background,
    pub on_battery: BatteryProbe,
}

/// What the preview shows about the effects (FR-8.33, FR-8.36, FR-8.37), and the debug
/// panel's effect time (`MUMCORD_TEST_EFFECT_STATS`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectStats {
    /// The running level; `None` while no effect runs (Background None, or unavailable).
    pub level: Option<Level>,
    pub load: Load,
    /// Median effect time per frame.
    pub median: Duration,
    /// The effects can't hold 15 fps: "Your PC is too busy for this background effect."
    pub too_busy: bool,
    /// Why background effects are off on this PC, when they are.
    pub unavailable: Option<String>,
    /// Frames that passed through the effect so far.
    pub frames: u64,
}

enum Command {
    Background(Background),
    Quality(CameraEffectQuality),
}

enum Input {
    Frame(CameraFrame),
    Ended(CameraError),
}

/// Handle of a running effects thread. Dropping it stops the thread.
pub struct Effects {
    commands: Option<Sender<Command>>,
    stats: Arc<Mutex<EffectStats>>,
    thread: Option<JoinHandle<()>>,
}

impl Effects {
    /// Starts the thread. Returns the handle and the sink the camera delivers frames to.
    pub fn start(
        config: EffectsConfig,
        output: Box<dyn FrameSink>,
    ) -> std::io::Result<(Self, Box<dyn FrameSink>)> {
        let (frame_tx, frame_rx) = mpsc::sync_channel(1);
        let (command_tx, command_rx) = mpsc::channel();
        let stats = Arc::new(Mutex::new(EffectStats::default()));
        let worker = Worker::new(config, Arc::clone(&stats));
        let thread = std::thread::Builder::new()
            .name("camera-effects".to_owned())
            .spawn(move || worker.run(&frame_rx, &command_rx, output))?;
        let effects = Self {
            commands: Some(command_tx),
            stats,
            thread: Some(thread),
        };
        Ok((effects, Box::new(EffectsInput { frames: frame_tx })))
    }

    /// Changes the background while the camera runs (FR-8.40).
    pub fn set_background(&self, background: Background) {
        self.send(Command::Background(background));
    }

    /// Changes the effect quality setting; Auto measures again.
    pub fn set_quality(&self, quality: CameraEffectQuality) {
        self.send(Command::Quality(quality));
    }

    pub fn stats(&self) -> EffectStats {
        self.stats
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn send(&self, command: Command) {
        if let Some(commands) = &self.commands
            && commands.send(command).is_err()
        {
            tracing::warn!("camera effects thread has stopped");
        }
    }
}

impl Drop for Effects {
    fn drop(&mut self) {
        // Closing the command channel tells the thread to stop.
        self.commands = None;
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("camera effects thread panicked");
        }
    }
}

struct EffectsInput {
    frames: SyncSender<Input>,
}

impl FrameSink for EffectsInput {
    fn frame(&mut self, frame: CameraFrame) {
        match self.frames.try_send(Input::Frame(frame)) {
            // Busy: dropping the frame returns its buffer to the camera's pool.
            Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
        }
    }

    fn ended(&mut self, reason: CameraError) {
        let _ = self.frames.send(Input::Ended(reason));
    }
}

struct Worker {
    fps: u32,
    quality: CameraEffectQuality,
    background: Background,
    on_battery: BatteryProbe,
    battery: bool,
    battery_read_at: Instant,
    controller: LevelController,
    pipeline: Option<Pipeline>,
    full: Option<Box<dyn Segmenter>>,
    light: Option<Box<dyn Segmenter>>,
    unavailable: Option<String>,
    unsupported_logged: bool,
    model_error_logged: bool,
    stats: Arc<Mutex<EffectStats>>,
    frames: u64,
}

impl Worker {
    fn new(config: EffectsConfig, stats: Arc<Mutex<EffectStats>>) -> Self {
        let battery = (config.on_battery)();
        Self {
            fps: config.fps,
            quality: config.quality,
            background: config.background,
            controller: LevelController::new(config.quality, config.fps, battery),
            on_battery: config.on_battery,
            battery,
            battery_read_at: Instant::now(),
            pipeline: None,
            full: None,
            light: None,
            unavailable: None,
            unsupported_logged: false,
            model_error_logged: false,
            stats,
            frames: 0,
        }
    }

    fn run(
        mut self,
        frames: &Receiver<Input>,
        commands: &Receiver<Command>,
        mut output: Box<dyn FrameSink>,
    ) {
        loop {
            loop {
                match commands.try_recv() {
                    Ok(command) => self.apply(command),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }
            match frames.recv_timeout(IDLE_WAIT) {
                Ok(Input::Frame(mut frame)) => {
                    self.handle(&mut frame);
                    output.frame(frame);
                }
                Ok(Input::Ended(reason)) => output.ended(reason),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    // The camera is gone; wait for the stop so late commands don't error.
                    while commands.recv().is_ok() {}
                    return;
                }
            }
        }
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Background(background) => {
                if let Some(pipeline) = &mut self.pipeline {
                    pipeline.set_background(background.clone());
                }
                self.background = background;
            }
            Command::Quality(quality) => {
                self.quality = quality;
                self.controller = LevelController::new(quality, self.fps, self.battery);
            }
        }
        self.publish();
    }

    fn handle(&mut self, frame: &mut CameraFrame) {
        if self.background.is_none() || self.unavailable.is_some() {
            return;
        }
        if !Pipeline::supports(frame.width, frame.height) {
            if !self.unsupported_logged {
                tracing::warn!(
                    width = frame.width,
                    height = frame.height,
                    "camera frame size not supported by the background effects"
                );
                self.unsupported_logged = true;
            }
            return;
        }
        if self.battery_read_at.elapsed() >= BATTERY_POLL {
            self.battery = (self.on_battery)();
            self.battery_read_at = Instant::now();
        }
        if self
            .pipeline
            .as_ref()
            .is_none_or(|p| p.size() != (frame.width, frame.height))
        {
            let mut pipeline = Pipeline::new(frame.width, frame.height, self.background.clone());
            pipeline.set_level(self.controller.level());
            self.pipeline = Some(pipeline);
        }
        let level = self.controller.level();
        let (slot, shape) = match level {
            Level::Full => (&mut self.full, ModelShape::Square),
            Level::Light => (&mut self.light, ModelShape::Landscape),
        };
        if slot.is_none() {
            match create_segmenter(shape, self.fps) {
                Ok(segmenter) => *slot = Some(segmenter),
                Err(EffectsError::Unavailable(reason)) => {
                    tracing::warn!(%reason, "camera background effects unavailable");
                    self.unavailable = Some(reason);
                    self.publish();
                    return;
                }
                Err(err) => {
                    self.log_model_error(&err);
                    return;
                }
            }
        }
        let (Some(segmenter), Some(pipeline)) = (slot.as_deref_mut(), self.pipeline.as_mut())
        else {
            return;
        };
        let started = Instant::now();
        let result = pipeline.process(&mut frame.data, frame.timestamp, segmenter);
        let elapsed = started.elapsed() + hooks::effect_slow();
        if let Err(err) = result {
            self.log_model_error(&err);
            return;
        }
        self.frames += 1;
        if let Some(level) = self
            .controller
            .record(Instant::now(), elapsed, self.battery)
        {
            tracing::info!(?level, "camera effect level changed");
            pipeline.set_level(level);
        }
        self.publish();
    }

    fn log_model_error(&mut self, err: &EffectsError) {
        if !self.model_error_logged {
            tracing::error!(%err, "camera background effect failed; sending the plain picture");
            self.model_error_logged = true;
        }
    }

    fn publish(&self) {
        let running = !self.background.is_none() && self.unavailable.is_none();
        let stats = EffectStats {
            level: running.then(|| self.controller.level()),
            load: if running {
                self.controller.load()
            } else {
                Load::Low
            },
            median: self.controller.median(),
            too_busy: running && self.controller.too_busy(),
            unavailable: self.unavailable.clone(),
            frames: self.frames,
        };
        *self.stats.lock().unwrap_or_else(PoisonError::into_inner) = stats;
    }
}

/// The fixture outline under `MUMCORD_TEST_SEGMENT_MASK=fixture`, otherwise the real model.
fn create_segmenter(shape: ModelShape, fps: u32) -> Result<Box<dyn Segmenter>, EffectsError> {
    if hooks::segment_mask_fixture() {
        Ok(Box::new(FixtureSegmenter::new(shape, fps)))
    } else {
        Ok(Box::new(OnnxSegmenter::new(shape)?))
    }
}
