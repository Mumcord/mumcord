//! Own avatar, comment and registration (FR-1.20, FR-1.34).

use std::path::PathBuf;

use mumcord_engine::Engine;
use tauri::State;

use super::types::ApiResult;

/// Sets the own avatar from an image file, scaled to the server limit; `null` removes it.
#[tauri::command]
#[specta::specta]
pub async fn set_own_avatar(engine: State<'_, Engine>, path: Option<String>) -> ApiResult<()> {
    Ok(engine.set_own_avatar(path.map(PathBuf::from)).await?)
}

/// Sets the own profile comment (Mumble-compatible HTML; empty removes it).
#[tauri::command]
#[specta::specta]
pub async fn set_own_comment(engine: State<'_, Engine>, html: String) -> ApiResult<()> {
    Ok(engine.set_own_comment(html).await?)
}

/// Registers the current name on the connected server with the current identity.
#[tauri::command]
#[specta::specta]
pub async fn register_self(engine: State<'_, Engine>) -> ApiResult<()> {
    Ok(engine.register_self().await?)
}
