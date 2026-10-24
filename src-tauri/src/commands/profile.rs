//! Own avatar, comment and registration (FR-1.20, FR-1.34).

use super::types::{ApiResult, not_implemented};

/// Sets the own avatar from an image file, scaled to the server limit; `null` removes it.
#[tauri::command]
#[specta::specta]
pub fn set_own_avatar(path: Option<String>) -> ApiResult<()> {
    let _ = path;
    not_implemented()
}

/// Sets the own profile comment (Mumble-compatible HTML; empty removes it).
#[tauri::command]
#[specta::specta]
pub fn set_own_comment(html: String) -> ApiResult<()> {
    let _ = html;
    not_implemented()
}

/// Registers the current name on the connected server with the current identity.
#[tauri::command]
#[specta::specta]
pub fn register_self() -> ApiResult<()> {
    not_implemented()
}
