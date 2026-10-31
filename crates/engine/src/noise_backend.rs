//! The `rnnoise-rs` noise suppression backend.
//!
//! `mumcord-codec` is published on crates.io, and a published crate cannot carry a git
//! dependency, so the implementation of `Backend::Rnnoise` lives here and the session registers
//! it on its controls. Transitional: `rnnoise-rs` should get a crate of its own (issue #176).

use mumcord_audio::dsp::{Backend, DspControls, Outcome, SuppressionBackend, SuppressionLevel};
use mumcord_audio::{FRAME_SAMPLES, Frame};

use rnnoise::DenoiseState;

/// Sample scale of the model: it works on values in the 16-bit integer range.
const I16_SCALE: f32 = 32_768.0;

/// The current RNNoise model. It keeps a 50 % overlap window, so the frame it returns is the one it
/// read on the previous call — 10 ms of algorithmic delay.
pub struct RnnoiseBackend {
    state: DenoiseState,
    scaled: Frame,
}

impl Default for RnnoiseBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl RnnoiseBackend {
    pub fn new() -> Self {
        Self {
            state: DenoiseState::new(),
            scaled: [0.0; FRAME_SAMPLES],
        }
    }
}

impl SuppressionBackend for RnnoiseBackend {
    fn kind(&self) -> Backend {
        Backend::Rnnoise
    }

    fn process(&mut self, input: &Frame, output: &mut Frame) -> Outcome {
        for (target, &sample) in self.scaled.iter_mut().zip(input.iter()) {
            *target = sample * I16_SCALE;
        }
        let probability = self.state.process_frame(output, &self.scaled);
        for sample in output.iter_mut() {
            *sample /= I16_SCALE;
        }
        Outcome {
            probability: Some(probability),
            failed: false,
        }
    }
}

// The model state is internal, so the backend reports only what it is.
impl std::fmt::Debug for RnnoiseBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("RnnoiseBackend")
    }
}

/// The builder the session registers. The level belongs to the interface; `rnnoise-rs` has no
/// runtime option, so nothing here uses it.
fn build(_level: SuppressionLevel) -> Box<dyn SuppressionBackend> {
    Box::new(RnnoiseBackend::new())
}

/// Lets the chain use `rnnoise-rs`, which `mumcord-codec` cannot carry itself.
pub fn register(controls: &DspControls) {
    controls.register_backend(Backend::Rnnoise, build);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mean_energy(x: &[f32]) -> f32 {
        x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32
    }

    fn db(ratio: f32) -> f32 {
        10.0 * ratio.log10()
    }

    /// Deterministic fan-like noise: low-passed white noise.
    fn fan(len: usize, amplitude: f32) -> Vec<f32> {
        let mut rng = 7u32;
        let mut low = 0.0f32;
        (0..len)
            .map(|_| {
                rng = rng.wrapping_mul(1_664_525).wrapping_add(1);
                low += 0.2 * (((rng as f32 / u32::MAX as f32) * 2.0 - 1.0) - low);
                amplitude * low
            })
            .collect()
    }

    /// A harmonic-rich tone at a typical male pitch, so a model that turns the voice down shows up.
    fn voice(len: usize, amplitude: f32) -> Vec<f32> {
        (0..len)
            .map(|i| {
                (1..12)
                    .map(|h| {
                        (2.0 * std::f32::consts::PI * 140.0 * h as f32 * i as f32 / 48000.0).sin()
                            / h as f32
                    })
                    .sum::<f32>()
                    * amplitude
            })
            .collect()
    }

    /// Runs `input` through the backend and returns the energy of the last processed frame.
    fn process(backend: &mut dyn SuppressionBackend, input: &[f32]) -> f32 {
        let mut out = [0.0; FRAME_SAMPLES];
        for frame in input.as_chunks::<FRAME_SAMPLES>().0 {
            backend.process(frame, &mut out);
        }
        mean_energy(&out)
    }

    #[test]
    fn rnnoise_reduces_noise_and_keeps_speech() {
        let mut backend = build(SuppressionLevel::High);
        let noise = fan(FRAME_SAMPLES * 10, 0.1);
        let reduction = db(mean_energy(&noise) / process(backend.as_mut(), &noise));
        let speech = voice(FRAME_SAMPLES * 10, 0.3);
        let speech_loss = db(mean_energy(&speech) / process(backend.as_mut(), &speech));
        println!("rnnoise-rs: noise {reduction:.1} dB down, speech {speech_loss:.1} dB down");
        assert!(
            reduction >= 10.0,
            "rnnoise-rs only reduced {reduction:.1} dB"
        );
        assert!(
            speech_loss <= 6.0,
            "rnnoise-rs took {speech_loss:.1} dB of speech"
        );
    }

    #[test]
    fn the_session_can_register_it() {
        let controls = DspControls::default();
        register(&controls);
        assert_eq!(
            controls
                .build(Backend::Rnnoise, SuppressionLevel::High)
                .kind(),
            Backend::Rnnoise
        );
    }
}
