//! Windows camera backend (FR-8.25): Media Foundation `IMFSourceReader` on the camera's media
//! source, device-interface notifications for hot-plug (FR-8.27).
//!
//! The reader hands over the camera's native pictures (converters disabled); the mode choice,
//! 16:9 crop, YUY2/MJPEG conversion and pump are the ones the Linux backend uses
//! ([`crate::mode`], [`crate::convert`], [`crate::pump`]).
//!
//! Virtual cameras that only register with DirectShow, such as the OBS Virtual Camera, are
//! listed after Media Foundation's cameras and captured through a DirectShow graph (`dshow`,
//! #631). Real cameras always go through Media Foundation.

mod bounded;
#[cfg(windows)]
mod capture;
#[cfg(windows)]
mod com;
#[cfg(windows)]
mod dshow;
mod dshow_type;
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
    use super::{capture, com, dshow, hotplug};
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
            let cameras = com::cameras()?;
            let virtual_cameras = dshow_cameras(&cameras);
            Ok(cameras
                .into_iter()
                .map(|camera| camera.device)
                .chain(virtual_cameras.into_iter().map(|camera| camera.device))
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
            let cameras = com::cameras()?;
            if let Some(camera) = cameras.iter().find(|camera| &camera.device.id == device) {
                let link = camera.symbolic_link.clone();
                return Ok(Box::new(capture::Session::start(link, request, sink)?));
            }
            let key = dshow_cameras(&cameras)
                .into_iter()
                .find(|camera| &camera.device.id == device)
                .map(|camera| camera.key)
                .ok_or(CameraError::NoCamera)?;
            Ok(Box::new(dshow::Session::start(key, request, sink)?))
        }
    }

    /// The cameras only DirectShow lists. A failure there must not cost the cameras Media
    /// Foundation found, so it only leaves them out.
    fn dshow_cameras(listed: &[com::Camera]) -> Vec<dshow::Camera> {
        dshow::cameras(listed).unwrap_or_else(|err| {
            tracing::warn!(%err, "DirectShow cameras not listed");
            Vec::new()
        })
    }
}
