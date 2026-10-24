//! Saved servers and `mumble://` link parsing (FR-1.7, FR-1.8, FR-1.11).

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use super::types::{ApiResult, ServerId, not_implemented};

/// A saved server as shown in the server bar; the list order is the bar order.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedServer {
    pub id: ServerId,
    pub label: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    /// The password itself stays in the OS secure storage and is never sent to the UI.
    pub has_password: bool,
    /// "Save history" switch (FR-1.33); changed via `history_set_saving`.
    pub save_history: bool,
}

/// How a server edit treats the stored password.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PasswordUpdate {
    /// Leave the stored password as it is (on add: no password).
    Keep,
    Clear,
    Set {
        value: String,
    },
}

/// Fields for adding or editing a saved server.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerInput {
    pub label: String,
    pub address: String,
    /// Default 64738.
    pub port: u16,
    pub username: String,
    pub password: PasswordUpdate,
}

/// Parsed `mumble://[user[:password]@]host[:port]/[channel/path]` link.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct MumbleLink {
    pub address: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    /// Channel names from the root down; empty for no channel.
    pub channel_path: Vec<String>,
}

/// A `mumble://` link was opened from outside (startup argument or second launch).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct DeepLinkOpened {
    pub link: MumbleLink,
}

/// Saved servers in server-bar order.
#[tauri::command]
#[specta::specta]
pub fn servers_list() -> ApiResult<Vec<SavedServer>> {
    not_implemented()
}

/// Adds a server at the end of the server bar.
#[tauri::command]
#[specta::specta]
pub fn server_add(input: ServerInput) -> ApiResult<SavedServer> {
    let _ = input;
    not_implemented()
}

#[tauri::command]
#[specta::specta]
pub fn server_update(id: ServerId, input: ServerInput) -> ApiResult<SavedServer> {
    let _ = (id, input);
    not_implemented()
}

/// Removes the server with its password, trust decision, follows and history.
#[tauri::command]
#[specta::specta]
pub fn server_remove(id: ServerId) -> ApiResult<()> {
    let _ = id;
    not_implemented()
}

/// Sets the server-bar order; `ids` must contain every saved server exactly once.
#[tauri::command]
#[specta::specta]
pub fn servers_reorder(ids: Vec<ServerId>) -> ApiResult<()> {
    let _ = ids;
    not_implemented()
}

/// Parses a pasted `mumble://` link; fails with `invalidInput` if malformed.
#[tauri::command]
#[specta::specta]
pub fn parse_mumble_link(url: String) -> ApiResult<MumbleLink> {
    let _ = url;
    not_implemented()
}
