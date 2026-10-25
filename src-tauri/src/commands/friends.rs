//! Friends by certificate hash with their online state (FR-1.64).

use mumcord_engine::{Engine, FriendPresence, FriendView};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::chat::rfc3339;
use super::types::{ApiResult, ChannelId, ServerId, SessionId, to_api_server_id};

/// Where a friend is online right now.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct FriendOnline {
    pub server_id: ServerId,
    pub session: SessionId,
    pub channel_id: ChannelId,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct Friend {
    pub cert_hash: String,
    /// Current name while online, otherwise the name last seen.
    pub name: String,
    /// RFC 3339.
    pub added_at: String,
    /// RFC 3339; `null` if not seen online since adding.
    pub last_seen_at: Option<String>,
    /// `null` while offline.
    pub online: Option<FriendOnline>,
}

/// The friends list changed (added, removed, online, moved, renamed, offline). Online first.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct FriendsChanged {
    pub friends: Vec<Friend>,
}

impl From<FriendPresence> for FriendOnline {
    fn from(p: FriendPresence) -> Self {
        Self {
            server_id: to_api_server_id(p.server_id),
            session: p.session,
            channel_id: p.channel_id,
        }
    }
}

impl From<FriendView> for Friend {
    fn from(v: FriendView) -> Self {
        Self {
            cert_hash: v.cert_hash,
            name: v.name,
            added_at: rfc3339(v.added_at),
            last_seen_at: v.last_seen_at.map(rfc3339),
            online: v.online.map(Into::into),
        }
    }
}

pub fn to_api_friends(friends: Vec<FriendView>) -> Vec<Friend> {
    friends.into_iter().map(Into::into).collect()
}

/// Friends with their online state, online first.
#[tauri::command]
#[specta::specta]
pub fn friends_list(engine: State<'_, Engine>) -> ApiResult<Vec<Friend>> {
    Ok(to_api_friends(engine.friends()))
}

/// Adds a person on the connected server as a friend; fails for people without a
/// certificate.
#[tauri::command]
#[specta::specta]
pub fn friend_add(engine: State<'_, Engine>, session: SessionId) -> ApiResult<()> {
    Ok(engine.friend_add(session)?)
}

/// Removes a friend by certificate hash.
#[tauri::command]
#[specta::specta]
pub fn friend_remove(engine: State<'_, Engine>, cert_hash: String) -> ApiResult<()> {
    Ok(engine.friend_remove(&cert_hash)?)
}
