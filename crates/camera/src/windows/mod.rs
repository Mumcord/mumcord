//! Windows camera backend (FR-8.25): Media Foundation `IMFSourceReader` on the camera's media
//! source, device-interface notifications for hot-plug (FR-8.27).
//!
//! The reader hands over the camera's native pictures (converters disabled); the mode choice,
//! 16:9 crop, YUY2/MJPEG conversion and pump are the ones the Linux backend uses
//! ([`crate::mode`], [`crate::convert`], [`crate::pump`]).

#[cfg(windows)]
mod capture;
#[cfg(windows)]
mod com;
#[cfg(windows)]
mod hotplug;
mod media_type;

#[cfg(windows)]
pub use self::camera::WindowsCamera;

/// The Windows Settings page for camera privacy, for the **Open camera settings** button shown
/// with [`crate::CameraError::PermissionDenied`] ("Windows blocks camera access for desktop
/// apps", FR-8.25).
#[cfg(windows)]
pub const PRIVACY_SETTINGS_URL: &str = "ms-settings:privacy-webcam";

#[cfg(windows)]
mod camera {
    use super::{capture, com, hotplug};
    use crate::{
        CameraBackend, CameraDevice, CameraError, CaptureRequest, CaptureSession, DeviceId,
        FrameSink, HotplugCallback, HotplugWatch,
    };

    /// The Windows [`CameraBackend`].
    #[derive(Default)]
    pub struct WindowsCamera;

    impl WindowsCamera {
        pub fn new() -> Self {
            Self
        }
    }

    impl CameraBackend for WindowsCamera {
        fn devices(&self) -> Result<Vec<CameraDevice>, CameraError> {
            Ok(com::cameras()?
                .into_iter()
                .map(|camera| camera.device)
                .collect())
        }

        fn watch(&self, changed: HotplugCallback) -> Result<Option<HotplugWatch>, CameraError> {
            Ok(Some(Box::new(hotplug::Watch::register(changed)?)))
        }

        fn open(
            &self,
            device: &DeviceId,
            request: CaptureRequest,
            sink: Box<dyn FrameSink>,
        ) -> Result<Box<dyn CaptureSession>, CameraError> {
            let link = com::cameras()?
                .into_iter()
                .find(|camera| &camera.device.id == device)
                .map(|camera| camera.symbolic_link)
                .ok_or(CameraError::NoCamera)?;
            Ok(Box::new(capture::Session::start(link, request, sink)?))
        }
    }
}
