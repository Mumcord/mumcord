//! Sample-rate conversion for devices that do not run at 48 kHz. Runs on the
//! non-real-time side of the rings only.

use std::collections::VecDeque;

use rubato::{FftFixedIn, Resampler as _};

use super::IoError;

/// Streaming mono resampler: push any number of samples, pop converted ones.
pub(crate) struct Resampler {
    inner: FftFixedIn<f32>,
    input: Vec<f32>,
    output_buf: Vec<Vec<f32>>,
    output: VecDeque<f32>,
}

impl Resampler {
    pub(crate) fn new(from_rate: u32, to_rate: u32) -> Result<Self, IoError> {
        // 10 ms input chunks keep the added delay at one frame.
        let chunk = (from_rate / 100).max(1) as usize;
        let inner = FftFixedIn::new(from_rate as usize, to_rate as usize, chunk, 1, 1)
            .map_err(|e| IoError::Resampler(e.to_string()))?;
        let output_buf = inner.output_buffer_allocate(true);
        Ok(Self {
            input: Vec::with_capacity(chunk * 4),
            output: VecDeque::with_capacity(inner.output_frames_max() * 4),
            inner,
            output_buf,
        })
    }

    pub(crate) fn push(&mut self, samples: &[f32]) {
        self.input.extend_from_slice(samples);
        let mut consumed = 0;
        while self.input.len() - consumed >= self.inner.input_frames_next() {
            let needed = self.inner.input_frames_next();
            let chunk = [&self.input[consumed..consumed + needed]];
            match self
                .inner
                .process_into_buffer(&chunk, &mut self.output_buf, None)
            {
                Ok((_, produced)) => self.output.extend(&self.output_buf[0][..produced]),
                // Only possible with mis-sized buffers, which `new` rules out.
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
}
