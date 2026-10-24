//! Audio devices and the microphone test (FR-1.4, §6).

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use super::types::{ApiResult, not_implemented};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevices {
    pub inputs: Vec<AudioDevice>,
    pub outputs: Vec<AudioDevice>,
}

/// Device list changed (hot-plug).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevicesChanged {
    pub devices: AudioDevices,
}

/// Processed input level, about 20 times per second while the mic test runs or while connected.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct MicLevel {
    /// 0.0 (silence) to 1.0 (full scale).
    pub level: f32,
}

#[tauri::command]
#[specta::specta]
pub fn audio_devices() -> ApiResult<AudioDevices> {
    not_implemented()
}

/// Starts level metering and plays the processed own voice back on the output device.
#[tauri::command]
#[specta::specta]
pub fn mic_test_start() -> ApiResult<()> {
    not_implemented()
}

#[tauri::command]
#[specta::specta]
pub fn mic_test_stop() -> ApiResult<()> {
    not_implemented()
}
