//! Mumcord engine: app state, the server connection, voice and diagnostics. The app shell
//! (`src-tauri`) talks only to [`Engine`] and converts its types into command/event types.

pub mod audio_rating;
mod blocking;
pub mod cache;
pub mod chat;
pub mod diag;
pub mod follow;
pub mod friends;
pub mod full_image;
pub mod history_sync;
pub mod ingame_overlay;
pub mod inspector;
pub mod latency;
mod maintenance;
pub mod mumcord;
pub mod now_gaming;
pub mod now_playing;
pub mod p2p;
pub mod people;
pub mod pm_overview;
pub mod pm_rules;
pub mod positional;
pub mod profile;
pub mod screen_share;
pub mod server_query;
pub mod session;
pub mod sounds;
pub mod stats;
pub mod voice;
pub mod watch_party;
pub mod web;

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
pub use server_query::{QueryOutcome, ServerQueries, ServerQuery};
pub use session::Engine;
pub use session::{
    AudioDeviceList, AudioDeviceView, AudioHealthView, CertificateState, ChannelPasswordChannel,
    ChannelPasswordState, ChannelPasswordView, ChannelPermissions, ChannelView, ConnState,
    ConnectionStatsView, ConversationRef, EngineError, EngineEvent, EngineResult, ErrorKind,
    EventReceiver, Events, IdentityView, MAX_RECONNECT_ATTEMPTS, MicTestPhase, PasswordChange,
    ProfileView, ServerFields, ServerInfoView, ServerTestInput, ServerView, SessionCtx, Snapshot,
    TaggedEvent, TestConnectionView, UnreadView, UserView, VoiceTransportKind,
};
pub use stats::UserStatsView;
pub use watch_party::{PLAYLIST_MAX, PlaylistEntry, WatchAction, WatchCommand, WatchPartyView};
pub use web::{InlineImage, LinkPreview, VideoEmbed, VideoProvider};
