//! Noise suppression backends for Settings → Voice.
//!
//! Two local implementations, both offline:
//!
//! - [`Backend::Nnnoiseless`] — `nnnoiseless`, the RNNoise model in Rust. Cheap, the default.
//! - [`Backend::Sonora`] — `sonora-ns`, the classical WebRTC noise suppressor (quantile noise
//!   estimation + Wiener filter), no neural network.
//!
//! [`Backend::Rnnoise`] (the current RNNoise architecture, convolution + GRU network) is not
//! part of this crate: `rnnoise-rs` is a git source, and a published crate cannot carry one, so
//! the engine supplies the implementation through [`BackendBuilder`] (issue #176). A consumer
//! that selects `Backend::Rnnoise` without a registered builder gets nnnoiseless.
//!
//! Only the selected backend processes the microphone. Every frame is 48 kHz mono, 10 ms
//! ([`FRAME_SAMPLES`] samples); the neural backends expect samples in the 16-bit integer range,
//! so each scales on the way in and back.
//!
//! # User settings
//!
//! `nnnoiseless` and `rnnoise-rs` expose no runtime parameters (`process_frame` only), so the UI
//! shows nothing for them. `sonora-ns` has the aggressiveness level ([`SuppressionLevel`]), the
//! only real option. A builder receives it even when the implementation has no use for it.
//!
//! # Speech detection
//!
//! `nnnoiseless` and `rnnoise-rs` report a speech probability with every frame; the voice
//! activation gate uses it. `sonora-ns` keeps its probability private, so the chain runs the
//! `nnnoiseless` model as a detector only for that backend — it reads the frame, it does not
//! change the audio. See the module comment in [`super`].
//!
//! # Real-time behaviour
//!
//! Backends are created by the settings thread, never in the frame path, and the processing thread
//! adopts the result through the pending slot in [`super::DspControls`]. After creation, `process`
//! allocates nothing for the neural backends; `sonora` keeps small internal temporaries (known
//! library behaviour, measured in the tests).

use nnnoiseless::DenoiseState;
use sonora::config::{NoiseSuppression, NoiseSuppressionLevel};
use sonora::{AudioProcessing, Config, StreamConfig};

use crate::{FRAME_SAMPLES, Frame};

/// Sample scale of the neural backends: they work on values in the 16-bit integer range.
const I16_SCALE: f32 = 32_768.0;

/// One of the three selectable backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backend {
    /// `nnnoiseless` (RNNoise): low CPU, good for steady background noise.
    #[default]
    Nnnoiseless,
    /// `rnnoise-rs` (current RNNoise model): newer network, better suppression.
    Rnnoise,
    /// `sonora-ns`: classical DSP suppressor, no neural network.
    Sonora,
}

/// Aggressiveness of `sonora-ns`. The neural backends have no runtime option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SuppressionLevel {
    /// About 6 dB of reduction.
    Low,
    /// About 12 dB (default).
    #[default]
    Moderate,
    /// About 18 dB.
    High,
    /// About 21 dB.
    VeryHigh,
}

impl Backend {
    /// Value stored in the atomic control; must stay stable across versions.
    pub const fn code(&self) -> u32 {
        match self {
            Backend::Nnnoiseless => 0,
            Backend::Rnnoise => 1,
            Backend::Sonora => 2,
        }
    }

    pub const fn from_code(code: u32) -> Self {
        match code {
            1 => Backend::Rnnoise,
            2 => Backend::Sonora,
            _ => Backend::Nnnoiseless,
        }
    }

    /// Whether this backend has a setting the user can change.
    pub const fn supports_level(&self) -> bool {
        matches!(self, Backend::Sonora)
    }

    /// Whether this backend reports a speech probability for voice activation.
    pub const fn reports_speech_probability(&self) -> bool {
        !matches!(self, Backend::Sonora)
    }
}

impl SuppressionLevel {
    pub const fn code(&self) -> u32 {
        match self {
            SuppressionLevel::Low => 0,
            SuppressionLevel::Moderate => 1,
            SuppressionLevel::High => 2,
            SuppressionLevel::VeryHigh => 3,
        }
    }

    pub const fn from_code(code: u32) -> Self {
        match code {
            0 => SuppressionLevel::Low,
            2 => SuppressionLevel::High,
            3 => SuppressionLevel::VeryHigh,
            _ => SuppressionLevel::Moderate,
        }
    }

    const fn to_sonora(self) -> NoiseSuppressionLevel {
        match self {
            SuppressionLevel::Low => NoiseSuppressionLevel::Low,
            SuppressionLevel::Moderate => NoiseSuppressionLevel::Moderate,
            SuppressionLevel::High => NoiseSuppressionLevel::High,
            SuppressionLevel::VeryHigh => NoiseSuppressionLevel::VeryHigh,
        }
    }
}

/// What one processed frame returns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outcome {
    /// Speech probability for the voice activation gate, when the backend has one.
    pub probability: Option<f32>,
    /// The backend rejected the frame; the audio passed through unchanged.
    pub failed: bool,
}

/// One frame of the noise suppression pipeline. Implementations allocate nothing in `process`.
/// `Send` because the settings thread builds a backend and hands it to the processing thread.
pub trait SuppressionBackend: std::fmt::Debug + Send {
    fn kind(&self) -> Backend;

    /// Suppresses `input` into `output`. `input` and `output` are `[-1.0, 1.0]`.
    fn process(&mut self, input: &Frame, output: &mut Frame) -> Outcome;
}

/// An implementation of a backend this crate cannot carry, called by the settings thread.
pub type BackendBuilder = fn(SuppressionLevel) -> Box<dyn SuppressionBackend>;

/// Builds one of the backends `mumcord-codec` carries itself. Called by the settings thread, not by
/// the audio path: the neural model is small, `sonora` allocates its buffers here.
pub fn build_backend(kind: Backend, level: SuppressionLevel) -> Box<dyn SuppressionBackend> {
    match kind {
        // Without a registered builder, rnnoise falls back to the backend this crate has.
        Backend::Nnnoiseless | Backend::Rnnoise => Box::new(NnnoiselessBackend::new()),
        Backend::Sonora => Box::new(SonoraBackend::new(level)),
    }
}

/// nnnoiseless: RNNoise in Rust, 480-sample frames, no runtime options.
pub struct NnnoiselessBackend {
    state: Box<DenoiseState<'static>>,
    scaled: Frame,
}

impl NnnoiselessBackend {
    pub fn new() -> Self {
        Self {
            state: DenoiseState::new(),
            scaled: [0.0; FRAME_SAMPLES],
        }
    }
}

impl SuppressionBackend for NnnoiselessBackend {
    fn kind(&self) -> Backend {
        Backend::Nnnoiseless
    }

    fn process(&mut self, input: &Frame, output: &mut Frame) -> Outcome {
        scale(input, &mut self.scaled);
        let probability = self.state.process_frame(output, &self.scaled);
        unscale(output);
        Outcome {
            probability: Some(probability),
            failed: false,
        }
    }
}

/// sonora-ns: WebRTC's noise suppressor. A single-purpose processing instance, so echo
/// cancellation and gain control stay switched off here.
pub struct SonoraBackend {
    level: SuppressionLevel,
    processing: AudioProcessing,
    scratch: Frame,
}

impl SonoraBackend {
    pub fn new(level: SuppressionLevel) -> Self {
        let stream = StreamConfig::new(crate::SAMPLE_RATE, 1);
        let processing = AudioProcessing::builder()
            .config(Config {
                noise_suppression: Some(NoiseSuppression {
                    level: level.to_sonora(),
                    ..NoiseSuppression::default()
                }),
                ..Config::default()
            })
            .capture_config(stream)
            .render_config(stream)
            .build();
        Self {
            level,
            processing,
            scratch: [0.0; FRAME_SAMPLES],
        }
    }
}

impl SuppressionBackend for SonoraBackend {
    fn kind(&self) -> Backend {
        Backend::Sonora
    }

    fn process(&mut self, input: &Frame, output: &mut Frame) -> Outcome {
        self.scratch = *input;
        if self
            .processing
            .process_capture_f32(&[&self.scratch], &mut [output])
            .is_err()
        {
            return Outcome {
                probability: None,
                failed: true,
            };
        }
        // The suppressor keeps its speech probability private; the chain uses the detector.
        Outcome {
            probability: None,
            failed: false,
        }
    }
}

// The state of the two model libraries is internal, so the backends report only what they are.
impl std::fmt::Debug for NnnoiselessBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("NnnoiselessBackend")
    }
}

impl std::fmt::Debug for SonoraBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("SonoraBackend")
            .field("level", &self.level)
            .finish_non_exhaustive()
    }
}

fn scale(input: &Frame, scaled: &mut Frame) {
    for (target, &sample) in scaled.iter_mut().zip(input.iter()) {
        *target = sample * I16_SCALE;
    }
}

fn unscale(frame: &mut Frame) {
    for sample in frame.iter_mut() {
        *sample /= I16_SCALE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for kind in [Backend::Nnnoiseless, Backend::Rnnoise, Backend::Sonora] {
            assert_eq!(Backend::from_code(kind.code()), kind);
        }
        for level in [
            SuppressionLevel::Low,
            SuppressionLevel::Moderate,
            SuppressionLevel::High,
            SuppressionLevel::VeryHigh,
        ] {
            assert_eq!(SuppressionLevel::from_code(level.code()), level);
        }
    }

    #[test]
    fn default_backend_is_nnnoiseless_and_default_level_moderate() {
        assert_eq!(Backend::default(), Backend::Nnnoiseless);
        assert_eq!(SuppressionLevel::default(), SuppressionLevel::Moderate);
    }

    /// What `mumcord-codec` cannot carry is what the caller registered; without one the backend
    /// falls back to the implementation this crate has.
    #[test]
    fn a_registered_builder_builds_the_backend() {
        assert_eq!(
            build_backend(Backend::Rnnoise, SuppressionLevel::High).kind(),
            Backend::Nnnoiseless
        );
    }

    #[test]
    fn only_sonora_has_a_user_setting() {
        assert!(!Backend::Nnnoiseless.supports_level());
        assert!(!Backend::Rnnoise.supports_level());
        assert!(Backend::Sonora.supports_level());
    }

    #[test]
    fn every_backend_processes_a_frame() {
        // Deterministic fan-like noise: low-passed white noise.
        let mut rng = 7u32;
        let mut low = 0.0f32;
        let input: Frame = (0..FRAME_SAMPLES)
            .map(|_| {
                rng = rng.wrapping_mul(1_664_525).wrapping_add(1);
                low += 0.2 * (((rng as f32 / u32::MAX as f32) * 2.0 - 1.0) - low);
                0.1 * low
            })
            .collect::<Vec<f32>>()
            .try_into()
            .expect("480 samples");
        for kind in [Backend::Nnnoiseless, Backend::Sonora] {
            let mut backend = build_backend(kind, SuppressionLevel::High);
            let mut out = [0.0; FRAME_SAMPLES];
            // Warm up (the neural models need a first frame, sonora a few analysis frames).
            for _ in 0..20 {
                let outcome = backend.process(&input, &mut out);
                assert!(!outcome.failed, "{kind:?} rejected the frame");
            }
            let energy_in: f32 = input.iter().map(|s| s * s).sum();
            let energy_out: f32 = out.iter().map(|s| s * s).sum();
            assert!(energy_out < energy_in, "{kind:?} did not reduce the noise");
        }
    }
}
