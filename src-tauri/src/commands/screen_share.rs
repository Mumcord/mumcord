//! Screen sharing over direct connections (FR-2.42 – FR-2.46, Beta).

use std::sync::Arc;

use mumcord_engine::Engine;
use mumcord_engine::screen_share::{ScreenShareView, WatchBlock as EngineWatchBlock};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use crate::screen_frames::{FrameChannel, ScreenFrames};

use super::types::{ApiResult, ServerId, SessionId, to_engine_server_id};

/// Why "Watch" is disabled (FR-2.46).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum WatchBlock {
    /// The user said no to direct connections.
    ConsentOff,
    /// The sharer's client cannot or does not want to take direct connections.
    PeerUnsupported,
    /// No direct path was found (there is no relay, e.g. behind CGNAT).
    NoDirectPath,
}

/// One person sharing their screen in the voice channel.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScreenShare {
    /// The sharing person; the own session for the user's share.
    pub session: SessionId,
    pub paused: bool,
    pub sound: bool,
    /// The own share has a sound source; always `false` for the shares of others.
    pub sound_available: bool,
    /// Who is watching (up to 8).
    pub viewers: Vec<SessionId>,
    /// Why the user cannot watch; always `null` for the own share.
    pub blocked: Option<WatchBlock>,
}

/// The screen shares on the server changed; carries the full list.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ScreenSharesChanged {
    pub server_id: ServerId,
    pub shares: Vec<ScreenShare>,
}

impl From<EngineWatchBlock> for WatchBlock {
    fn from(b: EngineWatchBlock) -> Self {
        match b {
            EngineWatchBlock::ConsentOff => Self::ConsentOff,
            EngineWatchBlock::PeerUnsupported => Self::PeerUnsupported,
            EngineWatchBlock::NoDirectPath => Self::NoDirectPath,
        }
    }
}

impl From<ScreenShareView> for ScreenShare {
    fn from(v: ScreenShareView) -> Self {
        Self {
            session: v.session,
            paused: v.paused,
            sound: v.sound,
            sound_available: v.sound_available,
            viewers: v.viewers,
            blocked: v.blocked.map(Into::into),
        }
    }
}

/// Starts sharing in the user's voice channel, optionally with sound. Closing the system
/// picker ends the call quietly without a share.
#[tauri::command]
#[specta::specta]
pub async fn screen_share_start(
    engine: State<'_, Engine>,
    window: tauri::WebviewWindow,
    server_id: ServerId,
    with_sound: bool,
) -> ApiResult<()> {
    // The Windows system picker opens over the window the share was started from.
    #[cfg(windows)]
    match window.hwnd() {
        Ok(hwnd) => {
            mumcord_engine::screen_share::capture_windows::set_picker_owner(hwnd.0 as isize)
        }
        Err(e) => tracing::warn!(error = %e, "window handle for the screen picker unavailable"),
    }
    #[cfg(not(windows))]
    let _ = window;
    engine
        .screen_share_start(to_engine_server_id(server_id), with_sound)
        .await?;
    Ok(())
}

/// Stops the own share.
#[tauri::command]
#[specta::specta]
pub fn screen_share_stop(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<()> {
    Ok(engine.screen_share_stop(to_engine_server_id(server_id))?)
}

/// Pauses or resumes the own share.
#[tauri::command]
#[specta::specta]
pub fn screen_share_set_paused(
    engine: State<'_, Engine>,
    server_id: ServerId,
    paused: bool,
) -> ApiResult<()> {
    Ok(engine.screen_share_set_paused(to_engine_server_id(server_id), paused)?)
}

/// Turns the own share's sound on or off.
#[tauri::command]
#[specta::specta]
pub fn screen_share_set_sound(
    engine: State<'_, Engine>,
    server_id: ServerId,
    enabled: bool,
) -> ApiResult<()> {
    Ok(engine.screen_share_set_sound(to_engine_server_id(server_id), enabled)?)
}

/// Level of watched shares' sound, 0.0–1.0, while people talk (auto-ducking, FR-2.76).
#[tauri::command]
#[specta::specta]
pub fn screen_share_set_sound_level(engine: State<'_, Engine>, level: f32) -> ApiResult<()> {
    engine.screen_share_set_sound_level(level);
    Ok(())
}

/// Starts watching a person's share; its frames arrive on `frames` as binary messages (layout
/// in `screen_frames`). Errors: `shareFull` when 8 people already watch, `directUnavailable`
/// (reason in `message`) when no direct connection can be made.
#[tauri::command]
#[specta::specta]
pub async fn screen_share_watch(
    engine: State<'_, Engine>,
    screen_frames: State<'_, Arc<ScreenFrames>>,
    server_id: ServerId,
    session: SessionId,
    frames: FrameChannel,
) -> ApiResult<()> {
    let server_id = to_engine_server_id(server_id);
    let channel_id = frames.id();
    // Before the stream opens, so the first keyframe is not missed.
    screen_frames.attach(server_id, session, frames);
    let watched = engine.screen_share_watch(server_id, session).await;
    if watched.is_err() {
        screen_frames.detach_channel(server_id, session, channel_id);
    }
    Ok(watched?)
}

/// Stops watching a person's share.
#[tauri::command]
#[specta::specta]
pub fn screen_share_unwatch(
    engine: State<'_, Engine>,
    screen_frames: State<'_, Arc<ScreenFrames>>,
    server_id: ServerId,
    session: SessionId,
) -> ApiResult<()> {
    let server_id = to_engine_server_id(server_id);
    screen_frames.detach(server_id, session);
    Ok(engine.screen_share_unwatch(server_id, session)?)
}

/// Decodes a watched share in Rust and sends RGBA pictures instead of VP9 frames, for a webview
/// without WebCodecs VP9 (FR-2.44).
#[tauri::command]
#[specta::specta]
pub fn screen_share_set_decode_fallback(
    engine: State<'_, Engine>,
    server_id: ServerId,
    session: SessionId,
    enabled: bool,
) -> ApiResult<()> {
    Ok(
        engine.screen_share_set_decode_fallback(
            to_engine_server_id(server_id),
            session,
            enabled,
        )?,
    )
}

/// Asks the sharer for a keyframe, e.g. after the webview's decoder failed on a frame.
#[tauri::command]
#[specta::specta]
pub fn screen_share_request_keyframe(
    engine: State<'_, Engine>,
    server_id: ServerId,
    session: SessionId,
) -> ApiResult<()> {
    Ok(engine.screen_share_request_keyframe(to_engine_server_id(server_id), session)?)
}

/// The shares in the user's channel (for a view opened after the last event).
#[tauri::command]
#[specta::specta]
pub fn screen_share_list(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> ApiResult<Vec<ScreenShare>> {
    Ok(engine
        .screen_shares(to_engine_server_id(server_id))
        .into_iter()
        .map(Into::into)
        .collect())
}
