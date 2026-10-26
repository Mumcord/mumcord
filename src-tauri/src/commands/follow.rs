//! Following channels and unread counts (FR-1.31).

use mumcord_engine::{Engine, FollowAllSummary as EngineSummary};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::{ApiResult, ChannelId, Conversation, ServerId, to_engine_server_id};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct FollowedChannelsChanged {
    pub server_id: ServerId,
    pub channel_ids: Vec<ChannelId>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct UnreadChanged {
    pub server_id: ServerId,
    pub conversation: Conversation,
    pub count: u32,
}

/// Starts listening to the channel with its audio muted locally.
/// Fails with `serverTooOld`, `permissionDenied` or `listenerLimitReached`.
#[tauri::command]
#[specta::specta]
pub async fn channel_follow(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
) -> ApiResult<()> {
    Ok(engine
        .channel_follow(to_engine_server_id(server_id), channel_id)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub fn channel_unfollow(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
) -> ApiResult<()> {
    Ok(engine.channel_unfollow(to_engine_server_id(server_id), channel_id)?)
}

/// Outcome of "Follow all channels" (QA.13); already followed channels are not counted.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct FollowAllSummary {
    pub followed: u32,
    /// Skipped because the server's listener limit was reached.
    pub limit_reached: u32,
    /// Skipped because the user may not listen to them.
    pub not_allowed: u32,
    /// Skipped because the server did not answer in time.
    pub no_answer: u32,
}

impl From<EngineSummary> for FollowAllSummary {
    fn from(s: EngineSummary) -> Self {
        Self {
            followed: s.followed,
            limit_reached: s.limit_reached,
            not_allowed: s.not_allowed,
            no_answer: s.no_answer,
        }
    }
}

/// Follows every channel the user may listen to, except the own voice channel; stops at the
/// server's listener limit. Fails with `serverTooOld`.
#[tauri::command]
#[specta::specta]
pub async fn channel_follow_all(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> ApiResult<FollowAllSummary> {
    Ok(engine
        .channel_follow_all(to_engine_server_id(server_id))
        .await?
        .into())
}

#[tauri::command]
#[specta::specta]
pub fn channel_unfollow_all(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<()> {
    Ok(engine.channel_unfollow_all(to_engine_server_id(server_id))?)
}

/// Resets the unread count of a conversation.
#[tauri::command]
#[specta::specta]
pub fn conversation_mark_read(
    engine: State<'_, Engine>,
    server_id: ServerId,
    conversation: Conversation,
) -> ApiResult<()> {
    Ok(engine.conversation_mark_read(to_engine_server_id(server_id), conversation.into())?)
}
