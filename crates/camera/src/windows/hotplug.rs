//! Hot-plug (FR-8.27): Configuration Manager notifications for camera device interfaces. They
//! need no window or message loop; the callback runs on a system thread pool thread.

use std::ffi::c_void;

use ::windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_NOTIFY_ACTION, CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL,
    CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL, CM_NOTIFY_EVENT_DATA, CM_NOTIFY_FILTER,
    CM_NOTIFY_FILTER_0, CM_NOTIFY_FILTER_0_0, CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
    CM_Register_Notification, CM_Unregister_Notification, CR_SUCCESS, HCMNOTIFICATION,
};
use ::windows::Win32::Media::KernelStreaming::{KSCATEGORY_CAPTURE, KSCATEGORY_VIDEO_CAMERA};
use ::windows::core::GUID;

use crate::{CameraError, HotplugCallback};

/// Media Foundation lists cameras from the video camera interface class (Windows 10 and
/// later); older drivers only register the capture class. A change in either may change the
/// list, and the caller re-reads the whole list, so a spare notification costs nothing.
const CLASSES: [GUID; 2] = [KSCATEGORY_VIDEO_CAMERA, KSCATEGORY_CAPTURE];

/// The registered notifications; dropping it unregisters them.
pub struct Watch {
    handles: Vec<HCMNOTIFICATION>,
    /// Handed to the callback as its context; freed only after every handle is unregistered.
    callback: *mut HotplugCallback,
}

// SAFETY: the handles are plain tokens for `CM_Unregister_Notification`, which may be called on
// any thread; the callback behind the pointer is `Send + Sync`.
unsafe impl Send for Watch {}

impl Watch {
    pub fn register(changed: HotplugCallback) -> Result<Self, CameraError> {
        let mut watch = Self {
            handles: Vec::with_capacity(CLASSES.len()),
            callback: Box::into_raw(Box::new(changed)),
        };
        for class in CLASSES {
            let filter = CM_NOTIFY_FILTER {
                cbSize: size_of::<CM_NOTIFY_FILTER>() as u32,
                FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
                u: CM_NOTIFY_FILTER_0 {
                    DeviceInterface: CM_NOTIFY_FILTER_0_0 { ClassGuid: class },
                },
                ..Default::default()
            };
            let mut handle = HCMNOTIFICATION::default();
            // SAFETY: the filter lives for the call; the context stays valid until `Drop` has
            // unregistered every handle.
            let result = unsafe {
                CM_Register_Notification(
                    &filter,
                    Some(watch.callback.cast_const().cast()),
                    Some(on_change),
                    &mut handle,
                )
            };
            if result != CR_SUCCESS {
                // `watch` drops here and unregisters what was registered so far.
                return Err(CameraError::Backend(format!(
                    "device notifications failed (CONFIGRET {})",
                    result.0
                )));
            }
            watch.handles.push(handle);
        }
        Ok(watch)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        for handle in self.handles.drain(..) {
            // SAFETY: registered in `register`. Unregistering waits for running callbacks, and
            // this never runs inside one.
            let result = unsafe { CM_Unregister_Notification(handle) };
            if result != CR_SUCCESS {
                tracing::warn!(code = result.0, "unregistering camera notifications failed");
            }
        }
        // SAFETY: created by `Box::into_raw` in `register`; no callback can run any more.
        drop(unsafe { Box::from_raw(self.callback) });
    }
}

unsafe extern "system" fn on_change(
    _notification: HCMNOTIFICATION,
    context: *const c_void,
    action: CM_NOTIFY_ACTION,
    _data: *const CM_NOTIFY_EVENT_DATA,
    _size: u32,
) -> u32 {
    if action == CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL
        || action == CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL
    {
        // SAFETY: the context is the `HotplugCallback` of a live `Watch` (see `Drop`).
        let changed = unsafe { &*context.cast::<HotplugCallback>() };
        changed();
    }
    0
}
