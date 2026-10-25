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
/// While on the fallback, a chosen device that is listed but does not open is tried again
/// after this delay, doubling up to `RETRY_MAX`. A device that newly appears is tried at once.
const RETRY_MIN: Duration = POLL_INTERVAL;
const RETRY_MAX: Duration = Duration::from_secs(60);

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
    /// Whether "chosen device unavailable" was already logged for the current choice.
    chosen_reported: bool,
    /// Whether the chosen device was in the last device list (to notice it reappearing).
    chosen_listed: bool,
    /// While on the fallback: the current retry delay and when to try the chosen device again.
    retry_delay: Duration,
    retry_at: Option<Instant>,
}

/// What a poll decided for one direction.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    Keep,
    /// Stop the stream and open the chosen device, else the default.
    Reopen,
    /// Try the chosen device while the fallback keeps running; switch only if it opens.
    TryChosen,
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
            chosen_reported: false,
            chosen_listed: false,
            retry_delay: RETRY_MIN,
            retry_at: None,
        }
    }

    /// A new choice: report and retry it afresh.
    fn choose(&mut self, preferred: Option<String>) {
        self.preferred = preferred;
        self.chosen_reported = false;
        self.chosen_listed = false;
        self.retry_delay = RETRY_MIN;
        self.retry_at = None;
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
        self.reopen(Direction::Input, Instant::now());
        self.reopen(Direction::Output, Instant::now());
        let mut next_poll = Instant::now() + POLL_INTERVAL;
        loop {
            let wait = next_poll.saturating_duration_since(Instant::now());
            match commands.recv_timeout(wait) {
                Ok(Command::SetDevice(direction, id)) => {
                    self.slot(direction).choose(id);
                    self.reopen(direction, Instant::now());
                }
                Ok(Command::Devices(direction, reply)) => {
                    // The caller may have given up waiting; nothing to do then.
                    let _ = reply.send(self.backend.devices(direction));
                }
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
            if Instant::now() >= next_poll {
                self.poll(Instant::now());
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
    fn poll(&mut self, now: Instant) {
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
            match self.check(direction, list, now) {
                Action::Keep => {}
                Action::Reopen => self.reopen(direction, now),
                Action::TryChosen => self.try_chosen(direction, now),
            }
        }
    }

    fn check(&mut self, direction: Direction, list: Option<&[DeviceInfo]>, now: Instant) -> Action {
        let slot = self.slot(direction);
        // Whether the chosen device just appeared in the list (it was missing before).
        let mut reappeared = false;
        if let (Some(list), Some(chosen)) = (list, slot.preferred.as_deref()) {
            let listed = list.iter().any(|d| d.id == chosen);
            reappeared = listed && !slot.chosen_listed;
            slot.chosen_listed = listed;
        }
        let Some(active) = &slot.active else {
            return Action::Reopen;
        };
        if active.guard.failed() {
            tracing::warn!("{direction:?} audio stream failed; reopening");
            return Action::Reopen;
        }
        let Some(list) = list else {
            return Action::Keep;
        };
        let follows_default = active.fallback || slot.preferred.is_none();
        let default_moved = list
            .iter()
            .any(|d| d.is_default && d.id != active.device.id);
        if !list.iter().any(|d| d.id == active.device.id) || (follows_default && default_moved) {
            return Action::Reopen;
        }
        // Listed is not the same as usable: a listed device that fails to open must not tear
        // down the running fallback every poll, so it is retried with a growing delay.
        let retry_due = slot.retry_at.is_none_or(|at| now >= at);
        if active.fallback && slot.chosen_listed && (reappeared || retry_due) {
            if reappeared {
                slot.retry_delay = RETRY_MIN;
            }
            return Action::TryChosen;
        }
        Action::Keep
    }

    /// While on the fallback: opens the chosen device and switches to it only if that works,
    /// so the fallback stream keeps running when the chosen device is still unusable.
    fn try_chosen(&mut self, direction: Direction, now: Instant) {
        let Some(chosen) = self.slot(direction).preferred.clone() else {
            return;
        };
        match self.open(direction, Some(&chosen)) {
            Ok((guard, device)) => {
                tracing::info!("chosen {direction:?} device is back; switched to it");
                self.started(direction, guard, device, false);
            }
            Err(e) => {
                tracing::debug!("chosen {direction:?} device still unavailable ({e})");
                self.schedule_retry(direction, now);
            }
        }
    }

    fn schedule_retry(&mut self, direction: Direction, now: Instant) {
        let slot = self.slot(direction);
        slot.retry_at = Some(now + slot.retry_delay);
        slot.retry_delay = (slot.retry_delay * 2).min(RETRY_MAX);
    }

    /// (Re)opens the stream for `direction`: the preferred device, else the
    /// OS default. Hands the new ring to the frame handle and reports it.
    fn reopen(&mut self, direction: Direction, now: Instant) {
        // Stop the old stream first; some devices allow only one stream.
        self.slot(direction).active = None;
        let preferred = self.slot(direction).preferred.clone();
        let result = match preferred.as_deref() {
            Some(id) => self
                .open(direction, Some(id))
                .map(|a| (a, false))
                .or_else(|e| {
                    let slot = self.slot(direction);
                    if !slot.chosen_reported {
                        slot.chosen_reported = true;
                        tracing::warn!(
                            "chosen {direction:?} device unavailable ({e}); using the default"
                        );
                    }
                    self.schedule_retry(direction, now);
                    self.open(direction, None).map(|a| (a, true))
                }),
            None => self.open(direction, None).map(|a| (a, false)),
        };
        match result {
            Ok(((guard, device), fallback)) => self.started(direction, guard, device, fallback),
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

    /// Records a newly started stream (replacing and stopping the old one) and reports it.
    fn started(
        &mut self,
        direction: Direction,
        guard: Box<dyn StreamGuard>,
        device: DeviceInfo,
        fallback: bool,
    ) {
        tracing::info!("{direction:?} audio stream started (fallback: {fallback})");
        let slot = self.slot(direction);
        slot.unavailable = false;
        if !fallback {
            slot.chosen_reported = false;
            slot.retry_delay = RETRY_MIN;
            slot.retry_at = None;
        }
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
        // Both streams are opened on the controller thread; the sink only
        // accepts frames once the output is up, which on a loaded machine
        // can be well after the first captured frame.
        let mut active = 0;
        while active < 2 {
            match events
                .recv_timeout(Duration::from_secs(10))
                .expect("stream events")
            {
                DeviceEvent::Active { .. } => active += 1,
                other => panic!("unexpected event {other:?}"),
            }
        }
        let mut frame: Frame = [0.0; FRAME_SAMPLES];
        let started = Instant::now();
        let mut frames = 0;
        while frames < 50 {
            assert!(
                source.wait_frame(&mut frame, Duration::from_secs(2)),
                "capture stalled"
            );
            // Paced like a real mixer: the capture and playback threads run
            // on independent clocks, and the capture backlog built up while
            // the output opened keeps the playback queue near full, so wait
            // for room instead of dropping the frame. A loaded runner can
            // deschedule the whole process for seconds and wake this thread
            // before the playback thread catches up, so only fail when the
            // queue stops draining for a long time, not on wall-clock alone.
            let mut queued = sink.queued_samples();
            let mut last_drain = Instant::now();
            while !sink.write_frame(&frame) {
                let now_queued = sink.queued_samples();
                if now_queued < queued {
                    last_drain = Instant::now();
                }
                queued = now_queued;
                assert!(
                    last_drain.elapsed() < Duration::from_secs(10),
                    "playback queue stuck full"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            frames += 1;
        }
        // Frames arrive in real time: 50 frames take about 500 ms.
        assert!(started.elapsed() >= Duration::from_millis(400));
        // Let the queued tail play out before the backend is closed.
        let deadline = Instant::now() + Duration::from_secs(5);
        while sink.queued_samples() > 0 {
            assert!(Instant::now() < deadline, "playback queue never drained");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(io.devices(Direction::Input).expect("devices").len(), 1);
        drop(io);

        let extra_active = events
            .try_iter()
            .filter(|e| matches!(e, DeviceEvent::Active { .. }))
            .count();
        assert_eq!(extra_active, 0, "streams reopened unexpectedly");

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

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mumcord-audio-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// 48 kHz mono 16-bit silence, written directly (hound is slow for minutes of audio).
    fn write_silence(path: &Path, seconds: u32) {
        let bytes = 48_000 * 2 * seconds;
        let mut header = Vec::with_capacity(44);
        header.extend_from_slice(b"RIFF");
        header.extend_from_slice(&(36 + bytes).to_le_bytes());
        header.extend_from_slice(b"WAVEfmt ");
        header.extend_from_slice(&16u32.to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes());
        header.extend_from_slice(&48_000u32.to_le_bytes());
        header.extend_from_slice(&96_000u32.to_le_bytes());
        header.extend_from_slice(&2u16.to_le_bytes());
        header.extend_from_slice(&16u16.to_le_bytes());
        header.extend_from_slice(b"data");
        header.extend_from_slice(&bytes.to_le_bytes());
        header.resize(44 + bytes as usize, 0);
        std::fs::write(path, header).expect("write silence");
    }

    fn wait_active(events: &Receiver<DeviceEvent>, count: usize, within: Duration) {
        let deadline = Instant::now() + within;
        let mut active = 0;
        while active < count {
            let left = deadline.saturating_duration_since(Instant::now());
            match events.recv_timeout(left) {
                Ok(DeviceEvent::Active { .. }) => active += 1,
                Ok(other) => panic!("unexpected event {other:?}"),
                Err(_) => panic!("only {active} of {count} streams active after {within:?}"),
            }
        }
    }

    /// Plays `frames` frames of `level` and waits until they reached the output file.
    fn play(sink: &mut FrameSink, level: f32, frames: usize) {
        let frame: Frame = [level; FRAME_SAMPLES];
        let deadline = Instant::now() + Duration::from_secs(10);
        for _ in 0..frames {
            while !sink.write_frame(&frame) {
                assert!(Instant::now() < deadline, "playback stuck");
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        while sink.queued_samples() > 0 {
            assert!(Instant::now() < deadline, "playback never drained");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn recorded(path: &Path) -> Vec<f32> {
        hound::WavReader::open(path)
            .expect("open out.wav")
            .samples::<i16>()
            .map(|s| f32::from(s.expect("sample")) / 32768.0)
            .collect()
    }

    /// F23: a long input file (E2E runs use ten minutes of silence) used to be decoded on the
    /// control thread before the output opened, so speakers started seconds late.
    #[test]
    fn file_backend_opens_output_promptly_with_a_long_input() {
        let dir = temp_dir("long-input");
        let (input, output) = (dir.join("in.wav"), dir.join("out.wav"));
        write_silence(&input, 600);
        let kind = BackendKind::File {
            input,
            output: output.clone(),
        };
        let handles = AudioIo::start_with(kind, None, None).expect("start");
        wait_active(&handles.events, 2, Duration::from_secs(2));
        drop(handles);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// F23: reopening the output (device switch, or the engine closing and reopening the
    /// devices between connections) continues the recording instead of starting it anew.
    #[test]
    fn file_backend_output_continues_across_reopens() {
        let dir = temp_dir("reopen");
        let (input, output) = (dir.join("in.wav"), dir.join("out.wav"));
        write_silence(&input, 1);
        let kind = BackendKind::File {
            input,
            output: output.clone(),
        };

        let mut handles = AudioIo::start_with(kind.clone(), None, None).expect("start");
        wait_active(&handles.events, 2, Duration::from_secs(10));
        play(&mut handles.sink, 0.5, 20);
        handles
            .io
            .set_output_device(Some(format!("file:{}", output.display())))
            .expect("switch");
        wait_active(&handles.events, 1, Duration::from_secs(10));
        play(&mut handles.sink, 0.25, 20);
        drop(handles);

        let mut handles = AudioIo::start_with(kind, None, None).expect("restart");
        wait_active(&handles.events, 2, Duration::from_secs(10));
        play(&mut handles.sink, 0.125, 20);
        drop(handles);

        let samples = recorded(&output);
        for level in [0.5f32, 0.25, 0.125] {
            let count = samples
                .iter()
                .filter(|s| (**s - level).abs() < 0.01)
                .count();
            assert!(
                count >= 20 * FRAME_SAMPLES,
                "only {count} samples at {level} in the recording"
            );
        }
        let first = |level: f32| samples.iter().position(|s| (*s - level).abs() < 0.01);
        assert!(first(0.5) < first(0.25) && first(0.25) < first(0.125));
        std::fs::remove_dir_all(&dir).expect("cleanup");
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
        // Count to a fixed number of frames under a generous deadline instead of counting
        // frames in a fixed window: a loaded CI runner may deliver them late, never early.
        const WANTED: u32 = 50;
        let started = Instant::now();
        let deadline = started + Duration::from_secs(10);
        let mut count = 0;
        while count < WANTED && Instant::now() < deadline {
            if source.wait_frame(&mut frame, Duration::from_millis(100)) {
                count += 1;
            }
        }
        assert_eq!(count, WANTED, "only {count} frames after switch");
        // Nominal 500 ms at 10 ms per frame; the margin allows for catch-up after a stall
        // while still failing if silence is delivered unpaced.
        assert!(
            started.elapsed() >= Duration::from_millis(250),
            "{WANTED} frames in {:?}: not paced",
            started.elapsed()
        );
    }

    /// Opens a real device by the id the device list shows (as saved in the settings); a sound
    /// card that PipeWire or PulseAudio holds reports "busy" here, not "not found":
    /// `MUMCORD_TEST_DEVICE=alsa:sysdefault:CARD=X cargo test -p mumcord-audio -- --ignored
    /// --nocapture opens_listed_device_by_id`.
    #[test]
    #[ignore = "needs audio hardware and MUMCORD_TEST_DEVICE"]
    fn opens_listed_device_by_id() {
        let id = std::env::var("MUMCORD_TEST_DEVICE").expect("MUMCORD_TEST_DEVICE");
        let backend = CpalBackend::new();
        let counters = Arc::new(Counters::default());
        if let Err(e) = backend.open_input(Some(&id), &counters) {
            panic!("opening the input failed: {e}");
        }
        if let Err(e) = backend.open_output(Some(&id), &counters) {
            panic!("opening the output failed: {e}");
        }
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

    mod fallback {
        use std::cell::{Cell, RefCell};
        use std::rc::Rc;

        use rtrb::RingBuffer;

        use super::*;

        const CHOSEN: &str = "alsa:sysdefault:CARD=II";
        const DEFAULT: &str = "alsa:default";

        /// A backend whose device list and openable devices the test controls.
        #[derive(Default)]
        struct Fake {
            listed: RefCell<Vec<String>>,
            openable: RefCell<Vec<String>>,
            /// Input open attempts, by device id.
            attempts: RefCell<Vec<String>>,
            /// Streams currently running.
            live: Rc<Cell<usize>>,
        }

        struct Guard(Rc<Cell<usize>>);

        impl StreamGuard for Guard {
            fn failed(&self) -> bool {
                false
            }
        }

        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.set(self.0.get() - 1);
            }
        }

        impl Fake {
            fn new(listed: &[&str], openable: &[&str]) -> Self {
                let fake = Self::default();
                fake.set(listed, openable);
                fake
            }

            fn set(&self, listed: &[&str], openable: &[&str]) {
                *self.listed.borrow_mut() = listed.iter().map(|s| (*s).to_owned()).collect();
                *self.openable.borrow_mut() = openable.iter().map(|s| (*s).to_owned()).collect();
            }

            fn attempts(&self, id: &str) -> usize {
                self.attempts.borrow().iter().filter(|a| *a == id).count()
            }

            fn open<R>(
                &self,
                direction: Direction,
                id: Option<&str>,
                ring: R,
            ) -> Result<Opened<R>, IoError> {
                let id = id.unwrap_or(DEFAULT).to_owned();
                if direction == Direction::Input {
                    self.attempts.borrow_mut().push(id.clone());
                }
                if !self.openable.borrow().contains(&id) {
                    return Err(IoError::DeviceNotFound);
                }
                self.live.set(self.live.get() + 1);
                Ok(Opened {
                    guard: Box::new(Guard(self.live.clone())),
                    ring,
                    rate: SAMPLE_RATE,
                    device: DeviceInfo {
                        is_default: id == DEFAULT,
                        name: id.clone(),
                        id,
                    },
                })
            }
        }

        impl Backend for Fake {
            fn devices(&self, _direction: Direction) -> Result<Vec<DeviceInfo>, IoError> {
                Ok(self
                    .listed
                    .borrow()
                    .iter()
                    .map(|id| DeviceInfo {
                        id: id.clone(),
                        name: id.clone(),
                        is_default: id == DEFAULT,
                    })
                    .collect())
            }

            fn polls_devices(&self) -> bool {
                true
            }

            fn open_input(
                &self,
                id: Option<&str>,
                _counters: &Arc<Counters>,
            ) -> Result<Opened<Consumer<f32>>, IoError> {
                self.open(Direction::Input, id, RingBuffer::new(64).1)
            }

            fn open_output(
                &self,
                id: Option<&str>,
                _counters: &Arc<Counters>,
            ) -> Result<Opened<Producer<f32>>, IoError> {
                self.open(Direction::Output, id, RingBuffer::new(64).0)
            }
        }

        /// A control thread state whose input prefers `CHOSEN` and output follows the default.
        fn control(fake: &Fake) -> (Control<'_>, Receiver<DeviceEvent>) {
            let (events, events_rx) = mpsc::channel();
            let control = Control {
                backend: fake,
                counters: Arc::new(Counters::default()),
                events,
                capture: mpsc::channel().0,
                playback: mpsc::channel().0,
                input: Slot::new(Some(CHOSEN.to_owned())),
                output: Slot::new(None),
                known_devices: None,
            };
            (control, events_rx)
        }

        fn actives(events: &Receiver<DeviceEvent>) -> Vec<bool> {
            events
                .try_iter()
                .filter_map(|e| match e {
                    DeviceEvent::Active {
                        direction: Direction::Input,
                        fallback,
                        ..
                    } => Some(fallback),
                    _ => None,
                })
                .collect()
        }

        #[test]
        fn listed_but_unusable_choice_keeps_the_fallback_running() {
            let fake = Fake::new(&[DEFAULT, CHOSEN], &[DEFAULT]);
            let (mut control, events) = control(&fake);
            let t0 = Instant::now();
            control.reopen(Direction::Input, t0);
            assert_eq!(actives(&events), [true]);
            // Two minutes of polls: the fallback is never restarted and the chosen device is
            // retried with a growing delay instead of on every poll.
            for step in 1..=60 {
                control.poll(t0 + POLL_INTERVAL * step);
            }
            assert_eq!(fake.attempts(DEFAULT), 1, "fallback opened once");
            assert!(actives(&events).is_empty(), "no stream restarts");
            let retries = fake.attempts(CHOSEN);
            assert!(
                (2..=8).contains(&retries),
                "backoff, got {retries} attempts"
            );
            assert_eq!(fake.live.get(), 2, "one input and one output stream run");
        }

        #[test]
        fn switches_back_once_the_choice_reappears_and_opens() {
            let fake = Fake::new(&[DEFAULT], &[DEFAULT]);
            let (mut control, events) = control(&fake);
            let t0 = Instant::now();
            control.reopen(Direction::Input, t0);
            for step in 1..=5 {
                control.poll(t0 + POLL_INTERVAL * step);
            }
            assert_eq!(fake.attempts(CHOSEN), 1, "missing device is not retried");
            assert_eq!(actives(&events), [true]);
            fake.set(&[DEFAULT, CHOSEN], &[DEFAULT, CHOSEN]);
            control.poll(t0 + POLL_INTERVAL * 6);
            assert_eq!(actives(&events), [false], "back on the chosen device");
            assert_eq!(fake.attempts(DEFAULT), 1);
            assert_eq!(fake.live.get(), 2, "the fallback input stream was stopped");
            control.poll(t0 + POLL_INTERVAL * 7);
            assert!(actives(&events).is_empty());
        }
    }
}
