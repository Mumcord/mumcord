//! Local persistence for Mumcord: the SQLite database (servers, trust decisions,
//! followed channels, chat history, per-person settings, app settings), the user's
//! identity certificate and the OS secure storage for secrets.
//!
//! Secrets (the identity PKCS#12 and server passwords) only ever go to the OS secure
//! storage via [`SecretStore`]; nothing secret is written to SQLite or plain files.

mod channel_tokens;
mod db;
mod error;
mod favorites;
mod friends;
mod history;
mod identity;
mod secrets;
mod servers;
mod settings;
mod social;
mod synced;

pub use channel_tokens::{ChannelToken, TokenChannel};
pub use db::{DB_FILE_NAME, Store, UnixMillis};
pub use error::{Error, Result};
pub use favorites::{GifFavorite, MAX_GIF_FAVORITES};
pub use friends::Friend;
pub use history::{ChatMessage, Conversation, NewChatMessage};
pub use identity::{Identity, cert_hash};
pub use secrets::{KEYRING_SERVICE, SecretStore};
pub use servers::{DEFAULT_PORT, NewServer, SavedServer, ServerId};
pub use settings::{
    ChatDisplay, CloseBehaviour, Consent, DEFAULT_APP_THEME_ID, DEFAULT_INGAME_OPACITY_PERCENT,
    DEFAULT_INGAME_SIZE_PERCENT, DEFAULT_MAX_CONNECTED_SERVERS, GameKey, GameOverlayEntry,
    InGameOverlayMode, InGameOverlaySettings, LogLevel, MAX_MAX_CONNECTED_SERVERS,
    MAX_VIDEO_DUCKING_PERCENT, MIN_MAX_CONNECTED_SERVERS, MIN_VIDEO_DUCKING_PERCENT,
    NotificationMode, OverlayCorner, OwnVoice, PositionalSettings, Settings, SoundSettings,
    TalkMode, Theme, ThemePreference, VoiceActivation,
};
pub use social::{FollowedChannel, PersonSetting};
pub use synced::RangeSummary;
