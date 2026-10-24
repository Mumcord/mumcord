//! "Now playing" status shared with others on the server (FR-1.58).

use mumcord_engine::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::ApiResult;

/// A track someone listens to. All text is plain text, never HTML. `positionMs` is the
/// position when the event or answer was sent; while `playing`, advance it with the local
/// clock from the moment it arrived, up to `durationMs`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub title: String,
    /// Empty when the player does not report one.
    pub artist: String,
    pub album: Option<String>,
    /// A cover can be loaded with `now_playing_cover`.
    pub has_cover: bool,
    pub position_ms: u32,
    pub duration_ms: Option<u32>,
    pub playing: bool,
}

/// Whether this platform can read the media player, and the own track as currently shared.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct NowPlayingStatus {
    /// `false` on macOS: there is no public API for the system's media information.
    pub supported: bool,
    /// `null` while the setting is off or nothing plays.
    pub track: Option<NowPlaying>,
}

/// Another person started, changed or stopped sharing what they listen to (`track: null`).
/// Forget all tracks on a new `serverSynced` and when a person leaves.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct NowPlayingChanged {
    pub session: u32,
    pub track: Option<NowPlaying>,
}

impl From<mumcord_engine::NowPlayingView> for NowPlaying {
    fn from(v: mumcord_engine::NowPlayingView) -> Self {
        let ms = |value: u64| u32::try_from(value).unwrap_or(u32::MAX);
        Self {
            title: v.title,
            artist: v.artist,
            album: v.album,
            has_cover: v.has_cover,
            position_ms: ms(v.position_ms),
            duration_ms: v.duration_ms.map(ms),
            playing: v.playing,
        }
    }
}

/// Platform support and the own shared track (for the settings).
#[tauri::command]
#[specta::specta]
pub fn now_playing_status(engine: State<'_, Engine>) -> ApiResult<NowPlayingStatus> {
    Ok(NowPlayingStatus {
        supported: crate::now_playing::SUPPORTED,
        track: engine.now_playing_local().map(Into::into),
    })
}

/// Cover of a person's track (or the own one) as a small `data:` URL, fetched and re-encoded
/// by the engine; `null` when link previews are off (nothing is fetched then) or there is none.
#[tauri::command]
#[specta::specta]
pub async fn now_playing_cover(
    engine: State<'_, Engine>,
    session: u32,
) -> ApiResult<Option<String>> {
    Ok(engine.now_playing_cover(session).await)
}
