//! The single app settings record. Every field has a serde default so settings saved
//! by an older version load cleanly and new fields need no database migration.

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
        }
    }
}

/// Screen corner the overlay sits in unless the user dragged it somewhere else (FR-1.45).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayCorner {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Top-left corner of the overlay in physical desktop pixels, set by dragging it in move mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayPoint {
    pub x: i32,
    pub y: i32,
}

/// Default overlay size in logical pixels: room for about six talking people.
pub const DEFAULT_OVERLAY_WIDTH: u32 = 240;
pub const DEFAULT_OVERLAY_HEIGHT: u32 = 320;

/// Overlay options (FR-1.44, FR-1.45).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlaySettings {
    pub enabled: bool,
    /// Show the whole current channel instead of only who is talking.
    pub show_whole_channel: bool,
    /// Show the name of the own channel above the people.
    pub show_channel_name: bool,
    /// 0.0 (transparent) – 1.0 (opaque).
    pub opacity: f32,
    pub corner: OverlayCorner,
    /// Where the user dragged the overlay; `None` places it in `corner`.
    pub custom_position: Option<OverlayPoint>,
    /// Logical pixels.
    pub width: u32,
    pub height: u32,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            show_whole_channel: false,
            show_channel_name: true,
            opacity: 0.9,
            corner: OverlayCorner::default(),
            custom_position: None,
            width: DEFAULT_OVERLAY_WIDTH,
            height: DEFAULT_OVERLAY_HEIGHT,
        }
    }
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The first-start wizard was finished (FR-1.1).
    pub wizard_completed: bool,
    pub theme: ThemePreference,
    /// UI language code (`en`, `es`, `de`); `None` follows the OS language.
    pub language: Option<String>,
    pub talk_mode: TalkMode,
    pub voice_activation: VoiceActivation,
    /// Manual voice-activation threshold in dBFS, -60 to 0 (FR-1.57). Used in manual mode only.
    pub voice_threshold_dbfs: i32,
    pub ptt_key: Option<String>,
    /// Linux Wayland: read mouse buttons for push-to-talk from input devices (needs the
    /// `input` group; opt-in, FR-1.39, QA.4).
    pub allow_mouse_buttons: bool,
    /// Linux Wayland: start through XWayland and use the X11 push-to-talk path, like
    /// TeamSpeak (FR-1.48, QA.5). Read before the window system starts; applies on restart.
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
    pub sounds: SoundSettings,
    pub notifications: NotificationMode,
    pub close_behaviour: CloseBehaviour,
    pub chat_display: ChatDisplay,
    pub log_level: LogLevel,
    pub overlay: OverlaySettings,
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
}

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
            sounds: SoundSettings::default(),
            notifications: NotificationMode::default(),
            close_behaviour: CloseBehaviour::default(),
            chat_display: ChatDisplay::default(),
            log_level: LogLevel::default(),
            overlay: OverlaySettings::default(),
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
            Some(json) => Ok(serde_json::from_str(&json)?),
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
        changed.overlay.corner = OverlayCorner::BottomRight;
        changed.overlay.custom_position = Some(OverlayPoint { x: -1, y: 2 });
        changed.overlay.width = 300;
        changed.overlay.opacity = 0.5;
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
    fn overlay_options_load_from_records_saved_before_they_existed() {
        let (_dir, store) = temp_store();
        // Shape written by earlier versions: no corner, size or channel name yet.
        store
            .conn
            .execute(
                "INSERT INTO settings (id, data) VALUES (1, ?1)",
                params![r#"{"overlay":{"enabled":true,"opacity":0.5,"bounds":null}}"#],
            )
            .expect("insert");
        let overlay = store.settings().expect("load").overlay;
        assert!(overlay.enabled);
        assert!((overlay.opacity - 0.5).abs() < f32::EPSILON);
        assert_eq!(overlay.corner, OverlayCorner::TopLeft);
        assert_eq!(overlay.custom_position, None);
        assert_eq!(
            (overlay.width, overlay.height),
            (DEFAULT_OVERLAY_WIDTH, DEFAULT_OVERLAY_HEIGHT)
        );
        assert!(overlay.show_channel_name);
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
    fn now_playing_switch_persists_on_records_saved_before_it_existed() {
        let (_dir, store) = temp_store();
        // A record written by a build without the switch (as found on a real install).
        let old = r#"{"wizard_completed":true,"input_device":"alsa:sysdefault:CARD=II","link_previews":true}"#;
        let parsed: Settings = serde_json::from_str(old).expect("parse");
        assert!(!parsed.now_playing, "off until the user turns it on");
        assert!(!parsed.now_gaming, "off until the user turns it on");
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
}
