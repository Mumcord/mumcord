//! Mumcord audio: device I/O (capture and playback via cpal) on top of `mumcord-codec`.
//!
//! The codec items (Opus, jitter buffer, mixer, bitrate control, spatial placement, processing
//! chain and the frame constants) are re-exported, so `mumcord_audio::codec`, `::mixer` and so
//! on keep working. Headless users that need no devices depend on `mumcord-codec` directly.
//!
//! # Frame contract
//!
//! Everything exchanges audio as [`Frame`]s: 48 kHz, mono, `f32` samples in `[-1.0, 1.0]`,
//! 10 ms (480 samples) per frame. Device sample rates and channel layouts are converted at the
//! I/O edge ([`io`]), so processing code never sees anything else.

pub mod io;
mod mic_test;
pub mod rt;

pub use mumcord_codec::{
    FRAME_SAMPLES, Frame, SAMPLE_RATE, bitrate, codec, jitter, mixer, spatial,
};

/// The processing chain of `mumcord-codec` plus the device-bound [`MicTest`](dsp::MicTest).
pub mod dsp {
    pub use crate::mic_test::{MicTest, MicTestPhase};
    pub use mumcord_codec::dsp::*;
}

// Lets the mixer's playback thread write into the device queue.
impl mixer::PlaybackSink for io::FrameSink {
    fn write_frame(&mut self, frame: &Frame) -> bool {
        io::FrameSink::write_frame(self, frame)
    }

    fn write_stereo(&mut self, left: &Frame, right: &Frame) -> bool {
        io::FrameSink::write_stereo(self, left, right)
    }

    fn queued_samples(&mut self) -> usize {
        io::FrameSink::queued_samples(self)
    }
}

// Compiles the README example as a doctest so it cannot rot.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
