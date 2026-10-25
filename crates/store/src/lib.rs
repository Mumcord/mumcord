//! Local persistence for Mumcord: the SQLite database (servers, trust decisions,
//! followed channels, chat history, per-person settings, app settings), the user's
//! identity certificate and the OS secure storage for secrets.
//!
//! Secrets (the identity PKCS#12 and server passwords) only ever go to the OS secure
//! storage via [`SecretStore`]; nothing secret is written to SQLite or plain files.

mod db;
mod error;
mod favorites;
mod history;
mod identity;
mod secrets;
mod servers;
mod settings;
mod social;

pub use db::{DB_FILE_NAME, Store, UnixMillis};
pub use error::{Error, Result};
pub use favorites::{GifFavorite, MAX_GIF_FAVORITES};
pub use history::{ChatMessage, Conversation, NewChatMessage};
pub use identity::{Identity, cert_hash};
pub use secrets::{KEYRING_SERVICE, SecretStore};
pub use servers::{DEFAULT_PORT, NewServer, SavedServer, ServerId};
pub use settings::{
    ChatDisplay, CloseBehaviour, LogLevel, NotificationMode, OverlayBounds, OverlaySettings,
    OwnVoice, PositionalSettings, Settings, SoundSettings, TalkMode, ThemePreference,
    VoiceActivation,
};
pub use social::{FollowedChannel, PersonSetting};
