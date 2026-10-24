//! Following channels and unread counts (FR-1.31).

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use super::types::{ApiResult, ChannelId, Conversation, not_implemented};

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
pub fn channel_follow(channel_id: ChannelId) -> ApiResult<()> {
    let _ = channel_id;
    not_implemented()
}

#[tauri::command]
#[specta::specta]
pub fn channel_unfollow(channel_id: ChannelId) -> ApiResult<()> {
    let _ = channel_id;
    not_implemented()
}

/// Resets the unread count of a conversation.
#[tauri::command]
#[specta::specta]
pub fn conversation_mark_read(conversation: Conversation) -> ApiResult<()> {
    let _ = conversation;
    not_implemented()
}
