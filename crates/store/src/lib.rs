//! Local persistence for Mumcord: the SQLite database (servers, identities, trust decisions,
//! followed channels, chat history, per-person settings, app settings), the user's identity
//! certificates and the OS secure storage for secrets.
//!
//! Secrets (the identity PKCS#12 and server passwords) only ever go to the OS secure
//! storage via [`SecretStore`]; nothing secret is written to SQLite or plain files.

mod accepted;
mod blocked;
mod channel_audio;
mod channel_tokens;
mod db;
mod error;
mod favorites;
mod friends;
mod history;
mod identities;
mod identity;
mod pm_overview;
mod relay_pin;
mod secrets;
mod servers;
mod settings;
mod social;
mod synced;

pub use accepted::AcceptedConversation;
pub use blocked::BlockedUser;
pub use channel_audio::{ChannelAudioProfile, SavedChannelAudio, resolve_channel_audio};
pub use channel_tokens::{ChannelToken, TokenChannel};
pub use db::{DB_FILE_NAME, Store, UnixMillis};
pub use error::{Error, Result};
pub use favorites::{GifFavorite, MAX_GIF_FAVORITES};
pub use friends::Friend;
pub use history::{ChatMessage, Conversation, NewChatMessage};
pub use identities::{IdentityId, IdentityRecord, MAX_IDENTITY_LABEL, NewIdentity};
pub use identity::{Identity, cert_hash};
pub use pm_overview::{MUTED_FOREVER, PmConversationPrefs, PrivateConversationSummary};
pub use relay_pin::RelayPin;
pub use secrets::{KEYRING_SERVICE, SecretStore};
pub use servers::{DEFAULT_PORT, NewServer, SavedServer, ServerId};
pub use settings::{
    BlockedMessages, ChatDisplay, CloseBehaviour, Consent, DEFAULT_APP_THEME_ID,
    DEFAULT_INGAME_OPACITY_PERCENT, DEFAULT_INGAME_SIZE_PERCENT, DEFAULT_MAX_CONNECTED_SERVERS,
    GameKey, GameOverlayEntry, InGameOverlayMode, InGameOverlaySettings, LogLevel,
    MAX_MAX_CONNECTED_SERVERS, MAX_VIDEO_DUCKING_PERCENT, MIN_MAX_CONNECTED_SERVERS,
    MIN_VIDEO_DUCKING_PERCENT, NotificationMode, OverlayCorner, OwnVoice, P2pMode, PmRule,
    PositionalSettings, PrivateMessageSettings, Settings, SoundSettings, TalkMode, Theme,
    ThemePreference, VoiceActivation,
};
pub use social::{FollowedChannel, PersonSetting};
pub use synced::RangeSummary;
