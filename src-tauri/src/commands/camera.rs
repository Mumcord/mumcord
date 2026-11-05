//! The own camera (spec 08): devices, the preview with its background effect, turning the
//! camera on and off, and its picture for the preview, the Settings preview and the self-view
//! (FR-8.10 – FR-8.15, FR-8.29, FR-8.30, FR-8.40). Availability per server is
//! `camera_unavailable` / `cameraAvailabilityChanged` (p2p.rs).

use std::sync::{Arc, Mutex, PoisonError};

use mumcord_engine::Engine;
use mumcord_engine::camera::{
    self as engine_camera, CameraFailure as EngineFailure, CameraState as EngineState,
    CameraView as EngineView, EffectLevel, EffectLoad, PreviewPicture,
};
use mumcord_engine::store;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::types::{
    ApiError, ApiResult, ErrorCode, ServerId, SessionId, to_api_server_id, to_engine_server_id,
};
use crate::screen_frames::{self, FrameChannel};

/// Where the own camera is (spec 08, section 6.4).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum CameraState {
    Off,
    /// The preview runs locally; nothing is sent (FR-8.10).
    Preview,
    /// **Turn on camera** was pressed; waiting for the broadcast (FR-8.13).
    Requesting,
    /// The camera is on air.
    OnAir,
}

impl From<EngineState> for CameraState {
    fn from(s: EngineState) -> Self {
        match s {
            EngineState::Off => Self::Off,
            EngineState::Preview => Self::Preview,
            EngineState::Requesting => Self::Requesting,
            EngineState::OnAir => Self::OnAir,
        }
    }
}

/// Why the preview could not open or the camera could not turn on; the UI shows the texts of
/// FR-8.25, FR-8.26 and FR-8.51.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum CameraFailure {
    NotConnected,
    /// Another camera runs already (one per Mumcord instance, FR-8.29), or the step does not fit
    /// the camera's state.
    Busy,
    NoCamera,
    PermissionDenied,
    DeviceGone,
    Unreadable,
    CameraFailed,
    RoomFull,
    RelayFull,
    UseNotAllowed,
    Denied,
    RelayDown,
    /// The relay ended the broadcast ("Camera stopped by the server's relay").
    RelayEnded,
    RateLimited,
    /// The P2P mode leaves the relay out.
    Mode,
    NoCertificate,
}

impl From<&EngineFailure> for CameraFailure {
    fn from(f: &EngineFailure) -> Self {
        use engine_camera::CameraError;
        match f {
            EngineFailure::NotConnected => Self::NotConnected,
            EngineFailure::Busy => Self::Busy,
            EngineFailure::Camera(CameraError::NoCamera) => Self::NoCamera,
            EngineFailure::Camera(CameraError::PermissionDenied) => Self::PermissionDenied,
            EngineFailure::Camera(CameraError::DeviceGone) => Self::DeviceGone,
            EngineFailure::Camera(CameraError::Unreadable) => Self::Unreadable,
            EngineFailure::Camera(CameraError::Backend(_)) => Self::CameraFailed,
            EngineFailure::RoomFull => Self::RoomFull,
            EngineFailure::RelayFull => Self::RelayFull,
            EngineFailure::UseNotAllowed => Self::UseNotAllowed,
            EngineFailure::Refused => Self::Denied,
            EngineFailure::RelayDown => Self::RelayDown,
            EngineFailure::RelayEnded => Self::RelayEnded,
            EngineFailure::RateLimited => Self::RateLimited,
            EngineFailure::Mode => Self::Mode,
            EngineFailure::NoCertificate => Self::NoCertificate,
        }
    }
}

/// `notConnected` keeps its own code; every other failure is `camera` with the reason
/// (a [`CameraFailure`] in camelCase) as `message`.
fn api_error(f: &EngineFailure) -> ApiError {
    let failure = CameraFailure::from(f);
    if failure == CameraFailure::NotConnected {
        return ApiError::new(ErrorCode::NotConnected, "not connected");
    }
    let code = serde_json::to_value(failure)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    ApiError::new(ErrorCode::Camera, code)
}

/// The own camera as the preview dialog, Settings, the room and the tray see it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct CameraView {
    pub server_id: ServerId,
    pub state: CameraState,
    /// Why the last **Turn on camera** failed; shown in the preview until the next try.
    pub error: Option<CameraFailure>,
    /// The broadcast id (16 hex) while on air.
    pub broadcast: Option<String>,
    /// The people who receive the camera now.
    pub viewers: Vec<SessionId>,
}

impl From<EngineView> for CameraView {
    fn from(v: EngineView) -> Self {
        Self {
            server_id: to_api_server_id(v.server_id),
            state: v.state.into(),
            error: v.error.as_ref().map(Into::into),
            broadcast: v.broadcast,
            viewers: v.viewers,
        }
    }
}

/// The own camera changed (state, error, viewers). `state: "off"` once it is off.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct CameraChanged {
    pub view: CameraView,
}

/// A camera of this computer. The name is for the picker only and never logged (FR-8.28).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct CameraDevice {
    /// Stable id; Settings remember it as `cameraDeviceId` (FR-8.27).
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// A background choice (FR-8.11): none, blur, a built-in image or an own image.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum CameraBackground {
    None,
    Blur,
    Builtin { id: String },
    Own { id: String },
}

impl From<CameraBackground> for store::CameraBackground {
    fn from(b: CameraBackground) -> Self {
        match b {
            CameraBackground::None => Self::None,
            CameraBackground::Blur => Self::Blur,
            CameraBackground::Builtin { id } => Self::Builtin(id),
            CameraBackground::Own { id } => Self::Own(id),
        }
    }
}

/// Effect quality (FR-8.35).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum CameraEffectQuality {
    Auto,
    Best,
    Light,
}

impl From<CameraEffectQuality> for store::CameraEffectQuality {
    fn from(q: CameraEffectQuality) -> Self {
        match q {
            CameraEffectQuality::Auto => Self::Auto,
            CameraEffectQuality::Best => Self::Best,
            CameraEffectQuality::Light => Self::Light,
        }
    }
}

/// The effect level that runs: "Best" or "Light" (FR-8.36).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum CameraEffectLevel {
    Best,
    Light,
}

/// The load line "Effects run on this device. Load: {low|medium|high}" (FR-8.36).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum CameraEffectLoad {
    Low,
    Medium,
    High,
}

/// The background effects of this PC and of the running camera.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct CameraEffects {
    /// Why background effects are off on this PC, for "Background effects are not available on
    /// this PC: {reason}" (FR-8.33); `null` when they can run.
    pub unavailable: Option<String>,
    /// The level that runs; `null` while no camera runs or the background is None.
    pub level: Option<CameraEffectLevel>,
    /// `null` while no camera runs.
    pub load: Option<CameraEffectLoad>,
    /// "Your PC is too busy for this background effect." (FR-8.37)
    pub too_busy: bool,
}

/// The cameras of this computer, in OS order (FR-8.11 picker, Settings → Camera).
#[tauri::command]
#[specta::specta]
pub fn camera_devices(engine: State<'_, Engine>) -> ApiResult<Vec<CameraDevice>> {
    let devices = engine.camera_devices().map_err(|e| api_error(&e))?;
    Ok(devices
        .into_iter()
        .map(|d| CameraDevice {
            id: d.id.as_str().to_owned(),
            name: d.name,
            is_default: d.is_default,
        })
        .collect())
}

/// The own camera now; `null` while it is off.
#[tauri::command]
#[specta::specta]
pub fn camera_view(engine: State<'_, Engine>) -> Option<CameraView> {
    engine.camera_view().map(Into::into)
}

/// Opens the preview dialog's camera on a server (Off → Preview, FR-8.10) with the default
/// background; nothing is sent. Takes over a running Settings preview (FR-8.29). Errors:
/// `notConnected`, `camera` (reason in `message`).
#[tauri::command]
#[specta::specta]
pub fn camera_preview_start(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<()> {
    engine
        .camera_preview_start(to_engine_server_id(server_id))
        .map_err(|e| api_error(&e))
}

/// **Turn on camera** (FR-8.13): resolves once the camera is on air. A failure leaves the
/// camera in the preview; the reason is the error's `message` and `CameraView.error`.
#[tauri::command]
#[specta::specta]
pub async fn camera_turn_on(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<()> {
    engine
        .camera_turn_on(to_engine_server_id(server_id))
        .await
        .map_err(|e| api_error(&e))
}

/// Turns the camera off from any state: Cancel / Esc in the preview, the camera button
/// (FR-8.12). Releases the device.
#[tauri::command]
#[specta::specta]
pub fn camera_turn_off(engine: State<'_, Engine>, server_id: ServerId) {
    engine.camera_turn_off(to_engine_server_id(server_id));
}

/// Starts the Settings → Camera preview (FR-8.30), without a server. Errors: `camera` with
/// `busy` while the preview dialog or the camera runs, or the camera's reason.
#[tauri::command]
#[specta::specta]
pub fn camera_local_preview_start(engine: State<'_, Engine>) -> ApiResult<()> {
    engine
        .camera_local_preview_start()
        .map_err(|e| api_error(&e))
}

/// Stops the Settings preview when the page or the Settings window closes (FR-8.30).
#[tauri::command]
#[specta::specta]
pub fn camera_local_preview_stop(engine: State<'_, Engine>) {
    engine.camera_local_preview_stop();
}

/// Switches the running preview to another camera (id from `camera_devices`). Does not save
/// the choice; the UI stores it as `cameraDeviceId`. Errors: `camera` with `busy` while on
/// air, or the new camera's reason (the old one keeps running).
#[tauri::command]
#[specta::specta]
pub fn camera_set_device(engine: State<'_, Engine>, device_id: String) -> ApiResult<()> {
    engine
        .camera_set_device(engine_camera::DeviceId::from_stored(device_id))
        .map_err(|e| api_error(&e))
}

/// Changes the background of the running camera, also on air (FR-8.11, FR-8.40). Does not
/// save it; "Use this background next time" saves the setting. Errors: `notFound` for an
/// unknown image, `invalidInput` for an image that can't be decoded.
#[tauri::command]
#[specta::specta]
pub fn camera_set_background(
    engine: State<'_, Engine>,
    background: CameraBackground,
    blur_strength: u8,
) -> ApiResult<()> {
    let resolved = engine
        .camera_resolve_background(&background.into(), blur_strength)
        .map_err(|e| match e {
            engine_camera::EffectsError::UnknownBackground => {
                ApiError::new(ErrorCode::NotFound, e.to_string())
            }
            _ => ApiError::new(ErrorCode::InvalidInput, e.to_string()),
        })?;
    engine.camera_set_background(resolved);
    Ok(())
}

/// Changes the effect quality of the running camera (FR-8.35); Auto measures again.
#[tauri::command]
#[specta::specta]
pub fn camera_set_effect_quality(engine: State<'_, Engine>, quality: CameraEffectQuality) {
    engine.camera_set_effect_quality(quality.into());
}

/// Whether effects can run on this PC, and the load line of the running camera (FR-8.33,
/// FR-8.36, FR-8.37). The preview polls it about once a second.
#[tauri::command]
#[specta::specta]
pub fn camera_effects(engine: State<'_, Engine>) -> CameraEffects {
    let stats = engine.camera_effect_stats();
    let unavailable = engine
        .camera_effects_available()
        .err()
        .or_else(|| stats.as_ref().and_then(|s| s.unavailable.clone()));
    CameraEffects {
        unavailable,
        level: stats.as_ref().and_then(|s| s.level).map(|l| match l {
            EffectLevel::Full => CameraEffectLevel::Best,
            EffectLevel::Light => CameraEffectLevel::Light,
        }),
        load: stats.as_ref().map(|s| match s.load {
            EffectLoad::Low => CameraEffectLoad::Low,
            EffectLoad::Medium => CameraEffectLoad::Medium,
            EffectLoad::High => CameraEffectLoad::High,
        }),
        too_busy: stats.is_some_and(|s| s.too_busy),
    }
}

/// The window that gets the own camera's picture now.
#[derive(Default)]
pub struct CameraPreview {
    channel: Mutex<Option<FrameChannel>>,
}

impl CameraPreview {
    fn send(&self, picture: &PreviewPicture<'_>) {
        let channel = self.channel.lock().unwrap_or_else(PoisonError::into_inner);
        if let (Some(channel), Some(message)) = (
            channel.as_ref(),
            screen_frames::rgba_message(picture.width, picture.height, picture.rgba),
        ) && let Err(e) = channel.send(message)
        {
            tracing::debug!(error = %e, "camera preview picture not delivered");
        }
    }
}

/// Sends the own camera's picture (after the effect, at most 640×360) to `frames` from now on:
/// binary messages with an RGBA picture in the screen-share layout (`ui/src/api/screenFrames.ts`).
/// One window at a time: the preview dialog, Settings or the room's self-view; attaching
/// replaces the earlier channel. Pictures come while the camera runs in any state.
#[tauri::command]
#[specta::specta]
pub fn camera_preview_attach(
    engine: State<'_, Engine>,
    preview: State<'_, Arc<CameraPreview>>,
    frames: FrameChannel,
) {
    *preview
        .channel
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(frames);
    let target = Arc::clone(&preview);
    engine.camera_set_preview_sink(Some(Box::new(move |picture| target.send(&picture))));
}

/// Stops sending the picture to the channel `channel_id` (`Channel.id` in JavaScript); does
/// nothing when another window attached since.
#[tauri::command]
#[specta::specta]
pub fn camera_preview_detach(
    engine: State<'_, Engine>,
    preview: State<'_, Arc<CameraPreview>>,
    channel_id: u32,
) {
    let mut channel = preview
        .channel
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if channel.as_ref().is_some_and(|c| c.id() == channel_id) {
        *channel = None;
        drop(channel);
        engine.camera_set_preview_sink(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_failures_go_to_the_ui_as_camel_case_reasons() {
        let e = api_error(&EngineFailure::RoomFull);
        assert_eq!(
            (e.code, e.message.as_str()),
            (ErrorCode::Camera, "roomFull")
        );
        let e = api_error(&EngineFailure::Refused);
        assert_eq!(e.message, "denied");
        let e = api_error(&EngineFailure::NotConnected);
        assert_eq!(e.code, ErrorCode::NotConnected);
    }

    #[test]
    fn background_choices_map_to_the_stored_setting() {
        let own: store::CameraBackground = CameraBackground::Own { id: "abc".into() }.into();
        assert_eq!(own, store::CameraBackground::Own("abc".into()));
        let json = serde_json::to_value(CameraBackground::Builtin { id: "beach".into() });
        assert_eq!(
            json.ok(),
            Some(serde_json::json!({ "kind": "builtin", "id": "beach" }))
        );
    }
}
