//! Connection lifecycle, certificate trust, server snapshot and self mute/deafen
//! (FR-1.9, FR-1.10, FR-1.12–1.14, FR-1.22, FR-1.26).

use mumcord_engine::{
    ConnState, ConnectionStatsView, Engine, ServerInfoView, Snapshot, UnreadView,
    VoiceTransportKind,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::channels::ChannelInfo;
use super::people::User;
use super::types::{
    ApiError, ApiResult, ChannelId, ServerId, SessionId, to_api_server_id, to_engine_server_id,
};

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

/// Live link figures for the connection info panel (AC-1.23). Polled by the UI while the
/// panel is open; counters restart with every (re)connection.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStats {
    /// `null` when the voice transport could not be started.
    pub transport: Option<Transport>,
    /// Smoothed UDP ping; `null` until measured and while tunnelling over TCP.
    pub rtt_ms: Option<f32>,
    /// Recent packet loss in whole percent, 0–100 (an integer, because specta exports floats
    /// as nullable).
    pub loss_percent: u8,
    /// Current Opus encoder target set by the adaptive bitrate controller.
    pub send_bitrate_kbps: u32,
    /// The server's bandwidth limit per user, if announced.
    pub server_max_bandwidth_kbps: Option<u32>,
    pub packets_sent: u32,
    pub packets_received: u32,
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

impl From<ConnState> for ConnectionState {
    fn from(state: ConnState) -> Self {
        match state {
            ConnState::Disconnected { error } => Self::Disconnected {
                error: error.map(ApiError::from),
            },
            ConnState::Connecting { server_id } => Self::Connecting {
                server_id: to_api_server_id(server_id),
            },
            ConnState::Connected { server_id } => Self::Connected {
                server_id: to_api_server_id(server_id),
            },
            ConnState::Reconnecting {
                server_id,
                attempt,
                max_attempts,
                next_retry_secs,
            } => Self::Reconnecting {
                server_id: to_api_server_id(server_id),
                attempt,
                max_attempts,
                next_retry_secs,
            },
        }
    }
}

impl From<ServerInfoView> for ServerInfo {
    fn from(v: ServerInfoView) -> Self {
        Self {
            version: v.version,
            follow_supported: v.follow_supported,
            welcome_text: v.welcome_text,
            max_message_length: v.max_message_length,
            max_image_message_length: v.max_image_message_length,
            allow_html: v.allow_html,
        }
    }
}

impl From<UnreadView> for UnreadCount {
    fn from(v: UnreadView) -> Self {
        Self {
            conversation: v.conversation.into(),
            count: v.count,
        }
    }
}

impl From<Snapshot> for ServerSnapshot {
    fn from(s: Snapshot) -> Self {
        Self {
            server_id: to_api_server_id(s.server_id),
            self_session: s.self_session,
            info: s.info.into(),
            channels: s.channels.into_iter().map(ChannelInfo::from).collect(),
            users: s.users.into_iter().map(User::from).collect(),
            followed_channel_ids: s.followed_channel_ids,
            unread: s.unread.into_iter().map(UnreadCount::from).collect(),
        }
    }
}

impl From<VoiceTransportKind> for Transport {
    fn from(t: VoiceTransportKind) -> Self {
        match t {
            VoiceTransportKind::Udp => Self::Udp,
            VoiceTransportKind::Tcp => Self::Tcp,
        }
    }
}

impl From<ConnectionStatsView> for ConnectionStats {
    fn from(v: ConnectionStatsView) -> Self {
        // 64-bit integers are not exported to TS; a session never gets near u32::MAX packets.
        let count = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
        Self {
            transport: v.transport.map(Transport::from),
            rtt_ms: v.rtt_ms,
            loss_percent: v.loss_percent.round().clamp(0.0, 100.0) as u8,
            send_bitrate_kbps: v.send_bitrate_bps / 1000,
            server_max_bandwidth_kbps: v.server_max_bandwidth_bps.map(|b| b / 1000),
            packets_sent: count(v.packets_sent),
            packets_received: count(v.packets_received),
        }
    }
}

/// Connects to a saved server, disconnecting the current one first (the UI confirms that).
/// `channel_path` (from a `mumble://` link) overrides rejoining the last channel.
#[tauri::command]
#[specta::specta]
pub fn connect(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_path: Option<Vec<String>>,
) -> ApiResult<()> {
    Ok(engine.connect(to_engine_server_id(server_id), channel_path)?)
}

/// Disconnects, also cancelling any pending reconnect.
#[tauri::command]
#[specta::specta]
pub fn disconnect(engine: State<'_, Engine>) -> ApiResult<()> {
    engine.disconnect();
    Ok(())
}

/// Stops automatic reconnecting; the state becomes `disconnected`.
#[tauri::command]
#[specta::specta]
pub fn reconnect_cancel(engine: State<'_, Engine>) -> ApiResult<()> {
    engine.reconnect_cancel();
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn connection_state(engine: State<'_, Engine>) -> ApiResult<ConnectionState> {
    Ok(engine.connection_state().into())
}

/// Current snapshot, or `null` when not connected.
#[tauri::command]
#[specta::specta]
pub fn session_snapshot(engine: State<'_, Engine>) -> ApiResult<Option<ServerSnapshot>> {
    Ok(engine.snapshot().map(ServerSnapshot::from))
}

/// Live link figures, or `null` when not connected (AC-1.23).
#[tauri::command]
#[specta::specta]
pub fn connection_stats(engine: State<'_, Engine>) -> ApiResult<Option<ConnectionStats>> {
    Ok(engine.connection_stats().map(ConnectionStats::from))
}

/// Answers a `CertTrustRequired`; trusting is remembered for the server.
#[tauri::command]
#[specta::specta]
pub fn cert_trust_decide(
    engine: State<'_, Engine>,
    server_id: ServerId,
    fingerprint: String,
    trust: bool,
) -> ApiResult<()> {
    Ok(engine.cert_trust_decide(to_engine_server_id(server_id), &fingerprint, trust)?)
}

/// Unmuting while deafened also undeafens.
#[tauri::command]
#[specta::specta]
pub fn set_self_mute(engine: State<'_, Engine>, muted: bool) -> ApiResult<()> {
    Ok(engine.set_self_mute(muted)?)
}

/// Deafening also mutes; undeafening restores the previous mute state.
#[tauri::command]
#[specta::specta]
pub fn set_self_deaf(engine: State<'_, Engine>, deafened: bool) -> ApiResult<()> {
    Ok(engine.set_self_deaf(deafened)?)
}
