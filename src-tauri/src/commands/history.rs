//! Local chat history (FR-1.32, FR-1.33).

use mumcord_engine::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::chat::ChatMessage;
use super::types::{ApiResult, Conversation, ServerId, to_engine_server_id};

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
    engine: State<'_, Engine>,
    conversation: Conversation,
    before: Option<String>,
    limit: u32,
) -> ApiResult<Vec<ChatMessage>> {
    Ok(engine
        .history_load(conversation.into(), before.as_deref(), limit)?
        .into_iter()
        .map(Into::into)
        .collect())
}

#[tauri::command]
#[specta::specta]
pub fn history_clear(engine: State<'_, Engine>, scope: HistoryScope) -> ApiResult<()> {
    Ok(engine.history_clear(scope.into())?)
}

impl From<HistoryScope> for mumcord_engine::HistoryScope {
    fn from(scope: HistoryScope) -> Self {
        match scope {
            HistoryScope::Conversation {
                server_id,
                conversation,
            } => Self::Conversation {
                server_id: to_engine_server_id(server_id),
                conversation: conversation.into(),
            },
            HistoryScope::Server { server_id } => Self::Server(to_engine_server_id(server_id)),
            HistoryScope::All => Self::All,
        }
    }
}

/// "Save history" switch per server (default on).
#[tauri::command]
#[specta::specta]
pub fn history_set_saving(
    engine: State<'_, Engine>,
    server_id: ServerId,
    enabled: bool,
) -> ApiResult<()> {
    Ok(engine.set_save_history(to_engine_server_id(server_id), enabled)?)
}
