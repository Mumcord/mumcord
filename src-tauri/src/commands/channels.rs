//! Channel tree and joining channels (FR-1.15, FR-1.16).

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use super::types::{ApiResult, ChannelId, not_implemented};

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

/// Enters the channel's voice; fails with `permissionDenied` if not permitted.
#[tauri::command]
#[specta::specta]
pub fn channel_join(channel_id: ChannelId) -> ApiResult<()> {
    let _ = channel_id;
    not_implemented()
}
