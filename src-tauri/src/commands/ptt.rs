//! Push-to-talk availability and key capture (FR-1.5, FR-1.21). Owned by the PTT task.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;

use super::types::ApiResult;
use crate::ptt;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PttUnavailableReason {
    /// Wayland session without the global-shortcuts portal.
    WaylandNoPortal,
    /// Global shortcuts could not be registered for another reason.
    RegistrationFailed,
    /// A mouse button is configured but "Allow mouse buttons" is off (Linux Wayland).
    MouseDisabled,
    /// Linux Wayland: the user is not in the `input` group, so mice cannot be read.
    MouseNeedsInputGroup,
    /// macOS: Mumcord lacks the Accessibility permission for the mouse event tap.
    MouseNeedsAccessibility,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum PttAvailability {
    Available,
    Unavailable { reason: PttUnavailableReason },
}

/// How mouse buttons reach push-to-talk on this system (FR-1.21, QA.4).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PttMouseSupport {
    /// Windows, macOS, X11: always available.
    Native,
    /// Linux Wayland: input devices are read directly, behind the opt-in setting.
    Evdev,
    None,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PttMouseState {
    Ready,
    /// Wayland: mice exist but may not be read; show `sudo usermod -aG input $USER`.
    InputGroupMissing,
    /// macOS: the Accessibility permission is missing.
    AccessibilityMissing,
    /// Wayland: no mouse is connected (one plugged in later is picked up).
    NoDevice,
    /// No mouse backend on this platform.
    Unsupported,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PttMouseStatus {
    pub support: PttMouseSupport,
    /// On Wayland this is the device permission, also while `allowMouseButtons` is off.
    pub state: PttMouseState,
}

/// The desktop session as far as push-to-talk is concerned (FR-1.48).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PttSession {
    /// Linux with a Wayland desktop: the compatibility mode switch applies.
    pub wayland_desktop: bool,
    /// Mumcord currently runs through XWayland and uses the X11 push-to-talk path.
    pub compatibility_active: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn ptt_availability() -> ApiResult<PttAvailability> {
    Ok(ptt::availability().await)
}

/// Whether mouse buttons can be used for push-to-talk, and why not (FR-1.21, AC-1.37).
#[tauri::command]
#[specta::specta]
pub async fn ptt_mouse_status() -> ApiResult<PttMouseStatus> {
    Ok(ptt::mouse_status())
}

/// Whether the compatibility mode switch applies and whether it is in effect right now; the
/// stored setting only takes effect after `app_restart`.
#[tauri::command]
#[specta::specta]
pub fn ptt_session() -> ApiResult<PttSession> {
    let env = ptt::SessionEnv::current();
    Ok(PttSession {
        wayland_desktop: env.wayland_desktop(),
        compatibility_active: env.compatibility_active(),
    })
}

/// Validates a key the UI captured (`KeyboardEvent.code` plus modifiers, e.g. "Ctrl+KeyA",
/// or a mouse button: "MouseMiddle", "Mouse4", "Mouse5") and returns the string to store as
/// `pttKey` (e.g. "Ctrl+A"). Mouse buttons never open a desktop dialog. On Wayland the
/// desktop's shortcut dialog opens with `key` preselected and the user's choice is returned.
/// Errors: `invalidInput` for unusable keys (incl. left/right click, or a mouse button on
/// Wayland while the mouse setting is off) or a cancelled capture; `shortcutNotConfirmed`
/// when the desktop did not confirm the shortcut (dialog dismissed, no key, portal failure).
#[tauri::command]
#[specta::specta]
pub async fn ptt_capture_key(app: AppHandle, key: String) -> ApiResult<String> {
    ptt::capture(&app, &key).await
}

/// Aborts a pending `ptt_capture_key`, which then fails with `invalidInput`.
#[tauri::command]
#[specta::specta]
pub fn ptt_capture_cancel() -> ApiResult<()> {
    ptt::cancel_capture();
    Ok(())
}
