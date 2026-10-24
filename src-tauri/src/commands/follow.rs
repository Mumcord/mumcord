//! Following channels and unread counts (FR-1.31).

use mumcord_engine::{Engine, FollowAllSummary as EngineSummary};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::{ApiResult, ChannelId, Conversation};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct FollowedChannelsChanged {
    pub channel_ids: Vec<ChannelId>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct UnreadChanged {
    pub conversation: Conversation,
    pub count: u32,
}

/// Starts listening to the channel with its audio muted locally.
/// Fails with `serverTooOld`, `permissionDenied` or `listenerLimitReached`.
#[tauri::command]
#[specta::specta]
pub async fn channel_follow(engine: State<'_, Engine>, channel_id: ChannelId) -> ApiResult<()> {
    Ok(engine.channel_follow(channel_id).await?)
}

#[tauri::command]
#[specta::specta]
pub fn channel_unfollow(engine: State<'_, Engine>, channel_id: ChannelId) -> ApiResult<()> {
    Ok(engine.channel_unfollow(channel_id)?)
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
pub async fn channel_follow_all(engine: State<'_, Engine>) -> ApiResult<FollowAllSummary> {
    Ok(engine.channel_follow_all().await?.into())
}

#[tauri::command]
#[specta::specta]
pub fn channel_unfollow_all(engine: State<'_, Engine>) -> ApiResult<()> {
    Ok(engine.channel_unfollow_all()?)
}

/// Resets the unread count of a conversation.
#[tauri::command]
#[specta::specta]
pub fn conversation_mark_read(
    engine: State<'_, Engine>,
    conversation: Conversation,
) -> ApiResult<()> {
    Ok(engine.conversation_mark_read(conversation.into())?)
}
