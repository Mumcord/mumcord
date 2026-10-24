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
            voice_join: true,
            hang_up: true,
        }
    }
}

/// Overlay window placement in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Overlay options (FR-1.44, FR-1.45).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlaySettings {
    pub enabled: bool,
    /// Show the whole current channel instead of only who is talking.
    pub show_whole_channel: bool,
    /// 0.0 (transparent) – 1.0 (opaque).
    pub opacity: f32,
    /// `None` until the user moves or resizes the overlay.
    pub bounds: Option<OverlayBounds>,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            show_whole_channel: false,
            opacity: 0.9,
            bounds: None,
        }
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
    pub sounds: SoundSettings,
    pub notifications: NotificationMode,
    pub close_behaviour: CloseBehaviour,
    pub chat_display: ChatDisplay,
    pub log_level: LogLevel,
    pub overlay: OverlaySettings,
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            wizard_completed: false,
            theme: ThemePreference::default(),
            language: None,
            talk_mode: TalkMode::default(),
            ptt_key: None,
            allow_mouse_buttons: false,
            ptt_compatibility_mode: false,
            input_device: None,
            output_device: None,
            noise_suppression: true,
            echo_cancellation: true,
            auto_gain: true,
            sounds: SoundSettings::default(),
            notifications: NotificationMode::default(),
            close_behaviour: CloseBehaviour::default(),
            chat_display: ChatDisplay::default(),
            log_level: LogLevel::default(),
            overlay: OverlaySettings::default(),
            positional: PositionalSettings::default(),
            enabled_plugins: Vec::new(),
            send_default_avatar: true,
            autostart: false,
            start_minimized: true,
            connect_muted: true,
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

        let mut changed = defaults;
        changed.theme = ThemePreference::Light;
        changed.talk_mode = TalkMode::PushToTalk;
        changed.ptt_key = Some("F13".into());
        changed.sounds.user_left = false;
        changed.sounds.voice_join = false;
        changed.sounds.hang_up = false;
        changed.overlay.bounds = Some(OverlayBounds {
            x: 1,
            y: 2,
            width: 300,
            height: 400,
        });
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
        assert!(!parsed.wizard_completed);
        assert_eq!(parsed.log_level, LogLevel::Info);
        assert_eq!(parsed.notifications, NotificationMode::Both);
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
