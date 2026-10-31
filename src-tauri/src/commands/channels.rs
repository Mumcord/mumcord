//! Channel tree, joining channels and channel passwords (FR-1.15, FR-1.16, FR-2.78).

use mumcord_engine::{ChannelPasswordState, ChannelPasswordView, ChannelView, Engine};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::chat::rfc3339;
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
    pub speak: bool,
    pub whisper: bool,
    /// Edit the channel's ACL; only true once the server confirmed it.
    pub write: bool,
    /// Move other people out of this channel (drag a person, "Move users here…"); only true
    /// once the server confirmed it.
    pub move_users: bool,
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
    /// User limit; `null` when the channel has none (the server's limit still applies).
    pub max_users: Option<u32>,
    pub temporary: bool,
    /// Channels linked to this one, sorted by id.
    pub links: Vec<ChannelId>,
    /// Entering needs a password (access token) or group membership.
    pub enter_restricted: bool,
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

/// A stored channel password without the password itself (FR-2.78, issue #146).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedChannelPassword {
    /// The user's own name; `null` shows the channels, or "Password N" without any.
    pub label: Option<String>,
    /// The channels the password was entered for.
    pub channels: Vec<SavedPasswordChannel>,
    /// RFC 3339; `null` if not used since Mumcord remembers it.
    pub last_used_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedPasswordChannel {
    pub channel_id: ChannelId,
    /// Path below the root as the channel was named then.
    pub path: Vec<String>,
    /// `false` when the channel was renamed or removed; `null` while not connected.
    pub found: Option<bool>,
}

impl From<ChannelPasswordView> for SavedChannelPassword {
    fn from(v: ChannelPasswordView) -> Self {
        Self {
            label: v.label,
            channels: v
                .channels
                .into_iter()
                .map(|c| SavedPasswordChannel {
                    channel_id: c.channel_id,
                    path: c.path,
                    found: c.found,
                })
                .collect(),
            last_used_at: v.last_used_at.map(rfc3339),
        }
    }
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
                speak: v.permissions.speak,
                whisper: v.permissions.whisper,
                write: v.permissions.write,
                move_users: v.permissions.move_users,
            },
            description: v.description,
            has_description: v.has_description,
            max_users: v.max_users,
            temporary: v.temporary,
            links: v.links,
            enter_restricted: v.enter_restricted,
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

/// Stops voice on the voice server but keeps every connection: the server turns read-only
/// (self-deafened) and no server has voice until the next explicit join.
#[tauri::command]
#[specta::specta]
pub fn voice_leave(engine: State<'_, Engine>) -> ApiResult<()> {
    engine.leave_voice();
    Ok(())
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

/// The stored channel passwords of a server, in order, with the channels they were entered
/// for; the passwords themselves are never handed out.
#[tauri::command]
#[specta::specta]
pub async fn channel_passwords(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> ApiResult<Vec<SavedChannelPassword>> {
    Ok(engine
        .channel_passwords(to_engine_server_id(server_id))
        .await?
        .into_iter()
        .map(Into::into)
        .collect())
}

/// Names a stored channel password by its position; an empty label goes back to the default.
#[tauri::command]
#[specta::specta]
pub async fn channel_password_rename(
    engine: State<'_, Engine>,
    server_id: ServerId,
    index: u32,
    label: String,
) -> ApiResult<()> {
    Ok(engine
        .channel_password_rename(to_engine_server_id(server_id), index, label)
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
