//! The single app settings record. Every field has a serde default so settings saved
//! by an older version load cleanly and new fields need no database migration.

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::db::Store;
use crate::error::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    /// Follow the OS setting (FR-1.35).
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TalkMode {
    #[default]
    VoiceActivation,
    PushToTalk,
}

/// How voice activation decides to open the mic (FR-1.57, QA.39).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceActivation {
    /// Speech detection (default).
    #[default]
    Automatic,
    /// Opens when the input level reaches `voice_threshold_dbfs`.
    Manual,
}

/// Default manual threshold in dBFS.
pub const DEFAULT_VOICE_THRESHOLD_DBFS: i32 = -40;
/// Default for `Settings::preview_cache_days` (QA.42).
pub const DEFAULT_PREVIEW_CACHE_DAYS: u32 = 7;

/// What closing the main window does while connected (FR-1.38).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehaviour {
    #[default]
    Ask,
    MinimizeToTray,
    Quit,
}

/// How chat messages are laid out (FR-1.48a, QA.25).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatDisplay {
    /// Avatar per message group, name and time above the indented text.
    #[default]
    Cozy,
    /// No avatars; time, name and text on one line.
    Compact,
}

/// Detail of the local log files (FR-1.40).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
}

/// Which message notifications are shown (FR-1.48c, QA.16).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationMode {
    /// In-app toasts only, while the window has focus.
    InApp,
    /// System notifications only, also while the window has focus.
    Desktop,
    /// Toasts while the window has focus, system notifications otherwise.
    #[default]
    Both,
    Off,
}

/// Per-event sound toggles (FR-1.27).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundSettings {
    pub user_joined: bool,
    pub user_left: bool,
    pub self_mute: bool,
    pub self_deafen: bool,
    /// New message in a followed channel or private chat.
    pub new_message: bool,
    /// A message mentions you: the "ping" in place of the message sound (QA.41).
    pub mention: bool,
    /// A mention gets its own highlighted notification (QA.41).
    pub mention_notify: bool,
    /// You join a channel for voice yourself (QA.23).
    pub voice_join: bool,
    /// You disconnect from a server yourself (QA.19).
    pub hang_up: bool,
    /// Someone starts a watch party in your channel (FR-2.77).
    pub party_started: bool,
    /// Someone joins the watch party (FR-2.77).
    pub party_joined: bool,
    /// Someone leaves the watch party (FR-2.77).
    pub party_left: bool,
    /// Someone starts a screen share in your channel (FR-2.77).
    pub share_started: bool,
    /// A screen share in your channel ends (FR-2.77).
    pub share_stopped: bool,
    /// Someone starts watching your screen share (FR-2.77).
    pub viewer_started: bool,
    /// Someone stops watching your screen share (FR-2.77).
    pub viewer_stopped: bool,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            user_joined: true,
            user_left: true,
            self_mute: true,
            self_deafen: true,
            new_message: true,
            mention: true,
            mention_notify: true,
            voice_join: true,
            hang_up: true,
            party_started: true,
            party_joined: true,
            party_left: true,
            share_started: true,
            share_stopped: true,
            viewer_started: true,
            viewer_stopped: true,
        }
    }
}

/// Screen corner the in-game overlay sits in (FR-1.46b).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayCorner {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Global switch of the in-game overlay (FR-1.46b, QA.55).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InGameOverlayMode {
    /// Never active: the Linux layer is not registered, nothing is loaded into games.
    #[default]
    Off,
    /// Active in every game unless switched off for that game (or the game is on the bundled
    /// blocklist and was not explicitly confirmed).
    On,
}

/// How a game is recognised: its Steam app id when Steam started it, else its executable name
/// (compared without regard to case).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum GameKey {
    Steam(u32),
    Exe(String),
}

impl GameKey {
    /// Whether this key names the same game, executable names compared ignoring case.
    pub fn matches(&self, other: &GameKey) -> bool {
        match (self, other) {
            (GameKey::Steam(a), GameKey::Steam(b)) => a == b,
            (GameKey::Exe(a), GameKey::Exe(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        }
    }
}

/// Default in-game overlay size and opacity in percent.
pub const DEFAULT_INGAME_SIZE_PERCENT: u8 = 100;
pub const DEFAULT_INGAME_OPACITY_PERCENT: u8 = 90;

/// Options of the in-game overlay for one game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameOverlayEntry {
    pub key: GameKey,
    /// Display name (from the game list or the process); plain text.
    pub name: String,
    /// `None` follows the global mode, `Some` overrides it for this game.
    pub enabled: Option<bool>,
    /// The user confirmed the stronger warning for a game on the blocklist; without it a
    /// blocklisted game stays off even when `enabled` is `Some(true)` (QA.56).
    pub risk_confirmed: bool,
    pub corner: OverlayCorner,
    /// Percent of the default size, 50–200.
    pub size_percent: u8,
    /// Percent, 20–100.
    pub opacity_percent: u8,
    pub show_whole_channel: bool,
    pub show_channel_name: bool,
}

impl Default for GameOverlayEntry {
    fn default() -> Self {
        Self {
            key: GameKey::Exe(String::new()),
            name: String::new(),
            enabled: None,
            risk_confirmed: false,
            corner: OverlayCorner::TopLeft,
            size_percent: DEFAULT_INGAME_SIZE_PERCENT,
            opacity_percent: DEFAULT_INGAME_OPACITY_PERCENT,
            show_whole_channel: false,
            show_channel_name: true,
        }
    }
}

/// In-game overlay options (FR-1.46b).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InGameOverlaySettings {
    pub mode: InGameOverlayMode,
    /// Games with their own options; at most one entry per game.
    pub games: Vec<GameOverlayEntry>,
}

impl InGameOverlaySettings {
    /// The entry for `key`, if the game has one.
    pub fn entry(&self, key: &GameKey) -> Option<&GameOverlayEntry> {
        self.games.iter().find(|g| g.key.matches(key))
    }
}

/// Positional audio options (FR-1.41).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PositionalSettings {
    pub enabled: bool,
    /// Metres; voices are at full volume up to this distance.
    pub min_distance: f32,
    /// Metres; voices are fully attenuated beyond this distance.
    pub max_distance: f32,
}

impl Default for PositionalSettings {
    // Same defaults as the official Mumble client.
    fn default() -> Self {
        Self {
            enabled: false,
            min_distance: 1.0,
            max_distance: 15.0,
        }
    }
}

/// Answer to a question asked once; `Unset` means the user was not asked yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Consent {
    #[default]
    Unset,
    Yes,
    No,
}

/// A custom colour theme (FR-2.14, FR-2.15) with a dark and a light version. Each colour map
/// holds the seven keys background, surface, text, accent, talking, warning and error as
/// lowercase `#rrggbb`; the UI validates them and generates a missing version, so the store only
/// keeps what it is given. Built-in presets live in the UI and are not stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    pub id: String,
    pub name: String,
    /// The single colour set of themes saved before themes had two versions. The UI reads it as
    /// the version matching its brightness; saving the theme again writes the two sets instead.
    #[serde(default, alias = "colours", skip_serializing_if = "BTreeMap::is_empty")]
    pub colors: BTreeMap<String, String>,
    #[serde(default, rename = "colours_dark", alias = "colors_dark")]
    pub colors_dark: BTreeMap<String, String>,
    #[serde(default, rename = "colours_light", alias = "colors_light")]
    pub colors_light: BTreeMap<String, String>,
    /// True while that version was generated from the other one and not edited by hand.
    #[serde(default)]
    pub light_generated: bool,
    #[serde(default)]
    pub dark_generated: bool,
}

/// Id of the built-in theme used until the user picks another one.
pub const DEFAULT_APP_THEME_ID: &str = "mumcord-dark";
/// Default and bounds of the number of simultaneously connected servers (FR-2.1).
pub const DEFAULT_MAX_CONNECTED_SERVERS: u8 = 5;
pub const MIN_MAX_CONNECTED_SERVERS: u8 = 1;
pub const MAX_MAX_CONNECTED_SERVERS: u8 = 15;
/// Range and default of the lowered video level while people talk, in percent (FR-2.76).
pub const MIN_VIDEO_DUCKING_PERCENT: u8 = 10;
pub const MAX_VIDEO_DUCKING_PERCENT: u8 = 80;
pub const DEFAULT_VIDEO_DUCKING_PERCENT: u8 = 30;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The first-start wizard was finished (FR-1.1).
    pub wizard_completed: bool,
    pub theme: ThemePreference,
    /// UI language as ISO 639-1 code (one of the 24 official EU languages); `None` follows the OS
    /// language. Kept as a plain string so unknown codes never break loading the settings file.
    pub language: Option<String>,
    pub talk_mode: TalkMode,
    pub voice_activation: VoiceActivation,
    /// Manual voice-activation threshold in dBFS, -60 to 0 (FR-1.57). Used in manual mode only.
    pub voice_threshold_dbfs: i32,
    pub ptt_key: Option<String>,
    /// Linux Wayland: read mouse buttons for push-to-talk from input devices (needs the
    /// `input` group; opt-in, FR-1.39, QA.4).
    pub allow_mouse_buttons: bool,
    /// Linux Wayland: start through XWayland and use the X11 push-to-talk path
    /// (FR-1.48, QA.5). Read before the window system starts; applies on restart.
    pub ptt_compatibility_mode: bool,
    /// Device names; `None` uses the system default device.
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub noise_suppression: bool,
    pub echo_cancellation: bool,
    pub auto_gain: bool,
    /// "Music quality" (Settings → Audio, FR-1.24, QA.37): the bitrate cap rises from 96 kbit/s to
    /// the server limit (at most 510 kbit/s), the encoder is tuned for music and noise
    /// suppression and automatic gain are bypassed. Off by default.
    pub music_quality: bool,
    /// "Low-latency mode" (Settings → Audio, issue #156) for playing music together: 10 ms
    /// low-delay Opus packets, a minimal jitter buffer, small device buffers, and echo
    /// cancellation, noise suppression and automatic gain bypassed. Off by default.
    pub low_latency: bool,
    /// Own bitrate cap in kbit/s (Settings → Audio, FR-2.64), independent of `music_quality`;
    /// `None` is "Max": the highest bitrate the server and the packet size allow. Defaults to
    /// 96 kbit/s (lower if the server allows less). Automatic lowering on packet loss works below it.
    pub bitrate_kbps: Option<u32>,
    /// "Lower video sound while people talk" (Settings → Audio, FR-2.76): the watch-party video
    /// and watched screen-share sound drop to `video_ducking_percent` while someone in the own
    /// voice channel talks. On by default.
    pub video_ducking: bool,
    /// The lowered level in percent of the normal volume, 10–80 (default 30). Clamped on load.
    pub video_ducking_percent: u8,
    pub sounds: SoundSettings,
    pub notifications: NotificationMode,
    pub close_behaviour: CloseBehaviour,
    pub chat_display: ChatDisplay,
    pub log_level: LogLevel,
    /// In-game overlay drawn inside games (FR-1.46b).
    pub ingame_overlay: InGameOverlaySettings,
    pub positional: PositionalSettings,
    /// Identifiers of individually enabled game plugins (FR-1.42).
    pub enabled_plugins: Vec<String>,
    /// Send the default avatar to servers connected to for the first time (FR-1.47).
    pub send_default_avatar: bool,
    /// Start Mumcord when the user logs in (FR-1.48d, QA.17). The OS entry follows this value.
    pub autostart: bool,
    /// When started at login, go straight to the tray instead of showing the window.
    pub start_minimized: bool,
    /// A fresh connect lands read-only (muted and deafened) until the user joins a channel.
    /// Off: it applies the last own mute/deafen choice right away (FR-1.14, QA.22).
    pub connect_muted: bool,
    /// Show link previews and video player cards in chat; Mumcord then fetches linked pages
    /// itself (FR-1.49, QA.34). Off: plain links only and no requests.
    pub link_previews: bool,
    /// Cached previews, thumbnails and images are deleted after this many days; `None` keeps
    /// them (QA.42).
    pub preview_cache_days: Option<u32>,
    /// Local chat history older than this many days is deleted; `None` keeps it (QA.42).
    pub history_max_age_days: Option<u32>,
    /// Share the track playing in the system's media player with others on the server
    /// (FR-1.58). Opt-in: nothing is read from the system while off.
    pub now_playing: bool,
    /// Share the game being played with others on the server (FR-1.59). Opt-in: no programs
    /// or Steam files are read while off.
    pub now_gaming: bool,
    /// Answer history sync requests of other Mumcord users with the stored channel messages
    /// (FR-1.60). On by default; private chats are never shared.
    pub history_sharing: bool,
    /// Show keyboard hints next to controls (FR-1.69). On by default.
    pub keyboard_hints: bool,
    /// Announce events to screen readers through live regions (FR-1.69). On by default.
    pub announcements: bool,
    /// Offer "skip to" links for keyboard users (FR-1.69). On by default.
    pub skip_links: bool,
    /// Offer the full-size version of own pictures to other Mumcord users (FR-1.74). On by
    /// default. Off: only the thumbnail is sent, no original is kept and none is served.
    pub offer_full_images: bool,
    /// Load the full-size version of pictures from other Mumcord users (FR-1.74). On by
    /// default. Off: only thumbnails are shown and originals are never requested.
    pub load_full_images: bool,
    /// Most servers connected at once, 1–15 (FR-2.1). Clamped on load.
    pub max_connected_servers: u8,
    /// Id of the theme for the whole app: a built-in preset or one of `custom_themes`.
    pub app_theme_id: String,
    /// Imported and user-made themes; built-in presets are not stored (FR-2.15).
    pub custom_themes: Vec<Theme>,
    /// Blur and translucency; ignored when the OS asks for reduced transparency or motion.
    pub visual_effects: bool,
    pub hide_empty_channels: bool,
    pub talking_first: bool,
    pub show_weak_connection_icon: bool,
    /// Direct connections to other Mumcord users (FR-2.36); `Unset` asks on first use.
    pub allow_direct_connections: Consent,
    /// Daily check for a new version (FR-2.56); `Unset` asks.
    pub update_check: Consent,
    /// When the last update check ran, in milliseconds since the Unix epoch.
    pub last_update_check: Option<i64>,
    /// AppImage only: the user's answer to "Add Mumcord to the app menu" (FR-2.55). The menu
    /// entry is written only on `Yes`.
    pub appimage_menu_entry: Consent,
    /// The one-time notice about the removed window overlay was shown (FR-2.33).
    pub window_overlay_notice_shown: bool,
    /// A device choice saved from the old ALSA device list could not be matched to one of the
    /// sound server's devices and was reset to the system default (issue #183); the user has
    /// not been told yet.
    pub audio_device_notice: bool,
    /// Open the "What's new" window once after an update (issue #145). On by default.
    pub whats_new_auto: bool,
    /// The version whose "What's new" the user has seen; `None` before the first start that
    /// knew about it.
    pub whats_new_seen: Option<String>,
    /// The user had Epic 1's window overlay enabled before it was removed (FR-2.33). Derived once
    /// from the old record on first load, then persisted.
    pub legacy_window_overlay_enabled: bool,
}

/// Default own bitrate cap in kbit/s: today's speech cap (FR-2.64).
pub const DEFAULT_BITRATE_KBPS: u32 = 96;

impl Default for Settings {
    fn default() -> Self {
        Self {
            wizard_completed: false,
            theme: ThemePreference::default(),
            language: None,
            talk_mode: TalkMode::default(),
            voice_activation: VoiceActivation::default(),
            voice_threshold_dbfs: DEFAULT_VOICE_THRESHOLD_DBFS,
            ptt_key: None,
            allow_mouse_buttons: false,
            ptt_compatibility_mode: false,
            input_device: None,
            output_device: None,
            noise_suppression: true,
            echo_cancellation: true,
            auto_gain: true,
            music_quality: false,
            low_latency: false,
            bitrate_kbps: Some(DEFAULT_BITRATE_KBPS),
            video_ducking: true,
            video_ducking_percent: DEFAULT_VIDEO_DUCKING_PERCENT,
            sounds: SoundSettings::default(),
            notifications: NotificationMode::default(),
            close_behaviour: CloseBehaviour::default(),
            chat_display: ChatDisplay::default(),
            log_level: LogLevel::default(),
            ingame_overlay: InGameOverlaySettings::default(),
            positional: PositionalSettings::default(),
            enabled_plugins: Vec::new(),
            send_default_avatar: true,
            autostart: false,
            start_minimized: true,
            connect_muted: true,
            link_previews: true,
            preview_cache_days: Some(DEFAULT_PREVIEW_CACHE_DAYS),
            history_max_age_days: None,
            now_playing: false,
            now_gaming: false,
            history_sharing: true,
            keyboard_hints: true,
            announcements: true,
            skip_links: true,
            offer_full_images: true,
            load_full_images: true,
            max_connected_servers: DEFAULT_MAX_CONNECTED_SERVERS,
            app_theme_id: DEFAULT_APP_THEME_ID.to_owned(),
            custom_themes: Vec::new(),
            visual_effects: true,
            hide_empty_channels: false,
            talking_first: false,
            show_weak_connection_icon: true,
            allow_direct_connections: Consent::Unset,
            update_check: Consent::Unset,
            last_update_check: None,
            appimage_menu_entry: Consent::Unset,
            window_overlay_notice_shown: false,
            audio_device_notice: false,
            whats_new_auto: true,
            whats_new_seen: None,
            legacy_window_overlay_enabled: false,
        }
    }
}

/// The user's last own mute/deafen choice while joined for voice (QA.7). `mute` is the mute
/// choice underneath a deafen, so undeafening after a restore brings it back.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OwnVoice {
    pub mute: bool,
    pub deaf: bool,
}

fn has_key(json: &str, key: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(json).is_ok_and(|v| v.get(key).is_some())
}

/// Whether an Epic 1 record (still carrying the removed `overlay` key) had the window overlay on.
/// A record already saved in the Epic 2 format carries the flag itself, so it is not re-derived.
fn legacy_window_overlay_enabled(json: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return false;
    };
    value.get("legacy_window_overlay_enabled").is_none()
        && value
            .pointer("/overlay/enabled")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
}

impl Store {
    /// The saved settings, or defaults if none were saved yet.
    pub fn settings(&self) -> Result<Settings> {
        let json: Option<String> = self
            .conn
            .query_row("SELECT data FROM settings WHERE id = 1", [], |row| {
                row.get(0)
            })
            .optional()?;
        match json {
            Some(json) => {
                let mut settings: Settings = serde_json::from_str(&json)?;
                settings.legacy_window_overlay_enabled |= legacy_window_overlay_enabled(&json);
                if settings.music_quality && !has_key(&json, "bitrate_kbps") {
                    // Music quality used to lift the cap; keep those users at "Max".
                    settings.bitrate_kbps = None;
                }
                settings.max_connected_servers = settings
                    .max_connected_servers
                    .clamp(MIN_MAX_CONNECTED_SERVERS, MAX_MAX_CONNECTED_SERVERS);
                settings.video_ducking_percent = settings
                    .video_ducking_percent
                    .clamp(MIN_VIDEO_DUCKING_PERCENT, MAX_VIDEO_DUCKING_PERCENT);
                Ok(settings)
            }
            None => Ok(Settings::default()),
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (id, data) VALUES (1, ?1)
             ON CONFLICT (id) DO UPDATE SET data = excluded.data",
            params![serde_json::to_string(settings)?],
        )?;
        Ok(())
    }

    /// The remembered own mute/deafen choice; neither muted nor deafened if none was saved yet.
    pub fn own_voice(&self) -> Result<OwnVoice> {
        Ok(self
            .conn
            .query_row("SELECT mute, deaf FROM own_voice WHERE id = 1", [], |row| {
                Ok(OwnVoice {
                    mute: row.get(0)?,
                    deaf: row.get(1)?,
                })
            })
            .optional()?
            .unwrap_or_default())
    }

    pub fn save_own_voice(&self, voice: OwnVoice) -> Result<()> {
        self.conn.execute(
            "INSERT INTO own_voice (id, mute, deaf) VALUES (1, ?1, ?2)
             ON CONFLICT (id) DO UPDATE SET mute = excluded.mute, deaf = excluded.deaf",
            params![voice.mute, voice.deaf],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_store;

    #[test]
    fn defaults_then_round_trip() {
        let (_dir, store) = temp_store();
        let defaults = store.settings().expect("load");
        assert_eq!(defaults, Settings::default());
        assert!(defaults.noise_suppression && defaults.echo_cancellation && defaults.auto_gain);
        assert_eq!(defaults.talk_mode, TalkMode::VoiceActivation);
        assert_eq!(defaults.voice_activation, VoiceActivation::Automatic);
        assert_eq!(defaults.voice_threshold_dbfs, -40);

        let mut changed = defaults;
        changed.theme = ThemePreference::Light;
        changed.talk_mode = TalkMode::PushToTalk;
        changed.ptt_key = Some("F13".into());
        changed.voice_activation = VoiceActivation::Manual;
        changed.voice_threshold_dbfs = -25;
        changed.sounds.user_left = false;
        changed.sounds.voice_join = false;
        changed.sounds.hang_up = false;
        changed.max_connected_servers = 9;
        changed.app_theme_id = "custom-1".into();
        changed.custom_themes = vec![Theme {
            id: "custom-1".into(),
            name: "Mine".into(),
            colors: BTreeMap::new(),
            colors_dark: BTreeMap::from([("accent".to_owned(), "#112233".to_owned())]),
            colors_light: BTreeMap::from([("accent".to_owned(), "#445566".to_owned())]),
            light_generated: true,
            dark_generated: false,
        }];
        changed.visual_effects = false;
        changed.hide_empty_channels = true;
        changed.talking_first = true;
        changed.show_weak_connection_icon = false;
        changed.allow_direct_connections = Consent::No;
        changed.update_check = Consent::Yes;
        changed.last_update_check = Some(1_700_000_000_000);
        changed.appimage_menu_entry = Consent::Yes;
        changed.window_overlay_notice_shown = true;
        changed.audio_device_notice = true;
        changed.whats_new_auto = false;
        changed.whats_new_seen = Some("0.4.4".into());
        changed.enabled_plugins = vec!["gtav".into()];
        changed.wizard_completed = true;
        changed.log_level = LogLevel::Debug;
        changed.ptt_compatibility_mode = true;
        changed.notifications = NotificationMode::InApp;
        changed.connect_muted = false;
        store.save_settings(&changed).expect("save");
        store.save_settings(&changed).expect("save twice");
        assert_eq!(store.settings().expect("reload"), changed);
    }

    #[test]
    fn own_voice_defaults_then_survives_reopen_and_settings_saves() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let store = Store::open(dir.path()).expect("open");
            assert_eq!(store.own_voice().expect("load"), OwnVoice::default());
            store
                .save_own_voice(OwnVoice {
                    mute: true,
                    deaf: false,
                })
                .expect("save");
            store
                .save_settings(&Settings::default())
                .expect("save settings");
        }
        let store = Store::open(dir.path()).expect("reopen");
        let muted = OwnVoice {
            mute: true,
            deaf: false,
        };
        assert_eq!(store.own_voice().expect("reload"), muted);
        let both = OwnVoice {
            mute: true,
            deaf: true,
        };
        store.save_own_voice(both).expect("overwrite");
        assert_eq!(store.own_voice().expect("reload"), both);
    }

    #[test]
    fn ingame_overlay_options_round_trip_and_default_off() {
        let (_dir, store) = temp_store();
        assert_eq!(
            store.settings().expect("load").ingame_overlay.mode,
            InGameOverlayMode::Off
        );
        let mut settings = Settings::default();
        settings.ingame_overlay.mode = InGameOverlayMode::On;
        settings.ingame_overlay.games.push(GameOverlayEntry {
            key: GameKey::Steam(730),
            name: "Counter-Strike 2".into(),
            enabled: Some(false),
            corner: OverlayCorner::BottomRight,
            opacity_percent: 40,
            ..GameOverlayEntry::default()
        });
        settings.ingame_overlay.games.push(GameOverlayEntry {
            key: GameKey::Exe("Game.exe".into()),
            ..GameOverlayEntry::default()
        });
        store.save_settings(&settings).expect("save");
        let loaded = store.settings().expect("load").ingame_overlay;
        assert_eq!(loaded, settings.ingame_overlay);
        assert!(loaded.entry(&GameKey::Exe("game.EXE".into())).is_some());
        assert!(loaded.entry(&GameKey::Steam(731)).is_none());
    }

    #[test]
    fn epic_1_record_with_window_overlay_loads_with_epic_2_defaults() {
        let (_dir, store) = temp_store();
        // Written by Epic 1: still has the removed window-overlay options.
        store
            .conn
            .execute(
                "INSERT INTO settings (id, data) VALUES (1, ?1)",
                params![
                    r#"{"wizard_completed":true,"autostart":true,"overlay":{"enabled":true,"opacity":0.5,"corner":"top_left"}}"#
                ],
            )
            .expect("insert");
        let loaded = store.settings().expect("load");
        assert!(loaded.wizard_completed && loaded.autostart);
        assert_eq!(loaded.max_connected_servers, 5);
        assert_eq!(loaded.app_theme_id, DEFAULT_APP_THEME_ID);
        assert!(loaded.custom_themes.is_empty());
        assert!(loaded.visual_effects && loaded.show_weak_connection_icon);
        assert!(!loaded.hide_empty_channels && !loaded.talking_first);
        assert_eq!(loaded.allow_direct_connections, Consent::Unset);
        assert_eq!(loaded.update_check, Consent::Unset);
        assert_eq!(loaded.appimage_menu_entry, Consent::Unset);
        assert_eq!(loaded.last_update_check, None);
        assert!(!loaded.window_overlay_notice_shown);
        assert!(loaded.whats_new_auto);
        assert_eq!(loaded.whats_new_seen, None);
        assert!(loaded.legacy_window_overlay_enabled);
    }

    #[test]
    fn legacy_window_overlay_flag_follows_epic_1_record_and_round_trips() {
        for (json, expected) in [
            (r#"{"overlay":{"enabled":true}}"#, true),
            (r#"{"overlay":{"enabled":false}}"#, false),
            (r#"{"autostart":true}"#, false),
        ] {
            let (_dir, store) = temp_store();
            store
                .conn
                .execute(
                    "INSERT INTO settings (id, data) VALUES (1, ?1)",
                    params![json],
                )
                .expect("insert");
            let loaded = store.settings().expect("load");
            assert_eq!(loaded.legacy_window_overlay_enabled, expected);
            store.save_settings(&loaded).expect("save");
            assert_eq!(
                store
                    .settings()
                    .expect("reload")
                    .legacy_window_overlay_enabled,
                expected
            );
        }
    }

    #[test]
    fn max_connected_servers_is_clamped_on_load() {
        let (_dir, store) = temp_store();
        for (stored, expected) in [(0, 1), (7, 7), (200, 15)] {
            store
                .conn
                .execute(
                    "INSERT INTO settings (id, data) VALUES (1, ?1)
                     ON CONFLICT (id) DO UPDATE SET data = excluded.data",
                    params![format!(r#"{{"max_connected_servers":{stored}}}"#)],
                )
                .expect("insert");
            assert_eq!(
                store.settings().expect("load").max_connected_servers,
                expected
            );
        }
    }

    #[test]
    fn video_ducking_is_on_at_30_percent_and_clamped_on_load() {
        let (_dir, store) = temp_store();
        let defaults = store.settings().expect("load");
        assert!(defaults.video_ducking);
        assert_eq!(defaults.video_ducking_percent, 30);
        for (stored, expected) in [(0, 10), (60, 60), (95, 80)] {
            store
                .conn
                .execute(
                    "INSERT INTO settings (id, data) VALUES (1, ?1)
                     ON CONFLICT (id) DO UPDATE SET data = excluded.data",
                    params![format!(
                        r#"{{"video_ducking":false,"video_ducking_percent":{stored}}}"#
                    )],
                )
                .expect("insert");
            let loaded = store.settings().expect("load");
            assert!(!loaded.video_ducking);
            assert_eq!(loaded.video_ducking_percent, expected);
        }
    }

    #[test]
    fn low_latency_is_off_by_default_and_persists() {
        let (_dir, store) = temp_store();
        assert!(!store.settings().expect("load").low_latency);
        let settings = Settings {
            low_latency: true,
            ..Settings::default()
        };
        store.save_settings(&settings).expect("save");
        assert!(store.settings().expect("reload").low_latency);
    }

    #[test]
    fn music_quality_is_off_by_default_and_persists() {
        let (_dir, store) = temp_store();
        assert!(!store.settings().expect("load").music_quality);
        let settings = Settings {
            music_quality: true,
            ..Settings::default()
        };
        store.save_settings(&settings).expect("save");
        assert!(store.settings().expect("reload").music_quality);
    }

    #[test]
    fn bitrate_cap_persists() {
        let (_dir, store) = temp_store();
        let settings = Settings {
            bitrate_kbps: Some(32),
            ..Settings::default()
        };
        store.save_settings(&settings).expect("save");
        assert_eq!(store.settings().expect("reload").bitrate_kbps, Some(32));
    }

    #[test]
    fn now_playing_switch_persists_on_records_saved_before_it_existed() {
        let (_dir, store) = temp_store();
        // A record written by a build without the switch (as found on a real install).
        let old = r#"{"wizard_completed":true,"input_device":"alsa:sysdefault:CARD=II","link_previews":true}"#;
        let parsed: Settings = serde_json::from_str(old).expect("parse");
        assert!(!parsed.now_playing, "off until the user turns it on");
        assert!(!parsed.now_gaming, "off until the user turns it on");
        assert!(parsed.history_sharing, "history sharing is on by default");
        assert!(
            parsed.keyboard_hints && parsed.announcements && parsed.skip_links,
            "accessibility switches are on for old records"
        );
        assert!(
            parsed.offer_full_images && parsed.load_full_images,
            "full-size image switches are on for old records"
        );
        store
            .save_settings(&Settings {
                now_playing: true,
                now_gaming: true,
                ..parsed
            })
            .expect("save");
        let reloaded = store.settings().expect("reload");
        assert!(reloaded.now_playing);
        assert!(reloaded.now_gaming);
        assert_eq!(
            reloaded.input_device.as_deref(),
            Some("alsa:sysdefault:CARD=II")
        );
    }

    #[test]
    fn bitrate_cap_defaults_to_96_and_music_users_start_at_max() {
        let (_dir, store) = temp_store();
        let save_raw = |json: &str| {
            store
                .conn
                .execute(
                    "INSERT INTO settings (id, data) VALUES (1, ?1)
                     ON CONFLICT (id) DO UPDATE SET data = excluded.data",
                    params![json],
                )
                .expect("write");
        };
        assert_eq!(store.settings().expect("fresh").bitrate_kbps, Some(96));
        save_raw(r#"{"theme":"dark"}"#);
        assert_eq!(store.settings().expect("old").bitrate_kbps, Some(96));
        save_raw(r#"{"music_quality":true}"#);
        assert_eq!(store.settings().expect("music").bitrate_kbps, None);
        save_raw(r#"{"music_quality":true,"bitrate_kbps":64}"#);
        assert_eq!(store.settings().expect("chosen").bitrate_kbps, Some(64));
        save_raw(r#"{"music_quality":false,"bitrate_kbps":null}"#);
        assert_eq!(store.settings().expect("max").bitrate_kbps, None);
    }

    #[test]
    fn partial_json_falls_back_to_defaults() {
        let parsed: Settings =
            serde_json::from_str(r#"{"theme":"dark","sounds":{"user_joined":false}}"#)
                .expect("parse");
        assert_eq!(parsed.theme, ThemePreference::Dark);
        assert_eq!(parsed.chat_display, ChatDisplay::Cozy);
        assert!(!parsed.sounds.user_joined);
        assert!(parsed.sounds.user_left);
        assert!(
            parsed.sounds.voice_join && parsed.sounds.hang_up,
            "new sounds are on for settings saved before they existed"
        );
        assert!(
            parsed.sounds.party_started && parsed.sounds.viewer_stopped,
            "activity sounds are on for older settings"
        );
        assert!(parsed.noise_suppression);
        assert!(
            !parsed.music_quality,
            "music quality is off for old records"
        );
        assert!(!parsed.wizard_completed);
        assert_eq!(parsed.log_level, LogLevel::Info);
        assert_eq!(parsed.notifications, NotificationMode::Both);
        assert!(parsed.link_previews, "link previews are on for old records");
        assert_eq!(parsed.preview_cache_days, Some(7), "7 days for old records");
        assert_eq!(
            parsed.history_max_age_days, None,
            "history is kept by default"
        );
        assert!(
            parsed.send_default_avatar,
            "on by default, also for old records"
        );
        assert!(
            !parsed.allow_mouse_buttons,
            "reading input devices stays off unless the user opts in"
        );
        assert!(
            !parsed.ptt_compatibility_mode,
            "the modern Wayland path stays the default"
        );
        assert!(!parsed.autostart, "starting at login is opt-in");
        assert!(parsed.start_minimized);
        assert!(parsed.connect_muted, "read-only connect stays the default");
    }

    #[test]
    fn theme_saved_with_one_colour_set_still_loads() {
        let theme: Theme =
            serde_json::from_str(r##"{"id":"a","name":"A","colors":{"accent":"#112233"}}"##)
                .expect("parse theme");
        assert_eq!(theme.colors["accent"], "#112233");
        assert!(theme.colors_dark.is_empty() && theme.colors_light.is_empty());
        assert!(!theme.light_generated && !theme.dark_generated);
    }

    #[test]
    fn theme_versions_use_the_spec_field_names() {
        let theme: Theme = serde_json::from_str(
            r##"{"id":"a","name":"A","colors_dark":{"text":"#ffffff"},"colours_light":{"text":"#000000"},"light_generated":true}"##,
        )
        .expect("parse theme");
        assert_eq!(theme.colors_dark["text"], "#ffffff");
        assert_eq!(theme.colors_light["text"], "#000000");
        let json = serde_json::to_string(&theme).expect("serialise");
        assert!(json.contains("\"colours_dark\"") && json.contains("\"colours_light\""));
        assert!(
            !json.contains("\"colors\""),
            "an empty legacy set is not written"
        );
    }

    #[test]
    fn theme_accepts_the_british_spelling_on_load() {
        let theme: Theme =
            serde_json::from_str(r##"{"id":"a","name":"A","colours":{"accent":"#112233"}}"##)
                .expect("parse theme");
        assert_eq!(theme.colors["accent"], "#112233");
        assert!(
            serde_json::to_string(&theme)
                .expect("serialise")
                .contains("\"colors\"")
        );
    }
}
