//! The single settings record (§7, FR-1.23, FR-1.27, FR-1.35, FR-1.36, FR-1.38).

use serde::{Deserialize, Serialize};
use specta::Type;

use super::types::{ApiResult, not_implemented};

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
pub enum CloseBehaviour {
    Ask,
    MinimizeToTray,
    Quit,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

/// Notification sound toggles per event (FR-1.27).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SoundSettings {
    pub join_leave: bool,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub messages: bool,
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
    /// Shortcut as returned by `ptt_capture_key`; `null` if none bound.
    pub ptt_key: Option<String>,
    /// Device ids from `audio_devices`; `null` uses the OS default.
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub noise_suppression: bool,
    pub echo_cancellation: bool,
    pub auto_gain: bool,
    pub sounds: SoundSettings,
    pub close_behaviour: CloseBehaviour,
    pub log_level: LogLevel,
}

#[tauri::command]
#[specta::specta]
pub fn settings_get() -> ApiResult<Settings> {
    not_implemented()
}

/// Replaces the whole record and applies changes immediately; returns the stored record.
#[tauri::command]
#[specta::specta]
pub fn settings_update(settings: Settings) -> ApiResult<Settings> {
    let _ = settings;
    not_implemented()
}
