//! Blocked users (issue #140).

use mumcord_engine::Engine;
use mumcord_engine::store::BlockedUser as StoredBlockedUser;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::chat::rfc3339;
use super::types::{ApiResult, ServerId, SessionId, to_api_server_id, to_engine_server_id};

/// A person one own identity blocked.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct BlockedUser {
    /// Id for `unblock`.
    pub id: u32,
    /// Certificate hash of the own identity that set the block.
    pub own_cert_hash: String,
    /// `null` for a name block: then only `name` on `serverId` is blocked. With a certificate
    /// the block covers every server.
    pub cert_hash: Option<String>,
    /// The name when blocked. Untrusted: render as plain text.
    pub name: String,
    /// The server the block was set on; `null` once that server was removed.
    pub server_id: Option<ServerId>,
    /// Label of that server when the block was set.
    pub server_label: String,
    /// RFC 3339.
    pub blocked_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct BlockedUsersChanged {
    pub users: Vec<BlockedUser>,
}

pub fn to_api_blocked(list: Vec<StoredBlockedUser>) -> Vec<BlockedUser> {
    list.into_iter()
        .map(|b| BlockedUser {
            id: u32::try_from(b.id).unwrap_or(u32::MAX),
            own_cert_hash: b.own_cert_hash,
            cert_hash: b.cert_hash,
            name: b.name,
            server_id: b.server_id.map(to_api_server_id),
            server_label: b.server_label,
            blocked_at: rfc3339(b.blocked_at),
        })
        .collect()
}

/// Blocked users of every own identity, by name.
#[tauri::command]
#[specta::specta]
pub fn blocked_users(engine: State<'_, Engine>) -> ApiResult<Vec<BlockedUser>> {
    Ok(to_api_blocked(engine.blocked_users()?))
}

/// Blocks a person on a connected server: by certificate, or by name on this server when
/// they have none.
#[tauri::command]
#[specta::specta]
pub fn block_user(
    engine: State<'_, Engine>,
    server_id: ServerId,
    session: SessionId,
) -> ApiResult<()> {
    Ok(engine.block_session(to_engine_server_id(server_id), session)?)
}

/// Blocks a person by certificate who need not be online, for the identity `serverId` uses.
#[tauri::command]
#[specta::specta]
pub fn block_cert(
    engine: State<'_, Engine>,
    server_id: ServerId,
    cert_hash: String,
    name: String,
) -> ApiResult<()> {
    Ok(engine.block_cert(to_engine_server_id(server_id), &cert_hash, &name)?)
}

/// Removes a block.
#[tauri::command]
#[specta::specta]
pub fn unblock(engine: State<'_, Engine>, id: u32) -> ApiResult<()> {
    Ok(engine.unblock(id.into())?)
}
