//! Local chat history (FR-1.32, FR-1.33).

use serde::{Deserialize, Serialize};
use specta::Type;

use super::chat::ChatMessage;
use super::types::{ApiResult, Conversation, ServerId, not_implemented};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HistoryScope {
    #[serde(rename_all = "camelCase")]
    Conversation {
        server_id: ServerId,
        conversation: Conversation,
    },
    #[serde(rename_all = "camelCase")]
    Server {
        server_id: ServerId,
    },
    All,
}

/// Stored messages of a conversation on the connected server, oldest first.
/// `before` is a message id to page backwards from; `null` loads the newest.
#[tauri::command]
#[specta::specta]
pub fn history_load(
    conversation: Conversation,
    before: Option<String>,
    limit: u32,
) -> ApiResult<Vec<ChatMessage>> {
    let _ = (conversation, before, limit);
    not_implemented()
}

#[tauri::command]
#[specta::specta]
pub fn history_clear(scope: HistoryScope) -> ApiResult<()> {
    let _ = scope;
    not_implemented()
}

/// "Save history" switch per server (default on).
#[tauri::command]
#[specta::specta]
pub fn history_set_saving(server_id: ServerId, enabled: bool) -> ApiResult<()> {
    let _ = (server_id, enabled);
    not_implemented()
}
