//! Sending and receiving text messages (FR-1.28–1.30).

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use super::types::{ApiResult, ChannelId, Conversation, SessionId, not_implemented};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    /// Locally unique id; also the paging cursor for `history_load`.
    pub id: String,
    pub conversation: Conversation,
    pub sender_name: String,
    pub sender_cert_hash: Option<String>,
    /// Sender's session while still connected; `null` for history entries.
    pub sender_session: Option<SessionId>,
    /// True for messages the own user sent.
    pub own: bool,
    /// Local receive (or send) time, RFC 3339.
    pub sent_at: String,
    /// Sanitized HTML; the UI sanitizes again before rendering.
    pub body: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct MessageReceived {
    pub message: ChatMessage,
}

/// Sends Mumble-compatible HTML (the UI converts Markdown and scales images).
/// Fails with `messageTooLarge` above the server limit or `permissionDenied`.
#[tauri::command]
#[specta::specta]
pub fn send_channel_message(channel_id: ChannelId, html: String) -> ApiResult<ChatMessage> {
    let _ = (channel_id, html);
    not_implemented()
}

#[tauri::command]
#[specta::specta]
pub fn send_private_message(session: SessionId, html: String) -> ApiResult<ChatMessage> {
    let _ = (session, html);
    not_implemented()
}
