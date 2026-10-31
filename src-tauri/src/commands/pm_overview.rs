//! The private messages overview (issue #173): newest message per private conversation, pinning
//! and muting.
use mumcord_engine::store::{MUTED_FOREVER, PmConversationPrefs as StoredPrefs};
use mumcord_engine::{Engine, PrivateConversationView};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::chat::{ChatMessage, rfc3339};
use super::types::{ApiResult, ServerId, to_api_server_id, to_engine_server_id};

/// A stored private conversation: its newest message (with the conversation) and the name the
/// other person last wrote under, `null` when only own messages are stored.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PrivateConversation {
    pub last: ChatMessage,
    /// Untrusted: render as plain text.
    pub peer_name: Option<String>,
}

/// Pin and mute state of a private conversation, by server and the person's certificate hash.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PmConversationPrefs {
    pub server_id: ServerId,
    pub cert_hash: String,
    pub pinned: bool,
    /// RFC 3339 end of a timed mute; `null` when not muted or muted until turned back on.
    /// A time in the past means the mute is over.
    pub muted_until: Option<String>,
    /// Muted until turned back on.
    pub muted_forever: bool,
}

/// Pinned or muted private conversations changed: the full list.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct PmConversationPrefsChanged {
    pub prefs: Vec<PmConversationPrefs>,
}

pub fn to_api_prefs(list: Vec<StoredPrefs>) -> Vec<PmConversationPrefs> {
    list.into_iter()
        .map(|p| PmConversationPrefs {
            server_id: to_api_server_id(p.server_id),
            cert_hash: p.cert_hash,
            pinned: p.pinned,
            muted_until: p.muted_until.filter(|&u| u != MUTED_FOREVER).map(rfc3339),
            muted_forever: p.muted_until == Some(MUTED_FOREVER),
        })
        .collect()
}

impl From<PrivateConversationView> for PrivateConversation {
    fn from(v: PrivateConversationView) -> Self {
        Self {
            last: v.last.into(),
            peer_name: v.peer_name,
        }
    }
}

/// The newest stored message of every private conversation on a connected server, newest first.
#[tauri::command]
#[specta::specta]
pub fn private_conversations(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> ApiResult<Vec<PrivateConversation>> {
    Ok(engine
        .private_conversations(to_engine_server_id(server_id))?
        .into_iter()
        .map(Into::into)
        .collect())
}

/// Pin and mute state of every private conversation that has any.
#[tauri::command]
#[specta::specta]
pub fn pm_conversation_prefs(engine: State<'_, Engine>) -> ApiResult<Vec<PmConversationPrefs>> {
    Ok(to_api_prefs(engine.pm_conversation_prefs()?))
}

/// Pins a private conversation to the top of the overview, or unpins it.
#[tauri::command]
#[specta::specta]
pub fn pm_conversation_set_pinned(
    engine: State<'_, Engine>,
    server_id: ServerId,
    cert_hash: String,
    pinned: bool,
) -> ApiResult<()> {
    Ok(engine.pm_conversation_set_pinned(to_engine_server_id(server_id), &cert_hash, pinned)?)
}

/// Mutes a private conversation for `minutes`, or until turned back on (`null`).
#[tauri::command]
#[specta::specta]
pub fn pm_conversation_mute(
    engine: State<'_, Engine>,
    server_id: ServerId,
    cert_hash: String,
    minutes: Option<u32>,
) -> ApiResult<()> {
    Ok(engine.pm_conversation_mute(to_engine_server_id(server_id), &cert_hash, minutes)?)
}

/// Turns notifications of a muted private conversation back on.
#[tauri::command]
#[specta::specta]
pub fn pm_conversation_unmute(
    engine: State<'_, Engine>,
    server_id: ServerId,
    cert_hash: String,
) -> ApiResult<()> {
    Ok(engine.pm_conversation_unmute(to_engine_server_id(server_id), &cert_hash)?)
}
