//! Camera background effects (spec 08, FR-8.32 … FR-8.41).
//!
//! A person segmenter (MediaPipe Selfie Segmenter run by ONNX Runtime on the CPU) finds the
//! person in each camera frame; the pipeline refines the mask's edges, smooths it over time and
//! blurs or replaces the background, in place on the NV12 frame, before the encoder sees it.
//! Viewers only ever receive the finished picture.
//!
//! [`Effects`] runs all of this on its own thread between the camera and the encoder, with the
//! quality levels Auto / Best / Light and the load monitor of FR-8.35. When ONNX Runtime can't
//! be loaded, effects are off with a reason ([`availability`]) and frames pass unchanged.

#[cfg(test)]
mod artifact_dir;
mod background;
mod guided;
pub mod hooks;
mod level;
mod ops;
mod pipeline;
mod runtime;
mod segmenter;
mod thread;

pub use background::{Background, BackgroundImage, resolve};
pub use level::{Level, LevelController, Load};
pub use pipeline::Pipeline;
pub use runtime::availability;
pub use segmenter::{FixtureSegmenter, ModelShape, OnnxSegmenter, Segmenter, fixture_mask};
pub use thread::{BatteryProbe, EffectStats, Effects, EffectsConfig};

#[derive(Debug, thiserror::Error)]
pub enum EffectsError {
    /// ONNX Runtime is missing or failed to load; the text is the user-facing reason.
    #[error("Background effects are not available on this PC: {0}")]
    Unavailable(String),
    #[error("segmentation failed: {0}")]
    Model(String),
    #[error("the background image can't be used: {0}")]
    Image(String),
    #[error("unknown background image")]
    UnknownBackground,
    #[error("the frame does not match the effect's frame size")]
    FrameSize,
}
