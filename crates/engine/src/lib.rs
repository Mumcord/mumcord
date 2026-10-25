//! Mumcord engine: app state, the server connection, voice and diagnostics. The app shell
//! (`src-tauri`) talks only to [`Engine`] and converts its types into command/event types.

pub mod chat;
pub mod diag;
pub mod follow;
mod maintenance;
pub mod mumcord;
pub mod now_gaming;
pub mod now_playing;
pub mod people;
pub mod profile;
pub mod session;
pub mod sounds;
pub mod voice;
pub mod web;

pub use chat::{ChatMessageView, HistoryScope};
pub use follow::FollowAllSummary;
/// Stored types (settings record etc.) as the engine hands them out.
pub use mumcord_store as store;
pub use mumcord_store::{SecretStore, ServerId, Settings};
pub use now_gaming::{GameSource, LocalGame, NowGamingView};
pub use now_playing::{LocalTrack, NowPlayingView};
pub use session::Engine;
pub use session::{
    AudioDeviceList, AudioDeviceView, ChannelPermissions, ChannelView, ConnState,
    ConnectionStatsView, ConversationRef, EngineError, EngineEvent, EngineResult, ErrorKind,
    Events, IdentityView, MAX_RECONNECT_ATTEMPTS, MicTestPhase, PasswordChange, ProfileView,
    ServerFields, ServerInfoView, ServerView, SessionCtx, Snapshot, UnreadView, UserView,
    VoiceTransportKind,
};
pub use web::{InlineImage, LinkPreview, VideoEmbed, VideoProvider};
