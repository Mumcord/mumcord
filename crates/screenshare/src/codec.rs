//! The codec-independent interface the screen share engine drives: one encoder and one decoder
//! trait, implemented by VP9 (libvpx) and AV1 (SVT-AV1 encode, dav1d decode), and later by the
//! hardware encoders. The types and constants here need no codec library, so they exist in
//! every build, whichever codec features are switched on.

use crate::{I420Frame, Result};

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

/// Number of VP9 temporal layers (L1T3). Layer ids run 0..TEMPORAL_LAYERS.
pub const TEMPORAL_LAYERS: u8 = 3;

/// Temporal layers (pattern 0-2-1-2). SVT-AV1's low-delay CBR mode allows at most three
/// (hierarchical levels 0–2), so unlike VP9 and the hardware encoders it has no fourth layer at
/// 60 fps; software AV1 is capped at 30 fps anyway (FR-7.37).
pub const AV1_TEMPORAL_LAYERS: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncoderConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

/// One compressed frame (VP9 or AV1), ready to send.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub keyframe: bool,
    /// Temporal layer 0..[`VideoEncoder::temporal_layers`]. A viewer that skips all frames above some layer
    /// still decodes the rest; keyframes are always layer 0.
    pub temporal_layer: u8,
    /// Milliseconds, as passed to [`VideoEncoder::encode`].
    pub pts: i64,
}

/// Software AV1 options that only exist for this encoder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Av1Options {
    /// AV1's screen-content tools (palette, intra block copy) for "Sharp text" (FR-7.38).
    pub screen_content: bool,
}

/// The libvpx thread count every software VP9 encoder is configured with.
pub fn threads() -> u32 {
    // More than four threads gains little at 720p (tile columns cap the parallelism) and takes
    // cores from the game or app being shared.
    std::thread::available_parallelism()
        .map_or(1, |n| n.get().min(4))
        .try_into()
        .unwrap_or(1)
}

/// SVT-AV1 4.2's SSSE3 code, which it picks on x86 CPUs without AVX2, reads a reference block
/// with an aligned load that is not always aligned (`svt_aom_sub_pixel_variance16xh_ssse3`), so
/// the encoder kills the whole app with a general protection fault. Such CPUs are also far too
/// slow for real-time AV1, so screen sharing uses VP9 there instead. Callers check this before
/// opening an encoder; [`crate::Av1Encoder::new`] refuses such CPUs as a safety net.
pub fn av1_cpu_supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        true
    }
}
