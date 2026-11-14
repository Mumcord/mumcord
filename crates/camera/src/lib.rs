//! Native camera capture for camera sharing (spec 08, FR-8.25 to FR-8.31).
//!
//! The per-OS backends (Windows Media Foundation, macOS AVFoundation, Linux portal/PipeWire with
//! a V4L2 fallback) implement [`CameraBackend`]. Every backend delivers the same thing: NV12
//! frames of at most 1280×720 in 16:9, written into buffers from a [`FramePool`] and pushed to a
//! [`FrameSink`]. Format negotiation, YUY2/MJPEG conversion and the 4:3 centre crop (FR-8.26)
//! happen inside the backend, so everything after capture sees one format.
//!
//! # Platform-fed capture (Android, iOS)
//!
//! On Android and iOS this crate has no OS capture backend: no module for those targets is
//! compiled, and `target_os = "ios"` never takes the macOS AVFoundation branch. The app's native
//! layer (CameraX, AVCaptureSession) captures the pictures and feeds them in through the same
//! interface the desktop backends use:
//!
//! - The app implements [`CameraBackend`]; its `open` returns a [`CaptureSession`] and keeps the
//!   boxed [`FrameSink`].
//! - Per picture the feeder takes a buffer with [`FramePool::acquire`] (4 buffers,
//!   [`POOL_BUFFERS`]; `None` means the consumer is behind, so the feeder drops that picture),
//!   converts into it as NV12 without row padding (see [`CameraFrame`], at most
//!   [`MAX_WIDTH`]×[`MAX_HEIGHT`], 16:9; conversion and cropping are the feeder's job), and calls
//!   [`FrameSink::frame`] with the time since the session started.
//! - Threading: [`FrameSink::frame`] is called from one feeder thread (the platform's camera
//!   callback thread) at a time and is on the real-time path: no blocking, locking, I/O or
//!   logging. The feeder owns the buffer until the call; the sink then owns the frame and
//!   dropping it returns the buffer to the pool. [`FrameSink::ended`] reports permission loss or
//!   unplugging; dropping the session stops the feeder before returning.
//!
//! [`TestCamera`] is the debug-build `MUMCORD_TEST_CAMERA` fixture (FR-8.31); it implements the
//! same trait, so all later pipeline stages run unchanged.

// Format choice, 16:9 crop, conversion to NV12 and MJPEG decoding for the backends that
// negotiate camera formats themselves. AVFoundation converts and scales on its own, so macOS
// only uses the crop geometry and the NV12 path.
#[cfg(any(target_os = "linux", windows, target_os = "macos"))]
#[cfg_attr(
    target_os = "macos",
    allow(dead_code, reason = "macOS only reads NV12")
)]
mod convert;
mod device;
// The pure helpers in `macos::mode` are unit tested on every platform.
#[cfg(any(target_os = "macos", test))]
mod macos;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(any(target_os = "linux", windows))]
mod mjpeg;
// Also built for tests everywhere: `macos::mode`'s pure helpers are unit tested on all platforms.
#[cfg(any(target_os = "linux", windows, target_os = "macos", test))]
#[cfg_attr(
    target_os = "macos",
    allow(dead_code, reason = "macOS only uses the crop geometry")
)]
mod mode;
mod pool;
#[cfg(any(target_os = "linux", windows))]
mod pump;
mod testcam;
// The plain-Rust Media Foundation helpers in `windows::media_type` are unit tested on Linux too.
#[cfg(any(windows, all(test, target_os = "linux")))]
mod windows;

use std::sync::Arc;
use std::time::Duration;

pub use device::{CameraDevice, DeviceEvent, DeviceId, DeviceList};
#[cfg(target_os = "macos")]
pub use macos::{MacCamera, PRIVACY_SETTINGS_URL};

#[cfg(target_os = "linux")]
pub use linux::LinuxCamera;
pub use pool::{FramePool, PooledBuffer};
pub use testcam::{TEST_CAMERA_NAME, TestCamera, TestCameraMode};
#[cfg(windows)]
pub use windows::{PRIVACY_SETTINGS_URL, WindowsCamera};

/// Largest frame a backend delivers (FR-8.26).
pub const MAX_WIDTH: u32 = 1280;
pub const MAX_HEIGHT: u32 = 720;
pub const MAX_FPS: u32 = 30;

/// Buffers per capture session: one being written, one or two queued in the pipeline, one spare.
pub const POOL_BUFFERS: usize = 4;

/// What the pipeline asks a backend for. Backends pick the closest mode at or below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureRequest {
    pub max_width: u32,
    pub max_height: u32,
    pub max_fps: u32,
}

impl Default for CaptureRequest {
    fn default() -> Self {
        Self {
            max_width: MAX_WIDTH,
            max_height: MAX_HEIGHT,
            max_fps: MAX_FPS,
        }
    }
}

/// Bytes of an NV12 picture: the Y plane, then one interleaved UV plane at half height.
/// Width and height are even.
pub fn nv12_len(width: u32, height: u32) -> usize {
    let (w, h) = (width as usize, height as usize);
    w * h + w * h / 2
}

/// One captured picture in NV12 without row padding: `width × height` luma bytes, then
/// `width × height / 2` bytes of interleaved U and V.
pub struct CameraFrame {
    pub width: u32,
    pub height: u32,
    /// Time since the session started.
    pub timestamp: Duration,
    /// Holds [`nv12_len`] bytes; dropping the frame returns the buffer to its pool.
    pub data: PooledBuffer,
}

/// Why a session ended or could not start. The UI turns these into the texts of FR-8.25/8.26.
///
/// `Backend` details must never contain device names or ids (FR-8.28).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CameraError {
    #[error("no camera found")]
    NoCamera,
    /// The OS or the portal refused camera access; the UI shows the platform's own text and
    /// settings button.
    #[error("camera access was denied")]
    PermissionDenied,
    /// The camera was unplugged or vanished while it was on.
    #[error("the camera was disconnected")]
    DeviceGone,
    /// More than 30 frames in a row could not be decoded (FR-8.26).
    #[error("Camera stopped: the camera sends unreadable pictures.")]
    Unreadable,
    #[error("camera backend error: {0}")]
    Backend(String),
}

/// Receives the frames of one session.
pub trait FrameSink: Send {
    /// Runs on the backend's capture thread for every frame. Real-time path: must not block,
    /// allocate, lock, do I/O or log. Hand the frame on through a lock-free queue.
    fn frame(&mut self, frame: CameraFrame);

    /// The session ended on its own. Called at most once, never after the session was dropped.
    fn ended(&mut self, reason: CameraError);
}

/// A running capture. Dropping it stops the camera and joins the capture thread; no
/// [`FrameSink`] call happens after the drop returns.
pub trait CaptureSession: Send {}

/// Called (on any thread) when the set of cameras may have changed.
pub type HotplugCallback = Arc<dyn Fn() + Send + Sync>;

/// Keeps OS hot-plug notifications alive; dropping it unregisters them.
pub type HotplugWatch = Box<dyn Send>;

/// One per-OS camera API.
pub trait CameraBackend: Send + Sync {
    /// Lists the cameras now. An empty list is not an error.
    fn devices(&self) -> Result<Vec<CameraDevice>, CameraError>;

    /// Registers `changed` for OS hot-plug notifications. Backends without them return
    /// `Ok(None)`; the caller then polls [`CameraBackend::devices`] every second to stay within
    /// the 2 s of FR-8.27. Either way the caller feeds the result into [`DeviceList::update`].
    fn watch(&self, changed: HotplugCallback) -> Result<Option<HotplugWatch>, CameraError> {
        let _ = changed;
        Ok(None)
    }

    /// Starts the camera. Frames go to `sink` until the returned session is dropped or the
    /// session ends on its own ([`FrameSink::ended`]). Only one session runs per Mumcord instance
    /// (FR-8.29); the caller makes sure of that.
    fn open(
        &self,
        device: &DeviceId,
        request: CaptureRequest,
        sink: Box<dyn FrameSink>,
    ) -> Result<Box<dyn CaptureSession>, CameraError>;
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    struct ChannelSink(mpsc::Sender<(u32, u32, Duration, Vec<u8>)>);

    impl FrameSink for ChannelSink {
        fn frame(&mut self, frame: CameraFrame) {
            let _ = self.0.send((
                frame.width,
                frame.height,
                frame.timestamp,
                frame.data.to_vec(),
            ));
        }

        fn ended(&mut self, _reason: CameraError) {}
    }

    /// What a mobile app does: render pictures itself and push them into the sink through the
    /// pool, with no backend involved. The test camera's renderer stands in for the OS camera.
    #[test]
    fn externally_fed_frames_reach_the_sink() {
        let (tx, rx) = mpsc::channel();
        let mut sink: Box<dyn FrameSink> = Box::new(ChannelSink(tx));
        let (width, height) = (MAX_WIDTH, MAX_HEIGHT);
        let pool = FramePool::new(POOL_BUFFERS, nv12_len(width, height));

        for index in 0..3u64 {
            let mut data = pool.acquire().expect("a free buffer");
            TestCamera::render(index, &mut data);
            sink.frame(CameraFrame {
                width,
                height,
                timestamp: Duration::from_millis(33 * index),
                data,
            });
        }

        for index in 0..3u64 {
            let (w, h, timestamp, bytes) = rx.try_recv().expect("a frame");
            let mut expected = vec![0; nv12_len(width, height)];
            TestCamera::render(index, &mut expected);
            assert_eq!((w, h), (width, height));
            assert_eq!(timestamp, Duration::from_millis(33 * index));
            assert_eq!(bytes, expected);
        }
        // Every frame was dropped by the sink, so all buffers are back in the pool.
        assert_eq!(pool.available(), POOL_BUFFERS);
    }
}
