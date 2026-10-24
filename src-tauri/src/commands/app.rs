//! Window close behaviour and `mumble://` handler registration (FR-1.6, FR-1.38).
//! Owned by the tray/window/deep-link task.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use super::types::{ApiResult, not_implemented};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum CloseChoice {
    MinimizeToTray,
    Quit,
}

/// The window was closed while connected and the close behaviour is "ask".
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct CloseRequested {}

/// Answers `CloseRequested`; `remember` stores the choice as the close behaviour.
#[tauri::command]
#[specta::specta]
pub fn close_choice(choice: CloseChoice, remember: bool) -> ApiResult<()> {
    let _ = (choice, remember);
    not_implemented()
}

/// Whether Mumcord is the registered handler for `mumble://` links.
#[tauri::command]
#[specta::specta]
pub fn link_handler_status() -> ApiResult<bool> {
    not_implemented()
}

#[tauri::command]
#[specta::specta]
pub fn link_handler_register() -> ApiResult<()> {
    not_implemented()
}
