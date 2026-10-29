//! Adaptive per-speaker jitter buffer (FR-1.25).
//!
//! Mumble sequence numbers count 10 ms frames, so `seq × 10 ms` is the
//! sender's media clock. For every packet the *transit* `arrival − seq × 10 ms`
//! is measured; the spread above the smallest transit of the current talk
//! burst is the network jitter. The estimate jumps up immediately when a
//! packet arrives late, is held for [`JITTER_HOLD_MS`] and then decays slowly,
//! so the buffer grows only while packets arrive unevenly and shrinks back
//! afterwards.
//!
//! The target delay is one packet plus the jitter estimate. Each burst starts
//! once that much audio is buffered (one packet on a clean connection). While
//! playing, a buffer that stays above target drops a quiet 10 ms frame; an
//! empty buffer is bridged with Opus concealment, which shifts the timeline
//! and so grows the buffer by itself. Lost packets are rebuilt from the next
//! packet's in-band FEC when possible, otherwise concealed. A burst ends with
//! the sender's last-frame flag or, if that got lost, after a timeout.
//!
//! Everything is allocation-free after [`JitterBuffer::new`].

#[cfg(test)]
pub(crate) mod tests;

use crate::codec::{CodecError, MAX_DECODED_SAMPLES, MAX_PACKET_BYTES, OpusDecoder};
use crate::{FRAME_SAMPLES, Frame, SAMPLE_RATE};

/// Packets buffered per speaker, indexed by sequence number (320 ms of 10 ms
/// frames, far beyond the maximum delay). A jump beyond it starts a new stream.
const SLOTS: usize = 32;
const FRAME_MS: u64 = 10;
/// Upper bound of the jitter estimate; with one packet on top this keeps the
/// delay inside the 150 ms mouth-to-ear budget for typical 20 ms packets.
const MAX_JITTER_MS: f32 = 120.0;
/// Safety margin added once any jitter has been seen (scheduling noise).
const JITTER_MARGIN_MS: f32 = 5.0;
/// How long a jitter peak is remembered before the estimate decays.
const JITTER_HOLD_MS: u64 = 2_000;
/// Decay of the jitter estimate after the hold time: 10 ms per second.
const JITTER_DECAY_PER_MS: f32 = 0.01;
/// Concealed frames bridged while waiting for a late packet; afterwards
/// silence is played until the packet arrives or the burst times out.
const MAX_CONCEAL_FRAMES: u32 = 5;
/// Silence after the last packet (on top of the target delay) that ends a
/// burst whose last-frame packet was lost.
const END_TIMEOUT_MS: u64 = 100;
/// The buffer must stay above target this long before a quiet frame is dropped…
const SHRINK_AFTER_MS: u64 = 300;
/// …or this long before any frame is dropped.
const FORCE_SHRINK_AFTER_MS: u64 = 1_500;
/// RMS below which a frame counts as quiet enough to drop unnoticed.
const QUIET_RMS: f32 = 0.02;
/// Gaps longer than this are skipped instead of concealed.
const MAX_GAP_FRAMES: u64 = 10;
const SAMPLES_PER_MS: usize = SAMPLE_RATE as usize / 1000;

/// Result of [`JitterBuffer::pull`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pull {
    /// No burst is playing; the frame is silent.
    Silent,
    /// Decoded audio (possibly the tail of a burst padded with silence).
    Audio,
    /// The burst is playing but the frame was concealed or bridged with silence.
    Concealed,
}

/// Counters for diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JitterStats {
    /// Packets that arrived after their audio was already played.
    pub late_packets: u64,
    /// Packets with an unparsable Opus header or oversized payload.
    pub invalid_packets: u64,
    /// 10 ms frames rebuilt from in-band FEC.
    pub fec_frames: u64,
    /// 10 ms frames synthesized by packet-loss concealment while the stream went on. Frames
    /// bridged after a stream's last packet (its end, when the last-frame flag is missing) are
    /// not losses and are not counted.
    pub concealed_frames: u64,
    /// 10 ms frames decoded from received packets.
    pub decoded_frames: u64,
    /// 10 ms frames dropped to shrink the buffer.
    pub dropped_frames: u64,
    pub decode_errors: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Buffering,
    Playing,
}

#[derive(Clone, Copy)]
struct Slot {
    used: bool,
    seq: u64,
    /// Duration in 10 ms frames (0 for an empty last-frame marker).
    frames: u64,
    is_last: bool,
    len: usize,
    data: [u8; MAX_PACKET_BYTES],
}

const EMPTY_SLOT: Slot = Slot {
    used: false,
    seq: 0,
    frames: 0,
    is_last: false,
    len: 0,
    data: [0; MAX_PACKET_BYTES],
};

/// Jitter buffer and Opus decoder for one remote speaker.
pub struct JitterBuffer {
    decoder: OpusDecoder,
    slots: Box<[Slot]>,
    pcm: Box<[f32]>,
    pcm_read: usize,
    pcm_len: usize,
    state: State,
    next_seq: u64,
    end_pending: bool,
    conceal_run: u32,
    /// Frames bridged while waiting for a packet that may never come (the stream may have
    /// ended); counted as concealed only once the stream continues.
    bridged: u64,
    first_arrival_ms: u64,
    last_arrival_ms: u64,
    /// Smallest transit of the current burst, in ms.
    transit_base: Option<i64>,
    jitter_ms: f32,
    jitter_peak_ms: u64,
    last_decay_ms: u64,
    packet_frames: u64,
    over_target_since: Option<u64>,
    stats: JitterStats,
}

impl JitterBuffer {
    pub fn new() -> Result<Self, CodecError> {
        Ok(Self {
            decoder: OpusDecoder::new()?,
            slots: vec![EMPTY_SLOT; SLOTS].into_boxed_slice(),
            pcm: vec![0.0; MAX_DECODED_SAMPLES + FRAME_SAMPLES].into_boxed_slice(),
            pcm_read: 0,
            pcm_len: 0,
            state: State::Idle,
            next_seq: 0,
            end_pending: false,
            conceal_run: 0,
            bridged: 0,
            first_arrival_ms: 0,
            last_arrival_ms: 0,
            transit_base: None,
            jitter_ms: 0.0,
            jitter_peak_ms: 0,
            last_decay_ms: 0,
            packet_frames: 2,
            over_target_since: None,
            stats: JitterStats::default(),
        })
    }

    /// Adds a received packet. `arrival_ms` is a monotonic timestamp on the
    /// same clock as the `now_ms` given to [`pull`](Self::pull).
    pub fn push(&mut self, seq: u64, payload: &[u8], is_last: bool, arrival_ms: u64) {
        let Some(frames) = packet_frames(payload) else {
            self.stats.invalid_packets += 1;
            return;
        };
        if self.state == State::Idle {
            self.start_burst(arrival_ms);
        }
        self.update_jitter(seq, arrival_ms);
        self.last_arrival_ms = arrival_ms;
        if frames > 0 {
            self.packet_frames = frames;
        }

        if self.state == State::Playing {
            if seq < self.next_seq {
                self.stats.late_packets += 1;
                return;
            }
            if seq >= self.next_seq + SLOTS as u64 {
                // The sender's counter jumped (e.g. reconnect): start over.
                self.clear();
                self.start_burst(arrival_ms);
            }
        } else if let Some((min, max)) = self.seq_range()
            && seq.max(max) - seq.min(min) >= SLOTS as u64
        {
            self.slots.fill(EMPTY_SLOT);
        }

        let slot = &mut self.slots[seq as usize % SLOTS];
        if slot.used && slot.seq == seq {
            return;
        }
        slot.used = true;
        slot.seq = seq;
        slot.frames = frames;
        slot.is_last = is_last;
        slot.len = payload.len();
        slot.data[..payload.len()].copy_from_slice(payload);
    }

    /// Produces the next 10 ms of this speaker's audio into `out`.
    pub fn pull(&mut self, out: &mut Frame, now_ms: u64) -> Pull {
        self.decay_jitter(now_ms);
        match self.state {
            State::Idle => {
                out.fill(0.0);
                return Pull::Silent;
            }
            State::Buffering if !self.ready_to_play(now_ms) => {
                out.fill(0.0);
                return Pull::Silent;
            }
            State::Buffering => {
                self.state = State::Playing;
                self.next_seq = self.seq_range().map_or(0, |(min, _)| min);
            }
            State::Playing => {}
        }

        let mut concealed = self.fill_pcm();
        if self.should_shrink(now_ms) {
            self.consume(FRAME_SAMPLES);
            self.stats.dropped_frames += 1;
            self.over_target_since = None;
            concealed |= self.fill_pcm();
        }

        let n = self.pcm_len.min(FRAME_SAMPLES);
        out[..n].copy_from_slice(&self.pcm[self.pcm_read..self.pcm_read + n]);
        out[n..].fill(0.0);
        self.consume(n);

        let drained = self.pcm_len == 0 && !self.has_packets();
        let timed_out =
            now_ms.saturating_sub(self.last_arrival_ms) > END_TIMEOUT_MS + self.target_delay_ms();
        if drained && (self.end_pending || timed_out) {
            self.finish_burst();
            return if n == 0 { Pull::Silent } else { Pull::Audio };
        }
        if n == 0 || concealed {
            Pull::Concealed
        } else {
            Pull::Audio
        }
    }

    /// Whether a burst is currently being played (the speaker is talking).
    pub fn is_playing(&self) -> bool {
        self.state == State::Playing
    }

    /// Whether nothing is buffered or playing.
    pub fn is_idle(&self) -> bool {
        self.state == State::Idle
    }

    /// Arrival time of the most recent packet.
    pub fn last_arrival_ms(&self) -> u64 {
        self.last_arrival_ms
    }

    /// Current target delay: one packet plus the jitter estimate, in 10 ms steps.
    pub fn target_delay_ms(&self) -> u64 {
        let margin = if self.jitter_ms > 0.5 {
            JITTER_MARGIN_MS
        } else {
            0.0
        };
        let jitter = (self.jitter_ms + margin).ceil() as u64;
        (self.packet_frames * FRAME_MS + jitter).div_ceil(FRAME_MS) * FRAME_MS
    }

    /// Audio currently buffered (decoded and not yet decoded), in ms.
    pub fn buffered_ms(&self) -> u64 {
        let pcm_ms = (self.pcm_len / SAMPLES_PER_MS) as u64;
        let start = match self.state {
            State::Playing => self.next_seq,
            _ => self.seq_range().map_or(0, |(min, _)| min),
        };
        let packets_ms = self
            .slots
            .iter()
            .filter(|s| s.used && s.seq >= start)
            .map(|s| s.seq + s.frames)
            .max()
            .map_or(0, |end| (end - start) * FRAME_MS);
        pcm_ms + packets_ms
    }

    pub fn stats(&self) -> JitterStats {
        self.stats
    }

    /// Forgets everything including the learned jitter, for reuse by another speaker.
    pub fn reset(&mut self) {
        self.clear();
        self.state = State::Idle;
        self.jitter_ms = 0.0;
        self.packet_frames = 2;
        self.stats = JitterStats::default();
    }

    fn start_burst(&mut self, arrival_ms: u64) {
        self.state = State::Buffering;
        self.first_arrival_ms = arrival_ms;
        self.transit_base = None;
        self.end_pending = false;
        self.conceal_run = 0;
        self.bridged = 0;
        self.over_target_since = None;
    }

    fn finish_burst(&mut self) {
        // Packets of the next burst may already be waiting behind the end.
        let next = self.next_seq;
        for slot in self.slots.iter_mut().filter(|s| s.used && s.seq < next) {
            slot.used = false;
        }
        self.pcm_read = 0;
        self.pcm_len = 0;
        // Frames bridged after the last packet were the stream's end, not a loss.
        self.bridged = 0;
        // The sender resets its encoder per talk burst; keep in step.
        if self.decoder.reset().is_err() {
            self.stats.decode_errors += 1;
        }
        if self.has_packets() {
            self.start_burst(self.last_arrival_ms);
        } else {
            self.state = State::Idle;
        }
    }

    fn clear(&mut self) {
        self.slots.fill(EMPTY_SLOT);
        self.bridged = 0;
        self.pcm_read = 0;
        self.pcm_len = 0;
        if self.decoder.reset().is_err() {
            self.stats.decode_errors += 1;
        }
    }

    fn ready_to_play(&self, now_ms: u64) -> bool {
        // Waiting one jitter span after the first packet is enough even when
        // the packets that should have filled the buffer were lost.
        let waited = now_ms.saturating_sub(self.first_arrival_ms);
        let target = self.target_delay_ms();
        self.buffered_ms() >= target
            || self.slots.iter().any(|s| s.used && s.is_last)
            || waited + self.packet_frames * FRAME_MS >= target
    }

    fn update_jitter(&mut self, seq: u64, arrival_ms: u64) {
        let transit = (arrival_ms as i64).wrapping_sub((seq as i64).wrapping_mul(FRAME_MS as i64));
        let base = self.transit_base.map_or(transit, |b| b.min(transit));
        self.transit_base = Some(base);
        let excess = (transit - base) as f32;
        if excess > self.jitter_ms {
            self.jitter_ms = excess.min(MAX_JITTER_MS);
            self.jitter_peak_ms = arrival_ms;
        }
    }

    fn decay_jitter(&mut self, now_ms: u64) {
        let elapsed = now_ms.saturating_sub(self.last_decay_ms);
        self.last_decay_ms = now_ms;
        if now_ms.saturating_sub(self.jitter_peak_ms) > JITTER_HOLD_MS {
            self.jitter_ms = (self.jitter_ms - elapsed as f32 * JITTER_DECAY_PER_MS).max(0.0);
        }
    }

    fn should_shrink(&mut self, now_ms: u64) -> bool {
        if self.buffered_ms() <= self.target_delay_ms() + FRAME_MS || self.pcm_len < FRAME_SAMPLES {
            self.over_target_since = None;
            return false;
        }
        let over_for = now_ms - *self.over_target_since.get_or_insert(now_ms);
        let head = &self.pcm[self.pcm_read..self.pcm_read + FRAME_SAMPLES];
        let rms = (head.iter().map(|s| s * s).sum::<f32>() / FRAME_SAMPLES as f32).sqrt();
        over_for >= FORCE_SHRINK_AFTER_MS || (over_for >= SHRINK_AFTER_MS && rms < QUIET_RMS)
    }

    /// Decodes until a frame is available or nothing more can be produced.
    /// Returns whether any concealment or FEC was involved.
    fn fill_pcm(&mut self) -> bool {
        let mut concealed = false;
        while self.pcm_len < FRAME_SAMPLES && !self.end_pending {
            let idx = self.next_seq as usize % SLOTS;
            let slot = self.slots[idx];
            if slot.used && slot.seq == self.next_seq {
                self.slots[idx].used = false;
                self.decode_slot(idx);
                self.next_seq += slot.frames;
                self.end_pending = slot.is_last;
                self.conceal_run = 0;
                self.commit_bridged();
                continue;
            }
            concealed = true;
            match self.earliest_after(self.next_seq) {
                Some(later) => {
                    self.commit_bridged();
                    let gap = self.slots[later].seq - self.next_seq;
                    if gap > MAX_GAP_FRAMES {
                        self.next_seq = self.slots[later].seq;
                    } else if gap <= self.slots[later].frames {
                        self.recover(later, gap);
                        self.next_seq += gap;
                    } else {
                        self.conceal();
                        self.next_seq += 1;
                    }
                }
                None if self.conceal_run < MAX_CONCEAL_FRAMES => {
                    // Probably just late: bridge without advancing, which
                    // delays the stream by one frame and grows the buffer.
                    self.compact();
                    self.synthesize(FRAME_SAMPLES, None);
                    self.bridged += 1;
                    self.conceal_run += 1;
                }
                None => break,
            }
        }
        concealed
    }

    fn decode_slot(&mut self, idx: usize) {
        self.compact();
        let slot = &self.slots[idx];
        if slot.len == 0 {
            return;
        }
        let out = &mut self.pcm[self.pcm_len..];
        match self.decoder.decode(&slot.data[..slot.len], out) {
            Ok(n) => {
                self.pcm_len += n;
                self.stats.decoded_frames += (n / FRAME_SAMPLES) as u64;
            }
            Err(_) => {
                self.stats.decode_errors += 1;
                let n = slot.frames as usize * FRAME_SAMPLES;
                self.synthesize(n, None);
            }
        }
    }

    fn recover(&mut self, later: usize, gap: u64) {
        self.compact();
        self.stats.fec_frames += gap;
        self.synthesize(gap as usize * FRAME_SAMPLES, Some(later));
    }

    /// The stream went on after a bridge: the bridged frames covered a real gap.
    fn commit_bridged(&mut self) {
        self.stats.concealed_frames += self.bridged;
        self.bridged = 0;
    }

    fn conceal(&mut self) {
        self.compact();
        self.stats.concealed_frames += 1;
        self.synthesize(FRAME_SAMPLES, None);
    }

    /// Appends `n` samples of FEC (from the packet in slot `fec_from`) or
    /// concealment; silence if the decoder fails, so playback always advances.
    fn synthesize(&mut self, n: usize, fec_from: Option<usize>) {
        let n = n.min(self.pcm.len() - self.pcm_len);
        let out = &mut self.pcm[self.pcm_len..self.pcm_len + n];
        let result = match fec_from {
            Some(idx) => {
                let slot = &self.slots[idx];
                self.decoder.recover_from_next(&slot.data[..slot.len], out)
            }
            None => self.decoder.conceal(out),
        };
        if !matches!(result, Ok(written) if written == n) {
            if result.is_err() {
                self.stats.decode_errors += 1;
            }
            out.fill(0.0);
        }
        self.pcm_len += n;
    }

    fn compact(&mut self) {
        self.pcm
            .copy_within(self.pcm_read..self.pcm_read + self.pcm_len, 0);
        self.pcm_read = 0;
    }

    fn consume(&mut self, n: usize) {
        self.pcm_read += n;
        self.pcm_len -= n;
        if self.pcm_len == 0 {
            self.pcm_read = 0;
        }
    }

    fn has_packets(&self) -> bool {
        self.slots.iter().any(|s| s.used)
    }

    fn seq_range(&self) -> Option<(u64, u64)> {
        self.slots
            .iter()
            .filter(|s| s.used)
            .fold(None, |acc, s| match acc {
                None => Some((s.seq, s.seq)),
                Some((min, max)) => Some((min.min(s.seq), max.max(s.seq))),
            })
    }

    fn earliest_after(&self, seq: u64) -> Option<usize> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.used && s.seq > seq)
            .min_by_key(|(_, s)| s.seq)
            .map(|(i, _)| i)
    }
}

/// Packet duration in 10 ms frames (rounded up), 0 for an empty last-frame
/// marker, `None` if the payload is not a valid Opus packet.
fn packet_frames(payload: &[u8]) -> Option<u64> {
    if payload.is_empty() {
        return Some(0);
    }
    if payload.len() > MAX_PACKET_BYTES {
        return None;
    }
    match opus::packet::get_nb_samples(payload, SAMPLE_RATE) {
        Ok(n) if n > 0 => Some(n.div_ceil(FRAME_SAMPLES) as u64),
        _ => None,
    }
}
