//! Opus encoding and decoding on top of the [`Frame`](crate::Frame) contract.
//!
//! Outgoing packets default to 20 ms ([`PacketDuration::Ms20`], two frames):
//! half the per-packet header overhead of 10 ms, better resilience under
//! loss, and still well inside the 150 ms mouth-to-ear budget. 10 ms remains
//! selectable. The decoder accepts any packet length other clients send
//! (2.5–120 ms).

mod decoder;
mod encoder;

pub use decoder::{MAX_DECODED_SAMPLES, OpusDecoder};
pub use encoder::{MAX_PACKET_BYTES, MAX_VOICE_PAYLOAD_BYTES, OpusEncoder, PacketDuration};

/// Errors from the Opus codec.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("opus: {0}")]
    Opus(#[from] opus::Error),
    #[error("output buffer too small: need {needed} samples, got {got}")]
    BufferTooSmall { needed: usize, got: usize },
}
