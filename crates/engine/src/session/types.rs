//! Engine-level data types handed to the app shell. The shell converts them into its own
//! command/event types (`src-tauri/src/commands`), so the engine stays free of UI framework code.

use std::fmt;

use crate::audio_rating::HealthTotals;
use mumcord_store::{IdentityId, ServerId, UnixMillis};

/// Error category; mirrors the command contract's `ErrorCode` (docs/engine-api.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NotConnected,
    ServerUnreachable,
    /// macOS reported no route to the server. Since macOS 15 that is also how a missing Local
    /// Network permission shows up, so the user is pointed to that setting.
    LocalNetworkBlocked,
    WrongPassword,
    NameInUse,
    CertificateRejected,
    CertificateChanged,
    ServerTooOld,
    PermissionDenied,
    ListenerLimitReached,
    /// As many servers are connected (or marked "Always connect at start") as the limit in
    /// the settings allows (FR-2.1, FR-2.7).
    ServerLimitReached,
    MessageTooLarge,
    InvalidInput,
    NotFound,
    /// The screen share already has the maximum of viewers (FR-2.43).
    ShareFull,
    /// Watching needs a direct connection and none can be made (FR-2.46); `message` is the
    /// reason, a [`crate::screen_share::WatchBlock::code`].
    DirectUnavailable,
    SecureStorageUnavailable,
    Io,
    Internal,
}

/// A failed engine call. `message` is technical detail or a server's denial text and never
/// contains secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineError {
    pub kind: ErrorKind,
    pub message: String,
}

impl EngineError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn not_connected() -> Self {
        Self::new(ErrorKind::NotConnected, "not connected")
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidInput, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for EngineError {}

pub type EngineResult<T> = Result<T, EngineError>;

/// Connection state machine of spec §7.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnState {
    /// `error` explains why the last connection ended unless it ended on request.
    Disconnected {
        error: Option<EngineError>,
    },
    Connecting {
        server_id: ServerId,
    },
    Connected {
        server_id: ServerId,
    },
    Reconnecting {
        server_id: ServerId,
        attempt: u32,
        max_attempts: u32,
        next_retry_secs: u32,
    },
}

impl ConnState {
    /// The server being connected to, if any.
    pub fn server_id(&self) -> Option<ServerId> {
        match self {
            Self::Disconnected { .. } => None,
            Self::Connecting { server_id }
            | Self::Connected { server_id }
            | Self::Reconnecting { server_id, .. } => Some(*server_id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelPermissions {
    pub enter: bool,
    pub text_message: bool,
    pub listen: bool,
    pub register_self: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelView {
    pub id: u32,
    pub parent_id: Option<u32>,
    pub name: String,
    pub position: i32,
    pub permissions: ChannelPermissions,
    /// Sanitized description HTML; `None` when there is none or only its hash is known yet.
    pub description: Option<String>,
    /// True when the channel has a description, even if its text still has to be requested
    /// (`Engine::channel_description_request`, FR-1.48b).
    pub has_description: bool,
    /// The channel's user limit; `None` when it has none (0 on the wire).
    pub max_users: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserView {
    pub session: u32,
    pub channel_id: u32,
    pub name: String,
    pub cert_hash: Option<String>,
    pub registered: bool,
    pub self_mute: bool,
    pub self_deaf: bool,
    /// Muted by the server/an admin.
    pub server_mute: bool,
    pub server_deaf: bool,
    /// May not speak in the current channel (no Speak permission or suppressed by an admin).
    pub suppressed: bool,
    pub listening_channel_ids: Vec<u32>,
    pub has_avatar: bool,
    /// Hex of the avatar texture hash, when the server sent one; changes when the avatar is replaced.
    pub avatar_hash: Option<String>,
    pub has_comment: bool,
    /// Local volume in percent (0–200).
    pub volume: u16,
    pub local_mute: bool,
    /// Features announced by another Mumcord client (FR-1.61); `None` for everyone else.
    pub mumcord_features: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfoView {
    pub version: String,
    pub release: Option<String>,
    pub os: Option<String>,
    pub follow_supported: bool,
    /// Sanitized HTML.
    pub welcome_text: Option<String>,
    /// The channel the server put the user in when connecting (its default channel, or the
    /// remembered one for registered users); the welcome text is pinned there too (FR-1.48b).
    pub default_channel_id: Option<u32>,
    pub max_message_length: u32,
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

/// A conversation as the UI addresses it (channel ids are per connection).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConversationRef {
    Channel { channel_id: u32 },
    Private { cert_hash: String },
    PrivateSession { session: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadView {
    pub conversation: ConversationRef,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub server_id: ServerId,
    pub self_session: u32,
    pub info: ServerInfoView,
    pub channels: Vec<ChannelView>,
    pub users: Vec<UserView>,
    pub followed_channel_ids: Vec<u32>,
    pub unread: Vec<UnreadView>,
    /// Self-muted and deafened by a fresh connect, until an explicit join or undeafen (QA.2).
    pub read_only: bool,
    /// Channel the user was joined to for voice when the app last quit, offered for
    /// rejoining on the first fresh connect after app start (QA.9).
    pub rejoin_channel_id: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceTransportKind {
    Udp,
    Tcp,
}

/// Live figures of the current connection for the connection info panel (AC-1.23) and the
/// diagnostic file. Counters start at zero with every (re)connection.
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionStatsView {
    /// `None` when the voice transport could not be started (no voice at all).
    pub transport: Option<VoiceTransportKind>,
    /// Smoothed UDP ping; `None` until measured and while tunnelling over TCP.
    pub rtt_ms: Option<f32>,
    /// Recent packet loss, 0–100.
    pub loss_percent: f32,
    /// Current encoder target set by the bitrate controller (FR-1.24).
    pub send_bitrate_bps: u32,
    /// The server's per-user bandwidth limit, if it announced one.
    pub server_max_bandwidth_bps: Option<u32>,
    pub packets_sent: u64,
    pub packets_received: u64,
    /// Health of the local audio path; `None` on a server without the voice pipeline.
    pub audio: Option<AudioHealthView>,
    /// Estimated mouth-to-ear latency (issue #156); `None` on a server without the voice
    /// pipeline.
    pub latency: Option<crate::latency::LatencyEstimateView>,
}

/// Glitch counters of the local audio path since the voice pipeline started (issue #123). A
/// rising count says where crackles or dropouts come from: the device, the threads feeding it
/// (scheduled too late), or the network (concealment).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct AudioHealthView {
    /// Under- or overruns the audio host reported for the devices.
    pub device_xruns: u64,
    /// Device streams reopened after an error.
    pub stream_restarts: u64,
    /// Captured audio dropped because the capture thread did not keep up.
    pub capture_overruns: u64,
    /// Playback buffers that ran dry part-way.
    pub playback_underruns: u64,
    /// Frames the capture thread processed with a backlog (it was scheduled late).
    pub capture_late_frames: u64,
    /// Times the playback thread woke up to a nearly empty device queue.
    pub playback_late_wakeups: u64,
    /// Received 10 ms frames rebuilt by FEC or concealment (late or lost packets).
    pub concealed_frames: u64,
    pub late_packets: u64,
    /// Jitter-buffer delay of the speakers playing (or of the last stream between bursts), in ms.
    pub jitter_delay_ms: u64,
    /// Average and worst capture processing time per 10 ms frame (DSP, Opus, send), in µs.
    pub capture_time_avg_us: u64,
    pub capture_time_max_us: u64,
    /// Average and worst mixing time per 10 ms frame (decode, concealment), in µs.
    pub mix_time_avg_us: u64,
    pub mix_time_max_us: u64,
    /// Voice threads (of capture and playback) that got real-time scheduling.
    pub realtime_threads: u32,
    /// 10 ms frames decoded from received voice (the base of the concealed share).
    pub received_frames: u64,
    /// Overall rating over the recent window (issue #152).
    pub rating: crate::audio_rating::AudioRatingView,
}

impl AudioHealthView {
    /// The counters the rating looks at, grouped by cause.
    pub fn totals(&self) -> HealthTotals {
        HealthTotals {
            dropouts: self.device_xruns
                + self.stream_restarts
                + self.capture_overruns
                + self.playback_underruns,
            late_frames: self.capture_late_frames + self.playback_late_wakeups,
            concealed_frames: self.concealed_frames,
            received_frames: self.received_frames,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDeviceView {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioDeviceList {
    pub inputs: Vec<AudioDeviceView>,
    pub outputs: Vec<AudioDeviceView>,
}

/// Public view of one identity; the certificate and its key stay in the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityView {
    pub id: IdentityId,
    /// Local label the user chose; the certificate is not changed by it.
    pub label: String,
    /// Common name inside the certificate (the default user name on servers).
    pub name: String,
    pub cert_hash: String,
    /// When the identity was added to Mumcord.
    pub added_at: UnixMillis,
    pub is_default: bool,
    /// Servers that use this identity directly (not through the default).
    pub server_ids: Vec<ServerId>,
}

/// A saved server as the UI sees it (the password itself never leaves the engine).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerView {
    pub id: ServerId,
    pub label: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub has_password: bool,
    pub save_history: bool,
    /// Connect this server when the app starts (FR-2.7).
    pub always_connect_at_start: bool,
    /// This was the voice server when the app was quit (FR-2.3).
    pub was_voice_server_at_quit: bool,
    /// No notifications from this server (FR-2.10).
    pub notifications_muted: bool,
    /// `@all` and `@channel` do not count as mentions on this server (issue #70).
    pub group_mentions_muted: bool,
    /// Theme for this server; `None` uses the app theme (FR-2.17).
    pub theme_id: Option<String>,
    /// Identity to connect with; `None` uses the default identity (issue #148).
    pub identity_id: Option<IdentityId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordChange {
    Keep,
    Clear,
    Set(String),
}

/// What the add/edit dialog sends for a "Test connection" (issue #151): the values as they are
/// in the form, so nothing has to be saved first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerTestInput {
    pub address: String,
    pub port: u16,
    pub username: String,
    /// `Keep` uses the saved password of `server_id`; `Set` uses the typed one.
    pub password: PasswordChange,
    /// Saved server the test belongs to; gives the saved password and the certificate decision.
    pub server_id: Option<ServerId>,
    /// Identity to try; `None` uses the default identity.
    pub identity_id: Option<IdentityId>,
}

/// Result of a test connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestConnectionView {
    /// Round trip of a Mumble ping over the control connection.
    pub ping_ms: u32,
    /// Version string the server reported, e.g. "1.5.0".
    pub version: String,
    /// SHA-256 fingerprint of the server certificate, as shown to users.
    pub fingerprint: String,
    /// How that certificate relates to the saved trust decision.
    pub certificate: CertificateState,
}

/// Trust state of a server certificate during a test connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertificateState {
    /// Matches the saved decision of the server.
    Trusted,
    /// No decision yet (also every new server).
    Unknown,
    /// Differs from the saved decision.
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerFields {
    pub label: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub password: PasswordChange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileView {
    /// Sanitized HTML.
    pub comment: Option<String>,
    /// `data:` URL.
    pub avatar: Option<String>,
    /// The user's private note about this person; empty when there is none (FR-1.63).
    pub note: String,
}

/// Everything the engine reports to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    StateChanged(ConnState),
    /// Voice moved to this server, or to none (FR-2.2).
    VoiceServerChanged(Option<ServerId>),
    CertTrustRequired {
        server_id: ServerId,
        fingerprint: String,
        previous_fingerprint: Option<String>,
    },
    ServerSynced(Box<Snapshot>),
    /// Channels received so far while the server is still syncing (FR-2.24).
    SyncProgress {
        channels: u32,
    },
    TransportChanged(VoiceTransportKind),
    ServerDenied {
        error: EngineError,
        channel_id: Option<u32>,
    },
    /// The channel-password dialog for a refused join changes (FR-2.78).
    ChannelPassword {
        channel_id: u32,
        state: super::ChannelPasswordState,
    },
    ChannelUpserted(ChannelView),
    /// Several channels changed at once, e.g. a burst of permission answers after a sync.
    ChannelsUpserted(Vec<ChannelView>),
    ChannelRemoved(u32),
    UserUpserted(UserView),
    UserRemoved(u32),
    TalkStateChanged {
        session: u32,
        talking: bool,
    },
    FollowedChannelsChanged(Vec<u32>),
    AudioDevicesChanged(AudioDeviceList),
    /// The chosen device is gone and the system default plays in its place (`input` is the
    /// microphone, otherwise the output). Sent once per switch, not on every poll.
    AudioDeviceFallback {
        input: bool,
    },
    /// The highest bitrate the bitrate slider can reach changed (FR-2.64); `None` without a
    /// voice server.
    BitrateMaxChanged(Option<u32>),
    /// Microphone level 0.0–1.0.
    MicLevel(f32),
    /// The mic test moved to a new phase (FR-1.4).
    MicTestPhaseChanged(MicTestPhase),
    /// Incoming channel, followed-channel or private message (own messages are only
    /// returned by the send call).
    MessageReceived(Box<crate::chat::ChatMessageView>),
    UnreadChanged(UnreadView),
    /// The default avatar could not be sent to this server because it cannot fit the
    /// server's image limit (FR-1.47).
    DefaultAvatarSkipped {
        server_id: ServerId,
    },
    /// A person (or the user, with the own session) started, changed or stopped sharing what they
    /// listen to (FR-1.58).
    NowPlayingChanged {
        session: u32,
        track: Option<crate::now_playing::NowPlayingView>,
    },
    /// A person (or the user, with the own session) started, changed or stopped sharing the
    /// game they play (FR-1.59).
    NowGamingChanged {
        session: u32,
        game: Option<crate::now_gaming::NowGamingView>,
    },
    /// Messages from another Mumcord user's history were added to a channel (FR-1.60), in
    /// send-time order; insert them by `sent_at` (ties by order here).
    HistorySynced {
        channel_id: u32,
        messages: Vec<crate::chat::ChatMessageView>,
    },
    /// A history sync of a channel started (`running`) or ended with `added` new messages.
    HistorySyncStatus {
        channel_id: u32,
        running: bool,
        added: u32,
    },
    /// A direct connection would be used while `allow_direct_connections` is still unset: ask
    /// the user once and answer with `Engine::p2p_consent_decide` (FR-2.36).
    DirectConsentRequired,
    /// Progress of a full-size image being fetched for the viewer (FR-1.60c): `received` of
    /// `total` bytes of the image with this reference.
    FullImageProgress {
        reference: String,
        received: u32,
        total: u32,
    },
    /// The friends list changed: a friend was added or removed, came online, moved, was
    /// renamed or went offline (FR-1.64). Online friends first.
    FriendsChanged(Vec<crate::friends::FriendView>),
    /// Held chat and message requests or sent chat requests of the server changed (issue #141).
    PmRequestsChanged(crate::pm_rules::PmRequestsView),
    /// The accepted private conversations of any own identity changed (issue #141).
    AcceptedConversationsChanged(Vec<mumcord_store::AcceptedConversation>),
    /// Pinned or muted private conversations changed (issue #173).
    PmConversationPrefsChanged(Vec<mumcord_store::PmConversationPrefs>),
    /// Positional audio was switched, or the game delivering positions changed (FR-1.43).
    PositionalStatusChanged(crate::positional::PositionalStatus),
    /// A person (or the user, with the own session) started or stopped sending positions
    /// (the "3D" icon, FR-1.72).
    PositionalSendingChanged {
        session: u32,
        sending: bool,
    },
    /// New UserStats of a person on the server (FR-2.25).
    UserStatsChanged(crate::stats::UserStatsView),
    /// The watch party in the user's channel started, changed or ended (`None`) (FR-2.39).
    WatchPartyChanged(Option<crate::watch_party::WatchPartyView>),
    /// The screen shares in the user's channel changed; the full list (FR-2.42 – FR-2.46).
    ScreenSharesChanged(Vec<crate::screen_share::ScreenShareView>),
    /// The identity chosen for this server is gone, so the connect uses the default one
    /// (issue #148).
    ServerIdentityMissing {
        server_id: ServerId,
    },
}

/// Phase of the mic test: record, play back, finished or failed to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicTestPhase {
    Recording,
    Playing,
    Done,
    Failed,
}
