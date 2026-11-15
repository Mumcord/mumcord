//! The dedicated effects thread (FR-8.38): sits between the camera and the encoder.
//!
//! The camera hands frames to [`Effects::start`]'s input sink; the thread applies the
//! background effect in place with [`bgmatte::Effects`] and passes each frame on to the output
//! sink (the encoder).
//! When the effects fall behind, new frames are dropped at the input (their buffers go straight
//! back to the camera's pool), so the camera runs at the rate the effects can deliver.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bgmatte::{Background, Config, Frame, ModelShape, OnnxSegmenter, Quality, Segmenter};
use mumcord_camera::{CameraError, CameraFrame, FrameSink};
use mumcord_store::CameraEffectQuality;

use crate::segmenter::{FixtureSegmenter, SlowSegmenter};
use crate::{EffectStats, hooks, runtime};

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
    effects: bgmatte::Effects,
    on_battery: BatteryProbe,
    battery_read_at: Instant,
    unsupported_logged: bool,
    model_error_logged: bool,
    stats: Arc<Mutex<EffectStats>>,
}

impl Worker {
    fn new(config: EffectsConfig, stats: Arc<Mutex<EffectStats>>) -> Self {
        let effects = bgmatte::Effects::with_segmenters(
            Config {
                fps: config.fps,
                quality: quality(config.quality),
                background: config.background,
                on_battery: (config.on_battery)(),
            },
            segmenters(config.fps),
        );
        Self {
            effects,
            on_battery: config.on_battery,
            battery_read_at: Instant::now(),
            unsupported_logged: false,
            model_error_logged: false,
            stats,
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
            Command::Background(background) => self.effects.set_background(background),
            Command::Quality(setting) => self.effects.set_quality(quality(setting)),
        }
        self.publish();
    }

    fn handle(&mut self, frame: &mut CameraFrame) {
        if self.battery_read_at.elapsed() >= BATTERY_POLL {
            self.effects.set_on_battery((self.on_battery)());
            self.battery_read_at = Instant::now();
        }
        let (width, height) = (frame.width, frame.height);
        match self
            .effects
            .process(Frame::nv12(&mut frame.data, width, height), frame.timestamp)
        {
            Ok(()) => {}
            Err(bgmatte::Error::Unavailable(reason)) => {
                tracing::warn!(%reason, "camera background effects unavailable");
            }
            Err(bgmatte::Error::UnsupportedSize { .. }) => {
                if !self.unsupported_logged {
                    tracing::warn!(
                        width,
                        height,
                        "camera frame size not supported by the background effects"
                    );
                    self.unsupported_logged = true;
                }
            }
            Err(err) => {
                if !self.model_error_logged {
                    tracing::error!(%err, "camera background effect failed; sending the plain picture");
                    self.model_error_logged = true;
                }
            }
        }
        self.publish();
    }

    fn publish(&self) {
        *self.stats.lock().unwrap_or_else(PoisonError::into_inner) = self.effects.stats();
    }
}

fn quality(setting: CameraEffectQuality) -> Quality {
    match setting {
        CameraEffectQuality::Auto => Quality::Auto,
        CameraEffectQuality::Best => Quality::Best,
        CameraEffectQuality::Light => Quality::Light,
    }
}

/// Makes the segmenter for each model shape: the fixture outline under
/// `MUMCORD_TEST_SEGMENT_MASK=fixture`, otherwise the real model with the ONNX Runtime Mumcord
/// ships; slowed down under `MUMCORD_TEST_EFFECT_SLOW`.
fn segmenters(
    fps: u32,
) -> impl FnMut(ModelShape) -> Result<Box<dyn Segmenter>, bgmatte::Error> + Send + 'static {
    let fixture = hooks::segment_mask_fixture();
    let delay = hooks::effect_slow();
    move |shape| {
        let segmenter: Box<dyn Segmenter> = if fixture {
            Box::new(FixtureSegmenter::new(shape, fps))
        } else {
            runtime::availability().map_err(bgmatte::Error::Unavailable)?;
            Box::new(OnnxSegmenter::new(shape)?)
        };
        if delay.is_zero() {
            Ok(segmenter)
        } else {
            Ok(Box::new(SlowSegmenter {
                inner: segmenter,
                delay,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_quality_setting() {
        assert_eq!(quality(CameraEffectQuality::Auto), Quality::Auto);
        assert_eq!(quality(CameraEffectQuality::Best), Quality::Best);
        assert_eq!(quality(CameraEffectQuality::Light), Quality::Light);
    }
}
