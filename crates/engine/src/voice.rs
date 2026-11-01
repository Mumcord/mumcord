//! Voice wiring: audio devices, the send path (capture → DSP → Opus → voice transport), the
//! receive path (voice transport → playback mixer), talk indicators, the mic level meter and
//! the microphone test.
//!
//! Threads:
//! - `mumcord-capture` (one per connection) runs the DSP chain and the encoder. It is not a
//!   device callback (those live in `mumcord_audio::io`), so allocation there is accepted.
//! - The playback thread belongs to [`Playback`]; it gets the echo-cancellation tap.
//! - Incoming packets are pushed by the session task, the single producer for UDP and TCP.
//! - Talk indicators and the level meter are polled by small async tasks.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bytes::Bytes;
use mumcord_audio::codec::{
    CodecError, MAX_PACKET_BYTES, MAX_VOICE_PAYLOAD_BYTES, OpusEncoder, PacketDuration,
};
use mumcord_audio::dsp::{DspChain, DspControls, EchoRefConsumer, Gate, MicTest, echo_reference};
use mumcord_audio::io::{
    AudioIo, DeviceEvent, DeviceInfo, Direction, FrameSink, FrameSource, IoError, IoStats,
};
use mumcord_audio::mixer::share::ShareGain;
use mumcord_audio::mixer::{
    FrameTime, PacketSender, Playback, PlaybackConfig, PlaybackSink, PlaybackStats,
};
use mumcord_audio::rt::RtGuard;
use mumcord_audio::{FRAME_SAMPLES, Frame, SAMPLE_RATE};
use mumcord_protocol::{OutgoingAudio, VoiceTransport};
use tokio::runtime::Handle as Runtime;
use tokio::task::JoinHandle as TaskHandle;

use crate::audio_rating::HealthHistory;
use crate::positional::PositionalShared;
use crate::screen_share::sound::ShareSoundHub;
use crate::session::batch::TalkBatch;
use crate::session::{
    AudioDeviceList, AudioDeviceView, EngineError, EngineEvent, EngineResult, Events,
};

/// Interval of `MicLevel` events (about 20 per second).
// The encoder caps every packet at the audio crate's budget; it must be the one the protocol
// crate derives from Mumble's voice packet limit, or packets would be dropped (issue #7).
const _: () = assert!(MAX_VOICE_PAYLOAD_BYTES == mumcord_protocol::MAX_VOICE_PAYLOAD);

const METER_INTERVAL: Duration = Duration::from_millis(50);
/// Poll interval of talk indicators; well inside the 200 ms of AC-1.7.
const TALK_POLL: Duration = Duration::from_millis(20);
/// Longest mic-test recording (B8: record, then play back).
pub const MIC_TEST_MAX: Duration = Duration::from_secs(5);
/// How long the hang-up player outlives its sound: covers the mixer's and the device's queues.
const TAIL_MARGIN: Duration = Duration::from_millis(250);
/// Initial encoder bitrate until the bitrate controller reports a target.
const START_BITRATE_BPS: u32 = 72_000;
/// Captured audio queued beyond this when a frame is read means the capture thread fell behind.
const CAPTURE_LATE_SAMPLES: usize = 2 * FRAME_SAMPLES;
/// Longest stretch the capture thread works through a backlog without sleeping. At real-time
/// priority Linux counts such a stretch toward `RLIMIT_RTTIME` and demotes the thread past it
/// (issue #139); a 1 ms nap resets the count and still lets it catch up.
const CAPTURE_BURST: Duration = Duration::from_millis(20);

/// Maps a peak level in dBFS to the 0–1 meter scale (−60 dBFS and below read as 0).
pub fn meter_level(peak_dbfs: f32) -> f32 {
    ((peak_dbfs + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// Playback sink shared between voice sessions and the mic test: [`Playback`] owns its sink by
/// value, so the device sink lives in this slot and is lent out. The lock is only contended
/// while switching between voice and mic test, never by a device callback.
#[derive(Clone)]
struct SharedSink {
    sink: Arc<Mutex<Option<FrameSink>>>,
    /// Master volume as `f32` bits (1.0 = unchanged); read by the playback thread, set by the UI.
    volume: Arc<AtomicU32>,
}

impl SharedSink {
    fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    fn with<R>(&self, f: impl FnOnce(&mut Option<FrameSink>) -> R) -> R {
        f(&mut self.sink.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl PlaybackSink for SharedSink {
    fn write_frame(&mut self, frame: &Frame) -> bool {
        let volume = self.volume();
        let scaled = frame.map(|sample| sample * volume);
        let frame = if volume == 1.0 { frame } else { &scaled };
        self.with(|sink| sink.as_mut().is_some_and(|s| s.write_frame(frame)))
    }

    fn write_stereo(&mut self, left: &Frame, right: &Frame) -> bool {
        let volume = self.volume();
        let (scaled_left, scaled_right) = (
            left.map(|sample| sample * volume),
            right.map(|sample| sample * volume),
        );
        let (left, right) = if volume == 1.0 {
            (left, right)
        } else {
            (&scaled_left, &scaled_right)
        };
        self.with(|sink| sink.as_mut().is_some_and(|s| s.write_stereo(left, right)))
    }

    fn queued_samples(&mut self) -> usize {
        self.with(|sink| sink.as_mut().map_or(0, FrameSink::queued_samples))
    }
}

/// Running audio devices; started on first use and kept for the app's lifetime.
pub(crate) struct Audio {
    io: Arc<AudioIo>,
    /// `None` while lent to a voice pipeline or the mic test.
    source: Option<FrameSource>,
    sink: SharedSink,
}

impl Audio {
    pub fn start(
        input: Option<String>,
        output: Option<String>,
        events: Events,
    ) -> EngineResult<Self> {
        let handles = AudioIo::start(input, output)
            .map_err(|e| EngineError::internal(format!("audio: {e}")))?;
        let io = Arc::new(handles.io);
        let weak = Arc::downgrade(&io);
        let device_events = handles.events;
        std::thread::Builder::new()
            .name("mumcord-devices".into())
            .spawn(move || forward_device_events(&device_events, &weak, &events))
            .map_err(|e| EngineError::internal(format!("device thread: {e}")))?;
        Ok(Self {
            io,
            source: Some(handles.source),
            sink: SharedSink {
                sink: Arc::new(Mutex::new(Some(handles.sink))),
                volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            },
        })
    }

    pub fn devices(&self) -> EngineResult<AudioDeviceList> {
        device_list(&self.io)
    }

    pub fn set_devices(&self, input: Option<String>, output: Option<String>) -> EngineResult<()> {
        let err = |e| EngineError::internal(format!("audio: {e}"));
        self.io.set_input_device(input).map_err(err)?;
        self.io.set_output_device(output).map_err(err)
    }

    /// Smallest safe device buffers in low-latency mode (issue #156); reopens the streams.
    pub fn set_low_latency(&self, on: bool) {
        if let Err(e) = self.io.set_low_latency(on) {
            tracing::warn!(error = %e, "switching the device buffers failed");
        }
    }

    /// Master volume of everything played, in percent (100 = unchanged).
    pub fn set_output_volume_percent(&self, percent: u16) {
        self.sink
            .volume
            .store((f32::from(percent) / 100.0).to_bits(), Ordering::Relaxed);
    }

    pub fn io(&self) -> &AudioIo {
        &self.io
    }

    /// Lends source and sink to a mic test.
    pub fn take_devices(&mut self) -> Option<(FrameSource, FrameSink)> {
        let source = self.source.take()?;
        match self.sink.with(Option::take) {
            Some(sink) => Some((source, sink)),
            None => {
                self.source = Some(source);
                None
            }
        }
    }

    pub fn return_devices(&mut self, source: FrameSource, sink: FrameSink) {
        self.source = Some(source);
        self.sink.with(|slot| *slot = Some(sink));
    }

    /// Plays `samples` independent of any voice pipeline (the hang-up sound, QA.19): a
    /// short-lived mixer feeds the shared sink and, together with the devices, lives until the
    /// sound is through, even if this `Audio` is released right after. Call it once the voice
    /// pipeline has stopped, so only one mixer writes to the sink. Never blocks. Returns whether
    /// playback started; a failure is logged.
    pub fn play_tail(&self, rt: &Runtime, samples: &[f32]) -> bool {
        let (playback, ports) = match Playback::start(self.sink.clone(), PlaybackConfig::default())
        {
            Ok(started) => started,
            Err(e) => {
                tracing::warn!(error = %e, "closing sound: starting playback failed");
                return false;
            }
        };
        let mut mix_in = ports.mix_in;
        if samples.iter().any(|&s| mix_in.push(s).is_err()) {
            tracing::debug!("closing sound cut short: mixer input is full");
        }
        let io = self.io.clone();
        let linger =
            Duration::from_secs_f64(samples.len() as f64 / f64::from(SAMPLE_RATE)) + TAIL_MARGIN;
        rt.spawn(async move {
            tokio::time::sleep(linger).await;
            drop((playback, mix_in, io));
        });
        true
    }
}

fn device_list(io: &AudioIo) -> EngineResult<AudioDeviceList> {
    device_list_with(|direction| io.devices(direction))
}

/// The device list while no audio runs, without opening any stream.
pub fn idle_device_list() -> EngineResult<AudioDeviceList> {
    device_list_with(AudioIo::list_devices)
}

fn device_list_with(
    devices: impl Fn(Direction) -> Result<Vec<DeviceInfo>, IoError>,
) -> EngineResult<AudioDeviceList> {
    let list = |direction| {
        devices(direction)
            .map(|devices| devices.into_iter().map(device_view).collect())
            .map_err(|e| EngineError::internal(format!("audio devices: {e}")))
    };
    Ok(AudioDeviceList {
        inputs: list(Direction::Input)?,
        outputs: list(Direction::Output)?,
    })
}

fn device_view(d: DeviceInfo) -> AudioDeviceView {
    AudioDeviceView {
        id: d.id,
        name: d.name,
        is_default: d.is_default,
    }
}

fn forward_device_events(
    rx: &std::sync::mpsc::Receiver<DeviceEvent>,
    io: &Weak<AudioIo>,
    events: &Events,
) {
    // Whether each direction is already known to run on the fallback, so the notice is sent once.
    let (mut input_fallback, mut output_fallback) = (false, false);
    while let Ok(event) = rx.recv() {
        match event {
            DeviceEvent::DevicesChanged => {
                let Some(io) = io.upgrade() else { return };
                match device_list(&io) {
                    Ok(list) => events.emit(EngineEvent::AudioDevicesChanged(list)),
                    Err(e) => tracing::warn!(error = %e, "listing audio devices failed"),
                }
            }
            DeviceEvent::Active {
                direction,
                fallback,
                ..
            } => {
                tracing::info!(?direction, fallback, "audio stream active");
                let input = direction == Direction::Input;
                let known = if input {
                    &mut input_fallback
                } else {
                    &mut output_fallback
                };
                if fallback && !*known {
                    events.emit(EngineEvent::AudioDeviceFallback { input });
                }
                *known = fallback;
            }
            // The reason can name a device; device names stay out of logs.
            DeviceEvent::Unavailable { direction, .. } => {
                tracing::warn!(?direction, "no audio device available");
            }
        }
    }
}

/// Send/receive switches shared with the capture thread and the session task.
#[derive(Debug)]
pub(crate) struct VoiceShared {
    pub muted: AtomicBool,
    pub deaf: AtomicBool,
    pub bitrate_bps: AtomicU32,
    pub loss_percent: AtomicU32,
    /// Highest bitrate the bitrate slider can reach (FR-2.64); 0 while no voice server is known.
    pub bitrate_max_bps: AtomicU32,
    /// Voice packets handed to the transport / received, for the connection info.
    pub packets_sent: AtomicU64,
    pub packets_received: AtomicU64,
    /// Time the capture thread needs per 10 ms frame (DSP chain and Opus encoder).
    pub capture_time: FrameTime,
    /// Frames the capture thread read with more than [`CAPTURE_LATE_SAMPLES`] still queued while
    /// sending (a backlog while the gate is closed delays nothing anyone hears).
    pub capture_late_frames: AtomicU64,
    /// Voice threads (capture, playback) running with real-time scheduling.
    pub realtime_threads: AtomicU32,
    /// Listener pose and own position for positional audio.
    pub positional: Arc<PositionalShared>,
    /// Where watched screen shares' sound goes while the pipeline runs (FR-2.42).
    pub share_sound: ShareSoundHub,
    /// Level of watched screen shares' sound, kept across pipelines (auto-ducking, FR-2.76).
    pub share_gain: Arc<ShareGain>,
}

impl VoiceShared {
    /// A new connection starts with the start bitrate and fresh counters.
    pub fn reset_link(&self) {
        self.bitrate_bps.store(START_BITRATE_BPS, Ordering::Relaxed);
        self.loss_percent.store(0, Ordering::Relaxed);
        self.bitrate_max_bps.store(0, Ordering::Relaxed);
        self.packets_sent.store(0, Ordering::Relaxed);
        self.packets_received.store(0, Ordering::Relaxed);
        self.capture_time.reset();
        self.capture_late_frames.store(0, Ordering::Relaxed);
    }
}

impl Default for VoiceShared {
    fn default() -> Self {
        Self {
            muted: AtomicBool::new(false),
            deaf: AtomicBool::new(false),
            bitrate_bps: AtomicU32::new(START_BITRATE_BPS),
            loss_percent: AtomicU32::new(0),
            bitrate_max_bps: AtomicU32::new(0),
            packets_sent: AtomicU64::new(0),
            packets_received: AtomicU64::new(0),
            capture_time: FrameTime::default(),
            capture_late_frames: AtomicU64::new(0),
            realtime_threads: AtomicU32::new(0),
            positional: Arc::default(),
            share_sound: ShareSoundHub::default(),
            share_gain: Arc::default(),
        }
    }
}

/// Which consumers want `MicLevel` events.
#[derive(Debug, Default)]
pub(crate) struct Meter {
    pub voice: AtomicBool,
    pub test: AtomicBool,
}

pub(crate) fn spawn_meter(
    rt: &Runtime,
    controls: Arc<DspControls>,
    meter: Arc<Meter>,
    events: Events,
) -> TaskHandle<()> {
    rt.spawn(async move {
        let mut tick = tokio::time::interval(METER_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            if meter.voice.load(Ordering::Relaxed) || meter.test.load(Ordering::Relaxed) {
                let level = meter_level(controls.level().peak_dbfs);
                events.emit(EngineEvent::MicLevel(level));
            }
        }
    })
}

/// The per-connection audio pipeline.
pub(crate) struct VoicePipeline {
    playback: Playback,
    stop: Arc<AtomicBool>,
    capture: Option<JoinHandle<FrameSource>>,
    talk_task: TaskHandle<()>,
    sounds: Arc<crate::sounds::SoundHub>,
    shared: Arc<VoiceShared>,
    /// Device counters when the pipeline started; the connection info shows what happened since.
    io_base: IoStats,
    health: HealthHistory,
    /// Low-latency mode of the playback side, as last sent to the mixer thread.
    low_latency: bool,
    queue_target_ms: u32,
}

impl VoicePipeline {
    /// Starts capture and playback on the devices of `audio`; returns the packet input of the
    /// mixer for the session task.
    pub fn start(
        rt: &Runtime,
        audio: &mut Audio,
        controls: Arc<DspControls>,
        transport: Arc<VoiceTransport>,
        shared: Arc<VoiceShared>,
        events: Events,
        own_session: u32,
    ) -> EngineResult<(Self, PacketSender)> {
        let source = audio
            .source
            .take()
            .ok_or_else(|| EngineError::internal("microphone is in use by the mic test"))?;
        let io_base = audio.io().stats();
        shared.realtime_threads.store(0, Ordering::Relaxed);
        let (mut echo_producer, echo_consumer) = echo_reference();
        let config = PlaybackConfig {
            far_end_tap: Some(Box::new(move |frame: &Frame| {
                // A full reference queue only degrades echo cancellation briefly.
                let _ = echo_producer.push(frame);
            })),
            listener: Some(shared.positional.listener.clone()),
            share_gain: shared.share_gain.clone(),
            on_thread_start: Some({
                let shared = shared.clone();
                Box::new(move || Box::new(realtime("playback", &shared)))
            }),
            ..PlaybackConfig::default()
        };
        let (mut playback, ports) = match Playback::start(audio.sink.clone(), config) {
            Ok(started) => started,
            Err(e) => {
                audio.source = Some(source);
                return Err(EngineError::internal(format!("playback: {e}")));
            }
        };
        // A fresh command queue has room; a refusal would only mean normal buffering.
        let low_latency = controls.low_latency() && playback.set_low_latency(true);

        let stop = Arc::new(AtomicBool::new(false));
        let capture = {
            let stop = stop.clone();
            let controls = controls.clone();
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("mumcord-capture".into())
                .spawn(move || {
                    capture_loop(source, controls, echo_consumer, &transport, &shared, &stop)
                })
                .map_err(|e| EngineError::internal(format!("capture thread: {e}")))?
        };

        // Notification sounds (B26) go through the mixer's mix-in port while this pipeline lives.
        let sounds = events.sounds().clone();
        let _ = playback.set_mix_in_gain(crate::sounds::SOUND_GAIN);
        let mut mix_in = ports.mix_in;
        sounds.attach(Box::new(move |samples| {
            if mix_in.slots() < samples.len() {
                return false;
            }
            samples.iter().for_each(|&s| {
                let _ = mix_in.push(s);
            });
            true
        }));

        shared.share_sound.attach(ports.shares);
        let pipeline_shared = shared.clone();

        let mut talk_events = ports.talk_events;
        let talk_task = rt.spawn(async move {
            // Rate-limited per person so busy channels do not flood the UI (FR-2.23).
            let mut batch = TalkBatch::default();
            let mut tick = tokio::time::interval(TALK_POLL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                let now = tick.tick().await;
                while let Ok(e) = talk_events.pop() {
                    batch.set(e.session, e.talking, now);
                }
                let demoted = mumcord_audio::rt::take_demotions();
                if demoted > 0 {
                    // Logged here, not in the voice threads: they must not block on logging.
                    tracing::warn!(
                        demoted,
                        "a voice thread ran past its real-time budget; back at normal priority"
                    );
                    // Only the voice threads' start adds to the count, before any demotion.
                    let threads = shared.realtime_threads.load(Ordering::Relaxed);
                    shared
                        .realtime_threads
                        .store(threads.saturating_sub(demoted), Ordering::Relaxed);
                }
                let talking = (controls.transmitting() || controls.voice_target() != 0)
                    && !shared.muted.load(Ordering::Relaxed);
                batch.set(own_session, talking, now);
                for (session, talking) in batch.take_due(now) {
                    events.emit(EngineEvent::TalkStateChanged { session, talking });
                }
            }
        });

        Ok((
            Self {
                playback,
                stop,
                capture: Some(capture),
                talk_task,
                sounds,
                shared: pipeline_shared,
                io_base,
                health: HealthHistory::new(Instant::now()),
                low_latency,
                queue_target_ms: PlaybackConfig::default().target_queue_ms,
            },
            ports.packets,
        ))
    }

    pub fn playback_stats(&self) -> PlaybackStats {
        self.playback.stats()
    }

    /// Worst mixing time of one frame since the previous call, in µs.
    pub fn take_recent_mix_max_us(&self) -> u64 {
        self.playback.take_recent_mix_max_us()
    }

    /// History of the health counters behind the audio rating.
    pub fn health(&self) -> &HealthHistory {
        &self.health
    }

    pub fn health_mut(&mut self) -> &mut HealthHistory {
        &mut self.health
    }

    /// Device counters at the start of this pipeline.
    pub fn io_base(&self) -> IoStats {
        self.io_base
    }

    /// Switches the receive side to low-latency mode (issue #156) while it plays; the capture
    /// thread follows the shared controls on its own.
    pub fn set_low_latency(&mut self, on: bool) {
        if on == self.low_latency {
            return;
        }
        if self.playback.set_low_latency(on) {
            self.low_latency = on;
        } else {
            tracing::warn!("playback command queue full; low-latency mode not applied");
        }
    }

    /// Output queue target of the playback thread; only sends a command when it changes.
    pub fn set_queue_target_ms(&mut self, ms: u32) {
        if ms != self.queue_target_ms && self.playback.set_queue_target_ms(ms) {
            self.queue_target_ms = ms;
        }
    }

    /// Local volume (0–200 %) and mute of a person.
    pub fn apply_person(&mut self, session: u32, volume: u16, muted: bool) {
        let ok = self.playback.set_volume(session, f32::from(volume) / 100.0)
            && self.playback.set_muted(session, muted);
        if !ok {
            tracing::warn!(
                session,
                "playback command queue full; person setting not applied"
            );
        }
    }

    pub fn forget(&mut self, session: u32) {
        let _ = self.playback.forget_speaker(session);
    }

    /// Stops both threads and returns the microphone to `audio`.
    pub fn stop(mut self, audio: &mut Audio) {
        self.talk_task.abort();
        self.stop.store(true, Ordering::Relaxed);
        if let Some(capture) = self.capture.take() {
            match capture.join() {
                Ok(source) => audio.source = Some(source),
                Err(_) => tracing::error!("capture thread panicked; microphone lost until restart"),
            }
        }
        // Dropping `self` stops the playback thread; the sink stays in the shared slot.
    }
}

impl Drop for VoicePipeline {
    fn drop(&mut self) {
        self.talk_task.abort();
        self.stop.store(true, Ordering::Relaxed);
        self.sounds.detach();
        self.shared.share_sound.detach();
    }
}

fn capture_loop(
    mut source: FrameSource,
    controls: Arc<DspControls>,
    echo: EchoRefConsumer,
    transport: &VoiceTransport,
    shared: &VoiceShared,
    stop: &AtomicBool,
) -> FrameSource {
    let mut bitrate = shared.bitrate_bps.load(Ordering::Relaxed);
    let mut loss = shared.loss_percent.load(Ordering::Relaxed);
    let mut encoders = match SendEncoders::new(bitrate as i32) {
        Ok(encoders) => encoders,
        Err(e) => {
            tracing::error!(error = %e, "creating the Opus encoder failed; not sending voice");
            return source;
        }
    };
    let mut music = false;
    let mut chain = DspChain::new(controls.clone(), echo);
    let mut input: Frame = [0.0; FRAME_SAMPLES];
    let mut output: Frame = [0.0; FRAME_SAMPLES];
    let silence: Frame = [0.0; FRAME_SAMPLES];
    let mut packet = [0u8; MAX_PACKET_BYTES];
    let mut sending = false;
    // Voice target of the current transmission; a whisper (issue #67) keeps its target until
    // its terminator went out, so no audio meant for the target leaks into the channel.
    let mut burst_target = 0u8;
    let send = |len: usize, packet: &[u8], frames: u8, is_last: bool, target: u8| {
        transport.send_audio(
            OutgoingAudio::new(Bytes::copy_from_slice(&packet[..len]), frames, is_last)
                .to_target(target)
                .with_position(shared.positional.own_position()),
        );
        shared.packets_sent.fetch_add(1, Ordering::Relaxed);
    };

    let _realtime = realtime("capture", shared);
    // Drop audio captured before this connection started.
    while source.read_frame(&mut input) {}
    let mut burst_start: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        if !source.wait_frame(&mut input, Duration::from_millis(50)) {
            continue;
        }
        let queued = source.queued_samples();
        let started = Instant::now();
        let target = shared.bitrate_bps.load(Ordering::Relaxed);
        let target_loss = shared.loss_percent.load(Ordering::Relaxed);
        // Switches only between packets, so no collected audio is lost.
        if let Err(e) = encoders.switch(controls.low_latency(), target, target_loss, music) {
            tracing::warn!(error = %e, "switching the encoder for low-latency mode failed");
        }
        let encoder = encoders.active();
        if target != bitrate && encoder.set_bitrate(target as i32).is_ok() {
            bitrate = target;
        }
        // Low-latency mode is for music: it always uses the music tuning (issue #156).
        let want_music = controls.music_quality() || controls.low_latency();
        if want_music != music {
            match encoder.set_music(want_music) {
                Ok(()) => music = want_music,
                Err(e) => {
                    // Retrying every frame would flood the log; stay on the current tuning.
                    tracing::warn!(error = %e, "switching the encoder tuning failed");
                    music = want_music;
                }
            }
        }
        if target_loss != loss && encoder.set_packet_loss_percent(target_loss as i32).is_ok() {
            loss = target_loss;
        }

        let gate = chain.process(&input, &mut output);
        // Holding a whisper target transmits like push-to-talk, in either talk mode.
        let target = controls.voice_target();
        let open = (gate == Gate::Open || target != 0) && !shared.muted.load(Ordering::Relaxed);
        if capture_frame_late(queued, open || sending) {
            shared.capture_late_frames.fetch_add(1, Ordering::Relaxed);
        }
        let frames = encoders.frames_per_packet();
        let encoder = encoders.active();
        // End of transmission (or a switch to another target): pad with silence so the final
        // packet carries the terminator flag, as receivers use it to end the talking indicator
        // promptly.
        if sending && (!open || target != burst_target) {
            sending = false;
            let last = match encoder.push_frame(&silence, &mut packet) {
                Ok(Some(len)) => Some(len),
                _ => encoder.flush(&mut packet).ok().flatten(),
            };
            if let Some(len) = last {
                send(len, &packet, frames, true, burst_target);
            }
        }
        if open {
            if !sending {
                sending = true;
                burst_target = target;
            }
            if let Ok(Some(len)) = encoder.push_frame(&output, &mut packet) {
                send(len, &packet, frames, false, burst_target);
            }
        }
        shared.capture_time.record(started.elapsed());
        // With a full frame still queued, the next wait returns without sleeping.
        if queued < FRAME_SAMPLES {
            burst_start = None;
        } else if burst_start.get_or_insert(started).elapsed() > CAPTURE_BURST {
            std::thread::sleep(Duration::from_millis(1));
            burst_start = None;
        }
    }
    source
}

/// The send encoders of both modes (issue #156): 20 ms VoIP packets normally, 10 ms
/// restricted-low-delay packets in low-latency mode. Both exist from the start because libopus
/// cannot switch an encoder into the low-delay application, and creating one allocates, which
/// the capture thread must not do.
struct SendEncoders {
    normal: OpusEncoder,
    low_delay: OpusEncoder,
    low_latency: bool,
}

impl SendEncoders {
    fn new(bitrate_bps: i32) -> Result<Self, CodecError> {
        Ok(Self {
            normal: OpusEncoder::new(bitrate_bps, PacketDuration::Ms20)?,
            low_delay: OpusEncoder::new_low_delay(bitrate_bps, PacketDuration::Ms10)?,
            low_latency: false,
        })
    }

    fn active(&mut self) -> &mut OpusEncoder {
        if self.low_latency {
            &mut self.low_delay
        } else {
            &mut self.normal
        }
    }

    fn frames_per_packet(&self) -> u8 {
        let encoder = if self.low_latency {
            &self.low_delay
        } else {
            &self.normal
        };
        encoder.packet_duration().frames() as u8
    }

    /// Moves to the encoder of `low_latency` at a packet boundary and hands it the current
    /// bitrate, loss hint and tuning. Does nothing while a packet is half collected.
    fn switch(
        &mut self,
        low_latency: bool,
        bitrate_bps: u32,
        loss_percent: u32,
        music: bool,
    ) -> Result<(), CodecError> {
        if low_latency == self.low_latency || self.active().has_pending() {
            return Ok(());
        }
        self.low_latency = low_latency;
        let encoder = self.active();
        encoder.reset()?;
        encoder.set_bitrate(bitrate_bps as i32)?;
        encoder.set_packet_loss_percent(loss_percent as i32)?;
        encoder.set_music(music)
    }
}

/// Whether a capture frame read with `queued` samples behind it counts as late. A backlog only
/// delays audio while it is sent; while the gate is closed nobody hears it.
fn capture_frame_late(queued: usize, transmitting: bool) -> bool {
    transmitting && queued > CAPTURE_LATE_SAMPLES
}

/// Real-time scheduling for a voice thread, so a loaded CPU does not delay its frames (issue
/// #123). Refused (no rtkit, a sandbox), the thread runs at normal priority as before.
///
/// Debug builds stay at normal priority: unoptimised DSP code is slow enough to use up the
/// real-time budget and get the thread demoted at once (issue #139).
fn realtime(thread: &'static str, shared: &VoiceShared) -> Option<RtGuard> {
    if cfg!(debug_assertions) {
        tracing::debug!(thread, "debug build: real-time scheduling off");
        return None;
    }
    match mumcord_audio::rt::promote_current_thread() {
        Ok(guard) => {
            shared.realtime_threads.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(thread, "real-time scheduling on");
            Some(guard)
        }
        Err(e) => {
            tracing::info!(thread, "{e}; running at normal priority");
            None
        }
    }
}

/// Converts a received packet for the mixer; `None` if the payload is oversized.
pub(crate) fn playback_packet(
    packet: &mumcord_protocol::VoicePacket,
) -> Option<mumcord_audio::mixer::VoicePacket> {
    mumcord_audio::mixer::VoicePacket::new(
        packet.session,
        packet.seq,
        &packet.payload,
        packet.is_last,
        packet.via_listener,
        Instant::now(),
    )
    .map(|p| p.with_position(packet.positional))
}

/// Starts a mic test on the devices of `audio` (they must not be lent to a pipeline).
pub(crate) fn start_mic_test(
    audio: &mut Audio,
    controls: Arc<DspControls>,
) -> EngineResult<MicTest> {
    let (source, sink) = audio
        .take_devices()
        .ok_or_else(|| EngineError::internal("audio devices busy"))?;
    // `MicTest::start` consumes the handles; on failure they are gone, so recreate is the
    // only recovery. Spawning a thread practically never fails.
    MicTest::start(source, sink, controls, MIC_TEST_MAX)
        .map_err(|e| EngineError::internal(format!("mic test: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #156: the send encoder switches to 10 ms low-delay packets only between packets
    /// and carries the bitrate over.
    #[test]
    fn send_encoders_switch_at_packet_boundaries() {
        let mut encoders = SendEncoders::new(48_000).expect("encoders");
        let frame: Frame = std::array::from_fn(|i| (i as f32 * 0.05).sin() * 0.3);
        let mut packet = [0u8; MAX_PACKET_BYTES];
        assert_eq!(encoders.frames_per_packet(), 2);
        assert!(!encoders.active().is_low_delay());

        // Half a 20 ms packet is collected: the switch waits.
        assert_eq!(
            encoders
                .active()
                .push_frame(&frame, &mut packet)
                .expect("encode"),
            None
        );
        encoders.switch(true, 96_000, 5, false).expect("switch");
        assert_eq!(encoders.frames_per_packet(), 2);
        assert!(
            encoders
                .active()
                .push_frame(&frame, &mut packet)
                .expect("encode")
                .is_some()
        );

        encoders.switch(true, 96_000, 5, false).expect("switch");
        assert_eq!(encoders.frames_per_packet(), 1);
        let encoder = encoders.active();
        assert!(encoder.is_low_delay());
        assert_eq!(encoder.bitrate().expect("bitrate"), 96_000);
        assert!(
            encoder
                .push_frame(&frame, &mut packet)
                .expect("encode")
                .is_some()
        );

        encoders
            .switch(false, 64_000, 0, true)
            .expect("switch back");
        assert_eq!(encoders.frames_per_packet(), 2);
        let encoder = encoders.active();
        assert!(!encoder.is_low_delay());
        assert!(encoder.is_music().expect("tuning"));
        assert_eq!(encoder.bitrate().expect("bitrate"), 64_000);
    }

    #[test]
    fn meter_scale() {
        assert_eq!(meter_level(-100.0), 0.0);
        assert_eq!(meter_level(-60.0), 0.0);
        assert!((meter_level(-30.0) - 0.5).abs() < 1e-6);
        assert_eq!(meter_level(0.0), 1.0);
        assert_eq!(meter_level(3.0), 1.0);
    }

    #[test]
    fn capture_backlog_counts_only_while_sending() {
        let backlog = CAPTURE_LATE_SAMPLES + 1;
        assert!(capture_frame_late(backlog, true));
        assert!(
            !capture_frame_late(backlog, false),
            "idle silence is not heard"
        );
        assert!(!capture_frame_late(CAPTURE_LATE_SAMPLES, true));
    }
}
