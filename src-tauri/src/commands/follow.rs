//! Following channels and unread counts (FR-1.31).

use mumcord_engine::Engine;
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

/// Resets the unread count of a conversation.
#[tauri::command]
#[specta::specta]
pub fn conversation_mark_read(
    engine: State<'_, Engine>,
    conversation: Conversation,
) -> ApiResult<()> {
    Ok(engine.conversation_mark_read(conversation.into())?)
}
