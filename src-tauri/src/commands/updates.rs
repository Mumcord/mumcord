//! Update hint and AppImage menu entry (FR-2.55, FR-2.56).

use mumcord_engine::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::ApiResult;
#[cfg(not(target_os = "linux"))]
use super::types::not_implemented;
#[cfg(target_os = "linux")]
use super::types::{ApiError, ErrorCode};

/// A newer Mumcord release on GitHub.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    /// The release page with the update instructions.
    pub url: String,
}

/// The daily check found a newer release.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAvailable {
    pub info: UpdateInfo,
}

/// Asks GitHub for the latest release now (only when `updateCheck` is `yes`); `null` when
/// this version is current, the user did not consent or the check failed (logged only).
#[tauri::command]
#[specta::specta]
pub async fn update_check_now(engine: State<'_, Engine>) -> ApiResult<Option<UpdateInfo>> {
    Ok(crate::update_check::check(&engine, true).await?)
}

/// AppImage only: adds Mumcord to the app menu (desktop entry and icon). The UI calls this
/// after the user said yes and then stores the choice in `appimageMenuEntry`.
#[cfg(target_os = "linux")]
#[tauri::command]
#[specta::specta]
pub fn appimage_menu_entry_add(app: tauri::AppHandle) -> ApiResult<()> {
    crate::desktop_entry::add_for_appimage(&app)
        .map_err(|e| ApiError::new(ErrorCode::Io, format!("adding the menu entry failed: {e}")))
}

/// Only the Linux AppImage has a menu entry to add.
#[cfg(not(target_os = "linux"))]
#[tauri::command]
#[specta::specta]
pub fn appimage_menu_entry_add() -> ApiResult<()> {
    Err(not_implemented("appimage_menu_entry_add"))
}
