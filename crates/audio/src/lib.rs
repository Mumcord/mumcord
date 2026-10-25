//! Mumcord audio: device I/O, Opus codec and (in later tasks) the processing
//! chain, jitter buffer and mixer.
//!
//! # Frame contract
//!
//! Everything inside the crate exchanges audio as [`Frame`]s: 48 kHz, mono,
//! `f32` samples in `[-1.0, 1.0]`, 10 ms (480 samples) per frame. Device
//! sample rates and channel layouts are converted at the I/O edge
//! ([`io`]), so processing code never sees anything else. The one exception
//! is playback: the mixer renders a stereo pair of frames (left, right) so
//! positional voices can be placed ([`spatial`]). Outgoing Opus
//! packets carry 20 ms (two frames) by default, see [`codec`].

pub mod bitrate;
pub mod codec;
pub mod dsp;
pub mod io;
pub mod jitter;
pub mod mixer;
pub mod spatial;

/// Internal sample rate of all audio frames, in Hz.
pub const SAMPLE_RATE: u32 = 48_000;

/// Samples per internal frame (10 ms at [`SAMPLE_RATE`]).
pub const FRAME_SAMPLES: usize = 480;

/// One 10 ms block of 48 kHz mono audio.
pub type Frame = [f32; FRAME_SAMPLES];
