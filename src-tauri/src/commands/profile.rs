//! Own avatar, default avatar, comment and registration (FR-1.20, FR-1.34, FR-1.47).

use std::path::PathBuf;

use mumcord_engine::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::{ApiResult, ServerId, to_engine_server_id};

/// The default avatar was not sent to a server because it cannot fit the server's image
/// limit (FR-1.47).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct DefaultAvatarSkipped {
    pub server_id: ServerId,
}

/// The default avatar as a `data:` URL, or `null` if none is set.
#[tauri::command]
#[specta::specta]
pub fn default_avatar_get(engine: State<'_, Engine>) -> ApiResult<Option<String>> {
    Ok(engine.default_avatar_get()?)
}

/// Sets the default avatar from an image file (stored as a 128×128 PNG) or clears it with
/// `null`; returns the stored image as a `data:` URL.
#[tauri::command]
#[specta::specta]
pub async fn default_avatar_set(
    engine: State<'_, Engine>,
    path: Option<String>,
) -> ApiResult<Option<String>> {
    Ok(engine.default_avatar_set(path.map(PathBuf::from)).await?)
}

/// Sets the own avatar from an image file, scaled to the server limit; `null` removes it.
#[tauri::command]
#[specta::specta]
pub async fn set_own_avatar(
    engine: State<'_, Engine>,
    server_id: ServerId,
    path: Option<String>,
) -> ApiResult<()> {
    Ok(engine
        .set_own_avatar(to_engine_server_id(server_id), path.map(PathBuf::from))
        .await?)
}

/// Sets the own profile comment (Mumble-compatible HTML; empty removes it).
#[tauri::command]
#[specta::specta]
pub async fn set_own_comment(
    engine: State<'_, Engine>,
    server_id: ServerId,
    html: String,
) -> ApiResult<()> {
    Ok(engine
        .set_own_comment(to_engine_server_id(server_id), html)
        .await?)
}

/// Registers the current name on the connected server with the current identity.
#[tauri::command]
#[specta::specta]
pub async fn register_self(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<()> {
    Ok(engine.register_self(to_engine_server_id(server_id)).await?)
}
