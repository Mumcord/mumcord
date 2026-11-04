//! Screen sharing media (FR-2.42, FR-2.44, FR-7.37, FR-7.59): realtime video encoders and
//! decoders behind one interface ([`VideoEncoder`], [`VideoDecoder`]).
//!
//! - VP9 on libvpx ([`Encoder`], [`Decoder`]): realtime mode with three temporal layers (L1T3,
//!   pattern 0-2-1-2).
//! - AV1: SVT-AV1 encodes ([`Av1Encoder`]) in its low-delay realtime mode with the same three
//!   temporal layers; dav1d decodes ([`Av1Decoder`]) where the webview has no
//!   WebCodecs AV1.
//! - Hardware encoders ([`open_hardware_encoder`]): NVENC, VA-API, AMF and Quick Sync, built
//!   only with the vendor SDK (`HWENC_SDK_DIR`) and usable only where the GPU driver loads.
//!
//! Dropping every frame of the top layer halves the frame rate, dropping the top two quarters
//! it, and the remaining frames still decode. Each [`EncodedFrame`] carries its layer id so the
//! sender can thin out the stream per viewer without re-encoding. Resolution steps are a runtime
//! reconfiguration of the same encoder.
//!
//! Frames are I420 in one tightly packed buffer (Y, then U, then V; chroma planes are half the
//! size, rounded up).

mod av1_dav1d;
mod av1_svt;
mod codec;
mod decoder;
mod encoder;
mod ffi;
mod frame;
mod hw;

pub use av1_dav1d::Av1Decoder;
pub use av1_svt::{AV1_TEMPORAL_LAYERS, Av1Encoder, Av1Options};
pub use codec::{Codec, VideoDecoder, VideoEncoder};
pub use decoder::Decoder;
pub use encoder::{EncodedFrame, Encoder, EncoderConfig, TEMPORAL_LAYERS, threads as vp9_threads};
pub use frame::I420Frame;
pub use hw::{HardwareEncoder, open_hardware_encoder};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{library} {operation} failed: {message}")]
    Codec {
        library: &'static str,
        operation: &'static str,
        message: String,
    },
    #[error("invalid frame: {0}")]
    InvalidFrame(String),
    #[error("invalid encoder configuration: {0}")]
    InvalidConfig(String),
    /// The hardware encoder can't be used here (build, driver or GPU); use a software encoder.
    #[error("{encoder} hardware encoder unavailable: {reason}")]
    HardwareUnavailable {
        encoder: HardwareEncoder,
        reason: String,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
