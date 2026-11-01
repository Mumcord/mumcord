//! Audio devices and the microphone test (FR-1.4, §6).

use mumcord_engine::{
    AudioDeviceList, AudioDeviceView, Engine, MicTestDevices as EngineMicTestDevices,
    MicTestPhase as EnginePhase,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::ApiResult;

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

/// The chosen device is gone and the system default plays in its place.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceFallback {
    /// `true` for the microphone, `false` for the speakers.
    pub input: bool,
}

/// The highest bitrate the bitrate slider can reach changed (FR-2.64), e.g. on connecting or
/// when the voice server changes. `null` without a voice server: show "depends on server".
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct BitrateMaxChanged {
    pub kbps: Option<u32>,
}

/// Processed input level, about 20 times per second while the mic test runs or while connected.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct MicLevel {
    /// 0.0 (silence) to 1.0 (full scale).
    pub level: f32,
}

impl From<AudioDeviceView> for AudioDevice {
    fn from(v: AudioDeviceView) -> Self {
        Self {
            id: v.id,
            name: v.name,
            is_default: v.is_default,
        }
    }
}

impl From<AudioDeviceList> for AudioDevices {
    fn from(v: AudioDeviceList) -> Self {
        Self {
            inputs: v.inputs.into_iter().map(AudioDevice::from).collect(),
            outputs: v.outputs.into_iter().map(AudioDevice::from).collect(),
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn audio_devices(engine: State<'_, Engine>) -> ApiResult<AudioDevices> {
    Ok(engine.audio_devices().await?.into())
}

/// Highest bitrate in kbit/s the bitrate slider can reach (FR-2.64); `null` without a voice
/// server. Follow `BitrateMaxChanged` afterwards.
#[tauri::command]
#[specta::specta]
pub fn bitrate_max(engine: State<'_, Engine>) -> ApiResult<Option<u32>> {
    Ok(engine.bitrate_max().map(|b| b / 1000))
}

/// Devices a microphone test runs on instead of the stored ones (wizard rerun, #149).
/// They are not saved.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct MicTestDevices {
    pub input_device: Option<String>,
    pub output_device: Option<String>,
}

impl From<MicTestDevices> for EngineMicTestDevices {
    fn from(devices: MicTestDevices) -> Self {
        Self {
            input: devices.input_device,
            output: devices.output_device,
        }
    }
}

/// Records up to 5 s of processed own voice with level metering, then plays it back.
/// Voice of a connected session pauses meanwhile. `devices` tests devices that are not saved
/// yet, e.g. a microphone picked in the wizard rerun (#149).
#[tauri::command]
#[specta::specta]
pub async fn mic_test_start(
    engine: State<'_, Engine>,
    devices: Option<MicTestDevices>,
) -> ApiResult<()> {
    Ok(engine.mic_test_start(devices.map(Into::into)).await?)
}

/// Plays a short sound on the output device ("Test sound" in the output popover).
#[tauri::command]
#[specta::specta]
pub fn audio_test_sound(engine: State<'_, Engine>) -> ApiResult<()> {
    engine.audio_test_sound();
    Ok(())
}

/// Plays the connect sound of `theme` ("Preview" next to the sound theme, #325), also before the
/// theme is saved. Only audible while voice audio runs.
#[tauri::command]
#[specta::specta]
pub fn audio_preview_sound_theme(
    engine: State<'_, Engine>,
    theme: super::settings::SoundTheme,
) -> ApiResult<()> {
    engine.preview_sound_theme(theme.into());
    Ok(())
}

/// Ends the test wherever it is (recording or playback).
#[tauri::command]
#[specta::specta]
pub fn mic_test_stop(engine: State<'_, Engine>) -> ApiResult<()> {
    engine.mic_test_stop();
    Ok(())
}

/// Phase of the mic test: `recording`, then `playing` the recording back, then `done`;
/// `failed` when the test could not start.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum MicTestPhase {
    Recording,
    Playing,
    Done,
    Failed,
}

impl From<EnginePhase> for MicTestPhase {
    fn from(v: EnginePhase) -> Self {
        match v {
            EnginePhase::Recording => Self::Recording,
            EnginePhase::Playing => Self::Playing,
            EnginePhase::Done => Self::Done,
            EnginePhase::Failed => Self::Failed,
        }
    }
}

/// The mic test changed phase.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct MicTestPhaseChanged {
    pub phase: MicTestPhase,
}
