//! macOS camera capture through AVFoundation (FR-8.25).
//!
//! The camera's own format is picked by [`mode::choose`]; AVFoundation's video output converts
//! whatever the camera sends (YUY2, MJPEG, NV12) to NV12 video range and, for formats larger than
//! 720p, scales it down in the system's converter. The only work left on the capture queue is the
//! 16:9 centre crop while copying into a pool buffer.

#[cfg(target_os = "macos")]
mod avf;
mod mode;

#[cfg(target_os = "macos")]
pub use avf::MacCamera;

/// The macOS Settings page for camera access (Privacy & Security → Camera), for the
/// **Open Privacy settings** button shown with [`crate::CameraError::PermissionDenied`].
#[cfg(target_os = "macos")]
pub const PRIVACY_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Camera";
