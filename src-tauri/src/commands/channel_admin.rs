//! The Channel window (Epic 4, issue #250): create, edit, move, link and delete channels.
//!
//! Each command waits for the server's answer. A refusal is a normal result with the server's
//! reason, so the open window words it (FR-10.59, FR-10.60) and no second notice shows.

use mumcord_engine::{ChannelChange, ChannelCreate, ChannelOutcome, Engine};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::session::{DenialKind, Right};
use super::types::{ApiResult, ChannelId, ServerId, to_engine_server_id};

/// A new channel from the Channel window's General tab (FR-10.21, FR-10.22).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelCreateInput {
    pub parent_id: ChannelId,
    pub name: String,
    /// Clean HTML from the description editor; `None` for none.
    pub description: Option<String>,
    pub position: i32,
    /// 0 is the server's own limit.
    pub max_users: u32,
    pub temporary: bool,
}

/// What the Channel window changed; unset fields stay as they are (FR-10.23, FR-10.24, FR-10.27).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelChangeInput {
    pub name: Option<String>,
    /// An empty string removes the description.
    pub description: Option<String>,
    /// A new parent moves the channel with all its sub-channels.
    pub parent_id: Option<ChannelId>,
    pub position: Option<i32>,
    pub max_users: Option<u32>,
    pub links_add: Vec<ChannelId>,
    pub links_remove: Vec<ChannelId>,
}

/// The server's answer to a channel change.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChannelResult {
    /// Applied; `channelId` is the new channel after a create.
    #[serde(rename_all = "camelCase")]
    Confirmed { channel_id: ChannelId },
    /// No answer within 5 s; the change was written to the server.
    Unconfirmed,
    /// Refused, with the kind, the missing right and the server's own text (untrusted).
    Refused {
        denial: DenialKind,
        permission: Option<Right>,
        reason: Option<String>,
    },
}

impl From<ChannelOutcome> for ChannelResult {
    fn from(outcome: ChannelOutcome) -> Self {
        match outcome {
            ChannelOutcome::Confirmed { channel_id } => Self::Confirmed { channel_id },
            ChannelOutcome::Unconfirmed => Self::Unconfirmed,
            ChannelOutcome::Refused {
                kind,
                permission,
                reason,
            } => Self::Refused {
                denial: kind.into(),
                permission: permission.map(Into::into),
                reason,
            },
        }
    }
}

/// Creates a channel (FR-10.21, FR-10.22). A temporary channel moves you into it.
#[tauri::command]
#[specta::specta]
pub async fn channel_create(
    engine: State<'_, Engine>,
    server_id: ServerId,
    input: ChannelCreateInput,
) -> ApiResult<ChannelResult> {
    let request = ChannelCreate {
        parent: input.parent_id,
        name: input.name,
        description: input.description.filter(|d| !d.is_empty()),
        position: input.position,
        max_users: input.max_users,
        temporary: input.temporary,
    };
    let outcome = engine
        .channel_create(to_engine_server_id(server_id), &request)
        .await?;
    Ok(outcome.into())
}

/// Edits, moves or links a channel (FR-10.23, FR-10.24, FR-10.27); also the menu's quick "Link with
/// my channel" and "Unlink all".
#[tauri::command]
#[specta::specta]
pub async fn channel_edit(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
    change: ChannelChangeInput,
) -> ApiResult<ChannelResult> {
    let change = ChannelChange {
        name: change.name,
        description: change.description,
        parent: change.parent_id,
        position: change.position,
        max_users: change.max_users,
        links_add: change.links_add,
        links_remove: change.links_remove,
    };
    let outcome = engine
        .channel_edit(to_engine_server_id(server_id), channel_id, &change)
        .await?;
    Ok(outcome.into())
}

/// Deletes a channel with its sub-channels; the people in them go to the parent (FR-10.25).
#[tauri::command]
#[specta::specta]
pub async fn channel_remove(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
) -> ApiResult<ChannelResult> {
    let outcome = engine
        .channel_remove(to_engine_server_id(server_id), channel_id)
        .await?;
    Ok(outcome.into())
}
