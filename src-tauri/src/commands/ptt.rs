//! Push-to-talk availability and key capture (FR-1.5, FR-1.21). Owned by the PTT task.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::types::{ApiResult, not_implemented};

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
pub fn ptt_availability() -> ApiResult<PttAvailability> {
    not_implemented()
}

/// Waits for the next key press and returns it as a shortcut string (e.g. "Ctrl+F13").
#[tauri::command]
#[specta::specta]
pub async fn ptt_capture_key() -> ApiResult<String> {
    not_implemented()
}

/// Aborts a pending `ptt_capture_key`, which then fails with `invalidInput`.
#[tauri::command]
#[specta::specta]
pub fn ptt_capture_cancel() -> ApiResult<()> {
    not_implemented()
}
