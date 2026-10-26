//! Screen sharing media (FR-2.42, FR-2.44): VP9 software encoder and decoder on libvpx.
//!
//! The encoder runs in libvpx realtime mode with three temporal layers (L1T3, pattern 0-2-1-2):
//! dropping every layer-2 frame halves the frame rate, dropping layers 1 and 2 quarters it, and
//! the remaining frames still decode. Each [`EncodedFrame`] carries its layer id so the sender can
//! thin out the stream per viewer without re-encoding. Resolution steps (720p ↔ 480p) are a
//! runtime reconfiguration of the same encoder.
//!
//! Frames are I420 in one tightly packed buffer (Y, then U, then V; chroma planes are half the
//! size, rounded up).

mod decoder;
mod encoder;
mod ffi;
mod frame;

pub use decoder::Decoder;
pub use encoder::{EncodedFrame, Encoder, EncoderConfig, TEMPORAL_LAYERS};
pub use frame::I420Frame;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("libvpx {operation} failed: {message}")]
    Codec {
        operation: &'static str,
        message: String,
    },
    #[error("invalid frame: {0}")]
    InvalidFrame(String),
    #[error("invalid encoder configuration: {0}")]
    InvalidConfig(String),
}

pub type Result<T> = std::result::Result<T, Error>;
