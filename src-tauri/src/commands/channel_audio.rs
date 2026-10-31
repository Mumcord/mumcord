//! Audio profiles per channel (issue #171, FR-3.62–FR-3.64): local overrides of Music quality,
//! low-latency mode and the bitrate cap while the user is in a channel.

use mumcord_engine::store::ChannelAudioProfile as StoreProfile;
use mumcord_engine::{ActiveAudioProfile as EngineActive, ChannelAudioView, Engine};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::{ApiResult, ChannelId, ServerId, to_api_server_id, to_engine_server_id};

/// The overrides of a channel audio profile; `null` keeps the global setting ("default").
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelAudioProfile {
    pub music_quality: Option<bool>,
    pub low_latency: Option<bool>,
    /// Own bitrate cap in kbit/s, 16 to 510; the server's limit still wins.
    pub bitrate_kbps: Option<u32>,
}

/// A channel of the server that has a profile.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelAudioEntry {
    pub channel_id: ChannelId,
    pub profile: ChannelAudioProfile,
}

/// The profile in effect: the own channel's on the server that carries voice.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAudioProfile {
    pub server_id: ServerId,
    pub channel_id: ChannelId,
    pub profile: ChannelAudioProfile,
}

/// The audio profiles of a server's channels changed.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ChannelAudioProfilesChanged {
    pub server_id: ServerId,
    pub profiles: Vec<ChannelAudioEntry>,
}

/// The channel profile in effect changed: the user joined or left a channel with a profile, or
/// its profile was edited. `null` runs on the global settings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAudioProfileChanged {
    pub active: Option<ActiveAudioProfile>,
}

impl From<StoreProfile> for ChannelAudioProfile {
    fn from(p: StoreProfile) -> Self {
        Self {
            music_quality: p.music_quality,
            low_latency: p.low_latency,
            bitrate_kbps: p.bitrate_kbps,
        }
    }
}

impl From<ChannelAudioProfile> for StoreProfile {
    fn from(p: ChannelAudioProfile) -> Self {
        Self {
            music_quality: p.music_quality,
            low_latency: p.low_latency,
            bitrate_kbps: p.bitrate_kbps,
        }
    }
}

impl From<ChannelAudioView> for ChannelAudioEntry {
    fn from(v: ChannelAudioView) -> Self {
        Self {
            channel_id: v.channel_id,
            profile: v.profile.into(),
        }
    }
}

impl From<EngineActive> for ActiveAudioProfile {
    fn from(a: EngineActive) -> Self {
        Self {
            server_id: to_api_server_id(a.server_id),
            channel_id: a.channel_id,
            profile: a.profile.into(),
        }
    }
}

/// The channels of a connected server that have an audio profile.
#[tauri::command]
#[specta::specta]
pub fn channel_audio_profiles(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> ApiResult<Vec<ChannelAudioEntry>> {
    Ok(engine
        .channel_audio_profiles(to_engine_server_id(server_id))?
        .into_iter()
        .map(Into::into)
        .collect())
}

/// Saves the audio profile of a channel, or removes it with `null` ("Remove profile"). Applies
/// at once when the user is in that channel.
#[tauri::command]
#[specta::specta]
pub fn channel_audio_profile_set(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
    profile: Option<ChannelAudioProfile>,
) -> ApiResult<()> {
    Ok(engine.channel_audio_profile_set(
        to_engine_server_id(server_id),
        channel_id,
        profile.map(Into::into),
    )?)
}

/// The channel profile in effect now; `null` runs on the global settings. Follow
/// `ActiveAudioProfileChanged` afterwards.
#[tauri::command]
#[specta::specta]
pub fn active_audio_profile(engine: State<'_, Engine>) -> ApiResult<Option<ActiveAudioProfile>> {
    Ok(engine.active_audio_profile().map(Into::into))
}
