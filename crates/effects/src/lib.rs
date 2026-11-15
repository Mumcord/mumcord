//! Camera background effects (spec 08, FR-8.32 … FR-8.41): Mumcord's side of the `bgmatte`
//! crate.
//!
//! [`bgmatte`] does the work: it finds the person in each camera frame with the MediaPipe Selfie
//! Segmenter (bundled, run by ONNX Runtime on the CPU), refines the mask's edges, smooths it
//! over time and blurs or replaces the background, in place on the NV12 frame, with the quality
//! levels Auto / Best / Light and the load monitor of FR-8.35. Viewers only ever receive the
//! finished picture.
//!
//! This crate keeps what is specific to Mumcord: the effects thread between the camera and the
//! encoder ([`Effects`]), turning the stored background setting into an effect ([`resolve`]),
//! finding the ONNX Runtime library Mumcord ships ([`availability`]; when it can't be loaded,
//! effects are off with a reason and frames pass unchanged), and the debug-only test hooks.

#[cfg(test)]
mod artifact_dir;
mod background;
pub mod hooks;
mod runtime;
mod segmenter;
mod thread;

pub use background::resolve;
pub use bgmatte::{Background, BackgroundImage, Level, Load, Stats as EffectStats};
pub use runtime::availability;
pub use segmenter::FixtureSegmenter;
pub use thread::{BatteryProbe, Effects, EffectsConfig};

#[derive(Debug, thiserror::Error)]
pub enum EffectsError {
    #[error("the background image can't be used: {0}")]
    Image(String),
    #[error("unknown background image")]
    UnknownBackground,
}
