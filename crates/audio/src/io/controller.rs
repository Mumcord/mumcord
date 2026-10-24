//! Control thread: owns the device streams, switches devices and recovers
//! from hot-plug events without disturbing [`FrameSource`]/[`FrameSink`] users.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer};

use super::cpal_backend::CpalBackend;
use super::frames::{CaptureEndpoint, FrameSink, FrameSource, PlaybackEndpoint};
use super::ring::Counters;
use super::{BackendKind, DeviceEvent, DeviceInfo, Direction, IoError, IoStats};

/// How often the control thread checks for device changes and failed streams.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Keeps a device stream alive; dropping it stops the stream.
pub(crate) trait StreamGuard {
    /// The device reported a fatal error (e.g. it was unplugged).
    fn failed(&self) -> bool;
}

/// A started device stream and the ring end the rest of the crate uses.
pub(crate) struct Opened<R> {
    pub(crate) guard: Box<dyn StreamGuard>,
    pub(crate) ring: R,
    pub(crate) rate: u32,
    pub(crate) device: DeviceInfo,
}

/// A device backend. `id: None` opens the OS default device.
pub(crate) trait Backend {
    fn devices(&self, direction: Direction) -> Result<Vec<DeviceInfo>, IoError>;
    /// Whether the device list can change at runtime and should be polled.
    fn polls_devices(&self) -> bool;
    fn open_input(
        &self,
        id: Option<&str>,
        counters: &Arc<Counters>,
    ) -> Result<Opened<Consumer<f32>>, IoError>;
    fn open_output(
        &self,
        id: Option<&str>,
        counters: &Arc<Counters>,
    ) -> Result<Opened<Producer<f32>>, IoError>;
}

enum Command {
    SetDevice(Direction, Option<String>),
    Devices(Direction, Sender<Result<Vec<DeviceInfo>, IoError>>),
    Shutdown,
}

/// Everything [`AudioIo::start`] hands out.
pub struct AudioHandles {
    pub io: AudioIo,
    /// Captured frames, for the processing chain.
    pub source: FrameSource,
    /// Playback frames, for the mixer.
    pub sink: FrameSink,
    /// Device notifications, for UI and diagnostics.
    pub events: Receiver<DeviceEvent>,
}

/// Controls audio devices. Dropping it stops all streams.
pub struct AudioIo {
    commands: Sender<Command>,
    counters: Arc<Counters>,
    thread: Option<JoinHandle<()>>,
}

impl AudioIo {
    /// Starts audio I/O on the backend chosen by
    /// [`BackendKind::from_env`]. `None` device ids mean the OS default.
    /// Succeeds even if no device can be opened yet (a
    /// [`DeviceEvent::Unavailable`] is sent and opening is retried).
    pub fn start(input: Option<String>, output: Option<String>) -> Result<AudioHandles, IoError> {
        Self::start_with(BackendKind::from_env()?, input, output)
    }

    pub fn start_with(
        kind: BackendKind,
        input: Option<String>,
        output: Option<String>,
    ) -> Result<AudioHandles, IoError> {
        let (commands, command_rx) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        let (capture_tx, capture_rx) = mpsc::channel();
        let (playback_tx, playback_rx) = mpsc::channel();
        let counters = Arc::new(Counters::default());
        let thread_counters = counters.clone();
        let thread = std::thread::Builder::new()
            .name("audio-ctl".to_owned())
            .spawn(move || {
                // Created on this thread: cpal hosts need not be `Send`.
                let backend = make_backend(kind);
                let mut control = Control {
                    backend: backend.as_ref(),
                    counters: thread_counters,
                    events: events_tx,
                    capture: capture_tx,
                    playback: playback_tx,
                    input: Slot::new(input),
                    output: Slot::new(output),
                    known_devices: None,
                };
                control.run(command_rx);
            })
            .map_err(|e| IoError::Backend(e.to_string()))?;
        Ok(AudioHandles {
            io: AudioIo {
                commands,
                counters: counters.clone(),
                thread: Some(thread),
            },
            source: FrameSource::new(capture_rx),
            sink: FrameSink::new(playback_rx, counters),
            events,
        })
    }

    /// Lists devices for `direction`.
    pub fn devices(&self, direction: Direction) -> Result<Vec<DeviceInfo>, IoError> {
        let (reply, response) = mpsc::channel();
        self.send(Command::Devices(direction, reply))?;
        response.recv().map_err(|_| IoError::ControlThreadGone)?
    }

    /// Switches the input device (`None` = OS default) while streams keep running.
    pub fn set_input_device(&self, id: Option<String>) -> Result<(), IoError> {
        self.send(Command::SetDevice(Direction::Input, id))
    }

    /// Switches the output device (`None` = OS default) while streams keep running.
    pub fn set_output_device(&self, id: Option<String>) -> Result<(), IoError> {
        self.send(Command::SetDevice(Direction::Output, id))
    }

    pub fn stats(&self) -> IoStats {
        self.counters.snapshot()
    }

    fn send(&self, command: Command) -> Result<(), IoError> {
        self.commands
            .send(command)
            .map_err(|_| IoError::ControlThreadGone)
    }
}

impl Drop for AudioIo {
    fn drop(&mut self) {
        // The thread may already be gone; joining below still cleans up.
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("audio control thread panicked");
        }
    }
}

fn make_backend(kind: BackendKind) -> Box<dyn Backend> {
    match kind {
        BackendKind::Cpal => Box::new(CpalBackend::new()),
        #[cfg(any(test, feature = "test-backend"))]
        BackendKind::Null => Box::new(super::test_backend::TestBackend::Null),
        #[cfg(any(test, feature = "test-backend"))]
        BackendKind::File { input, output } => {
            Box::new(super::test_backend::TestBackend::File { input, output })
        }
    }
}

/// State for one direction.
struct Slot {
    /// The user's choice; `None` follows the OS default.
    preferred: Option<String>,
    active: Option<Active>,
    /// Whether the last open attempt failed (to report it only once).
    unavailable: bool,
}

struct Active {
    guard: Box<dyn StreamGuard>,
    device: DeviceInfo,
    fallback: bool,
}

impl Slot {
    fn new(preferred: Option<String>) -> Self {
        Self {
            preferred,
            active: None,
            unavailable: false,
        }
    }
}

struct Control<'a> {
    backend: &'a dyn Backend,
    counters: Arc<Counters>,
    events: Sender<DeviceEvent>,
    capture: Sender<CaptureEndpoint>,
    playback: Sender<PlaybackEndpoint>,
    input: Slot,
    output: Slot,
    /// Last seen device list (ids and default flags), to detect hot-plug.
    known_devices: Option<Vec<(String, bool)>>,
}

impl Control<'_> {
    fn run(&mut self, commands: Receiver<Command>) {
        self.reopen(Direction::Input);
        self.reopen(Direction::Output);
        let mut next_poll = Instant::now() + POLL_INTERVAL;
        loop {
            let wait = next_poll.saturating_duration_since(Instant::now());
            match commands.recv_timeout(wait) {
                Ok(Command::SetDevice(direction, id)) => {
                    self.slot(direction).preferred = id;
                    self.reopen(direction);
                }
                Ok(Command::Devices(direction, reply)) => {
                    // The caller may have given up waiting; nothing to do then.
                    let _ = reply.send(self.backend.devices(direction));
                }
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
            if Instant::now() >= next_poll {
                self.poll();
                next_poll = Instant::now() + POLL_INTERVAL;
            }
        }
    }

    fn slot(&mut self, direction: Direction) -> &mut Slot {
        match direction {
            Direction::Input => &mut self.input,
            Direction::Output => &mut self.output,
        }
    }

    /// Detects device list changes and reopens streams that failed, lost
    /// their device, run on a fallback whose preferred device came back, or
    /// follow an OS default that moved.
    fn poll(&mut self) {
        let lists = if self.backend.polls_devices() {
            match (
                self.backend.devices(Direction::Input),
                self.backend.devices(Direction::Output),
            ) {
                (Ok(inputs), Ok(outputs)) => Some((inputs, outputs)),
                (Err(e), _) | (_, Err(e)) => {
                    tracing::warn!("listing audio devices failed: {e}");
                    None
                }
            }
        } else {
            None
        };
        if let Some((inputs, outputs)) = &lists {
            let seen: Vec<(String, bool)> = inputs
                .iter()
                .chain(outputs)
                .map(|d| (d.id.clone(), d.is_default))
                .collect();
            if self
                .known_devices
                .as_ref()
                .is_some_and(|known| *known != seen)
            {
                self.emit(DeviceEvent::DevicesChanged);
            }
            self.known_devices = Some(seen);
        }
        for direction in [Direction::Input, Direction::Output] {
            let list = lists.as_ref().map(|(inputs, outputs)| match direction {
                Direction::Input => inputs.as_slice(),
                Direction::Output => outputs.as_slice(),
            });
            if self.needs_reopen(direction, list) {
                self.reopen(direction);
            }
        }
    }

    fn needs_reopen(&mut self, direction: Direction, list: Option<&[DeviceInfo]>) -> bool {
        let slot = self.slot(direction);
        let Some(active) = &slot.active else {
            return true;
        };
        if active.guard.failed() {
            tracing::warn!("{direction:?} audio stream failed; reopening");
            return true;
        }
        let Some(list) = list else {
            return false;
        };
        let present = |id: &str| list.iter().any(|d| d.id == id);
        let follows_default = active.fallback || slot.preferred.is_none();
        let default_moved = list
            .iter()
            .any(|d| d.is_default && d.id != active.device.id);
        !present(&active.device.id)
            || (active.fallback && slot.preferred.as_deref().is_some_and(present))
            || (follows_default && default_moved)
    }

    /// (Re)opens the stream for `direction`: the preferred device, else the
    /// OS default. Hands the new ring to the frame handle and reports it.
    fn reopen(&mut self, direction: Direction) {
        // Stop the old stream first; some devices allow only one stream.
        self.slot(direction).active = None;
        let preferred = self.slot(direction).preferred.clone();
        let result = match preferred.as_deref() {
            Some(id) => self
                .open(direction, Some(id))
                .map(|a| (a, false))
                .or_else(|e| {
                    tracing::warn!(
                        "chosen {direction:?} device unavailable ({e}); using the default"
                    );
                    self.open(direction, None).map(|a| (a, true))
                }),
            None => self.open(direction, None).map(|a| (a, false)),
        };
        match result {
            Ok(((guard, device), fallback)) => {
                tracing::info!("{direction:?} audio stream started (fallback: {fallback})");
                let slot = self.slot(direction);
                slot.unavailable = false;
                slot.active = Some(Active {
                    guard,
                    device: device.clone(),
                    fallback,
                });
                self.emit(DeviceEvent::Active {
                    direction,
                    device,
                    fallback,
                });
            }
            Err(e) => {
                let slot = self.slot(direction);
                if !slot.unavailable {
                    slot.unavailable = true;
                    tracing::warn!("no {direction:?} audio device available: {e}");
                    self.emit(DeviceEvent::Unavailable {
                        direction,
                        reason: e.to_string(),
                    });
                }
            }
        }
    }

    fn open(
        &self,
        direction: Direction,
        id: Option<&str>,
    ) -> Result<(Box<dyn StreamGuard>, DeviceInfo), IoError> {
        // A send fails only if the frame handle was dropped; the stream then
        // simply has no reader, which is harmless.
        match direction {
            Direction::Input => {
                let opened = self.backend.open_input(id, &self.counters)?;
                let _ = self
                    .capture
                    .send(CaptureEndpoint::new(opened.ring, opened.rate)?);
                Ok((opened.guard, opened.device))
            }
            Direction::Output => {
                let opened = self.backend.open_output(id, &self.counters)?;
                let _ = self
                    .playback
                    .send(PlaybackEndpoint::new(opened.ring, opened.rate)?);
                Ok((opened.guard, opened.device))
            }
        }
    }

    fn emit(&self, event: DeviceEvent) {
        // Nobody listening for events is fine.
        let _ = self.events.send(event);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Instant;

    use super::*;
    use crate::{FRAME_SAMPLES, Frame, SAMPLE_RATE};

    fn write_sine(path: &Path, rate: u32, channels: u16, seconds: f32) {
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
        for i in 0..(rate as f32 * seconds) as usize {
            let s = 0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin();
            for _ in 0..channels {
                writer.write_sample((s * 32767.0) as i16).expect("write");
            }
        }
        writer.finalize().expect("finalize");
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    /// Plays `<in.wav>` through capture → frames → playback and checks the
    /// recorded `<out.wav>` carries the same tone at the same level.
    fn loopback(rate: u32, channels: u16) {
        let dir = std::env::temp_dir().join(format!("mumcord-audio-{}-{rate}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let (input, output) = (dir.join("in.wav"), dir.join("out.wav"));
        write_sine(&input, rate, channels, 0.6);

        let kind = BackendKind::File {
            input: input.clone(),
            output: output.clone(),
        };
        let AudioHandles {
            io,
            mut source,
            mut sink,
            events,
        } = AudioIo::start_with(kind, None, None).expect("start");
        let mut frame: Frame = [0.0; FRAME_SAMPLES];
        let started = Instant::now();
        let mut frames = 0;
        while frames < 50 {
            assert!(
                source.wait_frame(&mut frame, Duration::from_millis(500)),
                "capture stalled"
            );
            assert!(sink.write_frame(&frame), "playback queue full");
            frames += 1;
        }
        // Frames arrive in real time: 50 frames take about 500 ms.
        assert!(started.elapsed() >= Duration::from_millis(400));
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(io.devices(Direction::Input).expect("devices").len(), 1);
        drop(io);

        let active = events
            .try_iter()
            .filter(|e| matches!(e, DeviceEvent::Active { .. }))
            .count();
        assert_eq!(active, 2);

        let mut reader = hound::WavReader::open(&output).expect("open out.wav");
        assert_eq!(reader.spec().sample_rate, SAMPLE_RATE);
        let recorded: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| f32::from(s.expect("sample")) / 32768.0)
            .collect();
        assert!(
            recorded.len() >= 50 * FRAME_SAMPLES,
            "only {} samples",
            recorded.len()
        );
        let voiced: Vec<f32> = recorded
            .iter()
            .copied()
            .filter(|s| s.abs() > 0.01)
            .collect();
        assert!(voiced.len() >= 40 * FRAME_SAMPLES);
        let level = rms(&voiced);
        assert!((level - 0.5 / 2f32.sqrt()).abs() < 0.05, "level {level}");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn file_backend_loops_wav_through_frames() {
        loopback(SAMPLE_RATE, 1);
    }

    #[test]
    fn file_backend_resamples_and_downmixes_44_1_khz_stereo() {
        loopback(44_100, 2);
    }

    #[test]
    fn null_backend_paces_silence_and_survives_device_switch() {
        let handles = AudioIo::start_with(BackendKind::Null, None, None).expect("start");
        let mut source = handles.source;
        let mut frame: Frame = [1.0; FRAME_SAMPLES];
        assert!(source.wait_frame(&mut frame, Duration::from_millis(500)));
        assert!(frame.iter().all(|&s| s == 0.0));
        handles
            .io
            .set_input_device(Some("null".to_owned()))
            .expect("switch");
        let started = Instant::now();
        let mut count = 0;
        while started.elapsed() < Duration::from_millis(300) {
            if source.wait_frame(&mut frame, Duration::from_millis(100)) {
                count += 1;
            }
        }
        assert!(count >= 20, "only {count} frames after switch");
    }

    /// Lists this machine's real devices: `cargo test -p mumcord-audio -- --ignored --nocapture list_devices`.
    #[test]
    #[ignore = "needs audio hardware"]
    fn list_devices() {
        let handles = AudioIo::start_with(BackendKind::Cpal, None, None).expect("start");
        for direction in [Direction::Input, Direction::Output] {
            let started = Instant::now();
            let devices = handles.io.devices(direction).expect("devices");
            println!(
                "{direction:?} ({} devices, listed in {:?}):",
                devices.len(),
                started.elapsed()
            );
            for d in devices {
                println!(
                    "  {} {} [{}]",
                    if d.is_default { "*" } else { " " },
                    d.name,
                    d.id
                );
            }
        }
        let mut source = handles.source;
        let mut frame: Frame = [0.0; FRAME_SAMPLES];
        assert!(
            source.wait_frame(&mut frame, Duration::from_secs(2)),
            "no capture frames"
        );
        for event in handles.events.try_iter() {
            println!("event: {event:?}");
        }
        println!("stats: {:?}", handles.io.stats());
    }
}
