//! Chat and message requests and accepted conversations (issue #141).

use mumcord_engine::store::AcceptedConversation as StoredAcceptedConversation;
use mumcord_engine::{Engine, MessageRequestView, PmRequestsView, SentChatRequestView};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::chat::{ChatMessage, rfc3339};
use super::types::{ApiResult, ServerId, SessionId, to_engine_server_id};

/// A held chat request (from a Mumcord user) or held messages (from a standard client).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct MessageRequest {
    /// Id for accept and decline; unique per server while the client runs.
    pub id: u32,
    pub session: SessionId,
    /// Untrusted: render as plain text.
    pub name: String,
    pub cert_hash: Option<String>,
    pub chat_request: bool,
    /// Plain text note of a chat request (at most 200 characters). Untrusted: plain text only.
    pub note: Option<String>,
    /// Held messages, oldest first.
    pub messages: Vec<ChatMessage>,
    /// RFC 3339.
    pub received_at: String,
}

/// A chat request the user sent; a decline is never told, so it stays pending.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SentChatRequest {
    pub session: SessionId,
    pub name: String,
    pub cert_hash: Option<String>,
    pub accepted: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PmRequests {
    pub incoming: Vec<MessageRequest>,
    pub sent: Vec<SentChatRequest>,
}

/// The requests of one server changed. Memory only: gone after disconnect or quit.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct PmRequestsChanged {
    pub server_id: ServerId,
    pub requests: PmRequests,
}

/// A person who may keep sending private messages to one own identity.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedConversation {
    /// Certificate hash of the own identity.
    pub own_cert_hash: String,
    pub cert_hash: String,
    pub name: String,
    /// RFC 3339.
    pub accepted_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedConversationsChanged {
    pub conversations: Vec<AcceptedConversation>,
}

impl From<MessageRequestView> for MessageRequest {
    fn from(v: MessageRequestView) -> Self {
        Self {
            id: u32::try_from(v.id).unwrap_or(u32::MAX),
            session: v.session,
            name: v.name,
            cert_hash: v.cert_hash,
            chat_request: v.chat_request,
            note: v.note,
            messages: v.messages.into_iter().map(Into::into).collect(),
            received_at: rfc3339(v.received_at),
        }
    }
}

impl From<SentChatRequestView> for SentChatRequest {
    fn from(v: SentChatRequestView) -> Self {
        Self {
            session: v.session,
            name: v.name,
            cert_hash: v.cert_hash,
            accepted: v.accepted,
        }
    }
}

impl From<PmRequestsView> for PmRequests {
    fn from(v: PmRequestsView) -> Self {
        Self {
            incoming: v.incoming.into_iter().map(Into::into).collect(),
            sent: v.sent.into_iter().map(Into::into).collect(),
        }
    }
}

pub fn to_api_accepted(list: Vec<StoredAcceptedConversation>) -> Vec<AcceptedConversation> {
    list.into_iter()
        .map(|a| AcceptedConversation {
            own_cert_hash: a.own_cert_hash,
            cert_hash: a.cert_hash,
            name: a.name,
            accepted_at: rfc3339(a.accepted_at),
        })
        .collect()
}

/// Held and sent requests of a connected server.
#[tauri::command]
#[specta::specta]
pub fn pm_requests(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<PmRequests> {
    Ok(engine.pm_requests(to_engine_server_id(server_id)).into())
}

/// Accepts a request: held messages are shown and the sender may keep messaging.
#[tauri::command]
#[specta::specta]
pub fn pm_request_accept(engine: State<'_, Engine>, server_id: ServerId, id: u32) -> ApiResult<()> {
    Ok(engine.pm_request_accept(to_engine_server_id(server_id), id.into())?)
}

/// Declines a request silently.
#[tauri::command]
#[specta::specta]
pub fn pm_request_decline(
    engine: State<'_, Engine>,
    server_id: ServerId,
    id: u32,
) -> ApiResult<()> {
    Ok(engine.pm_request_decline(to_engine_server_id(server_id), id.into())?)
}

/// Sends a chat request with an optional note to another Mumcord user.
#[tauri::command]
#[specta::specta]
pub fn chat_request_send(
    engine: State<'_, Engine>,
    server_id: ServerId,
    session: SessionId,
    note: Option<String>,
) -> ApiResult<()> {
    Ok(engine.chat_request_send(to_engine_server_id(server_id), session, note.as_deref())?)
}

/// Accepted conversations of every own identity, by name.
#[tauri::command]
#[specta::specta]
pub fn accepted_conversations(engine: State<'_, Engine>) -> ApiResult<Vec<AcceptedConversation>> {
    Ok(to_api_accepted(engine.accepted_conversations()?))
}

/// Revokes an accepted conversation of one own identity.
#[tauri::command]
#[specta::specta]
pub fn accepted_conversation_revoke(
    engine: State<'_, Engine>,
    own_cert_hash: String,
    cert_hash: String,
) -> ApiResult<()> {
    Ok(engine.accepted_conversation_revoke(&own_cert_hash, &cert_hash)?)
}
