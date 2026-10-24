//! Engine-level data types handed to the app shell. The shell converts them into its own
//! command/event types (`src-tauri/src/commands`), so the engine stays free of UI framework code.

use std::fmt;

use mumcord_store::ServerId;

/// Error category; mirrors the command contract's `ErrorCode` (docs/engine-api.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NotConnected,
    ServerUnreachable,
    WrongPassword,
    NameInUse,
    CertificateRejected,
    CertificateChanged,
    ServerTooOld,
    PermissionDenied,
    ListenerLimitReached,
    MessageTooLarge,
    InvalidInput,
    NotFound,
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
    pub server_mute: bool,
    pub server_deaf: bool,
    pub listening_channel_ids: Vec<u32>,
    pub has_avatar: bool,
    /// Hex of the avatar texture hash, when the server sent one; changes when the avatar is replaced.
    pub avatar_hash: Option<String>,
    pub has_comment: bool,
    /// Local volume in percent (0–200).
    pub volume: u16,
    pub local_mute: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfoView {
    pub version: String,
    pub follow_supported: bool,
    /// Sanitized HTML.
    pub welcome_text: Option<String>,
    pub max_message_length: u32,
    pub max_image_message_length: u32,
    pub allow_html: bool,
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

/// Public view of the identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityView {
    pub name: String,
    pub cert_hash: String,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordChange {
    Keep,
    Clear,
    Set(String),
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
}

/// Everything the engine reports to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    StateChanged(ConnState),
    CertTrustRequired {
        server_id: ServerId,
        fingerprint: String,
        previous_fingerprint: Option<String>,
    },
    ServerSynced(Box<Snapshot>),
    TransportChanged(VoiceTransportKind),
    ServerDenied {
        error: EngineError,
        channel_id: Option<u32>,
    },
    ChannelUpserted(ChannelView),
    ChannelRemoved(u32),
    UserUpserted(UserView),
    UserRemoved(u32),
    TalkStateChanged {
        session: u32,
        talking: bool,
    },
    FollowedChannelsChanged(Vec<u32>),
    AudioDevicesChanged(AudioDeviceList),
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
}

/// Phase of the mic test: record, play back, finished or failed to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicTestPhase {
    Recording,
    Playing,
    Done,
    Failed,
}
