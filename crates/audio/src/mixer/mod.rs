//! Playback mixer: sums all remote speakers into 10 ms playback frames.
//!
//! [`Mixer`] is the allocation-free core: per-speaker jitter buffers, local
//! volume (0–200 %) and mute (FR-1.18/1.19), the global discard switch for
//! audio that only arrives because we listen to followed channels (FR-1.31),
//! talking-state events, an optional mix-in frame (notification sounds,
//! mic-test loopback) and a limiter. [`Playback`] runs it on its own thread
//! and paces it against the [`FrameSink`](crate::io::FrameSink).

mod playback;
#[cfg(test)]
mod tests;

pub use playback::{
    PacketSender, Playback, PlaybackConfig, PlaybackError, PlaybackPorts, PlaybackSink,
    PlaybackStats,
};

use std::time::Instant;

use crate::codec::{CodecError, MAX_PACKET_BYTES};
use crate::jitter::{JitterBuffer, Pull};
use crate::{FRAME_SAMPLES, Frame};

/// Speakers that can be buffered at the same time. Slots are created up front
/// (no allocation while playing) and reused once a speaker falls silent.
pub const MAX_SPEAKERS: usize = 32;
/// Sessions whose volume/mute settings are remembered.
pub const MAX_SPEAKER_SETTINGS: usize = 1024;
/// Highest per-person volume (200 %).
pub const MAX_VOLUME: f32 = 2.0;

/// Limiter ceiling; the soft clipper above it absorbs what the limiter's
/// gain ramp lets through at the start of a loud frame.
const LIMIT: f32 = 0.9;
/// Limiter release per 10 ms frame (≈ 50 ms time constant).
const LIMITER_RELEASE: f32 = 0.18;

/// One incoming voice packet as delivered by the voice transport.
#[derive(Clone)]
pub struct VoicePacket {
    pub session: u32,
    /// Sender's sequence number (counts 10 ms frames).
    pub seq: u64,
    /// The sender's last packet of a talk burst.
    pub is_last: bool,
    /// The packet reached us only because we listen to (follow) a channel.
    pub via_listener: bool,
    /// When the packet was received; drives the jitter estimate.
    pub arrival: Instant,
    len: usize,
    payload: [u8; MAX_PACKET_BYTES],
}

impl VoicePacket {
    /// Returns `None` if the Opus payload exceeds [`MAX_PACKET_BYTES`].
    pub fn new(
        session: u32,
        seq: u64,
        opus: &[u8],
        is_last: bool,
        via_listener: bool,
        arrival: Instant,
    ) -> Option<Self> {
        let mut payload = [0; MAX_PACKET_BYTES];
        payload.get_mut(..opus.len())?.copy_from_slice(opus);
        Some(Self {
            session,
            seq,
            is_last,
            via_listener,
            arrival,
            len: opus.len(),
            payload,
        })
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.len]
    }
}

/// A remote speaker started or stopped talking (audibly or not).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TalkEvent {
    pub session: u32,
    pub talking: bool,
}

/// Counters for diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MixerStats {
    /// Packets dropped because all speaker slots were busy.
    pub packets_no_slot: u64,
    pub late_packets: u64,
    /// Lost or late 10 ms frames rebuilt by FEC or concealment.
    pub concealed_frames: u64,
}

#[derive(Clone, Copy)]
struct Settings {
    volume: f32,
    muted: bool,
}

const DEFAULT_SETTINGS: Settings = Settings {
    volume: 1.0,
    muted: false,
};

struct Speaker {
    session: Option<u32>,
    jitter: JitterBuffer,
    /// Gain applied at the end of the last frame (ramped to avoid clicks).
    gain: f32,
    talking: bool,
    via_listener: bool,
    /// Forgotten by the caller; freed (with a stop event) on the next mix.
    released: bool,
    /// Stats already folded into the mixer totals.
    counted_late: u64,
    counted_concealed: u64,
}

pub struct Mixer {
    speakers: Vec<Speaker>,
    settings: Vec<(u32, Settings)>,
    discard_listener_audio: bool,
    limiter_gain: f32,
    scratch: Box<Frame>,
    stats: MixerStats,
}

impl Mixer {
    pub fn new() -> Result<Self, CodecError> {
        let speakers = (0..MAX_SPEAKERS)
            .map(|_| {
                Ok(Speaker {
                    session: None,
                    jitter: JitterBuffer::new()?,
                    gain: 0.0,
                    talking: false,
                    via_listener: false,
                    released: false,
                    counted_late: 0,
                    counted_concealed: 0,
                })
            })
            .collect::<Result<_, CodecError>>()?;
        Ok(Self {
            speakers,
            settings: Vec::with_capacity(MAX_SPEAKER_SETTINGS),
            discard_listener_audio: true,
            limiter_gain: 1.0,
            scratch: Box::new([0.0; FRAME_SAMPLES]),
            stats: MixerStats::default(),
        })
    }

    /// Routes a packet to its speaker's jitter buffer. `arrival_ms` is on the
    /// same monotonic clock as the `now_ms` given to [`mix`](Self::mix).
    pub fn push_packet(&mut self, packet: &VoicePacket, arrival_ms: u64) {
        let Some(idx) = self.slot_for(packet.session) else {
            self.stats.packets_no_slot += 1;
            return;
        };
        let speaker = &mut self.speakers[idx];
        speaker.via_listener = packet.via_listener;
        speaker
            .jitter
            .push(packet.seq, packet.payload(), packet.is_last, arrival_ms);
    }

    /// Sets a person's local volume (0.0–2.0 = 0–200 %). Returns `false` if
    /// the settings table is full.
    pub fn set_volume(&mut self, session: u32, volume: f32) -> bool {
        self.update_settings(session, |s| s.volume = volume.clamp(0.0, MAX_VOLUME))
    }

    /// Mutes or unmutes a person locally. Returns `false` if the settings table is full.
    pub fn set_muted(&mut self, session: u32, muted: bool) -> bool {
        self.update_settings(session, |s| s.muted = muted)
    }

    /// Forgets a session (the person left the server): settings and buffer.
    pub fn forget_speaker(&mut self, session: u32) {
        self.settings.retain(|(s, _)| *s != session);
        for speaker in &mut self.speakers {
            if speaker.session == Some(session) {
                speaker.released = true;
            }
        }
    }

    /// Whether audio that only arrives via channel listening is silenced
    /// (default on: followed channels are muted locally, FR-1.31).
    pub fn set_discard_listener_audio(&mut self, discard: bool) {
        self.discard_listener_audio = discard;
    }

    pub fn stats(&self) -> MixerStats {
        self.stats
    }

    /// Mixes the next 10 ms into `out`. `mix_in` is added unscaled after the
    /// speakers (notification sounds, mic-test loopback). Talking changes are
    /// reported through `on_event`.
    pub fn mix(
        &mut self,
        out: &mut Frame,
        mix_in: Option<&Frame>,
        now_ms: u64,
        on_event: &mut dyn FnMut(TalkEvent),
    ) {
        out.fill(0.0);
        for idx in 0..self.speakers.len() {
            let Some(session) = self.speakers[idx].session else {
                continue;
            };
            if self.speakers[idx].released {
                self.free_slot(idx, on_event);
                continue;
            }
            let target_gain = self.target_gain(idx, session);
            let speaker = &mut self.speakers[idx];
            let pull = speaker.jitter.pull(&mut self.scratch, now_ms);
            if speaker.jitter.is_playing() != speaker.talking {
                speaker.talking = !speaker.talking;
                on_event(TalkEvent {
                    session,
                    talking: speaker.talking,
                });
            }
            if pull != Pull::Silent {
                add_ramped(out, &self.scratch, speaker.gain, target_gain);
            }
            speaker.gain = target_gain;

            let stats = speaker.jitter.stats();
            let concealed = stats.concealed_frames + stats.fec_frames;
            self.stats.late_packets += stats.late_packets - speaker.counted_late;
            self.stats.concealed_frames += concealed - speaker.counted_concealed;
            speaker.counted_late = stats.late_packets;
            speaker.counted_concealed = concealed;
        }
        if let Some(extra) = mix_in {
            for (o, e) in out.iter_mut().zip(extra.iter()) {
                *o += e;
            }
        }
        self.limit(out);
    }

    fn target_gain(&self, idx: usize, session: u32) -> f32 {
        let speaker = &self.speakers[idx];
        let settings = self.settings_of(session);
        if settings.muted || (self.discard_listener_audio && speaker.via_listener) {
            0.0
        } else {
            settings.volume
        }
    }

    fn settings_of(&self, session: u32) -> Settings {
        self.settings
            .iter()
            .find(|(s, _)| *s == session)
            .map_or(DEFAULT_SETTINGS, |(_, settings)| *settings)
    }

    fn update_settings(&mut self, session: u32, change: impl FnOnce(&mut Settings)) -> bool {
        if let Some((_, settings)) = self.settings.iter_mut().find(|(s, _)| *s == session) {
            change(settings);
            return true;
        }
        if self.settings.len() == MAX_SPEAKER_SETTINGS {
            return false;
        }
        let mut settings = DEFAULT_SETTINGS;
        change(&mut settings);
        self.settings.push((session, settings));
        true
    }

    /// The slot of `session`, or a newly assigned one: a free slot first,
    /// otherwise the silent speaker heard least recently.
    fn slot_for(&mut self, session: u32) -> Option<usize> {
        let speakers = &self.speakers;
        if let Some(idx) = speakers
            .iter()
            .position(|s| s.session == Some(session) && !s.released)
        {
            return Some(idx);
        }
        let idx = speakers
            .iter()
            .position(|s| s.session.is_none())
            .or_else(|| {
                speakers
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.jitter.is_idle() && !s.talking && !s.released)
                    .min_by_key(|(_, s)| s.jitter.last_arrival_ms())
                    .map(|(i, _)| i)
            })?;
        let speaker = &mut self.speakers[idx];
        speaker.session = Some(session);
        speaker.jitter.reset();
        speaker.gain = 0.0;
        speaker.counted_late = 0;
        speaker.counted_concealed = 0;
        Some(idx)
    }

    fn free_slot(&mut self, idx: usize, on_event: &mut dyn FnMut(TalkEvent)) {
        let speaker = &mut self.speakers[idx];
        if let (true, Some(session)) = (speaker.talking, speaker.session) {
            on_event(TalkEvent {
                session,
                talking: false,
            });
        }
        speaker.session = None;
        speaker.talking = false;
        speaker.released = false;
        speaker.jitter.reset();
        speaker.counted_late = 0;
        speaker.counted_concealed = 0;
    }

    /// Peak limiter (instant attack, ramped within the frame) followed by a
    /// soft clipper, so the sum never exceeds full scale.
    fn limit(&mut self, out: &mut Frame) {
        let peak = out.iter().fold(0.0f32, |p, s| p.max(s.abs()));
        let desired = if peak > LIMIT { LIMIT / peak } else { 1.0 };
        let start = self.limiter_gain;
        let end = if desired < start {
            desired
        } else {
            start + (desired - start) * LIMITER_RELEASE
        };
        self.limiter_gain = end;
        let step = (end - start) / FRAME_SAMPLES as f32;
        for (i, s) in out.iter_mut().enumerate() {
            *s = soft_clip(*s * (start + step * (i + 1) as f32));
        }
    }
}

/// Adds `src` to `out` with a gain ramping linearly from `from` to `to`.
fn add_ramped(out: &mut Frame, src: &Frame, from: f32, to: f32) {
    let step = (to - from) / FRAME_SAMPLES as f32;
    for (i, (o, s)) in out.iter_mut().zip(src.iter()).enumerate() {
        *o += s * (from + step * (i + 1) as f32);
    }
}

/// Identity up to [`LIMIT`], then a tanh curve that approaches 1.0.
fn soft_clip(x: f32) -> f32 {
    let magnitude = x.abs();
    if magnitude <= LIMIT {
        return x;
    }
    let headroom = 1.0 - LIMIT;
    let shaped = LIMIT + headroom * ((magnitude - LIMIT) / headroom).tanh();
    shaped.copysign(x)
}
