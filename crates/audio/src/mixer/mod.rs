//! Playback mixer: sums all remote speakers into 10 ms playback frames.
//!
//! [`Mixer`] is the allocation-free core: per-speaker jitter buffers, local
//! volume (0–200 %) and mute (FR-1.18/1.19), the global discard switch for
//! audio that only arrives because we listen to followed channels (FR-1.31),
//! 3D placement of voices that carry a position (FR-1.41, see [`crate::spatial`]),
//! talking-state events, an optional mix-in frame (notification sounds,
//! mic-test loopback) and a limiter. Output is stereo. [`Playback`] runs it on its own thread
//! and paces it against a [`PlaybackSink`] (the device `FrameSink` with feature `io`).

mod playback;
pub mod share;
#[cfg(test)]
mod tests;

pub use playback::{
    PacketSender, Playback, PlaybackConfig, PlaybackError, PlaybackPorts, PlaybackSink,
    PlaybackStats, downmix,
};

use std::sync::Arc;
use std::time::Instant;

use crate::codec::{CodecError, MAX_PACKET_BYTES};
use crate::jitter::{JitterBuffer, Pull};
use crate::spatial::{Listener, Placement, Spatializer};
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
    /// The speaker's position (positional audio), if the packet carries one.
    pub position: Option<[f32; 3]>,
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
            position: None,
            len: opus.len(),
            payload,
        })
    }

    /// The same packet carrying the speaker's position.
    pub fn with_position(mut self, position: Option<[f32; 3]>) -> Self {
        self.position = position;
        self
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
    /// Linear gain derived from the slider volume by [`volume_gain`].
    gain: f32,
    muted: bool,
}

const DEFAULT_SETTINGS: Settings = Settings {
    gain: 1.0,
    muted: false,
};

/// Gain for a slider volume (0.0–2.0). Loudness is heard logarithmically, so a
/// linear gain would make the slider feel nearly dead (50 % is only −6 dB, and
/// 200 % is only +6 dB, which the limiter eats on normal speech). The cubic
/// curve – the convention desktop mixers use – gives −18 dB at 50 % and
/// +18 dB at 200 % while keeping 100 % at unity.
fn volume_gain(volume: f32) -> f32 {
    volume.clamp(0.0, MAX_VOLUME).powi(3)
}

struct Speaker {
    session: Option<u32>,
    jitter: JitterBuffer,
    /// Gain applied at the end of the last frame (ramped to avoid clicks).
    gain: f32,
    talking: bool,
    via_listener: bool,
    /// Position from the latest packet; `None` plays the voice unplaced.
    position: Option<[f32; 3]>,
    spatial: Spatializer,
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
    /// Voices with a position are placed around this listener while it has a pose.
    listener: Option<Arc<Listener>>,
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
                    position: None,
                    spatial: Spatializer::default(),
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
            listener: None,
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
        speaker.position = packet.position;
        speaker
            .jitter
            .push(packet.seq, packet.payload(), packet.is_last, arrival_ms);
    }

    /// Sets a person's local volume (0.0–2.0 = 0–200 %). Returns `false` if
    /// the settings table is full.
    pub fn set_volume(&mut self, session: u32, volume: f32) -> bool {
        let gain = volume_gain(volume);
        self.update_settings(session, |s| s.gain = gain)
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

    /// The listener voices are placed around (positional audio).
    pub fn set_listener(&mut self, listener: Arc<Listener>) {
        self.listener = Some(listener);
    }

    pub fn stats(&self) -> MixerStats {
        self.stats
    }

    /// Mixes the next 10 ms into `left`/`right`. `mix_in` is added unscaled to
    /// both after the speakers (notification sounds, mic-test loopback).
    /// Talking changes are reported through `on_event`.
    pub fn mix(
        &mut self,
        left: &mut Frame,
        right: &mut Frame,
        mix_in: Option<&Frame>,
        now_ms: u64,
        on_event: &mut dyn FnMut(TalkEvent),
    ) {
        left.fill(0.0);
        right.fill(0.0);
        let listener = self.listener.as_ref().and_then(|l| l.state());
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
            if pull == Pull::Silent {
                speaker.spatial.silence();
            } else {
                let placement = match (&listener, speaker.position) {
                    (Some(l), Some(p)) => Placement::at(l, p),
                    _ => Placement::CENTER,
                };
                speaker.spatial.render(
                    &self.scratch,
                    &placement,
                    speaker.gain,
                    target_gain,
                    left,
                    right,
                );
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
            for ((l, r), e) in left.iter_mut().zip(right.iter_mut()).zip(extra.iter()) {
                *l += e;
                *r += e;
            }
        }
        self.limit(left, right);
    }

    fn target_gain(&self, idx: usize, session: u32) -> f32 {
        let speaker = &self.speakers[idx];
        let settings = self.settings_of(session);
        if settings.muted || (self.discard_listener_audio && speaker.via_listener) {
            0.0
        } else {
            settings.gain
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
        speaker.position = None;
        speaker.spatial.reset();
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

    /// Peak limiter linked over both channels (instant attack, ramped within
    /// the frame) followed by a soft clipper, so the sum never exceeds full scale.
    fn limit(&mut self, left: &mut Frame, right: &mut Frame) {
        let peak = left
            .iter()
            .chain(right.iter())
            .fold(0.0f32, |p, s| p.max(s.abs()));
        let desired = if peak > LIMIT { LIMIT / peak } else { 1.0 };
        let start = self.limiter_gain;
        let end = if desired < start {
            desired
        } else {
            start + (desired - start) * LIMITER_RELEASE
        };
        self.limiter_gain = end;
        let step = (end - start) / FRAME_SAMPLES as f32;
        for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let gain = start + step * (i + 1) as f32;
            *l = soft_clip(*l * gain);
            *r = soft_clip(*r * gain);
        }
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
