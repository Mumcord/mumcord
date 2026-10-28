//! Sample-rate conversion for devices that do not run at 48 kHz. Runs on the
//! non-real-time side of the rings only.

use std::collections::VecDeque;

use rubato::audioadapter_buffers::direct::SequentialSliceOfSlices;
use rubato::{Fft, FixedSync, Resampler as _};

use super::IoError;

/// Single-channel views of audio, so rubato reads and writes in place without copying.
type MonoIn<'a> = SequentialSliceOfSlices<&'a [&'a [f32]]>;
type MonoOut<'a> = SequentialSliceOfSlices<&'a mut [&'a mut [f32]]>;

/// Streaming mono resampler: push any number of samples, pop converted ones.
pub(crate) struct Resampler {
    inner: Fft<f32>,
    input: Vec<f32>,
    /// Worst-case output buffer for one chunk, reused every call: the real-time
    /// path must not allocate.
    output_buf: Vec<f32>,
    output: VecDeque<f32>,
}

impl Resampler {
    pub(crate) fn new(from_rate: u32, to_rate: u32) -> Result<Self, IoError> {
        // 10 ms input chunks. rubato picks FFT sub-chunks near 256 frames, so the added
        // delay is half the internal FFT block, not one chunk (test: latency budget).
        let chunk = (from_rate / 100).max(1) as usize;
        let inner = Fft::new(
            from_rate as usize,
            to_rate as usize,
            chunk,
            1,
            FixedSync::Input,
        )
        .map_err(|e| IoError::Resampler(e.to_string()))?;
        let max_output = inner.output_frames_max();
        Ok(Self {
            input: Vec::with_capacity(chunk * 4),
            output: VecDeque::with_capacity(max_output * 4),
            output_buf: vec![0.0; max_output],
            inner,
        })
    }

    pub(crate) fn push(&mut self, samples: &[f32]) {
        self.input.extend_from_slice(samples);
        let mut consumed = 0;
        while self.input.len() - consumed >= self.inner.input_frames_next() {
            let needed = self.inner.input_frames_next();
            let in_channels: [&[f32]; 1] = [&self.input[consumed..consumed + needed]];
            let out_frames = self.output_buf.len();
            let mut out_channels: [&mut [f32]; 1] = [&mut self.output_buf];
            // Both buffers are sized to what the resampler asks for, so a size error here
            // would mean a bug in this module, not bad input.
            let input = match MonoIn::new(&in_channels, 1, needed) {
                Ok(buffer) => buffer,
                Err(e) => {
                    tracing::error!("resampler input buffer too small: {e}");
                    break;
                }
            };
            let mut output = match MonoOut::new_mut(&mut out_channels, 1, out_frames) {
                Ok(buffer) => buffer,
                Err(e) => {
                    tracing::error!("resampler output buffer too small: {e}");
                    break;
                }
            };
            match self.inner.process_into_buffer(&input, &mut output, None) {
                Ok((_, produced)) => self.output.extend(&self.output_buf[..produced]),
                Err(e) => tracing::error!("resampler failed: {e}"),
            }
            consumed += needed;
        }
        self.input.drain(..consumed);
    }

    /// Fills `out` completely if enough converted samples are available.
    pub(crate) fn pop_exact(&mut self, out: &mut [f32]) -> bool {
        if self.output.len() < out.len() {
            return false;
        }
        let n = out.len();
        for (dst, src) in out.iter_mut().zip(self.output.drain(..n)) {
            *dst = src;
        }
        true
    }

    /// Removes up to `max` converted samples.
    pub(crate) fn drain(&mut self, max: usize) -> std::collections::vec_deque::Drain<'_, f32> {
        let n = max.min(self.output.len());
        self.output.drain(..n)
    }

    pub(crate) fn pending(&self) -> usize {
        self.output.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, freq: f32, samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    /// Frequency estimate from zero crossings.
    fn frequency(samples: &[f32], rate: u32) -> f32 {
        let crossings = samples
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        crossings as f32 * rate as f32 / samples.len() as f32
    }

    #[test]
    fn converts_44_1_khz_to_48_khz_and_back_keeping_pitch() {
        let input = sine(44_100, 1000.0, 44_100);
        let mut up = Resampler::new(44_100, 48_000).expect("resampler");
        for block in input.chunks(441) {
            up.push(block);
        }
        let mut upsampled = vec![0.0; up.pending()];
        assert!(up.pop_exact(&mut upsampled));
        // One second in, one second out, minus the resampler's start-up delay.
        assert!(
            (47_000..=48_000).contains(&upsampled.len()),
            "{}",
            upsampled.len()
        );
        let f = frequency(&upsampled[4800..], 48_000);
        assert!((f - 1000.0).abs() < 5.0, "pitch {f}");

        let mut down = Resampler::new(48_000, 44_100).expect("resampler");
        for block in upsampled.chunks(480) {
            down.push(block);
        }
        let back: Vec<f32> = down.drain(usize::MAX).collect();
        let f = frequency(&back[4410..], 44_100);
        assert!((f - 1000.0).abs() < 5.0, "pitch {f}");
    }

    #[test]
    fn pop_exact_waits_for_a_full_block() {
        let mut r = Resampler::new(44_100, 48_000).expect("resampler");
        r.push(&[0.0; 200]);
        let mut frame = [0.0; 480];
        assert!(!r.pop_exact(&mut frame));
        r.push(&[0.0; 1000]);
        assert!(r.pop_exact(&mut frame));
    }

    /// The resampler must stay inside the latency budget of the foundation spec
    /// (specs/01-foundation-specification.md:338, audio processing adds at most 30 ms).
    #[test]
    fn added_delay_stays_within_the_latency_budget() {
        for (from, to) in [(44_100, 48_000), (48_000, 44_100), (16_000, 48_000)] {
            let r = Resampler::new(from, to).expect("resampler");
            let delay_ms = r.inner.output_delay() as f64 * 1000.0 / to as f64;
            assert!(delay_ms <= 30.0, "{from} -> {to}: {delay_ms:.1} ms");
        }
    }
}
