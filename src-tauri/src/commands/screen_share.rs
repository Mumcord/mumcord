//! Screen sharing over direct connections (FR-2.42 – FR-2.46, Beta).

use std::sync::Arc;

use mumcord_engine::Engine;
use mumcord_engine::screen_share::wire::Codec;
use mumcord_engine::screen_share::{
    self as engine_share, NoSoundReason as EngineNoSoundReason, QualityPrefs, ScreenShareView,
    ShareKind as EngineShareKind, ShareSoundMode as EngineSoundMode, SoundApp as EngineSoundApp,
    SoundUnavailable as EngineSoundUnavailable, WatchBlock as EngineWatchBlock,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use crate::screen_frames::{FrameChannel, ScreenFrames};

use super::p2p::RelayUnavailable;
use super::settings::{
    ScreenCodec, ScreenFps, ScreenOptimize, ScreenResolution, ScreenUploadLimit,
};
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
    /// What the own share's sound carries; `none` for the shares of others.
    pub sound_mode: ShareSoundMode,
    /// The app whose sound is shared, when `soundMode` is `app` (stays on the sharer's device).
    pub sound_app: Option<String>,
    /// Why the own share sends no sound, for the live bar's sound line (FR-7.85); `null` while
    /// it sends sound, for the shares of others, and when nothing explains it.
    pub sound_reason: Option<ShareSoundReason>,
    /// Set while the share waits for "Which app's sound?" (FR-7.83); the list to offer.
    pub sound_choice: Option<Vec<SoundApp>>,
    /// Who is watching (up to 8).
    pub viewers: Vec<SessionId>,
    /// Why the user cannot watch; always `null` for the own share.
    pub blocked: Option<WatchBlock>,
    /// Why the relay cannot be used, when `blocked` is `relayUnavailable`.
    pub relay_reason: Option<RelayUnavailable>,
    /// What the sharer picked in the system picker (issue #163); `null` where the platform cannot
    /// tell a window from the whole screen.
    pub kind: Option<ShareKind>,
    /// The picked window's title, else its app's name, for the own live bar (issue #558);
    /// `null` for a screen, where the platform does not say, and for the shares of others. It
    /// never leaves this device (FR-7.87).
    pub title: Option<String>,
    /// How long the share has been running (issue #163).
    pub elapsed_ms: u32,
}

/// What the own share's sound carries (FR-7.76).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum ShareSoundMode {
    Desktop,
    App,
    None,
}

impl From<EngineSoundMode> for ShareSoundMode {
    fn from(m: EngineSoundMode) -> Self {
        match m {
            EngineSoundMode::Desktop => Self::Desktop,
            EngineSoundMode::App => Self::App,
            EngineSoundMode::None => Self::None,
        }
    }
}

/// Why there is no sound to share (FR-7.84, FR-7.85).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum SoundUnavailableReason {
    /// The system is too old (Windows before 10 version 2004, macOS before 13).
    OsTooOld,
    /// Linux without a running PipeWire.
    NoPipeWire,
    /// The sound capture could not start or stopped.
    CaptureFailed,
}

impl From<EngineSoundUnavailable> for SoundUnavailableReason {
    fn from(r: EngineSoundUnavailable) -> Self {
        match r {
            EngineSoundUnavailable::OsTooOld => Self::OsTooOld,
            EngineSoundUnavailable::NoPipeWire => Self::NoPipeWire,
            EngineSoundUnavailable::CaptureFailed => Self::CaptureFailed,
        }
    }
}

/// Why the own share sends no sound (FR-7.85).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum ShareSoundReason {
    /// The Sound switch is off.
    Off,
    /// "No sound" was chosen in "Which app's sound?".
    NotChosen,
    /// The system is too old (Windows before 10 version 2004, macOS before 13).
    OsTooOld,
    /// Linux without a running PipeWire.
    NoPipeWire,
    /// The sound capture could not start or stopped.
    CaptureFailed,
}

impl From<EngineNoSoundReason> for ShareSoundReason {
    fn from(r: EngineNoSoundReason) -> Self {
        match r {
            EngineNoSoundReason::Off => Self::Off,
            EngineNoSoundReason::NotChosen => Self::NotChosen,
            EngineNoSoundReason::Unavailable(EngineSoundUnavailable::OsTooOld) => Self::OsTooOld,
            EngineNoSoundReason::Unavailable(EngineSoundUnavailable::NoPipeWire) => {
                Self::NoPipeWire
            }
            EngineNoSoundReason::Unavailable(EngineSoundUnavailable::CaptureFailed) => {
                Self::CaptureFailed
            }
        }
    }
}

/// One app offered in "Which app's sound?" (FR-7.83).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SoundApp {
    /// What `screenShareChooseSound` takes.
    pub id: String,
    pub name: String,
    /// The app's icon as a `data:` URL (Linux, where the system has one); the list shows a
    /// coloured square without it.
    pub icon: Option<String>,
}

impl std::fmt::Debug for SoundApp {
    /// App names and ids never reach a log (FR-7.87).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SoundApp(..)")
    }
}

impl From<EngineSoundApp> for SoundApp {
    fn from(a: EngineSoundApp) -> Self {
        Self {
            id: a.id,
            name: a.name.as_str().to_owned(),
            icon: a.icon.map(|icon| {
                use base64::Engine as _;
                let data = base64::engine::general_purpose::STANDARD.encode(icon.bytes);
                format!("data:{};base64,{data}", icon.mime)
            }),
        }
    }
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
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ShareQuality {
    /// The step the share runs at, as the bar names it, e.g. "1080p30".
    pub quality: String,
    /// Why the share runs below the sharer's pick ("Lowered to {quality}: {reason}").
    pub lowered: Option<LoweredReason>,
    /// The share left its encoder for VP9 (FR-7.57, FR-7.58); `null` while it did not.
    pub switched: Option<EncoderSwitch>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub kbps: u32,
    /// The encoder the own share runs on right now (codec badge); `null` until it opened.
    pub encoder: Option<ActiveEncoder>,
    pub viewers: Vec<ViewerQuality>,
    /// The sharer's current picks, which the Quality menu shows checked (FR-7.39).
    pub choice: QualityChoice,
    /// How the share reaches its viewers; `null` while the share has not asked for fan-out.
    pub fanout: Option<ShareFanout>,
}

/// How the own share reaches its viewers (FR-7.40, FR-7.47).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ShareFanout {
    /// Asking the relay bot for a broadcast.
    Starting,
    /// Sent once via the relay, up to `limit` viewers in all.
    Relay { limit: u16 },
    /// One copy per viewer, up to 8; the sharer sees the notice once per share.
    Direct { reason: FanoutReason },
}

/// Why the share sends one copy per viewer, as the fan-out notice names it (FR-7.47).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum FanoutReason {
    /// The server announces no relay.
    NoRelay,
    /// The relay offers no broadcasts (older relay, or broadcasts turned off).
    OldRelay,
    /// The relay does not allow screen sharing.
    Disallowed,
    /// The bot refused the broadcast.
    Refused,
    /// The relay runs as many broadcasts as it may.
    Full,
    /// The relay was lost during the share (FR-7.49).
    Lost,
    /// The user's P2P mode leaves the relay out ("off" or "direct only").
    Mode,
    /// The user has no client certificate.
    NoCertificate,
    /// The relay can't be used right now for another reason (not reachable, bot gone, …).
    Unavailable,
    /// The relay only serves registered users (#601).
    NotRegistered,
    /// The relay only serves members of certain groups (#601).
    GroupNotAllowed,
    /// The relay bot doesn't know this client yet; reconnecting fixes it (#601).
    NoHello,
}

impl From<engine_share::FanoutView> for ShareFanout {
    fn from(view: engine_share::FanoutView) -> Self {
        match view {
            engine_share::FanoutView::Starting => Self::Starting,
            engine_share::FanoutView::Relay { limit } => Self::Relay { limit },
            engine_share::FanoutView::Direct { reason } => Self::Direct {
                reason: reason.into(),
            },
        }
    }
}

impl From<engine_share::FanoutUnavailable> for FanoutReason {
    fn from(reason: engine_share::FanoutUnavailable) -> Self {
        use engine_share::BroadcastErrorReason as Bcast;
        use engine_share::FanoutUnavailable as F;
        use mumcord_engine::peer::RelayUnavailable as R;
        match reason {
            F::Relay(R::NoRelay) => Self::NoRelay,
            F::Relay(R::UseNotAllowed(_)) | F::Refused(Bcast::UseNotAllowed) => Self::Disallowed,
            F::Relay(R::RelayFull) | F::Refused(Bcast::RelayFull | Bcast::Full) => Self::Full,
            F::Relay(R::NotRegistered) | F::Refused(Bcast::NotRegistered) => Self::NotRegistered,
            F::Relay(R::GroupNotAllowed) | F::Refused(Bcast::GroupNotAllowed) => {
                Self::GroupNotAllowed
            }
            F::Relay(R::NoHello) | F::Refused(Bcast::NoHello) => Self::NoHello,
            F::Relay(R::Denied) | F::Refused(_) => Self::Refused,
            F::Relay(_) => Self::Unavailable,
            F::NotOffered => Self::OldRelay,
            F::Mode => Self::Mode,
            F::NoCertificate => Self::NoCertificate,
            F::Lost => Self::Lost,
        }
    }
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
    /// It lost a layer in the last 10 s: the viewer list shows "slow connection" (FR-7.52).
    pub slow: bool,
    /// The share's codec, the same for every viewer; `null` until the encoder opened.
    pub codec: Option<VideoCodec>,
    /// How the share reaches this viewer.
    pub path: ViewerPath,
}

/// How the share reaches one viewer (viewer list, FR-7.52).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum ViewerPath {
    Direct,
    Relay,
    Broadcast,
}

impl From<engine_share::ViewerPath> for ViewerPath {
    fn from(p: engine_share::ViewerPath) -> Self {
        match p {
            engine_share::ViewerPath::Direct => Self::Direct,
            engine_share::ViewerPath::Relay => Self::Relay,
            engine_share::ViewerPath::Broadcast => Self::Broadcast,
        }
    }
}

/// What the user's own watch of a share looks like (issue #163); the picture figures come from
/// the frames themselves and are counted in the UI.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct WatchStats {
    /// A copy of its own over the P2P connection, or the relay broadcast (FR-7.41).
    pub path: SharePath,
    /// Sound level of the shared sound at the viewer, 0.0–1.0.
    pub rms: f32,
}

/// How the frames of a watched share reach the viewer.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum SharePath {
    Direct,
    /// The sharer's single upload, copied to every viewer by the relay.
    Broadcast,
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
            sound_available: v.sound_status.available(),
            sound_mode: v.sound_status.mode.into(),
            sound_app: v.sound_status.app.as_ref().map(|a| a.as_str().to_owned()),
            sound_reason: v.sound_status.no_sound_reason().map(Into::into),
            sound_choice: v
                .sound_status
                .choice
                .map(|apps| apps.into_iter().map(Into::into).collect()),
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
            title: v.title,
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
    choice: Option<QualityChoice>,
    codec: Option<ScreenCodec>,
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
        .screen_share_start(
            to_engine_server_id(server_id),
            with_sound,
            choice.map(Into::into),
            codec.map(Into::into),
        )
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

/// Why this system cannot share sound (FR-7.84), asked before a share starts so the start
/// dialog and Settings can show the Sound switch off and disabled with the matching note;
/// `null` where sound can be shared.
#[tauri::command]
#[specta::specta]
pub fn screen_share_sound_support(engine: State<'_, Engine>) -> Option<SoundUnavailableReason> {
    engine.screen_share_sound_unavailable().map(Into::into)
}

/// The apps playing sound now, for "Which app's sound?" while the own share waits for the
/// pick (`soundChoice` is set). Call it again to refresh the open list; empty where the
/// system has no per-app choice.
#[tauri::command]
#[specta::specta]
pub fn screen_share_sound_apps(engine: State<'_, Engine>) -> ApiResult<Vec<SoundApp>> {
    Ok(engine
        .screen_share_sound_apps()?
        .into_iter()
        .map(Into::into)
        .collect())
}

/// Answers "Which app's sound?": an app id shares that app's sound, `null` ("No sound" or
/// Cancel) keeps the share silent. Errors: `notFound` when the app stopped playing (keep the
/// dialog open and refresh the list), `invalidInput` when nothing is asked.
#[tauri::command]
#[specta::specta]
pub fn screen_share_choose_sound(
    engine: State<'_, Engine>,
    server_id: ServerId,
    app: Option<String>,
) -> ApiResult<()> {
    Ok(engine.screen_share_choose_sound(to_engine_server_id(server_id), app.as_deref())?)
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

impl From<VideoCodec> for Codec {
    fn from(codec: VideoCodec) -> Self {
        match codec {
            VideoCodec::Av1 => Codec::Av1,
            VideoCodec::Vp9 => Codec::Vp9,
        }
    }
}

/// Decodes a watched share's frames of `codecs` in Rust and sends RGBA pictures instead, for the
/// codecs the webview's WebCodecs cannot decode (FR-7.59). Replaces the earlier list.
#[tauri::command]
#[specta::specta]
pub fn screen_share_set_decode_codecs(
    engine: State<'_, Engine>,
    server_id: ServerId,
    session: SessionId,
    codecs: Vec<VideoCodec>,
) -> ApiResult<()> {
    let codecs: Vec<Codec> = codecs.into_iter().map(Codec::from).collect();
    Ok(engine.screen_share_set_decode_codecs(to_engine_server_id(server_id), session, &codecs)?)
}

/// The viewer's frame-rate choice (FR-7.51): at most about `max_fps` frames per second of a
/// watched share, by dropping temporal layers; `null` is Auto. Through the relay broadcast the
/// relay drops the layers too (FR-7.45).
#[tauri::command]
#[specta::specta]
pub fn screen_share_set_viewer_fps(
    engine: State<'_, Engine>,
    server_id: ServerId,
    session: SessionId,
    max_fps: Option<u32>,
) -> ApiResult<()> {
    Ok(engine.screen_share_set_viewer_fps(to_engine_server_id(server_id), session, max_fps)?)
}

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
    let server = to_engine_server_id(server_id);
    let encoder = engine.screen_share_encoder(server).map(Into::into);
    let Some(choice) = engine.screen_share_choice(server) else {
        return Ok(None);
    };
    let fanout = engine
        .screen_share_fanout(server)
        .map(|info| info.view.into());
    Ok(engine
        .screen_share_media_stats(server)
        .map(|s| ShareQuality {
            quality: s.quality,
            lowered: s.lowered.map(Into::into),
            switched: s.switched.map(Into::into),
            width: s.width,
            height: s.height,
            fps: s.fps,
            kbps: s.kbps,
            encoder,
            viewers: s
                .viewers
                .into_iter()
                .map(|v| ViewerQuality {
                    session: v.session,
                    fps: v.fps,
                    struggling: v.struggling,
                    slow: v.slow,
                    codec: v.codec.map(Into::into),
                    path: v.path.into(),
                })
                .collect(),
            choice: choice.into(),
            fanout,
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
    let server_id = to_engine_server_id(server_id);
    let path = match engine.screen_share_watch_via_relay(server_id, session) {
        Some(true) => SharePath::Broadcast,
        Some(false) | None => SharePath::Direct,
    };
    Ok(engine
        .screen_share_sound_stats(server_id, session)
        .map(|stats| WatchStats {
            path,
            rms: stats.rms,
        }))
}

/// Why the share runs below the sharer's pick.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum LoweredReason {
    UploadTooSlow,
    EncoderTooSlow,
    ManyViewers,
}

impl From<engine_share::LoweredReason> for LoweredReason {
    fn from(r: engine_share::LoweredReason) -> Self {
        match r {
            engine_share::LoweredReason::UploadTooSlow => Self::UploadTooSlow,
            engine_share::LoweredReason::EncoderTooSlow => Self::EncoderTooSlow,
            engine_share::LoweredReason::ManyViewers => Self::ManyViewers,
        }
    }
}

/// A video codec of a share.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum VideoCodec {
    Vp9,
    Av1,
}

impl From<engine_share::wire::Codec> for VideoCodec {
    fn from(c: engine_share::wire::Codec) -> Self {
        match c {
            engine_share::wire::Codec::Vp9 => Self::Vp9,
            engine_share::wire::Codec::Av1 => Self::Av1,
        }
    }
}

/// Who made the graphics card an encoder runs on.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Unknown,
}

impl From<engine_share::GpuVendor> for GpuVendor {
    fn from(v: engine_share::GpuVendor) -> Self {
        match v {
            engine_share::GpuVendor::Nvidia => Self::Nvidia,
            engine_share::GpuVendor::Amd => Self::Amd,
            engine_share::GpuVendor::Intel => Self::Intel,
            engine_share::GpuVendor::Unknown => Self::Unknown,
        }
    }
}

/// The encoder a share runs on, for the codec badge ("AV1 · GPU", "VP9").
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActiveEncoder {
    pub codec: VideoCodec,
    /// `null` when it runs on the processor.
    pub gpu: Option<GpuVendor>,
}

impl From<engine_share::ActiveEncoder> for ActiveEncoder {
    fn from(e: engine_share::ActiveEncoder) -> Self {
        Self {
            codec: e.codec.into(),
            gpu: e.gpu.map(Into::into),
        }
    }
}

/// The numbers behind a load fallback, for the "Details" of the notice.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct LoadDetails {
    pub encode_ms: f64,
    pub budget_ms: f64,
    pub dropped: u32,
}

/// Why the share left its encoder for VP9.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SwitchReason {
    /// "Switched to VP9 to reduce load".
    Load { details: LoadDetails },
    /// "Switched to VP9 for {name}": this viewer cannot play AV1.
    Viewer { session: SessionId },
}

/// The share switched from AV1 to VP9 and still runs on VP9.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct EncoderSwitch {
    pub reason: SwitchReason,
    pub from: ActiveEncoder,
    pub to: ActiveEncoder,
    /// "Undo" is offered (the first load fallback of the share).
    pub undo: bool,
}

impl From<engine_share::EncoderSwitch> for EncoderSwitch {
    fn from(s: engine_share::EncoderSwitch) -> Self {
        Self {
            reason: match s.reason {
                engine_share::SwitchReason::Load(d) => SwitchReason::Load {
                    details: LoadDetails {
                        encode_ms: d.encode_ms,
                        budget_ms: d.budget_ms,
                        dropped: d.dropped,
                    },
                },
                engine_share::SwitchReason::Viewer { session } => SwitchReason::Viewer { session },
            },
            from: s.from.into(),
            to: s.to.into(),
            undo: s.undo,
        }
    }
}

/// Highest size and rate software AV1 keeps up with.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SoftwareLimit {
    pub height: u32,
    pub fps: u32,
}

/// Why Automatic does not pick AV1.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum Av1Skipped {
    TooHeavy,
    OnBattery,
    AboveSoftwareLimit,
}

impl From<engine_share::Av1Skipped> for Av1Skipped {
    fn from(s: engine_share::Av1Skipped) -> Self {
        match s {
            engine_share::Av1Skipped::TooHeavy => Self::TooHeavy,
            engine_share::Av1Skipped::OnBattery => Self::OnBattery,
            engine_share::Av1Skipped::AboveSoftwareLimit => Self::AboveSoftwareLimit,
        }
    }
}

/// What Settings → Screen sharing shows about the encoders (FR-7.60).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct EncoderStatus {
    /// The probe or a self-test is running ("Checking…").
    pub checking: bool,
    pub av1_hardware: Option<GpuVendor>,
    pub vp9_hardware: Option<GpuVendor>,
    /// `null` when software AV1 passed no self-test.
    pub av1_software_limit: Option<SoftwareLimit>,
    /// The encoder a share with the Settings defaults starts with.
    pub active: ActiveEncoder,
    /// Why that is not AV1 under Automatic.
    pub av1_skipped: Option<Av1Skipped>,
    /// Manual AV1 runs in software without a passed self-test.
    pub software_warning: bool,
}

impl From<engine_share::EncoderStatus> for EncoderStatus {
    fn from(s: engine_share::EncoderStatus) -> Self {
        Self {
            checking: s.checking,
            av1_hardware: s.av1_hardware.map(Into::into),
            vp9_hardware: s.vp9_hardware.map(Into::into),
            av1_software_limit: s.av1_software_limit.map(|l| SoftwareLimit {
                height: l.height,
                fps: l.fps,
            }),
            active: s.active.into(),
            av1_skipped: s.av1_skipped.map(Into::into),
            software_warning: s.software_warning,
        }
    }
}

/// The encoder status changed: a probe or self-test started or finished.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ScreenEncoderStatusChanged {
    pub status: EncoderStatus,
}

/// The picks of the start dialog or the Quality menu.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct QualityChoice {
    pub resolution: ScreenResolution,
    pub fps: ScreenFps,
    pub optimize: ScreenOptimize,
    pub upload_limit: ScreenUploadLimit,
}

impl From<QualityPrefs> for QualityChoice {
    fn from(p: QualityPrefs) -> Self {
        Self {
            resolution: p.resolution.into(),
            fps: p.fps.into(),
            optimize: p.optimize.into(),
            upload_limit: p.upload_limit.into(),
        }
    }
}

impl From<QualityChoice> for QualityPrefs {
    fn from(c: QualityChoice) -> Self {
        Self {
            resolution: c.resolution.into(),
            fps: c.fps.into(),
            optimize: c.optimize.into(),
            upload_limit: c.upload_limit.into(),
        }
    }
}

/// Changes the own share's resolution and frame rate ceiling, "Optimize for" and upload limit
/// while it runs (Quality menu, FR-7.39); applies within a frame or two.
#[tauri::command]
#[specta::specta]
pub fn screen_share_set_quality(
    engine: State<'_, Engine>,
    server_id: ServerId,
    choice: QualityChoice,
) -> ApiResult<()> {
    Ok(engine.screen_share_set_quality(to_engine_server_id(server_id), choice.into())?)
}

/// "Undo" on "Switched to VP9 to reduce load" (FR-7.57). Errors when no Undo is offered.
#[tauri::command]
#[specta::specta]
pub fn screen_share_undo_encoder_switch(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> ApiResult<()> {
    Ok(engine.screen_share_undo_encoder_switch(to_engine_server_id(server_id))?)
}

/// What Settings → Screen sharing shows about the encoders (FR-7.60).
#[tauri::command]
#[specta::specta]
pub fn screen_encoder_status(engine: State<'_, Engine>) -> EncoderStatus {
    engine.screen_encoder_status().into()
}

/// "Check again": probes and self-tests from scratch in the background; the result comes as
/// `screenEncoderStatusChanged`.
#[tauri::command]
#[specta::specta]
pub fn screen_encoder_check_again(engine: State<'_, Engine>) {
    engine.screen_encoder_check_again();
}

/// Runs the software AV1 self-test for the picks of the start dialog in the background
/// (FR-7.56); the result comes as `screenEncoderStatusChanged`.
#[tauri::command]
#[specta::specta]
pub fn screen_encoder_prepare(engine: State<'_, Engine>, choice: QualityChoice) {
    engine.screen_encoder_prepare(choice.into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_engine::screen_share::{AppIcon as EngineAppIcon, AppName, SoundStatus};

    fn view(sound_status: SoundStatus) -> ScreenShareView {
        ScreenShareView {
            session: 1,
            paused: false,
            sound: true,
            sound_status,
            viewers: Vec::new(),
            blocked: None,
            kind: None,
            title: None,
            elapsed_ms: 0,
        }
    }

    #[test]
    fn the_window_title_reaches_the_live_bar() {
        let s = ScreenShare::from(ScreenShareView {
            kind: Some(EngineShareKind::Window),
            title: Some("VS Code".into()),
            ..view(SoundStatus::default())
        });
        assert_eq!(s.kind, Some(ShareKind::Window));
        assert_eq!(s.title.as_deref(), Some("VS Code"));
    }

    #[test]
    fn sound_status_maps_to_mode_app_and_reason() {
        let s = ScreenShare::from(view(SoundStatus {
            mode: EngineSoundMode::App,
            app: Some(AppName::new("Firefox")),
            ..SoundStatus::default()
        }));
        assert!(s.sound_available);
        assert_eq!(s.sound_mode, ShareSoundMode::App);
        assert_eq!(s.sound_app.as_deref(), Some("Firefox"));
        assert_eq!((s.sound_reason, s.sound_choice), (None, None));

        let s = ScreenShare::from(view(SoundStatus {
            unavailable: Some(EngineSoundUnavailable::NoPipeWire),
            ..SoundStatus::default()
        }));
        assert!(!s.sound_available);
        assert_eq!(s.sound_mode, ShareSoundMode::None);
        assert_eq!(s.sound_reason, Some(ShareSoundReason::NoPipeWire));
    }

    #[test]
    fn the_switch_and_a_declined_choice_have_their_own_reason() {
        let off = ScreenShare::from(view(SoundStatus {
            off: true,
            ..SoundStatus::default()
        }));
        assert_eq!(off.sound_reason, Some(ShareSoundReason::Off));
        let declined = ScreenShare::from(view(SoundStatus {
            declined: true,
            ..SoundStatus::default()
        }));
        assert_eq!(declined.sound_reason, Some(ShareSoundReason::NotChosen));
    }

    #[test]
    fn pending_choice_maps_the_app_list() {
        let s = ScreenShare::from(view(SoundStatus {
            choice: Some(vec![EngineSoundApp {
                id: "a1".into(),
                name: AppName::new("Music"),
                icon: Some(EngineAppIcon {
                    mime: "image/png",
                    bytes: b"png".to_vec(),
                }),
            }]),
            ..SoundStatus::default()
        }));
        assert_eq!(
            s.sound_choice,
            Some(vec![SoundApp {
                id: "a1".into(),
                name: "Music".into(),
                icon: Some("data:image/png;base64,cG5n".into()),
            }])
        );
    }

    #[test]
    fn every_unavailable_reason_has_its_own_code() {
        use EngineSoundUnavailable as E;
        assert_eq!(
            SoundUnavailableReason::from(E::OsTooOld),
            SoundUnavailableReason::OsTooOld
        );
        assert_eq!(
            SoundUnavailableReason::from(E::CaptureFailed),
            SoundUnavailableReason::CaptureFailed
        );
    }

    #[test]
    fn engine_prefs_map_back_to_the_menu_choice() {
        let choice = QualityChoice {
            resolution: ScreenResolution::P1440,
            fps: ScreenFps::Fps60,
            optimize: ScreenOptimize::Motion,
            upload_limit: ScreenUploadLimit::Mbit5,
        };
        assert_eq!(QualityChoice::from(QualityPrefs::from(choice)), choice);
    }

    #[test]
    fn fanout_view_maps_for_the_notice_and_the_viewer_limit() {
        use engine_share::{BroadcastErrorReason as Bcast, FanoutUnavailable as F, FanoutView};
        use mumcord_engine::peer::RelayUnavailable as R;
        assert_eq!(
            ShareFanout::from(FanoutView::Relay { limit: 20 }),
            ShareFanout::Relay { limit: 20 }
        );
        assert_eq!(
            ShareFanout::from(FanoutView::Starting),
            ShareFanout::Starting
        );
        let direct = |reason| match ShareFanout::from(FanoutView::Direct { reason }) {
            ShareFanout::Direct { reason } => reason,
            other => panic!("expected direct copies, got {other:?}"),
        };
        assert_eq!(direct(F::Relay(R::NoRelay)), FanoutReason::NoRelay);
        assert_eq!(direct(F::NotOffered), FanoutReason::OldRelay);
        assert_eq!(
            direct(F::Refused(Bcast::UseNotAllowed)),
            FanoutReason::Disallowed
        );
        assert_eq!(direct(F::Relay(R::RelayFull)), FanoutReason::Full);
        assert_eq!(direct(F::Refused(Bcast::RelayFull)), FanoutReason::Full);
        assert_eq!(
            direct(F::Refused(Bcast::RateLimited)),
            FanoutReason::Refused
        );
        assert_eq!(direct(F::Relay(R::BotLeft)), FanoutReason::Unavailable);
        assert_eq!(direct(F::Lost), FanoutReason::Lost);
        assert_eq!(direct(F::Mode), FanoutReason::Mode);
        assert_eq!(direct(F::NoCertificate), FanoutReason::NoCertificate);
    }

    #[test]
    fn quality_choice_maps_to_engine_prefs() {
        let prefs: QualityPrefs = QualityChoice {
            resolution: ScreenResolution::P720,
            fps: ScreenFps::Fps60,
            optimize: ScreenOptimize::Text,
            upload_limit: ScreenUploadLimit::Mbit5,
        }
        .into();
        assert_eq!(prefs.upload_limit_kbps(), Some(5_000));
    }

    #[test]
    fn preview_path_maps_both_kinds() {
        assert_eq!(
            PreviewPath::from(engine_share::SharePath::Copies { max_viewers: 8 }),
            PreviewPath::Copies { max_viewers: 8 }
        );
        assert_eq!(
            PreviewPath::from(engine_share::SharePath::Broadcast { max_viewers: 20 }),
            PreviewPath::Broadcast { max_viewers: 20 }
        );
    }

    #[test]
    fn viewer_switch_keeps_the_session() {
        let av1 = engine_share::ActiveEncoder {
            codec: engine_share::wire::Codec::Av1,
            gpu: None,
        };
        let vp9 = engine_share::ActiveEncoder {
            codec: engine_share::wire::Codec::Vp9,
            gpu: None,
        };
        let switched: EncoderSwitch = engine_share::EncoderSwitch {
            reason: engine_share::SwitchReason::Viewer { session: 7 },
            from: av1,
            to: vp9,
            undo: false,
        }
        .into();
        assert_eq!(switched.reason, SwitchReason::Viewer { session: 7 });
        assert_eq!(switched.to.codec, VideoCodec::Vp9);
    }
}

/// How the share would reach its viewers, for the start dialog's "Viewers get" line.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PreviewPath {
    /// "sent once via relay, up to {maxViewers} viewers" (fan-out, FR-7.40).
    #[serde(rename_all = "camelCase")]
    Broadcast { max_viewers: u32 },
    /// "one copy per viewer, up to {maxViewers}".
    #[serde(rename_all = "camelCase")]
    Copies { max_viewers: u32 },
}

impl From<engine_share::SharePath> for PreviewPath {
    fn from(p: engine_share::SharePath) -> Self {
        let clamp = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        match p {
            engine_share::SharePath::Broadcast { max_viewers } => Self::Broadcast {
                max_viewers: clamp(max_viewers),
            },
            engine_share::SharePath::Copies { max_viewers } => Self::Copies {
                max_viewers: clamp(max_viewers),
            },
        }
    }
}

/// The start dialog's "Viewers get" line before the share starts (FR-7.36).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SharePreview {
    /// The share's top quality step, e.g. "1080p30".
    pub quality: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Target bitrate of that step for the encoder's codec.
    pub kbps: u32,
    pub encoder: ActiveEncoder,
    /// Manual AV1 would run in software without a passed self-test (FR-7.54).
    pub software_warning: bool,
    /// Why Automatic would not pick AV1.
    pub av1_skipped: Option<Av1Skipped>,
    pub path: PreviewPath,
    /// Threads the software VP9 encoder is configured with against the machine's cores; `None`
    /// for hardware encoders and software AV1.
    pub cpu_cores: Option<CpuCores>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct CpuCores {
    pub used: u32,
    pub total: u32,
}

impl From<engine_share::SharePreview> for SharePreview {
    fn from(p: engine_share::SharePreview) -> Self {
        Self {
            quality: p.quality,
            width: p.width,
            height: p.height,
            fps: p.fps,
            kbps: p.kbps,
            encoder: p.encoder.into(),
            software_warning: p.software_warning,
            av1_skipped: p.av1_skipped.map(Into::into),
            path: p.path.into(),
            cpu_cores: p.cpu_cores.map(|c| CpuCores {
                used: c.used,
                total: c.total,
            }),
        }
    }
}

/// What a share started now with these picks would get (FR-7.36). Starts no self-test.
#[tauri::command]
#[specta::specta]
pub fn screen_share_preview(
    engine: State<'_, Engine>,
    server_id: ServerId,
    choice: QualityChoice,
    codec: ScreenCodec,
) -> SharePreview {
    engine
        .screen_share_preview(to_engine_server_id(server_id), choice.into(), codec.into())
        .into()
}
