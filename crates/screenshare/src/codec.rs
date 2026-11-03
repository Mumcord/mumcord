//! The codec-independent interface the screen share engine drives: one encoder and one decoder
//! trait, implemented by VP9 (libvpx) and AV1 (SVT-AV1 encode, dav1d decode), and later by the
//! hardware encoders.

use crate::{EncodedFrame, EncoderConfig, I420Frame, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Codec {
    Vp9,
    Av1,
}

/// A realtime encoder with temporal layers. Every call is synchronous: a frame passed to
/// [`Self::encode`] comes back compressed from the same call.
pub trait VideoEncoder: Send {
    fn codec(&self) -> Codec;

    fn config(&self) -> EncoderConfig;

    /// Number of temporal layers the current stream uses; layer ids run `0..temporal_layers()`.
    /// Dropping every frame above some layer leaves a stream that still decodes.
    fn temporal_layers(&self) -> u8;

    /// Encodes one frame of the configured size. `pts` is in milliseconds and must increase.
    /// With `force_keyframe` the frame becomes a keyframe (and thus layer 0).
    fn encode(
        &mut self,
        frame: &I420Frame,
        pts: i64,
        force_keyframe: bool,
    ) -> Result<Vec<EncodedFrame>>;

    /// Changes the output size; frames passed to [`Self::encode`] afterwards must have it.
    fn set_resolution(&mut self, width: u32, height: u32) -> Result<()>;

    /// Changes the total bitrate of all layers.
    fn set_bitrate(&mut self, bitrate_kbps: u32) -> Result<()>;

    /// Changes the frame rate the rate control plans with.
    fn set_fps(&mut self, fps: u32) -> Result<()>;
}

/// Decodes one compressed frame at a time into I420.
pub trait VideoDecoder: Send {
    fn codec(&self) -> Codec;

    /// Decodes one compressed frame. Returns `None` when the decoder has no picture to show
    /// for it.
    fn decode(&mut self, data: &[u8]) -> Result<Option<I420Frame>>;
}
