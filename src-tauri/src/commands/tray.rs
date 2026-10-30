//! The UI's share of the tray menu (FR-3.132): unread counts, mentions and recent private chats
//! are only known in the UI, which matches mentions against the user's names.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tauri_specta::Event;

use super::types::{ApiResult, Conversation, ServerId};

/// A recent private chat for the tray's "Recent chats" menu.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct TrayChat {
    pub server_id: ServerId,
    pub conversation: Conversation,
    /// Display name of the person (plain text).
    pub name: String,
    pub unread: u32,
}

/// Unread messages, mentions and recent private chats across all servers, for the tray icon's
/// badge and menu.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct TrayAttention {
    pub unread: u32,
    pub mentions: u32,
    /// Most recent first.
    pub chats: Vec<TrayChat>,
}

/// "Settings…" was chosen in the tray menu; the window is already in front.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct TraySettingsRequested {}

/// Replaces the tray's unread summary and recent chats.
#[tauri::command]
#[specta::specta]
pub fn tray_attention_set(app: AppHandle, attention: TrayAttention) -> ApiResult<()> {
    crate::tray::attention_changed(&app, attention);
    Ok(())
}
