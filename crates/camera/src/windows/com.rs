//! COM and Media Foundation start-up, and the camera list.

use ::windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFAttributes, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_VERSION, MFCreateAttributes,
    MFEnumDeviceSources, MFSTARTUP_LITE, MFShutdown, MFStartup,
};
use ::windows::Win32::System::Com::{
    COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use ::windows::core::{GUID, PWSTR};

use super::media_type::{camera_error, device_key};
use crate::{CameraDevice, CameraError, DeviceId};

/// Error of a failed Media Foundation or COM call.
pub fn mf_error(err: &::windows::core::Error) -> CameraError {
    camera_error(err.code().0)
}

/// COM for the current thread, in the multithreaded apartment Media Foundation works in.
///
/// A thread that already runs a single-threaded apartment (the Tauri main thread) keeps it:
/// COM is usable there too, and this guard then leaves it alone.
pub struct Com {
    initialized: bool,
}

impl Com {
    pub fn init() -> Self {
        // SAFETY: plain COM start-up for this thread; balanced in `Drop` when it succeeded.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        Self {
            initialized: hr.is_ok(),
        }
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: balances the successful `CoInitializeEx` of this thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// Media Foundation, started for as long as this lives. `MFStartup` counts its callers, so
/// every user holds its own guard.
pub struct MediaFoundation(());

impl MediaFoundation {
    pub fn start() -> Result<Self, CameraError> {
        // SAFETY: balanced by `MFShutdown` in `Drop`.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_LITE) }.map_err(|err| mf_error(&err))?;
        Ok(Self(()))
    }
}

impl Drop for MediaFoundation {
    fn drop(&mut self) {
        // SAFETY: balances the successful `MFStartup` of `start`.
        if let Err(err) = unsafe { MFShutdown() } {
            tracing::warn!(code = err.code().0, "Media Foundation shutdown failed");
        }
    }
}

/// A camera as Media Foundation lists it.
pub struct Camera {
    pub device: CameraDevice,
    /// The OS key to open it with; never logged or stored (FR-8.28).
    pub symbolic_link: String,
}

/// Attributes that select video capture devices.
pub fn video_capture_attributes(capacity: u32) -> Result<IMFAttributes, CameraError> {
    let mut attributes = None;
    // SAFETY: out-pointer to a local; the result is checked.
    unsafe { MFCreateAttributes(&mut attributes, capacity) }.map_err(|err| mf_error(&err))?;
    let attributes =
        attributes.ok_or_else(|| CameraError::Backend("no Media Foundation attributes".into()))?;
    // SAFETY: both pointers refer to constants that outlive the call.
    unsafe {
        attributes.SetGUID(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        )
    }
    .map_err(|err| mf_error(&err))?;
    Ok(attributes)
}

/// Lists the cameras now. Windows has no default camera for desktop apps, so the first one
/// Media Foundation lists counts as the default.
pub fn cameras() -> Result<Vec<Camera>, CameraError> {
    let _com = Com::init();
    let _mf = MediaFoundation::start()?;
    let attributes = video_capture_attributes(1)?;

    let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: out-pointers to locals; on success `list` holds `count` activation objects in one
    // CoTaskMem block that is freed below after taking them out.
    unsafe { MFEnumDeviceSources(&attributes, &mut list, &mut count) }
        .map_err(|err| mf_error(&err))?;
    let activates: Vec<Option<IMFActivate>> = if list.is_null() {
        Vec::new()
    } else {
        // SAFETY: `list` points to `count` initialised entries; `read` moves each reference out
        // once, so freeing the block afterwards leaks nothing and releases nothing twice.
        let taken = (0..count as usize)
            .map(|index| unsafe { list.add(index).read() })
            .collect();
        // SAFETY: the block came from `MFEnumDeviceSources` and is no longer used.
        unsafe { CoTaskMemFree(Some(list.cast_const().cast())) };
        taken
    };

    let mut cameras = Vec::new();
    for activate in activates.into_iter().flatten() {
        let Some(symbolic_link) = string(
            &activate,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
        ) else {
            continue;
        };
        let name = string(&activate, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)
            .unwrap_or_else(|| "Camera".to_owned());
        cameras.push(Camera {
            device: CameraDevice {
                id: DeviceId::from_os_key("mf", &device_key(&symbolic_link)),
                name,
                is_default: cameras.is_empty(),
            },
            symbolic_link,
        });
    }
    Ok(cameras)
}

/// A string attribute, or `None` when it is missing.
fn string(activate: &IMFActivate, key: &GUID) -> Option<String> {
    let mut value = PWSTR::null();
    let mut length = 0u32;
    // SAFETY: out-pointers to locals; the string is CoTaskMem memory that is freed below.
    unsafe { activate.GetAllocatedString(key, &mut value, &mut length) }.ok()?;
    // SAFETY: on success `value` is a valid NUL-terminated wide string.
    let text = unsafe { value.to_string() }.ok();
    // SAFETY: allocated by `GetAllocatedString`, not used after this.
    unsafe { CoTaskMemFree(Some(value.0.cast_const().cast())) };
    text
}
