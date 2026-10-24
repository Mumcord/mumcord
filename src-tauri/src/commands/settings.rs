//! The single settings record (§7, FR-1.23, FR-1.27, FR-1.35, FR-1.36, FR-1.38).

use mumcord_engine::{Engine, diag, store};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use super::types::{ApiError, ApiResult, ErrorCode};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Dark,
    Light,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    En,
    Es,
    De,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum TalkMode {
    VoiceActivation,
    PushToTalk,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum VoiceActivation {
    Automatic,
    Manual,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum CloseBehaviour {
    Ask,
    MinimizeToTray,
    Quit,
}

/// "Message display: Cozy / Compact" (default cozy, FR-1.48a, QA.25).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum ChatDisplay {
    Cozy,
    Compact,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

/// Which message notifications are shown (FR-1.48c, QA.16).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum NotificationMode {
    /// In-app toasts only, while the window has focus.
    InApp,
    /// System notifications only, also while the window has focus.
    Desktop,
    /// Toasts while the window has focus, system notifications otherwise.
    Both,
    Off,
}

impl From<store::NotificationMode> for NotificationMode {
    fn from(mode: store::NotificationMode) -> Self {
        match mode {
            store::NotificationMode::InApp => Self::InApp,
            store::NotificationMode::Desktop => Self::Desktop,
            store::NotificationMode::Both => Self::Both,
            store::NotificationMode::Off => Self::Off,
        }
    }
}

impl From<NotificationMode> for store::NotificationMode {
    fn from(mode: NotificationMode) -> Self {
        match mode {
            NotificationMode::InApp => Self::InApp,
            NotificationMode::Desktop => Self::Desktop,
            NotificationMode::Both => Self::Both,
            NotificationMode::Off => Self::Off,
        }
    }
}

/// Notification sound toggles per event (FR-1.27).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SoundSettings {
    pub join_leave: bool,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub messages: bool,
    /// The "ping" when a message mentions you, in place of the message sound (QA.41).
    pub mention: bool,
    /// A mention is announced as such in notifications (QA.41).
    pub mention_notify: bool,
    /// You join a channel for voice yourself (QA.23).
    pub voice_join: bool,
    /// You disconnect from a server yourself (QA.19).
    pub hang_up: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// False until the first-start wizard was finished.
    pub wizard_completed: bool,
    pub theme: Theme,
    /// `null` follows the OS language (English if unsupported).
    pub language: Option<Language>,
    pub talk_mode: TalkMode,
    /// Voice activation: speech detection or a manual level threshold (FR-1.57).
    pub voice_activation: VoiceActivation,
    /// Manual threshold in dBFS, -60 to 0 (FR-1.57); clamped on save.
    pub voice_threshold_dbfs: i32,
    /// Shortcut as returned by `ptt_capture_key`; `null` if none bound.
    pub ptt_key: Option<String>,
    /// Linux Wayland: "Allow mouse buttons (requires input group)" (default off, FR-1.39).
    pub allow_mouse_buttons: bool,
    /// Linux Wayland: "Compatibility mode (like TeamSpeak)", runs through XWayland with the X11
    /// push-to-talk path (default off, FR-1.48). Takes effect after `app_restart`.
    pub ptt_compatibility_mode: bool,
    /// Device ids from `audio_devices`; `null` uses the OS default.
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub noise_suppression: bool,
    pub echo_cancellation: bool,
    pub auto_gain: bool,
    /// "Music quality" (default off; FR-1.24, QA.37): bitrate cap up to the server limit (max
    /// 510 kbit/s), music-tuned encoder, noise suppression and automatic volume bypassed.
    pub music_quality: bool,
    pub sounds: SoundSettings,
    /// "Notifications: In-app / Desktop / Both / Off" (default both, FR-1.48c).
    pub notifications: NotificationMode,
    pub close_behaviour: CloseBehaviour,
    pub chat_display: ChatDisplay,
    pub log_level: LogLevel,
    /// Send the default avatar to servers connected to for the first time (FR-1.47).
    pub send_default_avatar: bool,
    /// "Start Mumcord when I log in" (default off, FR-1.48d). Changing it adds or removes the
    /// OS autostart entry; the update fails without saving if that is not possible.
    pub autostart: bool,
    /// "Start minimized": a start at login goes straight to the tray (default on).
    pub start_minimized: bool,
    /// "Connect muted until I join a channel" (default on; QA.22).
    pub connect_muted: bool,
    /// "Show link previews and embeds" (default on; FR-1.49, QA.34). Off: plain links only
    /// and `link_preview` fetches nothing.
    pub link_previews: bool,
    /// "Share what I'm listening to" (default off; FR-1.58). Off: the media player is not read.
    pub now_playing: bool,
}

impl Settings {
    /// UI view of the stored record (1b fields such as overlay stay in the store only).
    pub fn from_store(s: &store::Settings) -> Self {
        Self {
            wizard_completed: s.wizard_completed,
            theme: match s.theme {
                store::ThemePreference::System => Theme::System,
                store::ThemePreference::Dark => Theme::Dark,
                store::ThemePreference::Light => Theme::Light,
            },
            language: s.language.as_deref().and_then(|l| match l {
                "en" => Some(Language::En),
                "es" => Some(Language::Es),
                "de" => Some(Language::De),
                _ => None,
            }),
            talk_mode: match s.talk_mode {
                store::TalkMode::VoiceActivation => TalkMode::VoiceActivation,
                store::TalkMode::PushToTalk => TalkMode::PushToTalk,
            },
            voice_activation: match s.voice_activation {
                store::VoiceActivation::Automatic => VoiceActivation::Automatic,
                store::VoiceActivation::Manual => VoiceActivation::Manual,
            },
            voice_threshold_dbfs: s.voice_threshold_dbfs,
            ptt_key: s.ptt_key.clone(),
            allow_mouse_buttons: s.allow_mouse_buttons,
            ptt_compatibility_mode: s.ptt_compatibility_mode,
            input_device: s.input_device.clone(),
            output_device: s.output_device.clone(),
            noise_suppression: s.noise_suppression,
            echo_cancellation: s.echo_cancellation,
            auto_gain: s.auto_gain,
            music_quality: s.music_quality,
            sounds: SoundSettings {
                join_leave: s.sounds.user_joined || s.sounds.user_left,
                self_mute: s.sounds.self_mute,
                self_deaf: s.sounds.self_deafen,
                messages: s.sounds.new_message,
                mention: s.sounds.mention,
                mention_notify: s.sounds.mention_notify,
                voice_join: s.sounds.voice_join,
                hang_up: s.sounds.hang_up,
            },
            notifications: s.notifications.into(),
            close_behaviour: match s.close_behaviour {
                store::CloseBehaviour::Ask => CloseBehaviour::Ask,
                store::CloseBehaviour::MinimizeToTray => CloseBehaviour::MinimizeToTray,
                store::CloseBehaviour::Quit => CloseBehaviour::Quit,
            },
            chat_display: match s.chat_display {
                store::ChatDisplay::Cozy => ChatDisplay::Cozy,
                store::ChatDisplay::Compact => ChatDisplay::Compact,
            },
            log_level: match s.log_level {
                store::LogLevel::Error => LogLevel::Error,
                store::LogLevel::Warn => LogLevel::Warn,
                store::LogLevel::Info => LogLevel::Info,
                store::LogLevel::Debug => LogLevel::Debug,
            },
            send_default_avatar: s.send_default_avatar,
            autostart: s.autostart,
            start_minimized: s.start_minimized,
            connect_muted: s.connect_muted,
            link_previews: s.link_previews,
            now_playing: s.now_playing,
        }
    }

    /// Applies this UI record onto `base`, keeping the fields the UI does not edit.
    pub fn to_store(&self, base: &store::Settings) -> store::Settings {
        let mut s = base.clone();
        s.wizard_completed = self.wizard_completed;
        s.theme = match self.theme {
            Theme::System => store::ThemePreference::System,
            Theme::Dark => store::ThemePreference::Dark,
            Theme::Light => store::ThemePreference::Light,
        };
        s.language = self.language.map(|l| {
            match l {
                Language::En => "en",
                Language::Es => "es",
                Language::De => "de",
            }
            .to_owned()
        });
        s.talk_mode = match self.talk_mode {
            TalkMode::VoiceActivation => store::TalkMode::VoiceActivation,
            TalkMode::PushToTalk => store::TalkMode::PushToTalk,
        };
        s.voice_activation = match self.voice_activation {
            VoiceActivation::Automatic => store::VoiceActivation::Automatic,
            VoiceActivation::Manual => store::VoiceActivation::Manual,
        };
        s.voice_threshold_dbfs = self.voice_threshold_dbfs.clamp(-60, 0);
        s.ptt_key = self.ptt_key.clone().filter(|k| !k.is_empty());
        s.allow_mouse_buttons = self.allow_mouse_buttons;
        s.ptt_compatibility_mode = self.ptt_compatibility_mode;
        s.input_device = self.input_device.clone().filter(|d| !d.is_empty());
        s.output_device = self.output_device.clone().filter(|d| !d.is_empty());
        s.noise_suppression = self.noise_suppression;
        s.echo_cancellation = self.echo_cancellation;
        s.auto_gain = self.auto_gain;
        s.music_quality = self.music_quality;
        s.sounds.user_joined = self.sounds.join_leave;
        s.sounds.user_left = self.sounds.join_leave;
        s.sounds.self_mute = self.sounds.self_mute;
        s.sounds.self_deafen = self.sounds.self_deaf;
        s.sounds.new_message = self.sounds.messages;
        s.sounds.mention = self.sounds.mention;
        s.sounds.mention_notify = self.sounds.mention_notify;
        s.sounds.voice_join = self.sounds.voice_join;
        s.sounds.hang_up = self.sounds.hang_up;
        s.notifications = self.notifications.into();
        s.close_behaviour = match self.close_behaviour {
            CloseBehaviour::Ask => store::CloseBehaviour::Ask,
            CloseBehaviour::MinimizeToTray => store::CloseBehaviour::MinimizeToTray,
            CloseBehaviour::Quit => store::CloseBehaviour::Quit,
        };
        s.chat_display = match self.chat_display {
            ChatDisplay::Cozy => store::ChatDisplay::Cozy,
            ChatDisplay::Compact => store::ChatDisplay::Compact,
        };
        s.log_level = match self.log_level {
            LogLevel::Error => store::LogLevel::Error,
            LogLevel::Warn => store::LogLevel::Warn,
            LogLevel::Info => store::LogLevel::Info,
            LogLevel::Debug => store::LogLevel::Debug,
        };
        s.send_default_avatar = self.send_default_avatar;
        s.autostart = self.autostart;
        s.start_minimized = self.start_minimized;
        s.connect_muted = self.connect_muted;
        s.link_previews = self.link_previews;
        s.now_playing = self.now_playing;
        s
    }
}

/// Log file detail for a stored setting.
pub fn diag_level(level: store::LogLevel) -> diag::LogLevel {
    match level {
        store::LogLevel::Error => diag::LogLevel::Error,
        store::LogLevel::Warn => diag::LogLevel::Warn,
        store::LogLevel::Info => diag::LogLevel::Info,
        store::LogLevel::Debug => diag::LogLevel::Debug,
    }
}

/// The push-to-talk key to register: only in push-to-talk mode.
pub fn active_ptt_key(s: &store::Settings) -> Option<String> {
    match s.talk_mode {
        store::TalkMode::PushToTalk => s.ptt_key.clone(),
        store::TalkMode::VoiceActivation => None,
    }
}

/// (Re)binds the global push-to-talk key in the background (FR-1.21).
pub fn configure_ptt(app: &AppHandle, key: Option<String>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(reason) = crate::ptt::configure(&app, key.as_deref()).await {
            tracing::warn!(?reason, "push-to-talk key could not be registered");
        }
    });
}

#[tauri::command]
#[specta::specta]
pub fn settings_get(engine: State<'_, Engine>) -> ApiResult<Settings> {
    Ok(Settings::from_store(&engine.settings()?))
}

/// Replaces the whole record and applies changes immediately; returns the stored record.
#[tauri::command]
#[specta::specta]
pub async fn settings_update(
    app: AppHandle,
    engine: State<'_, Engine>,
    settings: Settings,
) -> ApiResult<Settings> {
    let previous = engine.settings()?;
    // The OS entry first: if it cannot be changed, the stored choice must not claim otherwise.
    if settings.autostart != previous.autostart
        && let Err(e) = crate::autostart::apply(&app, settings.autostart)
    {
        tracing::warn!(error = %e, "changing the autostart entry failed");
        return Err(ApiError::new(ErrorCode::Internal, e.to_string()));
    }
    let stored = engine.update_settings(settings.to_store(&previous)).await?;
    if stored.log_level != previous.log_level
        && let Err(e) = diag::set_log_level(diag_level(stored.log_level))
    {
        tracing::warn!(error = %e, "changing the log level failed");
    }
    let key = active_ptt_key(&stored);
    let mouse_changed = stored.allow_mouse_buttons != previous.allow_mouse_buttons;
    if mouse_changed {
        crate::ptt::set_evdev_mouse_allowed(stored.allow_mouse_buttons);
    }
    if mouse_changed || key != active_ptt_key(&previous) {
        configure_ptt(&app, key);
    }
    Ok(Settings::from_store(&stored))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_stage_1b_fields() {
        let mut base = store::Settings::default();
        base.overlay.enabled = true;
        base.enabled_plugins = vec!["gtav".into()];
        base.language = Some("de".into());

        let mut ui = Settings::from_store(&base);
        assert_eq!(ui.language, Some(Language::De));
        assert!(!ui.wizard_completed);
        assert_eq!(ui.log_level, LogLevel::Info);
        assert!(ui.send_default_avatar);
        assert_eq!(ui.notifications, NotificationMode::Both);

        ui.wizard_completed = true;
        ui.talk_mode = TalkMode::PushToTalk;
        ui.ptt_key = Some("Ctrl+A".into());
        ui.voice_activation = VoiceActivation::Manual;
        ui.voice_threshold_dbfs = -25;
        ui.sounds.join_leave = false;
        ui.sounds.hang_up = false;
        ui.log_level = LogLevel::Debug;
        ui.input_device = Some(String::new());
        ui.send_default_avatar = false;
        ui.ptt_compatibility_mode = true;
        ui.autostart = true;
        ui.start_minimized = false;
        ui.connect_muted = false;
        ui.link_previews = false;
        ui.now_playing = true;
        ui.chat_display = ChatDisplay::Compact;
        let stored = ui.to_store(&base);
        assert_eq!(stored.chat_display, store::ChatDisplay::Compact);
        assert!(!stored.send_default_avatar);
        assert!(stored.ptt_compatibility_mode);
        assert!(stored.autostart && !stored.start_minimized);
        assert!(!stored.connect_muted);
        assert!(!stored.link_previews);
        assert!(stored.now_playing);
        assert_eq!(stored.voice_activation, store::VoiceActivation::Manual);
        assert_eq!(stored.voice_threshold_dbfs, -25);
        let too_loud = Settings {
            voice_threshold_dbfs: 12,
            ..ui.clone()
        };
        assert_eq!(too_loud.to_store(&base).voice_threshold_dbfs, 0);
        assert!(stored.overlay.enabled);
        assert_eq!(stored.enabled_plugins, ["gtav"]);
        assert!(stored.wizard_completed);
        assert!(!stored.sounds.user_joined && !stored.sounds.user_left);
        assert!(!stored.sounds.hang_up && stored.sounds.voice_join);
        assert_eq!(stored.log_level, store::LogLevel::Debug);
        assert_eq!(
            stored.input_device, None,
            "empty device id means OS default"
        );
        assert_eq!(active_ptt_key(&stored).as_deref(), Some("Ctrl+A"));

        let mut back = Settings::from_store(&stored);
        back.input_device = Some(String::new());
        assert_eq!(back, ui);
    }

    #[test]
    fn ptt_key_only_active_in_push_to_talk() {
        let mut s = store::Settings {
            ptt_key: Some("F13".into()),
            ..store::Settings::default()
        };
        assert_eq!(active_ptt_key(&s), None);
        s.talk_mode = store::TalkMode::PushToTalk;
        assert_eq!(active_ptt_key(&s).as_deref(), Some("F13"));
    }

    #[test]
    fn unknown_language_follows_os() {
        let s = store::Settings {
            language: Some("fr".into()),
            ..store::Settings::default()
        };
        assert_eq!(Settings::from_store(&s).language, None);
    }
}
