//! YouTube watch party in the user's voice channel (FR-2.39 – FR-2.41, FR-2.69, Beta).

use mumcord_engine::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::{
    ApiError, ApiResult, ChannelId, ErrorCode, ServerId, SessionId, to_engine_server_id,
};
use crate::watch_player::PlayerServer;

/// A playback command shared with the party; positions are milliseconds into the video.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WatchCommand {
    #[serde(rename_all = "camelCase")]
    Play { position_ms: u32 },
    #[serde(rename_all = "camelCase")]
    Pause { position_ms: u32 },
    #[serde(rename_all = "camelCase")]
    Seek { position_ms: u32 },
}

/// Who did what last, for the "Anna paused" line.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct WatchAction {
    pub session: SessionId,
    pub command: WatchCommand,
}

/// A video waiting in the party's playlist (FR-2.69).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    pub id: u32,
    pub video_id: String,
    pub added_by: SessionId,
    /// Their name when the entry arrived, for when they have left the server since.
    pub added_by_name: Option<String>,
}

/// The running party in the voice channel.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct WatchParty {
    pub channel_id: ChannelId,
    pub video_id: String,
    pub playing: bool,
    pub position_ms: u32,
    pub participants: Vec<SessionId>,
    pub last_action: Option<WatchAction>,
    /// Whether the user joined; otherwise the party is an invitation ("Join watch party").
    pub joined: bool,
    /// Entry id of the video playing; 0 for the one the party started with.
    pub current_entry: u32,
    /// What plays next, in order.
    pub playlist: Vec<PlaylistEntry>,
    /// Who skipped to the current video ("Anna skipped"), until the next playback command.
    pub skipped_by: Option<SessionId>,
}

/// The party on the server started, changed or ended (`party` is `null`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct WatchPartyChanged {
    pub server_id: ServerId,
    pub party: Option<WatchParty>,
}

impl From<mumcord_engine::WatchCommand> for WatchCommand {
    fn from(c: mumcord_engine::WatchCommand) -> Self {
        use mumcord_engine::WatchCommand as E;
        match c {
            E::Play { position_ms } => Self::Play { position_ms },
            E::Pause { position_ms } => Self::Pause { position_ms },
            E::Seek { position_ms } => Self::Seek { position_ms },
        }
    }
}

impl From<WatchCommand> for mumcord_engine::WatchCommand {
    fn from(c: WatchCommand) -> Self {
        match c {
            WatchCommand::Play { position_ms } => Self::Play { position_ms },
            WatchCommand::Pause { position_ms } => Self::Pause { position_ms },
            WatchCommand::Seek { position_ms } => Self::Seek { position_ms },
        }
    }
}

impl From<mumcord_engine::WatchPartyView> for WatchParty {
    fn from(v: mumcord_engine::WatchPartyView) -> Self {
        Self {
            channel_id: v.channel_id,
            video_id: v.video_id,
            playing: v.playing,
            position_ms: v.position_ms,
            participants: v.participants,
            last_action: v.last_action.map(|a| WatchAction {
                session: a.session,
                command: a.command.into(),
            }),
            joined: v.joined,
            current_entry: v.current_entry,
            playlist: v
                .playlist
                .into_iter()
                .map(|e| PlaylistEntry {
                    id: e.id,
                    video_id: e.video_id,
                    added_by: e.added_by,
                    added_by_name: e.added_by_name,
                })
                .collect(),
            skipped_by: v.skipped_by,
        }
    }
}

/// Starts a party for the YouTube video and invites the Mumcord users in the user's channel.
#[tauri::command]
#[specta::specta]
pub fn watch_party_start(
    engine: State<'_, Engine>,
    server_id: ServerId,
    video_id: String,
) -> ApiResult<WatchParty> {
    Ok(engine
        .watch_party_start(to_engine_server_id(server_id), &video_id)?
        .into())
}

/// Joins the party the user was invited to; its position is the current one.
#[tauri::command]
#[specta::specta]
pub fn watch_party_join(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<WatchParty> {
    Ok(engine
        .watch_party_join(to_engine_server_id(server_id))?
        .into())
}

/// Sends a playback command to everyone in the party.
#[tauri::command]
#[specta::specta]
pub fn watch_party_command(
    engine: State<'_, Engine>,
    server_id: ServerId,
    command: WatchCommand,
) -> ApiResult<()> {
    Ok(engine.watch_party_command(to_engine_server_id(server_id), command.into())?)
}

/// Leaves the party (also happens when leaving the channel).
#[tauri::command]
#[specta::specta]
pub fn watch_party_leave(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<()> {
    Ok(engine.watch_party_leave(to_engine_server_id(server_id))?)
}

/// The party in the user's channel, if any (for a view opened after the last event).
#[tauri::command]
#[specta::specta]
pub fn watch_party_get(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> ApiResult<Option<WatchParty>> {
    Ok(engine
        .watch_party(to_engine_server_id(server_id))
        .map(Into::into))
}

/// Adds a YouTube video to the end of the party's playlist; returns the entry id.
#[tauri::command]
#[specta::specta]
pub fn watch_party_add(
    engine: State<'_, Engine>,
    server_id: ServerId,
    video_id: String,
) -> ApiResult<u32> {
    Ok(engine.watch_party_add(to_engine_server_id(server_id), &video_id)?)
}

/// Removes an entry from the party's playlist.
#[tauri::command]
#[specta::specta]
pub fn watch_party_remove(
    engine: State<'_, Engine>,
    server_id: ServerId,
    entry: u32,
) -> ApiResult<()> {
    Ok(engine.watch_party_remove(to_engine_server_id(server_id), entry)?)
}

/// Moves an entry of the party's playlist to `index` (0 plays next).
#[tauri::command]
#[specta::specta]
pub fn watch_party_move(
    engine: State<'_, Engine>,
    server_id: ServerId,
    entry: u32,
    index: u32,
) -> ApiResult<()> {
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    Ok(engine.watch_party_move(to_engine_server_id(server_id), entry, index)?)
}

/// Goes on from entry `from` to the next playlist entry for everyone: at the end of a video,
/// or at once with `skip`.
#[tauri::command]
#[specta::specta]
pub fn watch_party_next(
    engine: State<'_, Engine>,
    server_id: ServerId,
    from: u32,
    skip: bool,
) -> ApiResult<()> {
    Ok(engine.watch_party_next(to_engine_server_id(server_id), from, skip)?)
}

/// URL of the loopback page that hosts the party's YouTube player (FR-2.41a); starts its server
/// on first use. It stops by itself once the user is in no party and no chat embed plays.
#[tauri::command]
#[specta::specta]
pub async fn watch_party_player_url(player: State<'_, PlayerServer>) -> ApiResult<String> {
    player
        .party_url()
        .map_err(|e| ApiError::new(ErrorCode::Io, format!("player server: {e}")))
}
