//! Push-to-talk availability and key capture (FR-1.5, FR-1.21). Owned by the PTT task.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::types::ApiResult;
use crate::ptt;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PttUnavailableReason {
    /// Wayland session without the global-shortcuts portal.
    WaylandNoPortal,
    /// Global shortcuts could not be registered for another reason.
    RegistrationFailed,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum PttAvailability {
    Available,
    Unavailable { reason: PttUnavailableReason },
}

#[tauri::command]
#[specta::specta]
pub async fn ptt_availability() -> ApiResult<PttAvailability> {
    Ok(ptt::availability().await)
}

/// Validates a key the UI captured (`KeyboardEvent.code` plus modifiers, e.g. "Ctrl+KeyA")
/// and returns the shortcut string to store as `pttKey` (e.g. "Ctrl+A"). On Wayland the
/// desktop's shortcut dialog opens with `key` preselected and the user's choice is returned.
/// Errors: `invalidInput` for unusable keys or a cancelled capture; `shortcutNotConfirmed`
/// when the desktop did not confirm the shortcut (dialog dismissed, no key, portal failure).
#[tauri::command]
#[specta::specta]
pub async fn ptt_capture_key(key: String) -> ApiResult<String> {
    ptt::capture(&key).await
}

/// Aborts a pending `ptt_capture_key`, which then fails with `invalidInput`.
#[tauri::command]
#[specta::specta]
pub fn ptt_capture_cancel() -> ApiResult<()> {
    ptt::cancel_capture();
    Ok(())
}
