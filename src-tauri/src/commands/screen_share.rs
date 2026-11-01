//! Screen sharing over direct connections (FR-2.42 – FR-2.46, Beta).

use std::sync::Arc;

use mumcord_engine::Engine;
use mumcord_engine::screen_share::{
    ScreenShareView, ShareKind as EngineShareKind, WatchBlock as EngineWatchBlock,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use crate::screen_frames::{FrameChannel, ScreenFrames};

use super::p2p::RelayUnavailable;
use super::types::{ApiResult, ServerId, SessionId, to_engine_server_id};

/// Why "Watch" is disabled (FR-2.46).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum WatchBlock {
    /// "Peer-to-peer is off" for this server.
    P2pOff,
    /// The user said no to direct connections and the relay cannot be used either.
    ConsentOff,
    /// The relay was needed but cannot be used; the reason is in `relayReason`.
    RelayUnavailable,
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
    /// Why the relay cannot be used, when `blocked` is `relayUnavailable`.
    pub relay_reason: Option<RelayUnavailable>,
    /// What the sharer picked in the system picker (issue #163); `null` where the platform cannot
    /// tell a window from the whole screen.
    pub kind: Option<ShareKind>,
    /// How long the share has been running (issue #163).
    pub elapsed_ms: u32,
}

/// What the sharer picked in the system picker (issue #163).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum ShareKind {
    Window,
    Screen,
}

/// The live picture of the own share for the sharer's bar (issue #163, FR-3.127).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareThumbnail {
    pub width: u32,
    pub height: u32,
    /// The last keyframe, scaled down, as a `data:` URL the UI shows as it is.
    pub data_url: String,
}

/// How the own share is sent right now, for the sharer's bar and the viewers popover (issue #163).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareQuality {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub kbps: u32,
    pub viewers: Vec<ViewerQuality>,
}

/// What one viewer gets (issue #163).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ViewerQuality {
    pub session: SessionId,
    pub fps: u32,
    /// It lost every layer it can and still does not keep up, so the quality step is lowered for
    /// everyone because of this viewer.
    pub struggling: bool,
}

/// What the user's own watch of a share looks like (issue #163); the picture figures come from
/// the frames themselves and are counted in the UI.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct WatchStats {
    /// Media only travels over a direct connection today (FR-2.46); the relay (#62) can add a
    /// second path.
    pub path: SharePath,
    /// Sound level of the shared sound at the viewer, 0.0–1.0.
    pub rms: f32,
}

/// How the frames of a watched share reach the viewer.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum SharePath {
    Direct,
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
            EngineWatchBlock::P2pOff => Self::P2pOff,
            EngineWatchBlock::ConsentOff => Self::ConsentOff,
            EngineWatchBlock::Relay(_) => Self::RelayUnavailable,
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
            relay_reason: match v.blocked {
                Some(EngineWatchBlock::Relay(reason)) => Some(reason.into()),
                _ => None,
            },
            kind: v.kind.map(|k| match k {
                EngineShareKind::Window => ShareKind::Window,
                EngineShareKind::Screen => ShareKind::Screen,
            }),
            elapsed_ms: v.elapsed_ms,
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

/// Sends the frames of a running watch to `frames` from now on, in place of the channel the watch
/// started with: the screen share pop-out (issue #164) takes the frames over, and the main window
/// takes them back when the pop-out closes, all without leaving the share (the viewers list does
/// not change). Asks the sharer for a keyframe so the new decoder can start; while the watch is
/// still connecting the first frame is a keyframe anyway. Only one window gets the frames at a
/// time.
#[tauri::command]
#[specta::specta]
pub fn screen_share_frames_attach(
    engine: State<'_, Engine>,
    screen_frames: State<'_, Arc<ScreenFrames>>,
    server_id: ServerId,
    session: SessionId,
    frames: FrameChannel,
) -> ApiResult<()> {
    let server_id = to_engine_server_id(server_id);
    screen_frames.attach(server_id, session, frames);
    if let Err(e) = engine.screen_share_request_keyframe(server_id, session) {
        tracing::debug!(error = %e, "no keyframe asked for the re-attached frames");
    }
    Ok(())
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

/// Why the user cannot start a share on the server: nobody could watch it (FR-4.3 – FR-4.5).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareStartBlock {
    pub blocked: WatchBlock,
    /// Why the relay cannot be used, when `blocked` is `relayUnavailable`.
    pub relay_reason: Option<RelayUnavailable>,
}

/// What blocks starting a share on the server right now; `null` when sharing is possible. The
/// settings and the relay change without an event, so the UI asks again before starting.
#[tauri::command]
#[specta::specta]
pub fn screen_share_block(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> ApiResult<Option<ShareStartBlock>> {
    Ok(engine
        .screen_share_block(to_engine_server_id(server_id))
        .map(|block| ShareStartBlock {
            blocked: block.into(),
            relay_reason: match block {
                EngineWatchBlock::Relay(reason) => Some(reason.into()),
                _ => None,
            },
        }))
}

/// The picture the sharer's bar shows: the last keyframe the encoder took (issue #163).
#[tauri::command]
#[specta::specta]
pub fn screen_share_thumbnail(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> ApiResult<Option<ShareThumbnail>> {
    Ok(engine
        .screen_share_thumbnail(to_engine_server_id(server_id))
        .map(|t| ShareThumbnail {
            width: t.width,
            height: t.height,
            data_url: t.data_url,
        }))
}

/// How the own share is sent right now, for the sharer's bar and the viewers popover (issue #163).
#[tauri::command]
#[specta::specta]
pub fn screen_share_quality(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> ApiResult<Option<ShareQuality>> {
    Ok(engine
        .screen_share_media_stats(to_engine_server_id(server_id))
        .map(|s| ShareQuality {
            width: s.width,
            height: s.height,
            fps: s.fps,
            kbps: s.kbps,
            viewers: s
                .viewers
                .into_iter()
                .map(|v| ViewerQuality {
                    session: v.session,
                    fps: v.fps,
                    struggling: v.struggling,
                })
                .collect(),
        }))
}

/// What the user's own watch of `session`'s share looks like (issue #163); the picture figures are
/// counted in the UI from the frames. `None` while not watching.
#[tauri::command]
#[specta::specta]
pub fn screen_share_watch_stats(
    engine: State<'_, Engine>,
    server_id: ServerId,
    session: SessionId,
) -> ApiResult<Option<WatchStats>> {
    Ok(engine
        .screen_share_sound_stats(to_engine_server_id(server_id), session)
        .map(|stats| WatchStats {
            path: SharePath::Direct,
            rms: stats.rms,
        }))
}
