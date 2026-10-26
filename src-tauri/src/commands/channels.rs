//! Channel tree, joining channels and channel passwords (FR-1.15, FR-1.16, FR-2.78).

use mumcord_engine::{ChannelPasswordState, ChannelView, Engine};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::{ApiResult, ChannelId, ServerId, to_engine_server_id};

/// What the server's ACL allows the own user in a channel (from PermissionQuery).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPermissions {
    pub enter: bool,
    pub text_message: bool,
    pub listen: bool,
    /// Only meaningful on the root channel: "Register self" (FR-1.34).
    pub register_self: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelInfo {
    pub id: ChannelId,
    /// `null` for the root channel.
    pub parent_id: Option<ChannelId>,
    pub name: String,
    /// Sort position among siblings (ties sorted by name).
    pub position: i32,
    pub permissions: ChannelPermissions,
    /// Sanitized description HTML; `null` when there is none or it has not been fetched yet.
    pub description: Option<String>,
    /// True when the channel has a description, even if only its hash is known yet; then
    /// `channel_description_request` fetches the text (FR-1.48b).
    pub has_description: bool,
}

/// A channel was added or changed.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ChannelUpserted {
    pub server_id: ServerId,
    pub channel: ChannelInfo,
}

/// Several channels changed at once, e.g. a burst of permission answers after connecting; sent
/// as one event so the UI updates once instead of once per channel.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsUpserted {
    pub server_id: ServerId,
    pub channels: Vec<ChannelInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRemoved {
    pub server_id: ServerId,
    pub channel_id: ChannelId,
}

/// What the channel-password dialog shows for a refused join (FR-2.78).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum ChannelPasswordStep {
    /// The join was refused: ask for a password.
    Required,
    /// The typed password was refused too.
    Wrong,
    /// The attempt is over (joined, or refused for another reason).
    Ended,
}

impl From<ChannelPasswordState> for ChannelPasswordStep {
    fn from(state: ChannelPasswordState) -> Self {
        match state {
            ChannelPasswordState::Required => Self::Required,
            ChannelPasswordState::Wrong => Self::Wrong,
            ChannelPasswordState::Ended => Self::Ended,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPasswordPrompt {
    pub server_id: ServerId,
    pub channel_id: ChannelId,
    pub step: ChannelPasswordStep,
}

impl From<ChannelView> for ChannelInfo {
    fn from(v: ChannelView) -> Self {
        Self {
            id: v.id,
            parent_id: v.parent_id,
            name: v.name,
            position: v.position,
            permissions: ChannelPermissions {
                enter: v.permissions.enter,
                text_message: v.permissions.text_message,
                listen: v.permissions.listen,
                register_self: v.permissions.register_self,
            },
            description: v.description,
            has_description: v.has_description,
        }
    }
}

/// Enters the channel's voice; fails with `permissionDenied` if not permitted.
#[tauri::command]
#[specta::specta]
pub fn channel_join(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
) -> ApiResult<()> {
    Ok(engine.join_channel(to_engine_server_id(server_id), channel_id)?)
}

/// Retries a refused join with a channel password, which is stored for the server; the
/// outcome arrives as `channelPasswordPrompt` (FR-2.78).
#[tauri::command]
#[specta::specta]
pub async fn channel_join_with_password(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
    password: String,
) -> ApiResult<()> {
    Ok(engine
        .channel_join_with_password(to_engine_server_id(server_id), channel_id, password)
        .await?)
}

/// Number of stored channel passwords of a server; they are never handed out.
#[tauri::command]
#[specta::specta]
pub async fn channel_password_count(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> ApiResult<u32> {
    Ok(engine
        .channel_password_count(to_engine_server_id(server_id))
        .await?)
}

/// Removes stored channel passwords by their position in the list.
#[tauri::command]
#[specta::specta]
pub async fn channel_passwords_remove(
    engine: State<'_, Engine>,
    server_id: ServerId,
    indices: Vec<u32>,
) -> ApiResult<()> {
    Ok(engine
        .channel_passwords_remove(to_engine_server_id(server_id), indices)
        .await?)
}

/// Fetches a description the server announced only by its hash (FR-1.48b). The text arrives
/// as `ChannelUpserted`; repeated calls for the same description send nothing.
#[tauri::command]
#[specta::specta]
pub fn channel_description_request(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
) -> ApiResult<()> {
    Ok(engine.channel_description_request(to_engine_server_id(server_id), channel_id)?)
}
