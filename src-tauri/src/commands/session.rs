//! Connection lifecycle, certificate trust, server snapshot and self mute/deafen
//! (FR-1.9, FR-1.10, FR-1.12–1.14, FR-1.22, FR-1.26).

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use super::channels::ChannelInfo;
use super::people::User;
use super::types::{ApiError, ApiResult, ChannelId, ServerId, SessionId, not_implemented};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ConnectionState {
    /// `error` explains why the last connection ended, if it did not end on request.
    Disconnected { error: Option<ApiError> },
    #[serde(rename_all = "camelCase")]
    Connecting { server_id: ServerId },
    #[serde(rename_all = "camelCase")]
    Connected { server_id: ServerId },
    #[serde(rename_all = "camelCase")]
    Reconnecting {
        server_id: ServerId,
        attempt: u32,
        max_attempts: u32,
        next_retry_secs: u32,
    },
}

/// Server facts and limits learned during the handshake.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    /// E.g. "1.5.735".
    pub version: String,
    /// False on servers older than 1.4: following is hidden with a hint (FR-1.12).
    pub follow_supported: bool,
    /// Sanitized HTML.
    pub welcome_text: Option<String>,
    /// Maximum text message length in characters (0 = unlimited).
    pub max_message_length: u32,
    /// Maximum length of messages containing images in characters (0 = unlimited).
    pub max_image_message_length: u32,
    pub allow_html: bool,
}

/// Unread count of a followed channel or private chat.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct UnreadCount {
    pub conversation: super::types::Conversation,
    pub count: u32,
}

/// Complete view of the connected server.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerSnapshot {
    pub server_id: ServerId,
    pub self_session: SessionId,
    pub info: ServerInfo,
    pub channels: Vec<ChannelInfo>,
    pub users: Vec<User>,
    pub followed_channel_ids: Vec<ChannelId>,
    pub unread: Vec<UnreadCount>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum Transport {
    Udp,
    /// UDP blocked; voice tunnelled through the TCP connection.
    Tcp,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStateChanged {
    pub state: ConnectionState,
}

/// The server's certificate is unknown or changed; the connection waits for `cert_trust_decide`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct CertTrustRequired {
    pub server_id: ServerId,
    /// SHA-256 fingerprint, hex with colons.
    pub fingerprint: String,
    /// Set when a previously trusted certificate changed (blocking warning); `null` on first use.
    pub previous_fingerprint: Option<String>,
}

/// Sent once the server finished syncing (ServerSync) and after a reconnect.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ServerSynced {
    pub snapshot: ServerSnapshot,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct TransportChanged {
    pub transport: Transport,
}

/// The server denied an action asynchronously (PermissionDenied message).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ServerDenied {
    pub error: ApiError,
    pub channel_id: Option<ChannelId>,
}

/// Connects to a saved server, disconnecting the current one first (the UI confirms that).
/// `channel_path` (from a `mumble://` link) overrides rejoining the last channel.
#[tauri::command]
#[specta::specta]
pub fn connect(server_id: ServerId, channel_path: Option<Vec<String>>) -> ApiResult<()> {
    let _ = (server_id, channel_path);
    not_implemented()
}

/// Disconnects, also cancelling any pending reconnect.
#[tauri::command]
#[specta::specta]
pub fn disconnect() -> ApiResult<()> {
    not_implemented()
}

/// Stops automatic reconnecting; the state becomes `disconnected`.
#[tauri::command]
#[specta::specta]
pub fn reconnect_cancel() -> ApiResult<()> {
    not_implemented()
}

#[tauri::command]
#[specta::specta]
pub fn connection_state() -> ApiResult<ConnectionState> {
    not_implemented()
}

/// Current snapshot, or `null` when not connected.
#[tauri::command]
#[specta::specta]
pub fn session_snapshot() -> ApiResult<Option<ServerSnapshot>> {
    not_implemented()
}

/// Answers a `CertTrustRequired`; trusting is remembered for the server.
#[tauri::command]
#[specta::specta]
pub fn cert_trust_decide(server_id: ServerId, fingerprint: String, trust: bool) -> ApiResult<()> {
    let _ = (server_id, fingerprint, trust);
    not_implemented()
}

#[tauri::command]
#[specta::specta]
pub fn set_self_mute(muted: bool) -> ApiResult<()> {
    let _ = muted;
    not_implemented()
}

/// Deafening also mutes; undeafening restores the previous mute state.
#[tauri::command]
#[specta::specta]
pub fn set_self_deaf(deafened: bool) -> ApiResult<()> {
    let _ = deafened;
    not_implemented()
}
