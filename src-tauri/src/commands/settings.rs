//! The single settings record (§7, FR-1.23, FR-1.27, FR-1.35, FR-1.36, FR-1.38).

use std::collections::BTreeMap;

use mumcord_engine::{Engine, diag, store};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use super::types::{ApiError, ApiResult, ErrorCode};

/// Answer to a question asked once (direct connections, update check, AppImage menu entry).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum Consent {
    /// Not asked yet.
    Unset,
    Yes,
    No,
}

/// A custom or imported colour theme (FR-2.14 – FR-2.16) with a dark and a light version. Each
/// colour map holds the key colours (background, surface, text, accent, talking, warning, error)
/// as `#rrggbb`; values that are not such a colour are dropped on save. Built-in presets live in
/// the UI.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThemeRecord {
    pub id: String,
    pub name: String,
    /// Single colour set of a theme saved before themes had two versions; the UI turns it into
    /// both versions and sends it back empty.
    pub colors: BTreeMap<String, String>,
    pub colors_dark: BTreeMap<String, String>,
    pub colors_light: BTreeMap<String, String>,
    /// True while that version was generated from the other one and not edited by hand.
    pub light_generated: bool,
    pub dark_generated: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Dark,
    Light,
}

/// UI languages: the 24 official EU languages (FR-1.36), serialised as ISO 639-1 codes.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    Bg,
    Cs,
    Da,
    De,
    El,
    En,
    Es,
    Et,
    Fi,
    Fr,
    Ga,
    Hr,
    Hu,
    It,
    Lt,
    Lv,
    Mt,
    Nl,
    Pl,
    Pt,
    Ro,
    Sk,
    Sl,
    Sv,
}

impl Language {
    const ALL: [Language; 24] = [
        Language::Bg,
        Language::Cs,
        Language::Da,
        Language::De,
        Language::El,
        Language::En,
        Language::Es,
        Language::Et,
        Language::Fi,
        Language::Fr,
        Language::Ga,
        Language::Hr,
        Language::Hu,
        Language::It,
        Language::Lt,
        Language::Lv,
        Language::Mt,
        Language::Nl,
        Language::Pl,
        Language::Pt,
        Language::Ro,
        Language::Sk,
        Language::Sl,
        Language::Sv,
    ];

    /// ISO 639-1 code as stored in the settings file.
    fn code(self) -> &'static str {
        match self {
            Language::Bg => "bg",
            Language::Cs => "cs",
            Language::Da => "da",
            Language::De => "de",
            Language::El => "el",
            Language::En => "en",
            Language::Es => "es",
            Language::Et => "et",
            Language::Fi => "fi",
            Language::Fr => "fr",
            Language::Ga => "ga",
            Language::Hr => "hr",
            Language::Hu => "hu",
            Language::It => "it",
            Language::Lt => "lt",
            Language::Lv => "lv",
            Language::Mt => "mt",
            Language::Nl => "nl",
            Language::Pl => "pl",
            Language::Pt => "pt",
            Language::Ro => "ro",
            Language::Sk => "sk",
            Language::Sl => "sl",
            Language::Sv => "sv",
        }
    }

    /// Unknown codes (e.g. from a newer version) fall back to following the OS language.
    fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.code() == code)
    }
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
    pub party_started: bool,
    pub party_joined: bool,
    pub party_left: bool,
    pub share_started: bool,
    pub share_stopped: bool,
    pub viewer_started: bool,
    pub viewer_stopped: bool,
}

/// Positional audio (FR-1.41): voices of people in the same game placed in 3D.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PositionalSettings {
    /// Default off.
    pub enabled: bool,
    /// Metres (default 1): voices are at full volume up to this distance.
    pub min_distance: f32,
    /// Metres (default 15): voices are silent beyond this distance. Kept above `minDistance`
    /// on save.
    pub max_distance: f32,
}

/// Largest accepted distance in metres.
const MAX_DISTANCE_M: f32 = 1000.0;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
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
    /// Linux Wayland: "Compatibility mode (X11)", runs through XWayland with the X11
    /// push-to-talk path (default off, FR-1.48). Takes effect after `app_restart`.
    pub ptt_compatibility_mode: bool,
    /// Device ids from `audio_devices`; `null` uses the OS default.
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub noise_suppression: bool,
    pub echo_cancellation: bool,
    pub auto_gain: bool,
    /// Manual microphone gain in percent, 0–200 (default 100).
    pub input_gain_percent: u16,
    /// Master volume of everything played, 0–100 (default 100).
    pub output_volume_percent: u16,
    /// "Music quality" (default off; FR-1.24, QA.37): bitrate cap up to the server limit (max
    /// 510 kbit/s), music-tuned encoder, noise suppression and automatic volume bypassed.
    pub music_quality: bool,
    /// "Low-latency mode" (default off; issue #156): 10 ms low-delay Opus packets, a minimal
    /// jitter buffer, small device buffers and echo cancellation, noise suppression and automatic
    /// volume bypassed, for playing music together. Applies live.
    pub low_latency: bool,
    /// Own bitrate cap in kbit/s (FR-2.64); `null` is "Max". Range 16 to the maximum from
    /// `bitrate_max`.
    pub bitrate_kbps: Option<u32>,
    /// "Lower video sound while people talk" (default on, FR-2.76).
    pub video_ducking: bool,
    /// The lowered level while people talk, 10–80 % (default 30).
    pub video_ducking_percent: u8,
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
    /// "Delete cached previews after X days" (default 7; `null` never; QA.42). Previews,
    /// thumbnails and images are pruned at start, periodically and when this changes.
    pub preview_cache_days: Option<u32>,
    /// "Delete chat history older than X days" (default `null` = keep; QA.42). Local history
    /// only; pruned at start, daily and when this changes.
    pub history_max_age_days: Option<u32>,
    /// "Share what I'm listening to" (default off; FR-1.58). Off: the media player is not read.
    pub now_playing: bool,
    /// "Share the game I'm playing" (default off; FR-1.59). Off: no programs or Steam files
    /// are read.
    pub now_gaming: bool,
    /// "Share my channel history" (default on; FR-1.60): answer history sync requests of
    /// other Mumcord users with the stored channel messages (never private chats).
    pub history_sharing: bool,
    /// Accessibility: "Show keyboard hints" (default on; FR-1.69).
    pub keyboard_hints: bool,
    /// Accessibility: "Screen reader announcements" (default on; FR-1.69).
    pub announcements: bool,
    /// Accessibility: "Skip links" (default on; FR-1.69).
    pub skip_links: bool,
    /// "Offer my full-size images" (default on; FR-1.74): off sends thumbnails only, keeps no
    /// original and serves none.
    pub offer_full_images: bool,
    /// "Load full-size images from others" (default on; FR-1.74): off shows thumbnails only
    /// and never requests an original.
    pub load_full_images: bool,
    /// Positional audio (FR-1.41). Off: no game positions are read.
    pub positional: PositionalSettings,
    /// How many servers may be connected at once, 1 – 15 (default 5; FR-2.1).
    pub max_connected_servers: u8,
    /// Id of the app theme: a built-in preset or one of `customThemes` (FR-2.14).
    pub app_theme_id: String,
    pub custom_themes: Vec<ThemeRecord>,
    /// Frosted glass and film grain (default on; FR-2.19).
    pub visual_effects: bool,
    /// Default off (FR-2.23).
    pub hide_empty_channels: bool,
    /// "Show talking people first" (default off; FR-2.23).
    pub talking_first: bool,
    /// Default on (FR-2.26).
    pub show_weak_connection_icon: bool,
    /// "Allow direct connections to other Mumcord users" (FR-2.36).
    pub allow_direct_connections: Consent,
    /// "Check for new versions" (FR-2.56).
    pub update_check: Consent,
    /// AppImage only: the one-time "Add Mumcord to the app menu" offer (FR-2.55).
    pub appimage_menu_entry: Consent,
    /// The one-time "window overlay removed" notice was shown (FR-2.33).
    pub window_overlay_notice_shown: bool,
    /// "Show automatically after updates" for the "What's new" window (default on; issue #145).
    pub whats_new_auto: bool,
    /// The version whose "What's new" was shown or skipped last; `null` before the first start
    /// that knew about it.
    pub whats_new_seen: Option<String>,
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
            language: s.language.as_deref().and_then(Language::from_code),
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
            input_gain_percent: s.input_gain_percent,
            output_volume_percent: s.output_volume_percent,
            music_quality: s.music_quality,
            low_latency: s.low_latency,
            bitrate_kbps: s.bitrate_kbps,
            video_ducking: s.video_ducking,
            video_ducking_percent: s.video_ducking_percent,
            sounds: SoundSettings {
                join_leave: s.sounds.user_joined || s.sounds.user_left,
                self_mute: s.sounds.self_mute,
                self_deaf: s.sounds.self_deafen,
                messages: s.sounds.new_message,
                mention: s.sounds.mention,
                mention_notify: s.sounds.mention_notify,
                voice_join: s.sounds.voice_join,
                hang_up: s.sounds.hang_up,
                party_started: s.sounds.party_started,
                party_joined: s.sounds.party_joined,
                party_left: s.sounds.party_left,
                share_started: s.sounds.share_started,
                share_stopped: s.sounds.share_stopped,
                viewer_started: s.sounds.viewer_started,
                viewer_stopped: s.sounds.viewer_stopped,
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
            preview_cache_days: s.preview_cache_days,
            history_max_age_days: s.history_max_age_days,
            now_playing: s.now_playing,
            now_gaming: s.now_gaming,
            history_sharing: s.history_sharing,
            keyboard_hints: s.keyboard_hints,
            announcements: s.announcements,
            skip_links: s.skip_links,
            offer_full_images: s.offer_full_images,
            load_full_images: s.load_full_images,
            positional: PositionalSettings {
                enabled: s.positional.enabled,
                min_distance: s.positional.min_distance,
                max_distance: s.positional.max_distance,
            },
            max_connected_servers: s.max_connected_servers,
            app_theme_id: s.app_theme_id.clone(),
            custom_themes: s
                .custom_themes
                .iter()
                .map(|t| ThemeRecord {
                    id: t.id.clone(),
                    name: t.name.clone(),
                    colors: t.colors.clone(),
                    colors_dark: t.colors_dark.clone(),
                    colors_light: t.colors_light.clone(),
                    light_generated: t.light_generated,
                    dark_generated: t.dark_generated,
                })
                .collect(),
            visual_effects: s.visual_effects,
            hide_empty_channels: s.hide_empty_channels,
            talking_first: s.talking_first,
            show_weak_connection_icon: s.show_weak_connection_icon,
            allow_direct_connections: s.allow_direct_connections.into(),
            update_check: s.update_check.into(),
            appimage_menu_entry: s.appimage_menu_entry.into(),
            window_overlay_notice_shown: s.window_overlay_notice_shown,
            whats_new_auto: s.whats_new_auto,
            whats_new_seen: s.whats_new_seen.clone(),
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
        s.language = self.language.map(|l| l.code().to_owned());
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
        s.input_gain_percent = self.input_gain_percent.min(200);
        s.output_volume_percent = self.output_volume_percent.min(100);
        s.music_quality = self.music_quality;
        s.low_latency = self.low_latency;
        s.bitrate_kbps = self.bitrate_kbps;
        s.video_ducking = self.video_ducking;
        s.video_ducking_percent = self.video_ducking_percent.clamp(
            store::MIN_VIDEO_DUCKING_PERCENT,
            store::MAX_VIDEO_DUCKING_PERCENT,
        );
        s.sounds.user_joined = self.sounds.join_leave;
        s.sounds.user_left = self.sounds.join_leave;
        s.sounds.self_mute = self.sounds.self_mute;
        s.sounds.self_deafen = self.sounds.self_deaf;
        s.sounds.new_message = self.sounds.messages;
        s.sounds.mention = self.sounds.mention;
        s.sounds.mention_notify = self.sounds.mention_notify;
        s.sounds.voice_join = self.sounds.voice_join;
        s.sounds.hang_up = self.sounds.hang_up;
        s.sounds.party_started = self.sounds.party_started;
        s.sounds.party_joined = self.sounds.party_joined;
        s.sounds.party_left = self.sounds.party_left;
        s.sounds.share_started = self.sounds.share_started;
        s.sounds.share_stopped = self.sounds.share_stopped;
        s.sounds.viewer_started = self.sounds.viewer_started;
        s.sounds.viewer_stopped = self.sounds.viewer_stopped;
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
        // 0 days would make every entry expire at once; treat it as 1.
        s.preview_cache_days = self.preview_cache_days.map(|d| d.max(1));
        s.history_max_age_days = self.history_max_age_days.map(|d| d.max(1));
        s.now_playing = self.now_playing;
        s.now_gaming = self.now_gaming;
        s.history_sharing = self.history_sharing;
        s.keyboard_hints = self.keyboard_hints;
        s.announcements = self.announcements;
        s.skip_links = self.skip_links;
        s.offer_full_images = self.offer_full_images;
        s.load_full_images = self.load_full_images;
        s.positional = positional_to_store(&self.positional);
        s.max_connected_servers = self.max_connected_servers.clamp(
            store::MIN_MAX_CONNECTED_SERVERS,
            store::MAX_MAX_CONNECTED_SERVERS,
        );
        s.app_theme_id = if self.app_theme_id.trim().is_empty() {
            store::DEFAULT_APP_THEME_ID.to_owned()
        } else {
            self.app_theme_id.clone()
        };
        s.custom_themes = self.custom_themes.iter().map(theme_to_store).collect();
        s.visual_effects = self.visual_effects;
        s.hide_empty_channels = self.hide_empty_channels;
        s.talking_first = self.talking_first;
        s.show_weak_connection_icon = self.show_weak_connection_icon;
        s.allow_direct_connections = self.allow_direct_connections.into();
        s.update_check = self.update_check.into();
        s.appimage_menu_entry = self.appimage_menu_entry.into();
        s.window_overlay_notice_shown = self.window_overlay_notice_shown;
        s.whats_new_auto = self.whats_new_auto;
        s.whats_new_seen.clone_from(&self.whats_new_seen);
        s
    }
}

impl From<store::Consent> for Consent {
    fn from(c: store::Consent) -> Self {
        match c {
            store::Consent::Unset => Self::Unset,
            store::Consent::Yes => Self::Yes,
            store::Consent::No => Self::No,
        }
    }
}

impl From<Consent> for store::Consent {
    fn from(c: Consent) -> Self {
        match c {
            Consent::Unset => Self::Unset,
            Consent::Yes => Self::Yes,
            Consent::No => Self::No,
        }
    }
}

/// Keeps only real colour values: themes come from chat messages too (FR-2.16).
fn theme_to_store(t: &ThemeRecord) -> store::Theme {
    let is_hex_colour = |v: &str| {
        v.len() == 7 && v.starts_with('#') && v[1..].bytes().all(|b| b.is_ascii_hexdigit())
    };
    let only_colours = |colours: &BTreeMap<String, String>| {
        colours
            .iter()
            .filter(|(_, v)| is_hex_colour(v))
            .map(|(k, v)| (k.clone(), v.to_ascii_lowercase()))
            .collect()
    };
    store::Theme {
        id: t.id.clone(),
        name: t.name.clone(),
        colors: only_colours(&t.colors),
        colors_dark: only_colours(&t.colors_dark),
        colors_light: only_colours(&t.colors_light),
        light_generated: t.light_generated,
        dark_generated: t.dark_generated,
    }
}

/// Validated distances: finite, `0 ≤ min < max ≤ 1000` m.
fn positional_to_store(p: &PositionalSettings) -> store::PositionalSettings {
    let defaults = store::PositionalSettings::default();
    let finite = |v: f32, default: f32| if v.is_finite() { v } else { default };
    let min = finite(p.min_distance, defaults.min_distance).clamp(0.0, MAX_DISTANCE_M - 1.0);
    let max = finite(p.max_distance, defaults.max_distance).clamp(min + 1.0, MAX_DISTANCE_M);
    store::PositionalSettings {
        enabled: p.enabled,
        min_distance: min,
        max_distance: max,
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

/// The one-time "window overlay is gone" notice is due for users who had it switched on.
fn window_overlay_notice_due(settings: &store::Settings) -> bool {
    settings.legacy_window_overlay_enabled && !settings.window_overlay_notice_shown
}

/// Whether the one-time notice about the removed window overlay still has to be shown.
#[tauri::command]
#[specta::specta]
pub fn window_overlay_notice_pending(engine: State<'_, Engine>) -> ApiResult<bool> {
    Ok(window_overlay_notice_due(&engine.settings()?))
}

/// Marks the notice as shown so it never comes back.
#[tauri::command]
#[specta::specta]
pub async fn window_overlay_notice_dismiss(engine: State<'_, Engine>) -> ApiResult<()> {
    let mut settings = engine.settings()?;
    settings.window_overlay_notice_shown = true;
    engine.update_settings(settings).await?;
    Ok(())
}

/// Whether the user still has to be told that their saved audio device was reset to the system
/// default because it could not be matched to the new device list (issue #183).
#[tauri::command]
#[specta::specta]
pub fn audio_device_notice_pending(engine: State<'_, Engine>) -> ApiResult<bool> {
    Ok(engine.settings()?.audio_device_notice)
}

/// Marks the audio device notice as shown so it never comes back.
#[tauri::command]
#[specta::specta]
pub async fn audio_device_notice_dismiss(engine: State<'_, Engine>) -> ApiResult<()> {
    let mut settings = engine.settings()?;
    settings.audio_device_notice = false;
    engine.update_settings(settings).await?;
    Ok(())
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
    if stored.language != previous.language {
        crate::tray::language_changed(&app);
    }
    Ok(Settings::from_store(&stored))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_code_round_trips_through_the_store() {
        let base = store::Settings::default();
        for language in Language::ALL {
            let stored = Settings {
                language: Some(language),
                ..Settings::from_store(&base)
            }
            .to_store(&base);
            assert_eq!(stored.language.as_deref(), Some(language.code()));
            assert_eq!(Settings::from_store(&stored).language, Some(language));
            let json = serde_json::to_string(&language).expect("serialise language");
            assert_eq!(json, format!("\"{}\"", language.code()));
        }
    }

    #[test]
    fn round_trip_keeps_stage_1b_fields() {
        let base = store::Settings {
            enabled_plugins: vec!["gtav".into()],
            language: Some("de".into()),
            ..Default::default()
        };

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
        ui.sounds.share_stopped = false;
        ui.log_level = LogLevel::Debug;
        ui.input_device = Some(String::new());
        ui.send_default_avatar = false;
        ui.ptt_compatibility_mode = true;
        ui.autostart = true;
        ui.start_minimized = false;
        ui.connect_muted = false;
        ui.link_previews = false;
        assert_eq!(ui.preview_cache_days, Some(7));
        assert_eq!(ui.history_max_age_days, None);
        ui.preview_cache_days = Some(14);
        ui.history_max_age_days = Some(30);
        ui.now_playing = true;
        ui.now_gaming = true;
        assert!(ui.history_sharing, "history sharing is on by default");
        ui.history_sharing = false;
        assert!(ui.keyboard_hints && ui.announcements && ui.skip_links);
        ui.skip_links = false;
        assert!(ui.offer_full_images && ui.load_full_images);
        ui.offer_full_images = false;
        ui.load_full_images = false;
        ui.chat_display = ChatDisplay::Compact;
        let stored = ui.to_store(&base);
        assert_eq!(stored.chat_display, store::ChatDisplay::Compact);
        assert!(!stored.send_default_avatar);
        assert!(stored.ptt_compatibility_mode);
        assert!(stored.autostart && !stored.start_minimized);
        assert!(!stored.connect_muted);
        assert!(!stored.link_previews);
        assert_eq!(stored.preview_cache_days, Some(14));
        assert_eq!(stored.history_max_age_days, Some(30));
        assert!(stored.now_playing);
        assert!(stored.now_gaming);
        assert!(!stored.history_sharing);
        assert!(stored.keyboard_hints && stored.announcements && !stored.skip_links);
        assert!(!stored.offer_full_images && !stored.load_full_images);
        assert_eq!(stored.voice_activation, store::VoiceActivation::Manual);
        assert_eq!(stored.voice_threshold_dbfs, -25);
        let too_loud = Settings {
            voice_threshold_dbfs: 12,
            ..ui.clone()
        };
        assert_eq!(too_loud.to_store(&base).voice_threshold_dbfs, 0);
        assert_eq!(stored.enabled_plugins, ["gtav"]);
        assert!(stored.wizard_completed);
        assert!(!stored.sounds.user_joined && !stored.sounds.user_left);
        assert!(!stored.sounds.hang_up && stored.sounds.voice_join);
        assert!(!stored.sounds.share_stopped && stored.sounds.party_started);
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
            language: Some("nb".into()),
            ..store::Settings::default()
        };
        assert_eq!(Settings::from_store(&s).language, None);
    }

    #[test]
    fn positional_distances_are_kept_valid() {
        let base = store::Settings::default();
        let mut ui = Settings::from_store(&base);
        assert!(!ui.positional.enabled);
        assert_eq!(
            (ui.positional.min_distance, ui.positional.max_distance),
            (1.0, 15.0)
        );
        ui.positional = PositionalSettings {
            enabled: true,
            min_distance: 5.0,
            max_distance: 2.0,
        };
        let stored = ui.to_store(&base).positional;
        assert!(stored.enabled);
        assert_eq!((stored.min_distance, stored.max_distance), (5.0, 6.0));
        ui.positional.min_distance = f32::NAN;
        ui.positional.max_distance = 1e9;
        let stored = ui.to_store(&base).positional;
        assert_eq!(
            (stored.min_distance, stored.max_distance),
            (1.0, MAX_DISTANCE_M)
        );
    }

    #[test]
    fn zero_day_limits_are_raised_to_one_day() {
        let base = store::Settings::default();
        let mut ui = Settings::from_store(&base);
        ui.preview_cache_days = Some(0);
        ui.history_max_age_days = Some(0);
        let stored = ui.to_store(&base);
        assert_eq!(stored.preview_cache_days, Some(1));
        assert_eq!(stored.history_max_age_days, Some(1));
    }

    #[test]
    fn window_overlay_notice_is_due_only_for_legacy_users_who_have_not_seen_it() {
        for (legacy, shown, due) in [
            (false, false, false),
            (false, true, false),
            (true, false, true),
            (true, true, false),
        ] {
            let settings = store::Settings {
                legacy_window_overlay_enabled: legacy,
                window_overlay_notice_shown: shown,
                ..store::Settings::default()
            };
            assert_eq!(window_overlay_notice_due(&settings), due);
        }
    }

    #[test]
    fn epic_2_fields_round_trip_with_limits() {
        let base = store::Settings::default();
        let mut ui = Settings::from_store(&base);
        assert_eq!(ui.max_connected_servers, 5);
        assert_eq!(ui.app_theme_id, "mumcord-dark");
        assert!(ui.visual_effects && ui.show_weak_connection_icon);
        assert!(!ui.hide_empty_channels && !ui.talking_first);
        assert_eq!(ui.allow_direct_connections, Consent::Unset);
        assert_eq!(ui.update_check, Consent::Unset);

        ui.max_connected_servers = 40;
        ui.app_theme_id = " ".into();
        ui.custom_themes = vec![ThemeRecord {
            id: "custom-1".into(),
            name: "Mine".into(),
            colors: BTreeMap::new(),
            colors_dark: BTreeMap::from([
                ("accent".into(), "#FF8800".into()),
                ("text".into(), "url(evil)".into()),
            ]),
            colors_light: BTreeMap::from([("text".into(), "red; x".into())]),
            light_generated: true,
            dark_generated: false,
        }];
        ui.talking_first = true;
        ui.allow_direct_connections = Consent::No;
        ui.update_check = Consent::Yes;
        ui.window_overlay_notice_shown = true;
        assert!(ui.whats_new_auto);
        assert_eq!(ui.whats_new_seen, None);
        ui.whats_new_auto = false;
        ui.whats_new_seen = Some("0.4.4".into());
        let stored = ui.to_store(&base);
        assert_eq!(stored.max_connected_servers, 15);
        assert_eq!(stored.app_theme_id, "mumcord-dark");
        assert_eq!(
            stored.custom_themes[0].colors_dark,
            BTreeMap::from([("accent".into(), "#ff8800".into())])
        );
        assert!(stored.custom_themes[0].colors_light.is_empty());
        assert!(stored.custom_themes[0].light_generated);

        let back = Settings::from_store(&stored);
        assert!(back.talking_first && back.window_overlay_notice_shown);
        assert!(!back.whats_new_auto);
        assert_eq!(back.whats_new_seen.as_deref(), Some("0.4.4"));
        assert_eq!(back.allow_direct_connections, Consent::No);
        assert_eq!(back.update_check, Consent::Yes);
        assert_eq!(back.custom_themes[0].name, "Mine");

        ui.max_connected_servers = 0;
        assert_eq!(ui.to_store(&base).max_connected_servers, 1);
    }

    #[test]
    fn video_ducking_round_trips_within_its_range() {
        let base = store::Settings::default();
        let mut ui = Settings::from_store(&base);
        assert!(ui.video_ducking);
        assert_eq!(ui.video_ducking_percent, 30);
        ui.video_ducking = false;
        ui.video_ducking_percent = 60;
        let stored = ui.to_store(&base);
        assert!(!stored.video_ducking);
        assert_eq!(stored.video_ducking_percent, 60);
        ui.video_ducking_percent = 5;
        assert_eq!(ui.to_store(&base).video_ducking_percent, 10);
        ui.video_ducking_percent = 90;
        assert_eq!(ui.to_store(&base).video_ducking_percent, 80);
    }
}
