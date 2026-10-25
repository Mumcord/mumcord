//! Lock-free sample rings between real-time callbacks and the rest of the
//! crate, plus the allocation-free callback bodies that fill and drain them.

use std::sync::atomic::{AtomicU64, Ordering};

use cpal::{FromSample, Sample};
use rtrb::{Consumer, Producer, RingBuffer};

use super::IoStats;

/// Ring capacity in milliseconds of device-rate audio. The reader drains
/// continuously, so this is headroom for scheduling hiccups, not latency.
const RING_MS: usize = 100;

/// Mono capture ring.
pub(crate) fn ring(device_rate: u32) -> (Producer<f32>, Consumer<f32>) {
    RingBuffer::new(device_rate as usize * RING_MS / 1000)
}

/// Playback ring of interleaved stereo samples (left, right).
pub(crate) fn stereo_ring(device_rate: u32) -> (Producer<f32>, Consumer<f32>) {
    RingBuffer::new(2 * device_rate as usize * RING_MS / 1000)
}

/// Shared counters, updated with relaxed atomics from real-time callbacks.
#[derive(Debug, Default)]
pub(crate) struct Counters {
    capture_overruns: AtomicU64,
    playback_underruns: AtomicU64,
    playback_frames_dropped: AtomicU64,
}

impl Counters {
    pub(crate) fn capture_overrun(&self) {
        self.capture_overruns.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn playback_underrun(&self) {
        self.playback_underruns.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn playback_frame_dropped(&self) {
        self.playback_frames_dropped.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> IoStats {
        IoStats {
            capture_overruns: self.capture_overruns.load(Ordering::Relaxed),
            playback_underruns: self.playback_underruns.load(Ordering::Relaxed),
            playback_frames_dropped: self.playback_frames_dropped.load(Ordering::Relaxed),
        }
    }
}

/// Real-time capture body: downmixes interleaved device samples to mono and
/// pushes them. Samples that do not fit are dropped and counted.
pub(crate) fn capture<T>(
    data: &[T],
    channels: usize,
    producer: &mut Producer<f32>,
    counters: &Counters,
) where
    T: Sample,
    f32: FromSample<T>,
{
    let frames = data.len() / channels;
    let writable = frames.min(producer.slots());
    if writable < frames {
        counters.capture_overrun();
    }
    if let Ok(chunk) = producer.write_chunk_uninit(writable) {
        let scale = 1.0 / channels as f32;
        chunk.fill_from_iter(
            data.chunks_exact(channels)
                .map(|frame| frame.iter().map(|&s| f32::from_sample(s)).sum::<f32>() * scale),
        );
    }
}

/// Real-time playback body: pops interleaved stereo pairs onto the device
/// channels (mono devices get the mix of both, channels beyond the first two
/// get it too, so non-positional audio sounds as before on every layout) and
/// fills the rest with silence. Only a buffer that runs dry part-way counts as
/// an underrun; an empty queue is the normal "nobody talks" state.
pub(crate) fn playback<T>(
    data: &mut [T],
    channels: usize,
    consumer: &mut Consumer<f32>,
    counters: &Counters,
) where
    T: Sample + FromSample<f32>,
{
    let frames = data.len() / channels;
    let available = consumer.slots() / 2;
    let readable = frames.min(available);
    if available > 0 && readable < frames {
        counters.playback_underrun();
    }
    let mut out = data.chunks_exact_mut(channels);
    if let Ok(chunk) = consumer.read_chunk(readable * 2) {
        let (a, b) = chunk.as_slices();
        let mut samples = a.iter().chain(b);
        for _ in 0..readable {
            let (Some(&left), Some(&right), Some(frame)) =
                (samples.next(), samples.next(), out.next())
            else {
                break;
            };
            let mid = T::from_sample((left + right) * 0.5);
            match frame {
                [only] => *only = mid,
                [l, r, rest @ ..] => {
                    *l = T::from_sample(left);
                    *r = T::from_sample(right);
                    rest.fill(mid);
                }
                [] => {}
            }
        }
        chunk.commit_all();
    }
    for frame in out {
        frame.fill(T::EQUILIBRIUM);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_downmixes_and_counts_overruns() {
        let (mut producer, mut consumer) = RingBuffer::new(3);
        let counters = Counters::default();
        let stereo: [i16; 8] = [i16::MAX, 0, 0, 0, i16::MIN, i16::MIN, 100, 100];
        capture(&stereo, 2, &mut producer, &counters);
        assert_eq!(consumer.slots(), 3);
        let first = consumer.pop().expect("sample");
        assert!((first - 0.5).abs() < 1e-3);
        assert_eq!(consumer.pop().expect("sample"), 0.0);
        assert_eq!(consumer.pop().expect("sample"), -1.0);
        assert_eq!(counters.snapshot().capture_overruns, 1);
    }

    #[test]
    fn playback_maps_stereo_and_pads_with_silence() {
        let (mut producer, mut consumer) = RingBuffer::new(16);
        let counters = Counters::default();
        producer.push(0.5).expect("push");
        producer.push(0.25).expect("push");
        let mut out = [1.0f32; 6];
        playback(&mut out, 2, &mut consumer, &counters);
        assert_eq!(out, [0.5, 0.25, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(counters.snapshot().playback_underruns, 1);

        playback(&mut out, 2, &mut consumer, &counters);
        assert_eq!(out, [0.0; 6]);
        assert_eq!(
            counters.snapshot().playback_underruns,
            1,
            "empty queue is not an underrun"
        );

        for s in [0.5, 0.25, -0.5, 0.0] {
            producer.push(s).expect("push");
        }
        let mut mono = [1.0f32; 2];
        playback(&mut mono, 1, &mut consumer, &counters);
        assert_eq!(mono, [0.375, -0.25], "mono devices get the mix");

        producer.push(0.5).expect("push");
        producer.push(0.25).expect("push");
        let mut surround = [1.0f32; 4];
        playback(&mut surround, 4, &mut consumer, &counters);
        assert_eq!(surround, [0.5, 0.25, 0.375, 0.375]);
    }

    #[test]
    fn ring_preserves_order_across_wraparound() {
        let (mut producer, mut consumer) = ring(48_000);
        let counters = Counters::default();
        let mut next = 0.0f32;
        let mut expected = 0.0f32;
        for _ in 0..50 {
            let block: Vec<f32> = (0..1000)
                .map(|_| {
                    next += 1.0;
                    next
                })
                .collect();
            // Capture writes mono samples; read back as pairs on a stereo device.
            capture(&block, 1, &mut producer, &counters);
            let mut out = vec![0.0f32; 1000];
            playback(&mut out, 2, &mut consumer, &counters);
            for s in out {
                expected += 1.0;
                assert_eq!(s, expected);
            }
        }
        assert_eq!(counters.snapshot(), IoStats::default());
    }
}
