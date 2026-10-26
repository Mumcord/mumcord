//! Sound of watched screen shares (FR-2.42): each share is its own source of the playback mix.
//!
//! The viewer decodes a share's Opus packets off the playback thread ([`ShareSoundWriter`]) and
//! writes the samples into a lock-free single-producer ring; the playback thread reads every
//! ring once per frame ([`ShareSlots`]) and adds it to the mix at the [`ShareGain`] the engine
//! sets (auto-ducking, FR-2.76), ramped per sample so a change never clicks. New rings reach the
//! playback thread through [`SharePort`]. A ring whose writer is gone is played out and then
//! handed back through the port, so its memory is freed on the engine side and never on the
//! playback thread.
//!
//! A ring starts playing once [`START_SAMPLES`] are queued, which absorbs network jitter, and
//! skips ahead when more than [`MAX_QUEUED_SAMPLES`] piled up, so the sound never drifts far
//! behind the picture.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::codec::{CodecError, MAX_DECODED_SAMPLES, OpusDecoder, OpusEncoder, PacketDuration};
use crate::{FRAME_SAMPLES, Frame, SAMPLE_RATE};

/// Shares with sound that can play at the same time; slots exist up front.
pub const MAX_SHARE_SOURCES: usize = 8;
/// Opus bitrate of share sound: music tuning, mono. Small next to the video's 700–2000 kbps.
pub const SHARE_BITRATE_BPS: i32 = 96_000;
/// Ring size per share (500 ms).
const RING_SAMPLES: usize = SAMPLE_RATE as usize / 2;
/// Queued audio before a share (re)starts playing (60 ms, three 20 ms packets).
pub const START_SAMPLES: usize = 3 * SAMPLE_RATE as usize / 50;
/// Above this much queued audio (200 ms) the share skips back to [`START_SAMPLES`].
pub const MAX_QUEUED_SAMPLES: usize = SAMPLE_RATE as usize / 5;

/// Ramp of the share gain per sample: a full 0–1 swing takes 150 ms down (FR-2.76) …
const GAIN_DOWN_STEP: f32 = 1.0 / (SAMPLE_RATE as f32 * 0.150);
/// … and 300 ms back up, so the sound returns softly.
const GAIN_UP_STEP: f32 = 1.0 / (SAMPLE_RATE as f32 * 0.300);

/// Gain of all watched shares' sound, 0.0–1.0, set by the engine and read by the playback thread
/// (auto-ducking, FR-2.76).
#[derive(Debug)]
pub struct ShareGain(AtomicU32);

impl Default for ShareGain {
    fn default() -> Self {
        Self(AtomicU32::new(1.0_f32.to_bits()))
    }
}

impl ShareGain {
    /// Sets the gain the share sound ramps to; clamped to 0.0–1.0, NaN counts as 1.0.
    pub fn set(&self, gain: f32) {
        let gain = if gain.is_nan() {
            1.0
        } else {
            gain.clamp(0.0, 1.0)
        };
        self.0.store(gain.to_bits(), Ordering::Relaxed);
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// The sharer's encoder: its own instance, tuned for music (any app sound, not speech), with
/// 20 ms packets.
pub fn share_encoder() -> Result<OpusEncoder, CodecError> {
    let mut encoder = OpusEncoder::new(SHARE_BITRATE_BPS, PacketDuration::Ms20)?;
    encoder.set_music(true)?;
    Ok(encoder)
}

/// Counters of one watched share's sound, shared by its writer and the playback thread.
#[derive(Debug, Default)]
pub struct ShareStats {
    packets: AtomicU64,
    /// Decoded samples that did not fit into the ring.
    overflow_samples: AtomicU64,
    played_samples: AtomicU64,
    underruns: AtomicU64,
    skipped_samples: AtomicU64,
    /// RMS of the last played frame, as `f32` bits.
    level: AtomicU32,
}

impl ShareStats {
    pub fn snapshot(&self) -> ShareSoundStats {
        ShareSoundStats {
            packets: self.packets.load(Ordering::Relaxed),
            overflow_samples: self.overflow_samples.load(Ordering::Relaxed),
            played_samples: self.played_samples.load(Ordering::Relaxed),
            underruns: self.underruns.load(Ordering::Relaxed),
            skipped_samples: self.skipped_samples.load(Ordering::Relaxed),
            rms: f32::from_bits(self.level.load(Ordering::Relaxed)),
        }
    }
}

/// How a watched share's sound plays (test hook and diagnostics).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ShareSoundStats {
    pub packets: u64,
    pub overflow_samples: u64,
    pub played_samples: u64,
    /// Times the ring ran dry while playing.
    pub underruns: u64,
    pub skipped_samples: u64,
    /// RMS of the last played 10 ms, 0.0–1.0.
    pub rms: f32,
}

/// A new share ring: the writer for the engine, the source for [`SharePort::add`].
pub fn share_channel(
    stats: Arc<ShareStats>,
) -> Result<(ShareSoundWriter, ShareSource), CodecError> {
    let (producer, consumer) = RingBuffer::new(RING_SAMPLES);
    let writer = ShareSoundWriter {
        decoder: OpusDecoder::new()?,
        producer,
        pcm: vec![0.0; MAX_DECODED_SAMPLES],
        stats: stats.clone(),
    };
    let source = ShareSource {
        consumer,
        playing: false,
        stats,
    };
    Ok((writer, source))
}

/// Decodes one share's packets into its ring. Not real-time: runs on an engine task.
pub struct ShareSoundWriter {
    decoder: OpusDecoder,
    producer: Producer<f32>,
    pcm: Vec<f32>,
    stats: Arc<ShareStats>,
}

impl ShareSoundWriter {
    /// Decodes `packet` and queues its samples; what does not fit is dropped and counted (the
    /// playback side skips ahead long before the ring is full, so this means it stopped).
    pub fn push_packet(&mut self, packet: &[u8]) -> Result<(), CodecError> {
        let n = self.decoder.decode(packet, &mut self.pcm)?;
        self.stats.packets.fetch_add(1, Ordering::Relaxed);
        let fits = n.min(self.producer.slots());
        if fits < n {
            self.stats
                .overflow_samples
                .fetch_add((n - fits) as u64, Ordering::Relaxed);
        }
        if let Ok(chunk) = self.producer.write_chunk_uninit(fits) {
            chunk.fill_from_iter(self.pcm[..fits].iter().copied());
        }
        Ok(())
    }

    /// Whether the playback thread still reads this ring.
    pub fn is_playing_out(&self) -> bool {
        !self.producer.is_abandoned()
    }
}

/// The playback side of one share ring.
pub struct ShareSource {
    consumer: Consumer<f32>,
    playing: bool,
    stats: Arc<ShareStats>,
}

impl ShareSource {
    /// Adds the next 10 ms to `out`; `false` when nothing was added.
    fn mix_into(&mut self, out: &mut Frame) -> bool {
        let mut queued = self.consumer.slots();
        if queued > MAX_QUEUED_SAMPLES {
            let skip = queued - START_SAMPLES;
            if let Ok(chunk) = self.consumer.read_chunk(skip) {
                chunk.commit_all();
            }
            self.stats
                .skipped_samples
                .fetch_add(skip as u64, Ordering::Relaxed);
            queued = START_SAMPLES;
        }
        if !self.playing {
            // Once the writer is gone, whatever is left plays out without waiting.
            let ending = self.consumer.is_abandoned() && queued > 0;
            if queued < START_SAMPLES && !ending {
                return false;
            }
            self.playing = true;
        }
        let n = queued.min(FRAME_SAMPLES);
        if n < FRAME_SAMPLES {
            self.playing = false;
            if !self.consumer.is_abandoned() {
                self.stats.underruns.fetch_add(1, Ordering::Relaxed);
            }
        }
        if n == 0 {
            return false;
        }
        let Ok(chunk) = self.consumer.read_chunk(n) else {
            return false;
        };
        let (a, b) = chunk.as_slices();
        let mut energy = 0.0_f32;
        for (dst, src) in out.iter_mut().zip(a.iter().chain(b)) {
            *dst += src;
            energy += src * src;
        }
        chunk.commit_all();
        self.stats
            .played_samples
            .fetch_add(n as u64, Ordering::Relaxed);
        let rms = (energy / FRAME_SAMPLES as f32).sqrt();
        self.stats.level.store(rms.to_bits(), Ordering::Relaxed);
        true
    }

    /// The writer is gone and everything was played.
    fn is_finished(&self) -> bool {
        self.consumer.is_abandoned() && self.consumer.is_empty()
    }
}

/// Engine side of the share inputs of one playback thread.
pub struct SharePort {
    add: Producer<ShareSource>,
    retired: Consumer<ShareSource>,
}

impl SharePort {
    /// Hands `source` to the playback thread; gives it back when [`MAX_SHARE_SOURCES`] are
    /// already queued.
    pub fn add(&mut self, source: ShareSource) -> Result<(), ShareSource> {
        self.collect();
        self.add.push(source).map_err(|rtrb::PushError::Full(s)| s)
    }

    /// Frees the rings the playback thread is done with.
    pub fn collect(&mut self) {
        while self.retired.pop().is_ok() {}
    }
}

/// Playback side: the share sources being mixed. Real-time safe: no allocation, no locks.
pub(crate) struct ShareSlots {
    slots: [Option<ShareSource>; MAX_SHARE_SOURCES],
    add: Consumer<ShareSource>,
    retired: Producer<ShareSource>,
    gain: Arc<ShareGain>,
    /// The gain applied to the last sample, ramping towards `gain`.
    current_gain: f32,
    /// The shares' sum before the gain, so the gain never touches the rest of `out`.
    sum: Frame,
}

/// A connected [`SharePort`] and [`ShareSlots`] whose sound plays at `gain`.
pub(crate) fn share_slots(gain: Arc<ShareGain>) -> (SharePort, ShareSlots) {
    let (add_tx, add_rx) = RingBuffer::new(MAX_SHARE_SOURCES);
    let (retired_tx, retired_rx) = RingBuffer::new(MAX_SHARE_SOURCES);
    let port = SharePort {
        add: add_tx,
        retired: retired_rx,
    };
    let slots = ShareSlots {
        slots: std::array::from_fn(|_| None),
        add: add_rx,
        retired: retired_tx,
        current_gain: gain.get(),
        gain,
        sum: [0.0; FRAME_SAMPLES],
    };
    (port, slots)
}

impl ShareSlots {
    /// Takes in new sources and hands back finished ones. A source waits in its queue while
    /// every slot is busy, and a finished one stays in its slot while the engine has not
    /// collected the earlier ones yet.
    pub fn update(&mut self) {
        for slot in &mut self.slots {
            if slot.as_ref().is_some_and(ShareSource::is_finished)
                && !self.retired.is_full()
                && let Some(done) = slot.take()
            {
                // Not full, checked above; the engine side frees it.
                let _ = self.retired.push(done);
            }
            if slot.is_none() {
                *slot = self.add.pop().ok();
            }
        }
    }

    /// Adds every share's next 10 ms to `out`; `false` when no share added anything.
    pub fn mix_into(&mut self, out: &mut Frame) -> bool {
        let target = self.gain.get();
        self.sum.fill(0.0);
        let mut any = false;
        for source in self.slots.iter_mut().flatten() {
            any |= source.mix_into(&mut self.sum);
        }
        if !any {
            // Nothing plays, so jumping to the target cannot click.
            self.current_gain = target;
            return false;
        }
        let mut gain = self.current_gain;
        for (dst, src) in out.iter_mut().zip(&self.sum) {
            gain = if gain > target {
                (gain - GAIN_DOWN_STEP).max(target)
            } else {
                (gain + GAIN_UP_STEP).min(target)
            };
            *dst += src * gain;
        }
        self.current_gain = gain;
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::codec::MAX_PACKET_BYTES;

    const TONE_HZ: f32 = 440.0;
    const AMPLITUDE: f32 = 0.25;

    fn tone(frame: usize) -> Frame {
        let mut out = [0.0; FRAME_SAMPLES];
        for (i, s) in out.iter_mut().enumerate() {
            let t = (frame * FRAME_SAMPLES + i) as f32 / SAMPLE_RATE as f32;
            *s = AMPLITUDE * (std::f32::consts::TAU * TONE_HZ * t).sin();
        }
        out
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    /// Encodes `frames` of the test tone with the share encoder.
    fn tone_packets(frames: usize) -> Vec<Vec<u8>> {
        let mut encoder = share_encoder().unwrap();
        let mut out = [0u8; MAX_PACKET_BYTES];
        (0..frames)
            .filter_map(|n| {
                encoder
                    .push_frame(&tone(n), &mut out)
                    .unwrap()
                    .map(|len| out[..len].to_vec())
            })
            .collect()
    }

    fn channel() -> (ShareSoundWriter, ShareSource, Arc<ShareStats>) {
        let stats = Arc::new(ShareStats::default());
        let (writer, source) = share_channel(stats.clone()).unwrap();
        (writer, source, stats)
    }

    #[test]
    fn the_encoder_uses_the_music_tuning() {
        let mut encoder = share_encoder().unwrap();
        assert!(encoder.is_music().unwrap());
        assert_eq!(encoder.bitrate().unwrap(), SHARE_BITRATE_BPS);
    }

    #[test]
    fn a_tone_round_trips_at_its_level() {
        let (mut writer, mut source, stats) = channel();
        let mut played = Vec::new();
        let mut frame = [0.0; FRAME_SAMPLES];
        // One 20 ms packet arrives per two played frames, as in real time.
        for packet in tone_packets(100) {
            writer.push_packet(&packet).unwrap();
            for _ in 0..2 {
                frame.fill(0.0);
                if source.mix_into(&mut frame) {
                    played.extend_from_slice(&frame);
                }
            }
        }
        assert!(played.len() >= 45 * 960, "played {} samples", played.len());
        // Past the codec's start-up, the level matches the sent tone (RMS of a sine: A/√2).
        let expected = AMPLITUDE / std::f32::consts::SQRT_2;
        let level = rms(&played[4800..]);
        assert!((level - expected).abs() < expected * 0.1, "rms {level}");
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.packets, 50);
        assert!((snapshot.rms - expected).abs() < expected * 0.15);
    }

    #[test]
    fn playback_waits_for_the_start_buffer_and_rebuffers_after_an_underrun() {
        let (mut writer, mut source, stats) = channel();
        let packets = tone_packets(20);
        let mut frame = [0.0; FRAME_SAMPLES];
        writer.push_packet(&packets[0]).unwrap();
        writer.push_packet(&packets[1]).unwrap();
        assert!(!source.mix_into(&mut frame), "40 ms is not enough to start");
        writer.push_packet(&packets[2]).unwrap();
        for _ in 0..6 {
            assert!(source.mix_into(&mut frame));
        }
        assert!(!source.mix_into(&mut frame), "ran dry");
        assert_eq!(stats.snapshot().underruns, 1);
        writer.push_packet(&packets[3]).unwrap();
        assert!(
            !source.mix_into(&mut frame),
            "waits for the start buffer again"
        );
        assert_eq!(
            stats.snapshot().underruns,
            1,
            "silence while waiting is no underrun"
        );
    }

    #[test]
    fn a_backlog_skips_ahead_and_a_full_ring_drops_new_samples() {
        let (mut writer, mut source, stats) = channel();
        let packet = &tone_packets(2)[0];
        // 30 packets of 20 ms: 600 ms, more than the 500 ms ring.
        for _ in 0..30 {
            writer.push_packet(packet).unwrap();
        }
        let snapshot = stats.snapshot();
        assert_eq!(snapshot.overflow_samples, (30 * 960 - RING_SAMPLES) as u64);
        let mut frame = [0.0; FRAME_SAMPLES];
        assert!(source.mix_into(&mut frame));
        let snapshot = stats.snapshot();
        assert_eq!(
            snapshot.skipped_samples,
            (RING_SAMPLES - START_SAMPLES) as u64
        );
        assert_eq!(source.consumer.slots(), START_SAMPLES - FRAME_SAMPLES);
    }

    #[test]
    fn slots_mix_every_share_and_hand_finished_rings_back() {
        let (mut port, mut slots) = share_slots(Arc::default());
        let (mut a, source_a, _) = channel();
        let (mut b, source_b, _) = channel();
        assert!(port.add(source_a).is_ok());
        assert!(port.add(source_b).is_ok());
        slots.update();
        let packets = tone_packets(10);
        for packet in &packets[..3] {
            a.push_packet(packet).unwrap();
            b.push_packet(packet).unwrap();
        }
        let mut both = [0.0; FRAME_SAMPLES];
        assert!(slots.mix_into(&mut both));
        let (mut c, mut alone, _) = channel();
        for packet in &packets[..3] {
            c.push_packet(packet).unwrap();
        }
        let mut one = [0.0; FRAME_SAMPLES];
        assert!(alone.mix_into(&mut one));
        for (sum, single) in both.iter().zip(&one) {
            assert!((sum - 2.0 * single).abs() < 1e-6, "two shares add up");
        }

        drop(a);
        let mut frame = [0.0; FRAME_SAMPLES];
        for _ in 0..5 {
            slots.mix_into(&mut frame);
        }
        slots.update();
        assert_eq!(
            slots.slots.iter().flatten().count(),
            1,
            "a's ring played out"
        );
        assert!(port.retired.pop().is_ok(), "and came back for freeing");
        assert!(b.is_playing_out());
    }

    #[test]
    fn the_gain_is_clamped_and_starts_at_unity() {
        let gain = ShareGain::default();
        assert_eq!(gain.get(), 1.0);
        gain.set(0.3);
        assert_eq!(gain.get(), 0.3);
        gain.set(-1.0);
        assert_eq!(gain.get(), 0.0);
        gain.set(2.0);
        assert_eq!(gain.get(), 1.0);
        gain.set(f32::NAN);
        assert_eq!(gain.get(), 1.0);
    }

    /// Slots playing 200 ms of a constant 0.5 from one share; samples bypass the codec so gains
    /// read exactly. Keep the producer, or the share counts as ended.
    fn constant_share(gain: &Arc<ShareGain>) -> (ShareSlots, Producer<f32>, SharePort) {
        let (mut port, mut slots) = share_slots(gain.clone());
        let (mut producer, consumer) = RingBuffer::new(RING_SAMPLES);
        for _ in 0..MAX_QUEUED_SAMPLES {
            producer.push(0.5).unwrap();
        }
        let source = ShareSource {
            consumer,
            playing: false,
            stats: Arc::default(),
        };
        assert!(port.add(source).is_ok());
        slots.update();
        (slots, producer, port)
    }

    #[test]
    fn the_share_gain_ramps_down_in_150_ms_and_up_in_300_ms() {
        let gain = Arc::new(ShareGain::default());
        let (mut slots, _producer, _port) = constant_share(&gain);
        let mut frame = [0.0; FRAME_SAMPLES];
        assert!(slots.mix_into(&mut frame));
        assert!(frame.iter().all(|s| (s - 0.5).abs() < 1e-6), "unity gain");

        gain.set(0.3);
        frame.fill(0.0);
        assert!(slots.mix_into(&mut frame));
        // 0.7 down at 1/7200 per sample: still on the way after 10 ms, no jump.
        assert!(
            frame[0] < 0.5 && frame[0] > 0.499,
            "first sample {}",
            frame[0]
        );
        assert!(frame.windows(2).all(|w| w[1] <= w[0]), "falls smoothly");
        let expected = 0.5 * (1.0 - FRAME_SAMPLES as f32 * GAIN_DOWN_STEP);
        assert!((frame[FRAME_SAMPLES - 1] - expected).abs() < 1e-4);
        // 150 ms in all reaches the lowered level.
        for _ in 0..14 {
            frame.fill(0.0);
            slots.mix_into(&mut frame);
        }
        assert!((frame[FRAME_SAMPLES - 1] - 0.15).abs() < 1e-4, "at 30 %");

        gain.set(1.0);
        frame.fill(0.0);
        slots.mix_into(&mut frame);
        assert!(frame.windows(2).all(|w| w[1] >= w[0]), "rises smoothly");
        let expected = 0.5 * (0.3 + FRAME_SAMPLES as f32 * GAIN_UP_STEP);
        assert!((frame[FRAME_SAMPLES - 1] - expected).abs() < 1e-4);
    }

    #[test]
    fn the_share_gain_leaves_other_sounds_alone() {
        let gain = Arc::new(ShareGain::default());
        gain.set(0.0);
        let (mut slots, _producer, _port) = constant_share(&gain);
        // A notification sound already in the frame keeps its level.
        let mut frame = [0.25; FRAME_SAMPLES];
        assert!(slots.mix_into(&mut frame));
        assert!(frame.iter().all(|s| (s - 0.25).abs() < 1e-6));
    }

    #[test]
    fn more_sources_than_slots_wait_their_turn() {
        let (mut port, mut slots) = share_slots(Arc::default());
        let mut writers = Vec::new();
        for _ in 0..MAX_SHARE_SOURCES {
            let (writer, source, _) = channel();
            assert!(port.add(source).is_ok());
            writers.push(writer);
        }
        slots.update();
        for _ in 0..MAX_SHARE_SOURCES {
            let (writer, source, _) = channel();
            assert!(port.add(source).is_ok(), "queued while the slots are busy");
            writers.push(writer);
        }
        let (_, extra, _) = channel();
        assert!(port.add(extra).is_err(), "the queue is full too");
        writers.remove(0);
        slots.update();
        port.collect();
        assert_eq!(slots.slots.iter().flatten().count(), MAX_SHARE_SOURCES);
        assert_eq!(slots.add.slots(), MAX_SHARE_SOURCES - 1);
    }
}
