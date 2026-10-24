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

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bytes::Bytes;
use mumcord_audio::codec::{MAX_PACKET_BYTES, OpusEncoder, PacketDuration};
use mumcord_audio::dsp::{DspChain, DspControls, EchoRefConsumer, Gate, MicTest, echo_reference};
use mumcord_audio::io::{AudioIo, DeviceEvent, DeviceInfo, Direction, FrameSink, FrameSource};
use mumcord_audio::mixer::{PacketSender, Playback, PlaybackConfig, PlaybackSink};
use mumcord_audio::{FRAME_SAMPLES, Frame};
use mumcord_protocol::{OutgoingAudio, VoiceTransport};
use tokio::runtime::Handle as Runtime;
use tokio::task::JoinHandle as TaskHandle;

use crate::session::{
    AudioDeviceList, AudioDeviceView, EngineError, EngineEvent, EngineResult, Events,
};

/// Interval of `MicLevel` events (about 20 per second).
const METER_INTERVAL: Duration = Duration::from_millis(50);
/// Poll interval of talk indicators; well inside the 200 ms of AC-1.7.
const TALK_POLL: Duration = Duration::from_millis(20);
/// Longest mic-test recording (B8: record, then play back).
pub const MIC_TEST_MAX: Duration = Duration::from_secs(5);
/// Initial encoder bitrate until the bitrate controller reports a target.
const START_BITRATE_BPS: u32 = 72_000;

/// Maps a peak level in dBFS to the 0–1 meter scale (−60 dBFS and below read as 0).
pub fn meter_level(peak_dbfs: f32) -> f32 {
    ((peak_dbfs + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// Playback sink shared between voice sessions and the mic test: [`Playback`] owns its sink by
/// value, so the device sink lives in this slot and is lent out. The lock is only contended
/// while switching between voice and mic test, never by a device callback.
#[derive(Clone)]
struct SharedSink(Arc<Mutex<Option<FrameSink>>>);

impl SharedSink {
    fn with<R>(&self, f: impl FnOnce(&mut Option<FrameSink>) -> R) -> R {
        f(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl PlaybackSink for SharedSink {
    fn write_frame(&mut self, frame: &Frame) -> bool {
        self.with(|sink| sink.as_mut().is_some_and(|s| s.write_frame(frame)))
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
            sink: SharedSink(Arc::new(Mutex::new(Some(handles.sink)))),
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
}

fn device_list(io: &AudioIo) -> EngineResult<AudioDeviceList> {
    let list = |direction| {
        io.devices(direction)
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
            } => tracing::info!(?direction, fallback, "audio stream active"),
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
}

impl Default for VoiceShared {
    fn default() -> Self {
        Self {
            muted: AtomicBool::new(false),
            deaf: AtomicBool::new(false),
            bitrate_bps: AtomicU32::new(START_BITRATE_BPS),
            loss_percent: AtomicU32::new(0),
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
        let (mut echo_producer, echo_consumer) = echo_reference();
        let config = PlaybackConfig {
            far_end_tap: Some(Box::new(move |frame: &Frame| {
                // A full reference queue only degrades echo cancellation briefly.
                let _ = echo_producer.push(frame);
            })),
            ..PlaybackConfig::default()
        };
        let (playback, ports) = match Playback::start(audio.sink.clone(), config) {
            Ok(started) => started,
            Err(e) => {
                audio.source = Some(source);
                return Err(EngineError::internal(format!("playback: {e}")));
            }
        };

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
        let mut playback = playback;
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

        let mut talk_events = ports.talk_events;
        let talk_task = rt.spawn(async move {
            let mut own_talking = false;
            let mut tick = tokio::time::interval(TALK_POLL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                while let Ok(e) = talk_events.pop() {
                    events.emit(EngineEvent::TalkStateChanged {
                        session: e.session,
                        talking: e.talking,
                    });
                }
                let talking = controls.transmitting() && !shared.muted.load(Ordering::Relaxed);
                if talking != own_talking {
                    own_talking = talking;
                    events.emit(EngineEvent::TalkStateChanged {
                        session: own_session,
                        talking,
                    });
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
            },
            ports.packets,
        ))
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
    let mut encoder = match OpusEncoder::new(bitrate as i32, PacketDuration::Ms20) {
        Ok(encoder) => encoder,
        Err(e) => {
            tracing::error!(error = %e, "creating the Opus encoder failed; not sending voice");
            return source;
        }
    };
    let frames_per_packet = encoder.packet_duration().frames() as u8;
    let mut chain = DspChain::new(controls, echo);
    let mut input: Frame = [0.0; FRAME_SAMPLES];
    let mut output: Frame = [0.0; FRAME_SAMPLES];
    let silence: Frame = [0.0; FRAME_SAMPLES];
    let mut packet = [0u8; MAX_PACKET_BYTES];
    let mut sending = false;
    let send = |len: usize, packet: &[u8], is_last: bool| {
        transport.send_audio(OutgoingAudio::new(
            Bytes::copy_from_slice(&packet[..len]),
            frames_per_packet,
            is_last,
        ));
    };

    // Drop audio captured before this connection started.
    while source.read_frame(&mut input) {}
    while !stop.load(Ordering::Relaxed) {
        if !source.wait_frame(&mut input, Duration::from_millis(50)) {
            continue;
        }
        let target = shared.bitrate_bps.load(Ordering::Relaxed);
        if target != bitrate && encoder.set_bitrate(target as i32).is_ok() {
            bitrate = target;
        }
        let target_loss = shared.loss_percent.load(Ordering::Relaxed);
        if target_loss != loss && encoder.set_packet_loss_percent(target_loss as i32).is_ok() {
            loss = target_loss;
        }

        let gate = chain.process(&input, &mut output);
        let open = gate == Gate::Open && !shared.muted.load(Ordering::Relaxed);
        if open {
            sending = true;
            if let Ok(Some(len)) = encoder.push_frame(&output, &mut packet) {
                send(len, &packet, false);
            }
        } else if sending {
            // End of transmission: pad with silence so the final packet carries the
            // terminator flag, as receivers use it to end the talking indicator promptly.
            sending = false;
            let last = match encoder.push_frame(&silence, &mut packet) {
                Ok(Some(len)) => Some(len),
                _ => encoder.flush(&mut packet).ok().flatten(),
            };
            if let Some(len) = last {
                send(len, &packet, true);
            }
        }
    }
    source
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

    #[test]
    fn meter_scale() {
        assert_eq!(meter_level(-100.0), 0.0);
        assert_eq!(meter_level(-60.0), 0.0);
        assert!((meter_level(-30.0) - 0.5).abs() < 1e-6);
        assert_eq!(meter_level(0.0), 1.0);
        assert_eq!(meter_level(3.0), 1.0);
    }
}
