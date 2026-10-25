//! "Now gaming" status shared with others on the server (FR-1.59).

use mumcord_engine::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use tauri_specta::Event;

use super::types::{ApiError, ApiResult, ErrorCode};

/// A game someone plays. `name` is plain text, never HTML. `playedMs` is the play time when the
/// event or answer was sent; advance it with the local clock from the moment it arrived.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct NowGaming {
    pub name: String,
    /// An icon can be loaded with `now_gaming_icon`.
    pub has_icon: bool,
    pub played_ms: u32,
    /// The person's game exposes a server to join (`now_gaming_join`).
    pub can_join: bool,
}

/// Whether this platform can look for games, and the own game as currently shared.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct NowGamingStatus {
    pub supported: bool,
    /// `null` while the setting is off or no game runs.
    pub game: Option<NowGaming>,
}

/// A person started, changed or stopped sharing the game they play (`game: null`). Also sent
/// with the own session while the own game is shared (it never comes back from the server).
/// Forget all games on a new `serverSynced` and when a person leaves.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct NowGamingChanged {
    pub session: u32,
    pub game: Option<NowGaming>,
}

impl From<mumcord_engine::NowGamingView> for NowGaming {
    fn from(v: mumcord_engine::NowGamingView) -> Self {
        Self {
            name: v.name,
            has_icon: v.has_icon,
            played_ms: u32::try_from(v.played_ms).unwrap_or(u32::MAX),
            can_join: v.can_join,
        }
    }
}

/// Platform support and the own shared game (for the settings).
#[tauri::command]
#[specta::specta]
pub fn now_gaming_status(engine: State<'_, Engine>) -> ApiResult<NowGamingStatus> {
    Ok(NowGamingStatus {
        supported: crate::now_gaming::SUPPORTED,
        game: engine.now_gaming_local().map(Into::into),
    })
}

/// Icon of a person's game (or the own one) as a small `data:` URL, fetched from the Steam CDN
/// and re-encoded by the engine; `null` when link previews are off (nothing is fetched then)
/// or there is none.
#[tauri::command]
#[specta::specta]
pub async fn now_gaming_icon(engine: State<'_, Engine>, session: u32) -> ApiResult<Option<String>> {
    Ok(engine.now_gaming_icon(session).await)
}

/// Opens the person's game server in Steam (`steam://connect/<host:port>`). Only links the
/// engine validated are opened; ask the user to confirm first. `NotFound` when the person
/// shares no join target (any more).
#[tauri::command]
#[specta::specta]
pub fn now_gaming_join(app: AppHandle, engine: State<'_, Engine>, session: u32) -> ApiResult<()> {
    let link = engine
        .now_gaming_join_link(session)
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "no game to join"))?;
    tracing::info!("now gaming: opening a join link");
    app.opener()
        .open_url(link, None::<&str>)
        .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))
}
