//! People on the server and local per-person controls (FR-1.17–1.19).

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use super::types::{ApiResult, ChannelId, SessionId, not_implemented};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub session: SessionId,
    pub channel_id: ChannelId,
    pub name: String,
    /// `null` for people without a client certificate.
    pub cert_hash: Option<String>,
    pub registered: bool,
    pub self_mute: bool,
    pub self_deaf: bool,
    /// Muted or suppressed by the server/an admin.
    pub server_mute: bool,
    pub server_deaf: bool,
    /// Channels this person listens to (ear icon).
    pub listening_channel_ids: Vec<ChannelId>,
    pub has_avatar: bool,
    pub has_comment: bool,
    /// Local playback volume in percent, 0–200 (FR-1.18).
    pub volume: u16,
    pub local_mute: bool,
}

/// Comment and avatar, fetched on demand when the profile card opens.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersonProfile {
    /// Sanitized HTML.
    pub comment: Option<String>,
    /// `data:` URL of the avatar image.
    pub avatar: Option<String>,
}

/// A person appeared or changed (channel, mute state, name, …).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct UserUpserted {
    pub user: User,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct UserRemoved {
    pub session: SessionId,
}

/// Someone (including the own user) started or stopped talking.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct TalkStateChanged {
    pub session: SessionId,
    pub talking: bool,
}

/// Sets local volume (0–200 %); remembered by certificate hash per server.
#[tauri::command]
#[specta::specta]
pub fn person_set_volume(session: SessionId, volume: u16) -> ApiResult<()> {
    let _ = (session, volume);
    not_implemented()
}

/// Mutes a person locally; remembered by certificate hash per server.
#[tauri::command]
#[specta::specta]
pub fn person_set_local_mute(session: SessionId, muted: bool) -> ApiResult<()> {
    let _ = (session, muted);
    not_implemented()
}

#[tauri::command]
#[specta::specta]
pub fn person_profile(session: SessionId) -> ApiResult<PersonProfile> {
    let _ = session;
    not_implemented()
}
