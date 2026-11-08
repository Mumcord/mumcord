//! Mumcord engine: app state, the server connection, voice and diagnostics. The app shell
//! (`src-tauri`) talks only to [`Engine`] and converts its types into command/event types.

pub mod acl_eval;
pub mod admin;
pub mod audio_rating;
pub mod banlist;
mod blocking;
pub mod cache;
pub mod camera;
pub mod channel_acl;
pub mod channel_admin;
pub mod chat;
pub mod crash;
pub mod diag;
pub mod files;
pub mod follow;
pub mod friends;
pub mod full_image;
pub mod history_sync;
pub mod ingame_overlay;
pub mod inspector;
pub mod latency;
mod maintenance;
pub mod mumble_import;
pub mod mumcord;
pub mod noise_backend;
pub mod now_gaming;
pub mod now_playing;
pub mod p2p;
pub mod peer;
pub mod people;
pub mod plain_text;
pub mod plugin_relay;
pub mod plugin_sounds;
pub mod plugins;
pub mod pm_overview;
pub mod pm_rules;
pub mod positional;
pub mod profile;
pub mod public_servers;
pub mod registered;
pub mod screen_share;
pub mod server_query;
pub mod session;
pub mod sounds;
pub mod stats;
pub mod sysinfo;
pub mod test_hooks;
pub mod voice;
pub mod watch_party;
pub mod web;
pub mod whisper;

pub use admin::{
    ActionOutcome, BanOutcome, BanRequest, MassMoveOutcome, ModerateAction, PersonView,
};
pub use banlist::{BanChange, BanSaveOutcome, BanView};
pub use channel_acl::{AclGroup, AclRule, AclSaveOutcome, ChannelAcl, PasswordState, password_of};
pub use channel_admin::{ChannelChange, ChannelCreate, ChannelOutcome};
pub use chat::{ChatMessageView, HistoryScope};
pub use follow::FollowAllSummary;
pub use friends::{FriendPresence, FriendView};
pub use ingame_overlay::{
    BlockedGame, Blocklist, OverlayDecision, blocklist, decide as decide_overlay,
};
pub use latency::LatencyEstimateView;
/// Position sources (Mumble Link, plugin host) as the engine uses them.
pub use mumcord_positional as positional_sources;
/// Stored types (settings record etc.) as the engine hands them out.
pub use mumcord_store as store;
pub use mumcord_store::{IdentityId, SecretStore, ServerId, Settings};
pub use now_gaming::{GameSource, LocalGame, NowGamingView};
pub use now_playing::{LocalTrack, NowPlayingView};
pub use pm_overview::PrivateConversationView;
pub use pm_rules::{MessageRequestView, PmRequestsView, SentChatRequestView};
pub use positional::PositionalStatus;
pub use registered::{RegisteredOutcome, RegisteredView};
pub use server_query::{QueryOutcome, ServerQueries, ServerQuery};
pub use session::Engine;
pub use session::{
    ActiveAudioProfile, AudioDeviceList, AudioDeviceView, AudioHealthView, CertificateState,
    ChannelAudioView, ChannelPasswordChannel, ChannelPasswordState, ChannelPasswordView,
    ChannelPermissions, ChannelView, ConnState, ConnectionStatsView, ConversationRef, EngineError,
    EngineEvent, EngineResult, ErrorKind, EventReceiver, Events, HistoryImageState, IdentityView,
    MAX_RECONNECT_ATTEMPTS, MicTestDevices, MicTestPhase, PasswordChange, ProfileView,
    ServerFields, ServerInfoView, ServerTestInput, ServerView, SessionCtx, Snapshot, TaggedEvent,
    TestConnectionView, UnreadView, UserView, VoiceTransportKind,
};
pub use session::{DenialKind, MumbleImportChoice, MumbleImportPreview, MumbleImportResult, Right};
pub use stats::UserStatsView;
pub use watch_party::{
    HISTORY_MAX, HistoryItem, PLAYLIST_MAX, PlaylistEntry, WatchAction, WatchCommand,
    WatchPartyView,
};
pub use web::{
    AudioFile, AudioFormat, AudioInfo, InlineImage, LinkPreview, MusicEmbed, MusicKind,
    MusicService, VideoEmbed, VideoProvider,
};
