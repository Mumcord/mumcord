//! Local chat history (FR-1.32, FR-1.33).

use mumcord_engine::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::chat::{ChatMessage, rfc3339};
use super::types::{ApiResult, ChannelId, Conversation, ServerId, to_engine_server_id};

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
    server_id: ServerId,
    conversation: Conversation,
    before: Option<String>,
    limit: u32,
) -> ApiResult<Vec<ChatMessage>> {
    Ok(engine
        .history_load(
            to_engine_server_id(server_id),
            conversation.into(),
            before.as_deref(),
            limit,
        )?
        .into_iter()
        .map(Into::into)
        .collect())
}

#[tauri::command]
#[specta::specta]
pub fn history_clear(engine: State<'_, Engine>, scope: HistoryScope) -> ApiResult<()> {
    Ok(engine.history_clear(scope.into())?)
}

/// When the user last cleared the history of a channel (RFC 3339), for channel info (issue
/// #170); `null` if never.
#[tauri::command]
#[specta::specta]
pub fn history_cleared_at(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
) -> ApiResult<Option<String>> {
    let conversation = Conversation::Channel { channel_id };
    Ok(engine
        .history_cleared_at(to_engine_server_id(server_id), &conversation.into())?
        .map(rfc3339))
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

/// Messages from another Mumcord user's history were added to a channel (FR-1.60), sorted
/// by `sentAt` (ties by hash), each with `syncedFrom` set. Insert them at their time position
/// and keep the reading position (FR-1.60b).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct HistorySynced {
    pub server_id: ServerId,
    pub channel_id: ChannelId,
    pub messages: Vec<ChatMessage>,
}

/// A channel's history sync started (`running: true`) or ended with `added` new messages.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct HistorySyncStatus {
    pub server_id: ServerId,
    pub channel_id: ChannelId,
    pub running: bool,
    pub added: u32,
}

/// "Load earlier messages" (FR-1.60): asks other Mumcord users for the channel's messages
/// from before the user started receiving it live. The channel must be the own or a followed
/// one (`notFound` otherwise); `serverTooOld` on servers that cannot forward plugin data.
/// Progress arrives as `historySyncStatus`, messages as `historySynced`.
#[tauri::command]
#[specta::specta]
pub fn history_sync_request(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
) -> ApiResult<()> {
    Ok(engine.history_sync_request(to_engine_server_id(server_id), channel_id, None)?)
}
