//! Local persistence for Mumcord: the SQLite database (servers, identities, trust decisions,
//! followed channels, chat history, per-person settings, app settings), the user's identity
//! certificates and the OS secure storage for secrets.
//!
//! Secrets (the identity PKCS#12 and server passwords) only ever go to the OS secure
//! storage via [`SecretStore`]; nothing secret is written to SQLite or plain files.
//! On the desktop platforms [`SecretStore::os`] uses the OS store; on Android and iOS the
//! embedding app implements [`SecretBackend`] on top of the phone's secure storage and passes
//! it to [`SecretStore::with_backend`].

mod accepted;
mod blocked;
mod camera_backgrounds;
mod channel_audio;
mod channel_tokens;
mod db;
mod encoder_cache;
mod error;
mod favorites;
mod file_cards;
mod friends;
mod history;
mod identities;
mod identity;
mod plugin_trust;
mod plugins;
mod pm_overview;
mod relay_pin;
mod secrets;
mod servers;
mod settings;
mod settings_compat;
mod social;
mod synced;
pub mod transfer;

pub use accepted::AcceptedConversation;
pub use blocked::BlockedUser;
pub use camera_backgrounds::{
    CameraBackgrounds, MAX_BACKGROUND_FILE_BYTES, MAX_BACKGROUND_PX, OwnBackground,
};
pub use channel_audio::{ChannelAudioProfile, SavedChannelAudio, resolve_channel_audio};
pub use channel_tokens::{ChannelToken, TokenChannel};
pub use db::{DB_FILE_NAME, Rollback, Store, UnixMillis, check_schema, roll_back, rollback_plan};
pub use encoder_cache::{EncoderCache, EncoderEntry, SvtSelftest};
pub use error::{Error, Result, SecretBackendError};
pub use favorites::{GifFavorite, MAX_GIF_FAVORITES};
pub use file_cards::FileCardRecord;
pub use friends::Friend;
pub use history::{ChatMessage, Conversation, NewChatMessage};
pub use identities::{IdentityId, IdentityRecord, MAX_IDENTITY_LABEL, NewIdentity};
pub use identity::{Identity, cert_hash};
pub use plugin_trust::{OriginApproval, OriginCounters, VersionHistoryEntry};
pub use plugins::{
    MAX_PLUGIN_KEY_BYTES, MAX_PLUGIN_REPOSITORIES, MAX_PLUGIN_VALUE_BYTES, NewPluginRepository,
    PluginRecord, PluginRepository, PluginSuggestion, PluginVisibility, REMOVED_DATA_RETENTION_MS,
};
pub use pm_overview::{MUTED_FOREVER, PmConversationPrefs, PrivateConversationSummary};
pub use relay_pin::RelayPin;
pub use secrets::{KEYRING_SERVICE, SecretBackend, SecretStore};
pub use servers::{DEFAULT_PORT, NewServer, SavedServer, ServerId};
pub use settings::{
    ActivityNotifySettings, BlockedMessages, CameraBackground, CameraEffectQuality,
    CameraReduceMotion, CameraSendQuality, CameraShow, CameraStripPosition, ChatDisplay,
    CloseBehaviour, Consent, DEFAULT_APP_THEME_ID, DEFAULT_INGAME_OPACITY_PERCENT,
    DEFAULT_INGAME_SIZE_PERCENT, DEFAULT_MAX_CONNECTED_SERVERS, FilesMaxSize, GameKey,
    GameOverlayEntry, Hotkey, HotkeyAction, InGameOverlayMode, InGameOverlaySettings, LogLevel,
    MAX_CAMERA_BLUR_STRENGTH, MAX_MAX_CONNECTED_SERVERS, MAX_UPDATE_INTERVAL_HOURS,
    MAX_VIDEO_DUCKING_PERCENT, MIN_MAX_CONNECTED_SERVERS, MIN_UPDATE_INTERVAL_HOURS,
    MIN_VIDEO_DUCKING_PERCENT, NoiseBackend, NotificationMode, OverlayCorner, OwnVoice, P2pMode,
    PluginUpdates, PmRule, PositionalSettings, PrivateMessageSettings, ScreenCodec, ScreenFps,
    ScreenOptimize, ScreenResolution, ScreenUploadLimit, Settings, SetupPreset, SoundSettings,
    SoundTheme, TalkMode, Theme, ThemePreference, UpdateChannel, VoiceActivation, WhisperReceivers,
    WhisperTarget,
};
pub use social::{FollowedChannel, PersonSetting};
pub use synced::RangeSummary;
