//! Connection lifecycle, certificate trust, server snapshot and self mute/deafen
//! (FR-1.9, FR-1.10, FR-1.12–1.14, FR-1.22, FR-1.26).

use mumcord_engine::{
    AudioHealthView, ConnState, ConnectionStatsView, Engine, ServerInfoView, Snapshot, UnreadView,
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
    pub release: Option<String>,
    pub os: Option<String>,
    /// False on servers older than 1.4: following is hidden with a hint (FR-1.12).
    pub follow_supported: bool,
    /// Sanitized HTML.
    pub welcome_text: Option<String>,
    /// Channel the server put the user in on connect; its chat also shows the welcome text
    /// when the channel has no description (FR-1.48b).
    pub default_channel_id: Option<super::types::ChannelId>,
    /// Maximum text message length in characters (0 = unlimited).
    pub max_message_length: u32,
    /// Maximum length of messages containing images in characters (0 = unlimited).
    pub max_image_message_length: u32,
    pub allow_html: bool,
    pub max_bandwidth: Option<u32>,
    pub max_users: Option<u32>,
    pub recording_allowed: bool,
    pub server_label: Option<String>,
    pub server_address: Option<String>,
    pub server_port: Option<u16>,
    pub tls_fingerprint: Option<String>,
    pub users_online: u32,
    pub session_id: Option<u32>,
    pub registered: bool,
    pub current_channel_permissions: Option<u32>,
    pub mumcord_users: u32,
    pub tls_trusted: bool,
    pub tls_issuer: Option<String>,
    pub tls_valid_from: Option<String>,
    pub tls_valid_until: Option<String>,
    pub tls_certificate_pem: Option<String>,
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
    /// Self-muted and deafened by a fresh connect, until an explicit join or undeafen.
    pub read_only: bool,
    /// Channel the user was joined to for voice when the app last quit; offer "Rejoin?"
    /// (only on the first fresh connect to this server after app start).
    pub rejoin_channel_id: Option<ChannelId>,
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
    /// Health of the local audio path; `null` on a server without voice.
    pub audio: Option<AudioHealth>,
}

/// Overall audio rating over the recent window (issue #152).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum AudioRating {
    Good,
    Fair,
    Poor,
}

impl From<mumcord_engine::audio_rating::AudioRating> for AudioRating {
    fn from(r: mumcord_engine::audio_rating::AudioRating) -> Self {
        use mumcord_engine::audio_rating::AudioRating as R;
        match r {
            R::Good => Self::Good,
            R::Fair => Self::Fair,
            R::Poor => Self::Poor,
        }
    }
}

/// The most likely cause of a Fair or Poor rating.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum AudioIssue {
    /// Voice from others was concealed: packets lost or late on the way.
    Network,
    /// The audio threads ran late or processing was slow: high CPU load.
    Cpu,
    /// The audio device reported dropouts.
    Device,
}

impl From<mumcord_engine::audio_rating::AudioIssue> for AudioIssue {
    fn from(i: mumcord_engine::audio_rating::AudioIssue) -> Self {
        use mumcord_engine::audio_rating::AudioIssue as I;
        match i {
            I::Network => Self::Network,
            I::Cpu => Self::Cpu,
            I::Device => Self::Device,
        }
    }
}

/// A rating with its cause, for compact places such as the packet inspector.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct AudioRatingLabel {
    pub rating: AudioRating,
    /// `null` while the rating is good.
    pub issue: Option<AudioIssue>,
}

/// Glitch counters of the local audio path since voice started on this connection (issue #123),
/// with the rating over the recent window (issue #152).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct AudioHealth {
    /// Device problems: xruns, stream restarts, capture overruns and playback underruns.
    pub dropouts: u32,
    /// Frames the capture or playback thread handled too late (it was not scheduled in time).
    pub late_frames: u32,
    /// Received frames rebuilt by concealment because packets came late or not at all.
    pub concealed_frames: u32,
    /// Jitter-buffer delay of the people talking, or of the last one between bursts.
    pub jitter_buffer_ms: u32,
    /// Average and worst capture processing time per 10 ms frame, in µs.
    pub processing_avg_us: u32,
    pub processing_max_us: u32,
    /// Whether the voice threads run with real-time scheduling.
    pub realtime: bool,
    pub rating: AudioRating,
    /// `null` while the rating is good.
    pub issue: Option<AudioIssue>,
    /// Length of the recent window the `recent*` figures cover, in seconds.
    pub window_secs: u32,
    pub recent_dropouts: u32,
    pub recent_late_frames: u32,
    pub recent_concealed_frames: u32,
    /// Received voice frames in the window (the base of the concealed share).
    pub recent_received_frames: u32,
    pub recent_processing_max_us: u32,
    /// Per 10 s bucket over the window, oldest first, for sparklines.
    pub dropouts_history: Vec<u32>,
    pub late_frames_history: Vec<u32>,
    pub concealed_history: Vec<u32>,
    pub processing_max_history_us: Vec<u32>,
}

impl From<AudioHealthView> for AudioHealth {
    fn from(a: AudioHealthView) -> Self {
        let count = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
        Self {
            dropouts: count(
                a.device_xruns + a.stream_restarts + a.capture_overruns + a.playback_underruns,
            ),
            late_frames: count(a.capture_late_frames + a.playback_late_wakeups),
            concealed_frames: count(a.concealed_frames),
            jitter_buffer_ms: count(a.jitter_delay_ms),
            processing_avg_us: count(a.capture_time_avg_us),
            processing_max_us: count(a.capture_time_max_us),
            realtime: a.realtime_threads > 0,
            rating: a.rating.rating.into(),
            issue: a.rating.issue.map(Into::into),
            window_secs: a.rating.window_secs,
            recent_dropouts: count(a.rating.dropouts),
            recent_late_frames: count(a.rating.late_frames),
            recent_concealed_frames: count(a.rating.concealed_frames),
            recent_received_frames: count(a.rating.received_frames),
            recent_processing_max_us: count(a.rating.processing_max_us),
            dropouts_history: a.rating.dropouts_history.to_vec(),
            late_frames_history: a.rating.late_frames_history.to_vec(),
            concealed_history: a.rating.concealed_history.to_vec(),
            processing_max_history_us: a.rating.processing_max_history_us.to_vec(),
        }
    }
}

/// The voice server changed (FR-2.2); `null` when no server has voice.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct VoiceServerChanged {
    pub server_id: Option<ServerId>,
}

/// One server's connection state, as listed by `connections_list`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ServerConnection {
    pub server_id: ServerId,
    pub state: ConnectionState,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStateChanged {
    pub server_id: ServerId,
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

/// Channels received so far while a server is still syncing (FR-2.24).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub server_id: ServerId,
    pub channels: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct TransportChanged {
    pub server_id: ServerId,
    pub transport: Transport,
}

/// The server denied an action asynchronously (PermissionDenied message).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ServerDenied {
    pub server_id: ServerId,
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
            release: v.release,
            os: v.os,
            follow_supported: v.follow_supported,
            welcome_text: v.welcome_text,
            default_channel_id: v.default_channel_id,
            max_message_length: v.max_message_length,
            max_image_message_length: v.max_image_message_length,
            allow_html: v.allow_html,
            max_bandwidth: v.max_bandwidth,
            max_users: v.max_users,
            recording_allowed: v.recording_allowed,
            server_label: v.server_label,
            server_address: v.server_address,
            server_port: v.server_port,
            tls_fingerprint: v.tls_fingerprint,
            users_online: v.users_online,
            session_id: v.session_id,
            registered: v.registered,
            current_channel_permissions: v.current_channel_permissions,
            mumcord_users: v.mumcord_users,
            tls_trusted: v.tls_trusted,
            tls_issuer: v.tls_issuer,
            tls_valid_from: v.tls_valid_from,
            tls_valid_until: v.tls_valid_until,
            tls_certificate_pem: v.tls_certificate_pem,
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
            read_only: s.read_only,
            rejoin_channel_id: s.rejoin_channel_id,
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
            audio: v.audio.map(AudioHealth::from),
        }
    }
}

/// Connects to a saved server next to the connected ones (FR-2.1, FR-2.6), read-only.
/// Errors: `serverLimitReached` when `maxConnectedServers` servers are connected already.
/// `channel_path` (from a `mumble://` link) joins that channel and moves voice there; on an
/// already connected server only that join happens. `password` replaces the saved password for
/// this connection only (retry after `wrongPassword`); it is never stored or logged.
#[tauri::command]
#[specta::specta]
pub fn connect(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_path: Option<Vec<String>>,
    password: Option<String>,
) -> ApiResult<()> {
    Ok(engine.connect_with_password(to_engine_server_id(server_id), channel_path, password)?)
}

/// Disconnects the server, also cancelling any pending reconnect. A server that is not
/// connected is left alone.
#[tauri::command]
#[specta::specta]
pub fn disconnect(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<()> {
    engine.disconnect(to_engine_server_id(server_id));
    Ok(())
}

/// Stops automatic reconnecting of the server; its state becomes `disconnected`.
#[tauri::command]
#[specta::specta]
pub fn reconnect_cancel(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<()> {
    engine.reconnect_cancel(to_engine_server_id(server_id));
    Ok(())
}

/// Connection state of every server that is not disconnected (empty when none is).
#[tauri::command]
#[specta::specta]
pub fn connections_list(engine: State<'_, Engine>) -> ApiResult<Vec<ServerConnection>> {
    Ok(engine
        .connections()
        .into_iter()
        .filter_map(|state| {
            state.server_id().map(|id| ServerConnection {
                server_id: to_api_server_id(id),
                state: state.into(),
            })
        })
        .collect())
}

/// The server that has voice, or `null` when every connected server is read-only.
#[tauri::command]
#[specta::specta]
pub fn voice_server_get(engine: State<'_, Engine>) -> ApiResult<Option<ServerId>> {
    Ok(engine.voice_server().map(to_api_server_id))
}

/// Current snapshot of the server, or `null` when it is not connected.
#[tauri::command]
#[specta::specta]
pub fn session_snapshot(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> ApiResult<Option<ServerSnapshot>> {
    Ok(engine
        .snapshot_of(to_engine_server_id(server_id))
        .map(ServerSnapshot::from))
}

/// Live link figures of the server, or `null` when it is not connected (AC-1.23).
#[tauri::command]
#[specta::specta]
pub fn connection_stats(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> ApiResult<Option<ConnectionStats>> {
    Ok(engine
        .connection_stats(to_engine_server_id(server_id))
        .map(ConnectionStats::from))
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
