//! Picking the capture backend for the own share and starting it.

use std::time::Duration;

use super::capture::CaptureSource;
#[cfg(feature = "test-backend")]
use super::capture::TestPattern;
use crate::session::{EngineError, EngineResult, ErrorKind};

/// How often the running share is checked for having ended on its own.
pub(super) const ENDED_POLL: Duration = Duration::from_millis(500);

/// What [`Engine::screen_share_start`](crate::session::Engine::screen_share_start) came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    Started,
    /// The user closed the system picker without choosing: nothing is shared, nothing to report.
    Cancelled,
}

/// Where the pictures come from on this system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "test-backend", allow(dead_code))]
pub(super) enum Backend {
    Wayland,
    X11,
    /// The system picker of Windows.Graphics.Capture.
    Windows,
    /// ScreenCaptureKit: the system picker on macOS 14+, the main screen before.
    MacOs,
    Unsupported,
}

/// Picks the backend from the session's environment: on Linux Wayland sessions the portal
/// picker, otherwise the whole X11 screen.
#[cfg_attr(feature = "test-backend", allow(dead_code))]
pub(super) fn choose_backend(
    linux: bool,
    wayland_display: Option<&str>,
    session_type: Option<&str>,
) -> Backend {
    if !linux {
        return Backend::Unsupported;
    }
    let wayland = wayland_display.is_some_and(|d| !d.is_empty())
        || session_type.is_some_and(|t| t.eq_ignore_ascii_case("wayland"));
    if wayland {
        Backend::Wayland
    } else {
        Backend::X11
    }
}

#[cfg_attr(feature = "test-backend", allow(dead_code))]
fn current_backend() -> Backend {
    if cfg!(windows) {
        return Backend::Windows;
    }
    if cfg!(target_os = "macos") {
        return Backend::MacOs;
    }
    choose_backend(
        cfg!(target_os = "linux"),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
    )
}

/// Why no source could be opened.
#[derive(Debug)]
#[cfg_attr(feature = "test-backend", allow(dead_code))]
pub(super) enum OpenError {
    Cancelled,
    Failed(EngineError),
}

/// The message of the `PermissionDenied` error when macOS does not allow screen recording; the
/// UI points the user to System Settings.
#[cfg(target_os = "macos")]
#[cfg_attr(feature = "test-backend", allow(dead_code))]
const SCREEN_RECORDING_DENIED: &str = "screenRecording";

#[cfg_attr(feature = "test-backend", allow(dead_code))]
fn failed(message: impl std::fmt::Display) -> OpenError {
    OpenError::Failed(EngineError::new(ErrorKind::Internal, message.to_string()))
}

/// Opens the capture for this system. Tests (`test-backend`) always get a [`TestPattern`].
pub(super) async fn open_source() -> Result<Box<dyn CaptureSource>, OpenError> {
    #[cfg(feature = "test-backend")]
    {
        Ok(Box::new(TestPattern::default()))
    }
    #[cfg(not(feature = "test-backend"))]
    open_backend(current_backend()).await
}

#[cfg(not(feature = "test-backend"))]
async fn open_backend(backend: Backend) -> Result<Box<dyn CaptureSource>, OpenError> {
    match backend {
        #[cfg(target_os = "linux")]
        Backend::Wayland => match super::capture_wayland::WaylandCapture::open().await {
            Ok(capture) => Ok(Box::new(capture)),
            Err(super::capture_wayland::WaylandCaptureError::Cancelled) => {
                Err(OpenError::Cancelled)
            }
            Err(e) => Err(failed(e)),
        },
        #[cfg(target_os = "linux")]
        Backend::X11 => super::capture_x11::X11Capture::new()
            .map(|c| Box::new(c) as Box<dyn CaptureSource>)
            .map_err(failed),
        #[cfg(windows)]
        Backend::Windows => match super::capture_windows::WindowsCapture::open().await {
            Ok(capture) => Ok(Box::new(capture)),
            Err(super::capture_windows::WindowsCaptureError::Cancelled) => {
                Err(OpenError::Cancelled)
            }
            Err(e) => Err(failed(e)),
        },
        #[cfg(target_os = "macos")]
        Backend::MacOs => {
            use super::capture_macos::{MacCapture, MacCaptureError};
            match MacCapture::open().await {
                Ok(capture) => Ok(Box::new(capture)),
                Err(MacCaptureError::Cancelled) => Err(OpenError::Cancelled),
                Err(MacCaptureError::ScreenRecordingDenied) => Err(OpenError::Failed(
                    EngineError::new(ErrorKind::PermissionDenied, SCREEN_RECORDING_DENIED),
                )),
                Err(e) => Err(failed(e)),
            }
        }
        _ => Err(failed("screen sharing is not supported on this system yet")),
    }
}

/// `None` when the user cancelled.
pub(super) fn into_outcome(
    opened: Result<Box<dyn CaptureSource>, OpenError>,
) -> EngineResult<Option<Box<dyn CaptureSource>>> {
    match opened {
        Ok(source) => Ok(Some(source)),
        Err(OpenError::Cancelled) => Ok(None),
        Err(OpenError::Failed(e)) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_when_display_or_session_type_says_so() {
        assert_eq!(
            choose_backend(true, Some("wayland-0"), None),
            Backend::Wayland
        );
        assert_eq!(
            choose_backend(true, None, Some("wayland")),
            Backend::Wayland
        );
        assert_eq!(choose_backend(true, Some(""), Some("x11")), Backend::X11);
        assert_eq!(choose_backend(true, None, None), Backend::X11);
    }

    #[test]
    fn other_systems_are_unsupported() {
        assert_eq!(
            choose_backend(false, Some("wayland-0"), None),
            Backend::Unsupported
        );
    }

    struct Fake;
    impl CaptureSource for Fake {
        fn frame(&mut self, _: u32, _: u32) -> Option<mumcord_screenshare::I420Frame> {
            None
        }
    }

    #[test]
    fn cancelled_is_quiet_and_failure_is_an_error() {
        assert!(
            into_outcome(Err(OpenError::Cancelled))
                .expect("ok")
                .is_none()
        );
        assert!(into_outcome(Ok(Box::new(Fake))).expect("ok").is_some());
        let err = into_outcome(Err(failed("boom"))).err().expect("an error");
        assert_eq!(err.kind, ErrorKind::Internal);
    }
}
