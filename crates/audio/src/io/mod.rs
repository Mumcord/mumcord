//! Audio device I/O.
//!
//! [`AudioIo::start`] opens capture and playback and returns two stable
//! handles: a [`FrameSource`] that yields captured 10 ms [`Frame`](crate::Frame)s
//! and a [`FrameSink`] that accepts mono or stereo frames for playback. Both stay valid
//! across device switches and hot-plug recovery, so the processing chain and
//! mixer never have to re-wire.
//!
//! # Threading
//!
//! - Real-time device callbacks only move samples between the device buffer
//!   and lock-free `rtrb` rings (down/up-mixing channels on the way). They
//!   never allocate, lock, log or do I/O.
//! - Sample-rate conversion (when a device does not run at 48 kHz) happens
//!   on the non-real-time side of the rings, inside [`FrameSource`] and
//!   [`FrameSink`], i.e. on whatever thread calls them.
//! - A control thread owns the device streams, executes device switches and
//!   polls for hot-plug changes every two seconds.
//!
//! # Backends
//!
//! The real backend is `cpal`. With the `test-backend` cargo feature, the
//! environment variable `MUMCORD_AUDIO_BACKEND` selects a replacement for
//! headless tests and E2E runs (see [`BackendKind::from_env`]):
//!
//! - `null` – capture delivers silence, playback is discarded (both paced in
//!   real time).
//! - `file:<in.wav>:<out.wav>` – capture plays `<in.wav>` in real time
//!   (any rate/channel count, silence after its end); playback is recorded
//!   to `<out.wav>` (48 kHz mono 16-bit, the mix of both playback channels, header refreshed every second so it
//!   stays readable if the process is killed).

mod controller;
mod cpal_backend;
mod devices;
mod frames;
mod resample;
mod ring;
#[cfg(any(test, feature = "test-backend"))]
mod test_backend;

use std::path::PathBuf;

pub use controller::{AudioHandles, AudioIo};
pub use frames::{FrameSink, FrameSource};

/// Environment variable that selects the test backend (feature `test-backend`).
pub const BACKEND_ENV: &str = "MUMCORD_AUDIO_BACKEND";

/// Capture (microphone) or playback (speakers/headphones).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Input,
    Output,
}

/// An audio device as shown in device pickers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Stable identifier; persist it to remember the user's choice.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Whether this is the OS default device for its direction.
    pub is_default: bool,
}

/// Notifications from the control thread, for UI and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    /// Devices were plugged in or removed; re-query [`AudioIo::devices`].
    DevicesChanged,
    /// A stream is running on `device`. `fallback` is true when the chosen
    /// device is unavailable and the OS default is used instead; Mumcord
    /// switches back automatically once the chosen device returns.
    Active {
        direction: Direction,
        device: DeviceInfo,
        fallback: bool,
    },
    /// No device could be opened for `direction`; retried on every poll.
    Unavailable {
        direction: Direction,
        reason: String,
    },
}

/// Counters for diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IoStats {
    /// Device callbacks whose captured samples did not fit the ring (reader too slow).
    pub capture_overruns: u64,
    /// Device callbacks that ran out of queued playback mid-buffer.
    pub playback_underruns: u64,
    /// Frames rejected by [`FrameSink::write_frame`] because the queue was full.
    pub playback_frames_dropped: u64,
}

/// Errors from audio device I/O.
#[derive(Debug, thiserror::Error)]
pub enum IoError {
    /// Carries no device name: names can contain personal data and errors end up in logs.
    #[error("audio device not found")]
    DeviceNotFound,
    #[error("unsupported sample format: {0}")]
    UnsupportedFormat(String),
    #[error("audio backend: {0}")]
    Backend(String),
    #[error("resampler: {0}")]
    Resampler(String),
    #[error("invalid {BACKEND_ENV} value: {0}")]
    InvalidBackend(String),
    #[error("audio control thread stopped")]
    ControlThreadGone,
}

/// Which audio backend to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendKind {
    /// Real devices via cpal.
    Cpal,
    /// Silence in, output discarded.
    #[cfg(any(test, feature = "test-backend"))]
    Null,
    /// Capture from a WAV file, record playback to a WAV file.
    #[cfg(any(test, feature = "test-backend"))]
    File { input: PathBuf, output: PathBuf },
}

impl BackendKind {
    /// Reads [`BACKEND_ENV`]. Unset, empty or `cpal` selects real devices.
    /// Test values are rejected in builds without the `test-backend` feature
    /// rather than silently using real devices.
    pub fn from_env() -> Result<Self, IoError> {
        match std::env::var(BACKEND_ENV) {
            Ok(value) => Self::parse(&value),
            Err(_) => Ok(BackendKind::Cpal),
        }
    }

    fn parse(value: &str) -> Result<Self, IoError> {
        match value {
            "" | "cpal" => Ok(BackendKind::Cpal),
            #[cfg(any(test, feature = "test-backend"))]
            "null" => Ok(BackendKind::Null),
            #[cfg(any(test, feature = "test-backend"))]
            _ if value.starts_with("file:") => parse_file_paths(&value["file:".len()..])
                .map(|(input, output)| BackendKind::File { input, output })
                .ok_or_else(|| IoError::InvalidBackend(value.to_owned())),
            _ => Err(IoError::InvalidBackend(value.to_owned())),
        }
    }
}

/// Splits `<in>:<out>`. On Windows a colon right after a leading drive
/// letter (`C:\`) belongs to the path, not the separator.
#[cfg_attr(not(any(test, feature = "test-backend")), allow(dead_code))]
fn parse_file_paths(spec: &str) -> Option<(PathBuf, PathBuf)> {
    let is_drive_colon =
        |pos: usize| cfg!(windows) && pos == 1 && spec.as_bytes()[0].is_ascii_alphabetic();
    let split = spec
        .char_indices()
        .find(|&(pos, c)| c == ':' && !is_drive_colon(pos))
        .map(|(pos, _)| pos)?;
    let (input, output) = (&spec[..split], &spec[split + 1..]);
    if input.is_empty() || output.is_empty() {
        return None;
    }
    Some((PathBuf::from(input), PathBuf::from(output)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_backend_values() {
        assert_eq!(BackendKind::parse("").expect("empty"), BackendKind::Cpal);
        assert_eq!(BackendKind::parse("cpal").expect("cpal"), BackendKind::Cpal);
        assert!(BackendKind::parse("bogus").is_err());
    }

    #[test]
    fn splits_file_paths() {
        let (input, output) = parse_file_paths("/tmp/in.wav:/tmp/out.wav").expect("paths");
        assert_eq!(input, PathBuf::from("/tmp/in.wav"));
        assert_eq!(output, PathBuf::from("/tmp/out.wav"));
        assert!(parse_file_paths("/tmp/in.wav").is_none());
        assert!(parse_file_paths(":/tmp/out.wav").is_none());
    }

    #[cfg(any(test, feature = "test-backend"))]
    #[test]
    fn parses_test_backends() {
        assert_eq!(BackendKind::parse("null").expect("null"), BackendKind::Null);
        assert_eq!(
            BackendKind::parse("file:a.wav:b.wav").expect("file"),
            BackendKind::File {
                input: "a.wav".into(),
                output: "b.wav".into()
            }
        );
    }
}
