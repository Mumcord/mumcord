//! Slow loudness leveler for "Music quality" (QA.37).
//!
//! Music skips the speech gain control because its fast, speech-tuned adaptation pumps and
//! flattens music. Without any gain stage, though, a music sender reached listeners far quieter
//! than speech senders (F14). This stage brings music to the same average level as the speech
//! gain control produces, but only by a slowly moving overall gain: dynamics within a song stay
//! intact and there is no compression. A peak guard pulls the gain down at once when a boosted
//! peak would come close to full scale, so the level lift never clips.

use crate::{FRAME_SAMPLES, Frame};

/// Average level (mean square, dBFS) the speech gain control produces for normal speech; music
/// is brought to the same level so both sound equally loud to listeners.
const TARGET_DBFS: f32 = -33.0;
/// Frames quieter than this (dBFS) are pauses or noise: they neither raise the level estimate
/// nor get lifted on their own.
const SILENCE_DBFS: f32 = -55.0;
/// The leveler only lifts, like the speech gain control; loud sources pass unchanged.
const MAX_GAIN_DB: f32 = 30.0;
/// Highest peak a lifted frame may reach (≈ -1 dBFS).
const PEAK_CEILING: f32 = 0.89;
/// Time constant of the level estimate in frames (3 s); long enough to follow songs, not notes.
const LEVEL_FRAMES: f32 = 300.0;
/// How fast the gain may rise per frame (6 dB/s): slow enough that recovering from a peak
/// never sounds like pumping, fast enough to reach the target within a few seconds.
const RISE_DB_PER_FRAME: f32 = 0.06;

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub(super) struct Leveler {
    /// Smoothed level of non-silent frames (dBFS); `None` until the first one.
    level_dbfs: Option<f32>,
    /// Gain applied at the end of the previous frame (linear).
    gain: f32,
    rise: f32,
    max_gain: f32,
}

impl Default for Leveler {
    fn default() -> Self {
        Self {
            level_dbfs: None,
            gain: 1.0,
            rise: db_to_gain(RISE_DB_PER_FRAME),
            max_gain: db_to_gain(MAX_GAIN_DB),
        }
    }
}

impl Leveler {
    /// Applies the leveling gain to one frame in place.
    pub(super) fn process(&mut self, frame: &mut Frame) {
        let mut peak = 0.0f32;
        let mut sum_sq = 0.0f32;
        for &sample in frame.iter() {
            peak = peak.max(sample.abs());
            sum_sq += sample * sample;
        }
        let frame_dbfs = 10.0 * (sum_sq / FRAME_SAMPLES as f32).max(1e-12).log10();
        if frame_dbfs > SILENCE_DBFS {
            // Averaging in dB keeps loud notes from dominating the estimate.
            let level = self.level_dbfs.get_or_insert(frame_dbfs);
            *level += (frame_dbfs - *level) / LEVEL_FRAMES;
        }

        let wanted = self.level_dbfs.map_or(1.0, |level| {
            db_to_gain(TARGET_DBFS - level).clamp(1.0, self.max_gain)
        });
        let mut next = wanted.min(self.gain * self.rise);
        if peak > 0.0 {
            next = next.min((PEAK_CEILING / peak).max(1.0));
        }
        if next < self.gain {
            // A peak needs less gain: apply it to the whole frame at once, as there is no
            // look-ahead and a ramp would let the peak through.
            self.gain = next;
            for sample in frame.iter_mut() {
                *sample *= next;
            }
            return;
        }
        // Ramp within the frame so gain changes never step audibly.
        let step = (next - self.gain) / FRAME_SAMPLES as f32;
        for (i, sample) in frame.iter_mut().enumerate() {
            *sample *= self.gain + step * (i + 1) as f32;
        }
        self.gain = next;
    }
}
