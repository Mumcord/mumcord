//! Channel tree and joining channels (FR-1.15, FR-1.16).

use mumcord_engine::{ChannelView, Engine};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::{ApiResult, ChannelId};

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
    pub channel: ChannelInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRemoved {
    pub channel_id: ChannelId,
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
pub fn channel_join(engine: State<'_, Engine>, channel_id: ChannelId) -> ApiResult<()> {
    Ok(engine.join_channel(channel_id)?)
}

/// Fetches a description the server announced only by its hash (FR-1.48b). The text arrives
/// as `ChannelUpserted`; repeated calls for the same description send nothing.
#[tauri::command]
#[specta::specta]
pub fn channel_description_request(
    engine: State<'_, Engine>,
    channel_id: ChannelId,
) -> ApiResult<()> {
    Ok(engine.channel_description_request(channel_id)?)
}
