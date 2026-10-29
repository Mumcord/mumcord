//! The playback thread: drains incoming packets and control commands, mixes
//! and keeps the sink's queue at the target latency.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::share::{ShareGain, SharePort, ShareSlots, share_slots};
use super::{Mixer, TalkEvent, VoicePacket};
use crate::codec::CodecError;
use crate::spatial::Listener;
use crate::{FRAME_SAMPLES, Frame, SAMPLE_RATE};

/// Queued incoming packets (≈ 2.5 s of 20 ms packets from 2 speakers).
const PACKET_QUEUE: usize = 256;
const COMMAND_QUEUE: usize = 64;
const EVENT_QUEUE: usize = 256;
/// Mix-in samples that can be queued ahead (2 s).
const MIX_IN_QUEUE: usize = 2 * SAMPLE_RATE as usize;
/// Polling interval of the playback thread. Coarse OS timers (up to ~15 ms on
/// Windows) are absorbed by the 25 ms default queue target.
const POLL_INTERVAL: Duration = Duration::from_millis(2);
/// Upper bound of frames mixed per poll, so a sink that never fills (no
/// device) cannot make the thread drain the jitter buffers faster than real time.
const MAX_FRAMES_PER_POLL: u32 = 4;
const FRAME_DURATION: Duration = Duration::from_millis(10);
/// Device queue below which a wake-up counts as late (5 ms).
const LATE_QUEUE_SAMPLES: usize = FRAME_SAMPLES / 2;

/// Where mixed frames go; implemented by the device `FrameSink` of `mumcord-audio` and by test doubles.
pub trait PlaybackSink: Send + 'static {
    /// Queues a mono frame; `false` if it was dropped (queue full or no device).
    fn write_frame(&mut self, frame: &Frame) -> bool;
    /// Queues a stereo frame. Sinks without stereo output play the mix of both channels.
    fn write_stereo(&mut self, left: &Frame, right: &Frame) -> bool {
        let mut mono = [0.0; FRAME_SAMPLES];
        downmix(left, right, &mut mono);
        self.write_frame(&mono)
    }
    /// Audio queued but not yet played, in 48 kHz samples.
    fn queued_samples(&mut self) -> usize;
}

/// The mono mix of a stereo frame.
pub fn downmix(left: &Frame, right: &Frame, out: &mut Frame) {
    for ((o, l), r) in out.iter_mut().zip(left).zip(right) {
        *o = (l + r) * 0.5;
    }
}

/// Setup hook of the playback thread; what it returns is kept until the thread ends (such as a
/// real-time scheduling guard that must be dropped on the same thread).
pub type ThreadStart = Box<dyn FnOnce() -> Box<dyn std::any::Any> + Send>;

/// Far-end reference hook: called on the playback thread with every final
/// frame (mono mix) right before it is written to the sink. Must not block or allocate.
pub type FarEndTap = Box<dyn FnMut(&Frame) + Send>;

pub struct PlaybackConfig {
    /// Output latency the sink queue is kept at, in ms (20–30 recommended).
    pub target_queue_ms: u32,
    /// Receives every played frame, e.g. as echo-cancellation reference.
    pub far_end_tap: Option<FarEndTap>,
    /// Places voices that carry a position around this listener (positional audio).
    pub listener: Option<Arc<Listener>>,
    /// Gain of watched screen shares' sound (auto-ducking, FR-2.76).
    pub share_gain: Arc<ShareGain>,
    /// Runs first on the playback thread, e.g. to request real-time scheduling for it.
    pub on_thread_start: Option<ThreadStart>,
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            target_queue_ms: 25,
            far_end_tap: None,
            listener: None,
            share_gain: Arc::default(),
            on_thread_start: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PlaybackError {
    #[error("creating decoders: {0}")]
    Codec(#[from] CodecError),
    #[error("spawning playback thread: {0}")]
    Thread(#[from] std::io::Error),
}

/// Diagnostics counters of the playback thread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlaybackStats {
    /// Packets dropped because the packet queue was full or no slot was free.
    pub packets_dropped: u64,
    pub late_packets: u64,
    pub concealed_frames: u64,
    /// 10 ms frames decoded from received packets.
    pub received_frames: u64,
    /// Frames the sink refused (queue full or no output device).
    pub frames_refused: u64,
    /// Times the playback thread woke up to a nearly empty device queue while it was playing
    /// sound: it was scheduled too late, and the device cut the sound short or will shortly.
    /// Late wake-ups while only silence is queued are not heard and not counted.
    pub late_wakeups: u64,
    /// Largest jitter-buffer target delay of the speakers playing (or of the last stream), in ms.
    pub jitter_delay_ms: u64,
    /// Average and worst time to mix (decode, conceal, place) one 10 ms frame, in µs.
    pub mix_time_avg_us: u64,
    pub mix_time_max_us: u64,
}

#[derive(Default)]
struct Counters {
    queue_full: AtomicU64,
    no_slot: AtomicU64,
    late: AtomicU64,
    concealed: AtomicU64,
    received: AtomicU64,
    refused: AtomicU64,
    late_wakeups: AtomicU64,
    jitter_delay_ms: AtomicU64,
    mix_time: FrameTime,
}

/// Processing time per frame, written by one thread with relaxed atomics and read anywhere.
#[derive(Debug, Default)]
pub struct FrameTime {
    frames: AtomicU64,
    total_us: AtomicU64,
    max_us: AtomicU64,
    /// Worst time since the last [`take_recent_max_us`](Self::take_recent_max_us).
    recent_max_us: AtomicU64,
}

impl FrameTime {
    /// Records the time one frame took.
    pub fn record(&self, elapsed: Duration) {
        let us = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        self.frames.fetch_add(1, Ordering::Relaxed);
        self.total_us.fetch_add(us, Ordering::Relaxed);
        self.max_us.fetch_max(us, Ordering::Relaxed);
        self.recent_max_us.fetch_max(us, Ordering::Relaxed);
    }

    /// Average time per frame in µs (0 before the first frame).
    pub fn avg_us(&self) -> u64 {
        let frames = self.frames.load(Ordering::Relaxed);
        self.total_us
            .load(Ordering::Relaxed)
            .checked_div(frames)
            .unwrap_or(0)
    }

    /// Worst time of a single frame in µs.
    pub fn max_us(&self) -> u64 {
        self.max_us.load(Ordering::Relaxed)
    }

    /// Worst time of a single frame since the previous call, in µs; for a rating over a recent
    /// window rather than since start. Meant for a single reader.
    pub fn take_recent_max_us(&self) -> u64 {
        self.recent_max_us.swap(0, Ordering::Relaxed)
    }

    pub fn reset(&self) {
        self.frames.store(0, Ordering::Relaxed);
        self.total_us.store(0, Ordering::Relaxed);
        self.max_us.store(0, Ordering::Relaxed);
        self.recent_max_us.store(0, Ordering::Relaxed);
    }
}

enum Command {
    SetVolume(u32, f32),
    SetMuted(u32, bool),
    Forget(u32),
    DiscardListenerAudio(bool),
    MixInGain(f32),
}

/// Producer side of the incoming packet queue. Single producer: route UDP
/// and TCP-tunnelled voice through one thread.
pub struct PacketSender {
    producer: rtrb::Producer<VoicePacket>,
    counters: Arc<Counters>,
}

impl PacketSender {
    /// Queues a packet without blocking; `false` (and counted) if the queue is full.
    pub fn push(&mut self, packet: VoicePacket) -> bool {
        let pushed = self.producer.push(packet).is_ok();
        if !pushed {
            self.counters.queue_full.fetch_add(1, Ordering::Relaxed);
        }
        pushed
    }
}

/// Endpoints handed to the engine; each can live on a different thread.
pub struct PlaybackPorts {
    /// Incoming voice packets.
    pub packets: PacketSender,
    /// Talking start/stop per speaker.
    pub talk_events: rtrb::Consumer<TalkEvent>,
    /// 48 kHz mono samples mixed into the output (notification sounds,
    /// mic-test loopback). Write whole sounds; underflow plays silence.
    pub mix_in: rtrb::Producer<f32>,
    /// Sound of watched screen shares, each its own source ([`super::share`]).
    pub shares: SharePort,
}

/// Owns the playback thread; dropping it stops the thread.
pub struct Playback {
    commands: rtrb::Producer<Command>,
    counters: Arc<Counters>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Playback {
    pub fn start<S: PlaybackSink>(
        sink: S,
        config: PlaybackConfig,
    ) -> Result<(Self, PlaybackPorts), PlaybackError> {
        let mut mixer = Mixer::new()?;
        if let Some(listener) = &config.listener {
            mixer.set_listener(Arc::clone(listener));
        }
        let (packet_tx, packet_rx) = rtrb::RingBuffer::new(PACKET_QUEUE);
        let (command_tx, command_rx) = rtrb::RingBuffer::new(COMMAND_QUEUE);
        let (event_tx, event_rx) = rtrb::RingBuffer::new(EVENT_QUEUE);
        let (mix_in_tx, mix_in_rx) = rtrb::RingBuffer::new(MIX_IN_QUEUE);
        let (share_port, shares) = share_slots(Arc::clone(&config.share_gain));
        let counters = Arc::new(Counters::default());
        let stop = Arc::new(AtomicBool::new(false));

        let worker = Worker {
            mixer,
            sink,
            config,
            packets: packet_rx,
            commands: command_rx,
            events: event_tx,
            mix_in: mix_in_rx,
            mix_in_gain: 1.0,
            shares,
            counters: Arc::clone(&counters),
            stop: Arc::clone(&stop),
        };
        let thread = std::thread::Builder::new()
            .name("mumcord-playback".into())
            .spawn(move || worker.run())?;

        let ports = PlaybackPorts {
            packets: PacketSender {
                producer: packet_tx,
                counters: Arc::clone(&counters),
            },
            talk_events: event_rx,
            mix_in: mix_in_tx,
            shares: share_port,
        };
        let playback = Self {
            commands: command_tx,
            counters,
            stop,
            thread: Some(thread),
        };
        Ok((playback, ports))
    }

    /// Local volume of a person, 0.0–2.0 (0–200 %).
    pub fn set_volume(&mut self, session: u32, volume: f32) -> bool {
        self.send(Command::SetVolume(session, volume))
    }

    pub fn set_muted(&mut self, session: u32, muted: bool) -> bool {
        self.send(Command::SetMuted(session, muted))
    }

    /// The person left: drop their settings and buffer.
    pub fn forget_speaker(&mut self, session: u32) -> bool {
        self.send(Command::Forget(session))
    }

    /// Silences audio received only via channel listening (default on).
    pub fn set_discard_listener_audio(&mut self, discard: bool) -> bool {
        self.send(Command::DiscardListenerAudio(discard))
    }

    /// Gain of the mix-in input (default 1.0).
    pub fn set_mix_in_gain(&mut self, gain: f32) -> bool {
        self.send(Command::MixInGain(gain.max(0.0)))
    }

    pub fn stats(&self) -> PlaybackStats {
        let c = &self.counters;
        PlaybackStats {
            packets_dropped: c.queue_full.load(Ordering::Relaxed)
                + c.no_slot.load(Ordering::Relaxed),
            late_packets: c.late.load(Ordering::Relaxed),
            concealed_frames: c.concealed.load(Ordering::Relaxed),
            received_frames: c.received.load(Ordering::Relaxed),
            frames_refused: c.refused.load(Ordering::Relaxed),
            late_wakeups: c.late_wakeups.load(Ordering::Relaxed),
            jitter_delay_ms: c.jitter_delay_ms.load(Ordering::Relaxed),
            mix_time_avg_us: c.mix_time.avg_us(),
            mix_time_max_us: c.mix_time.max_us(),
        }
    }

    /// Worst mixing time of one frame since the previous call, in µs (see
    /// [`FrameTime::take_recent_max_us`]).
    pub fn take_recent_mix_max_us(&self) -> u64 {
        self.counters.mix_time.take_recent_max_us()
    }

    /// `false` if the command queue is full (the caller may retry).
    fn send(&mut self, command: Command) -> bool {
        self.commands.push(command).is_ok()
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("playback thread panicked");
        }
    }
}

struct Worker<S> {
    mixer: Mixer,
    sink: S,
    config: PlaybackConfig,
    packets: rtrb::Consumer<VoicePacket>,
    commands: rtrb::Consumer<Command>,
    events: rtrb::Producer<TalkEvent>,
    mix_in: rtrb::Consumer<f32>,
    mix_in_gain: f32,
    shares: ShareSlots,
    counters: Arc<Counters>,
    stop: Arc<AtomicBool>,
}

impl<S: PlaybackSink> Worker<S> {
    fn run(mut self) {
        let _thread_setup = self.config.on_thread_start.take().map(|start| start());
        let epoch = Instant::now();
        let target_samples = self.config.target_queue_ms as usize * SAMPLE_RATE as usize / 1000;
        let mut left = [0.0; FRAME_SAMPLES];
        let mut right = [0.0; FRAME_SAMPLES];
        let mut mono = [0.0; FRAME_SAMPLES];
        let mut extra = [0.0; FRAME_SAMPLES];
        // When the sink refuses frames (no device), pace by the wall clock.
        let mut next_frame_due = Instant::now();
        let mut sink_refusing = false;
        // A late wake-up only counts once the queue was filled at least once.
        let mut primed = false;
        // Whether the frames queued for the device carry sound; a late wake-up over queued
        // silence is not heard.
        let mut audible = false;

        while !self.stop.load(Ordering::Relaxed) {
            self.apply_commands();
            self.shares.update();
            while let Ok(packet) = self.packets.pop() {
                let arrival_ms = millis_since(epoch, packet.arrival);
                self.mixer.push_packet(&packet, arrival_ms);
            }
            if !sink_refusing {
                let queued = self.sink.queued_samples();
                if primed && audible && queued < LATE_QUEUE_SAMPLES {
                    self.counters.late_wakeups.fetch_add(1, Ordering::Relaxed);
                }
            }

            let mut mixed = 0;
            while mixed < MAX_FRAMES_PER_POLL {
                let now = Instant::now();
                let due = if sink_refusing {
                    now >= next_frame_due
                } else {
                    self.sink.queued_samples() < target_samples
                };
                if !due {
                    primed |= !sink_refusing;
                    break;
                }
                let mut has_extra = self.read_mix_in(&mut extra);
                if !has_extra {
                    extra.fill(0.0);
                }
                has_extra |= self.shares.mix_into(&mut extra);
                let events = &mut self.events;
                let mix_start = Instant::now();
                audible = self.mixer.mix(
                    &mut left,
                    &mut right,
                    has_extra.then_some(&extra),
                    millis_since(epoch, now),
                    &mut |event| {
                        // A full queue means the engine stopped reading; the
                        // indicator state resyncs with the next event.
                        let _ = events.push(event);
                    },
                );
                self.counters.mix_time.record(mix_start.elapsed());
                if let Some(tap) = &mut self.config.far_end_tap {
                    downmix(&left, &right, &mut mono);
                    tap(&mono);
                }
                sink_refusing = !self.sink.write_stereo(&left, &right);
                if sink_refusing {
                    self.counters.refused.fetch_add(1, Ordering::Relaxed);
                    next_frame_due = now.max(next_frame_due) + FRAME_DURATION;
                }
                mixed += 1;
            }
            self.publish_stats();
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    fn apply_commands(&mut self) {
        while let Ok(command) = self.commands.pop() {
            match command {
                Command::SetVolume(session, volume) => {
                    self.mixer.set_volume(session, volume);
                }
                Command::SetMuted(session, muted) => {
                    self.mixer.set_muted(session, muted);
                }
                Command::Forget(session) => self.mixer.forget_speaker(session),
                Command::DiscardListenerAudio(discard) => {
                    self.mixer.set_discard_listener_audio(discard);
                }
                Command::MixInGain(gain) => self.mix_in_gain = gain,
            }
        }
    }

    /// Reads up to one frame of mix-in samples; `false` if none were queued.
    fn read_mix_in(&mut self, extra: &mut Frame) -> bool {
        let n = self.mix_in.slots().min(FRAME_SAMPLES);
        if n == 0 {
            return false;
        }
        let Ok(chunk) = self.mix_in.read_chunk(n) else {
            return false;
        };
        let (a, b) = chunk.as_slices();
        for (dst, src) in extra.iter_mut().zip(a.iter().chain(b)) {
            *dst = src * self.mix_in_gain;
        }
        chunk.commit_all();
        extra[n..].fill(0.0);
        true
    }

    fn publish_stats(&self) {
        let stats = self.mixer.stats();
        let c = &self.counters;
        c.no_slot.store(stats.packets_no_slot, Ordering::Relaxed);
        c.late.store(stats.late_packets, Ordering::Relaxed);
        c.concealed.store(stats.concealed_frames, Ordering::Relaxed);
        c.received.store(stats.received_frames, Ordering::Relaxed);
        c.jitter_delay_ms
            .store(stats.jitter_delay_ms, Ordering::Relaxed);
    }
}

fn millis_since(epoch: Instant, at: Instant) -> u64 {
    at.saturating_duration_since(epoch).as_millis() as u64
}
