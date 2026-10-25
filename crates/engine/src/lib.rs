//! Mumcord engine: app state, the server connection, voice and diagnostics. The app shell
//! (`src-tauri`) talks only to [`Engine`] and converts its types into command/event types.

pub mod chat;
pub mod diag;
pub mod follow;
pub mod friends;
pub mod full_image;
pub mod history_sync;
pub mod ingame_overlay;
mod maintenance;
pub mod mumcord;
pub mod now_gaming;
pub mod now_playing;
pub mod people;
pub mod positional;
pub mod profile;
pub mod session;
pub mod sounds;
pub mod voice;
pub mod web;

pub use chat::{ChatMessageView, HistoryScope};
pub use follow::FollowAllSummary;
pub use friends::{FriendPresence, FriendView};
pub use ingame_overlay::{
    BlockedGame, Blocklist, OverlayDecision, blocklist, decide as decide_overlay,
};
/// Position sources (Mumble Link, plugin host) as the engine uses them.
pub use mumcord_positional as positional_sources;
/// Stored types (settings record etc.) as the engine hands them out.
pub use mumcord_store as store;
pub use mumcord_store::{SecretStore, ServerId, Settings};
pub use now_gaming::{GameSource, LocalGame, NowGamingView};
pub use now_playing::{LocalTrack, NowPlayingView};
pub use positional::PositionalStatus;
pub use session::Engine;
pub use session::{
    AudioDeviceList, AudioDeviceView, ChannelPermissions, ChannelView, ConnState,
    ConnectionStatsView, ConversationRef, EngineError, EngineEvent, EngineResult, ErrorKind,
    Events, IdentityView, MAX_RECONNECT_ATTEMPTS, MicTestPhase, PasswordChange, ProfileView,
    ServerFields, ServerInfoView, ServerView, SessionCtx, Snapshot, UnreadView, UserView,
    VoiceTransportKind,
};
pub use web::{InlineImage, LinkPreview, VideoEmbed, VideoProvider};
