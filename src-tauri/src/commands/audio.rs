//! Audio devices and the microphone test (FR-1.4, §6).

use mumcord_engine::{AudioDeviceList, AudioDeviceView, Engine, MicTestPhase as EnginePhase};
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

/// Records up to 5 s of processed own voice with level metering, then plays it back.
/// Voice of a connected session pauses meanwhile.
#[tauri::command]
#[specta::specta]
pub async fn mic_test_start(engine: State<'_, Engine>) -> ApiResult<()> {
    Ok(engine.mic_test_start().await?)
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
