//! People on the server and local per-person controls (FR-1.17–1.19).

use mumcord_engine::{Engine, ProfileView, UserView};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::{ApiResult, ChannelId, SessionId};

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
    /// Hex of the avatar texture hash; changes when the avatar is replaced.
    pub avatar_hash: Option<String>,
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

impl From<UserView> for User {
    fn from(v: UserView) -> Self {
        Self {
            session: v.session,
            channel_id: v.channel_id,
            name: v.name,
            cert_hash: v.cert_hash,
            registered: v.registered,
            self_mute: v.self_mute,
            self_deaf: v.self_deaf,
            server_mute: v.server_mute,
            server_deaf: v.server_deaf,
            listening_channel_ids: v.listening_channel_ids,
            has_avatar: v.has_avatar,
            avatar_hash: v.avatar_hash,
            has_comment: v.has_comment,
            volume: v.volume,
            local_mute: v.local_mute,
        }
    }
}

impl From<ProfileView> for PersonProfile {
    fn from(v: ProfileView) -> Self {
        Self {
            comment: v.comment,
            avatar: v.avatar,
        }
    }
}

/// Sets local volume (0–200 %); remembered by certificate hash per server.
#[tauri::command]
#[specta::specta]
pub fn person_set_volume(
    engine: State<'_, Engine>,
    session: SessionId,
    volume: u16,
) -> ApiResult<()> {
    Ok(engine.person_set_volume(session, volume)?)
}

/// Mutes a person locally; remembered by certificate hash per server.
#[tauri::command]
#[specta::specta]
pub fn person_set_local_mute(
    engine: State<'_, Engine>,
    session: SessionId,
    muted: bool,
) -> ApiResult<()> {
    Ok(engine.person_set_local_mute(session, muted)?)
}

/// Comment and avatar of a person, fetched from the server when not known yet.
#[tauri::command]
#[specta::specta]
pub async fn person_profile(
    engine: State<'_, Engine>,
    session: SessionId,
) -> ApiResult<PersonProfile> {
    Ok(engine.person_profile(session).await?.into())
}
