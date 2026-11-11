//! The own camera (spec 08, section 6.4): Off → Preview → Requesting → OnAir.
//!
//! - **Preview**: the camera runs locally, nothing is sent (FR-8.10): no `bcast-req`, no
//!   broadcast, no announcement.
//! - **Requesting**: **Turn on camera** asks the bot for a broadcast with `k = cam` (FR-8.13,
//!   FR-8.42) and binds as its publisher. A refusal goes back to Preview with the reason
//!   (FR-8.51).
//! - **OnAir**: the broadcast is live, the channel gets `mumcord.camera on`, newcomers get it too
//!   (FR-8.8), viewers are admitted on `cam-join` and removed with a rekey (FR-8.47 – FR-8.49),
//!   and the encoder follows their `want` reports (FR-8.45).
//!
//! One camera per Mumcord instance (FR-8.29). The stop rules (leave channel, lock screen, …) live
//! in `stop`.

use std::collections::HashMap;
use std::sync::Arc;

use mumcord_camera::{CameraBackend, CameraError, TestCamera, TestCameraMode};
use mumcord_p2p::broadcast::BindRole;
use mumcord_p2p::quinn::{RecvStream, SendStream};
use mumcord_p2p::{
    BroadcastEnd, BroadcastErrorReason, BroadcastId, P2pError, RelayRefusal, RelayToken,
};
use mumcord_protocol::PluginData;
use mumcord_store::{ScreenFps, ScreenOptimize, ScreenResolution};
use tokio::sync::oneshot;

use super::admit::{CameraViewers, Join, MAX_VIEWERS};
use super::preview::PreviewTap;
use super::send::{CameraSend, SendCeiling};
use super::signal::{self, CameraMessage, CameraSignals, Codec};
use super::stop::StopReason;
use crate::ServerId;
use crate::mumcord::ServerPeers;
use crate::p2p::DirectError;
use crate::peer::{RelayUnavailable, RelayUse};
use crate::screen_share::QualityPrefs;
use crate::screen_share::broadcast::keyx::CertKey;
use crate::screen_share::broadcast::link::{OnEnd, Publisher};
use crate::screen_share::broadcast::{FanoutUnavailable, cert_key};
use crate::session::{Engine, EngineEvent, SessionCtx, WeakEngine, lock};

/// Where the own camera is (section 6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraState {
    Off,
    /// The preview dialog runs the camera locally; nothing is sent.
    Preview,
    /// Asking the bot for the broadcast.
    Requesting,
    /// The broadcast is live.
    OnAir,
}

/// What went wrong while opening the preview or turning the camera on. The UI shows the texts of
/// FR-8.25/8.26 (camera) and FR-8.51 (`bcast-req`); [`Self::code`] names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CameraFailure {
    NotConnected,
    /// Another camera runs already (one per Mumcord instance, FR-8.29), or the step does not fit
    /// the state (Turn on without a preview).
    Busy,
    Camera(CameraError),
    /// The channel has as many cameras as the bot allows (`room-full`).
    RoomFull,
    /// The relay runs as many camera broadcasts as it may (`relay-full`).
    RelayFull,
    /// The relay does not allow camera sharing (`use-not-allowed`, or an older relay).
    UseNotAllowed,
    /// The relay or the bot refused (`not-same-channel`, `denied`).
    Refused,
    /// The relay isn't reachable (`relay-down`, no answer, link failed), or it became unavailable
    /// while the camera was on (FR-8.24).
    RelayDown,
    /// The relay ended the broadcast (`end` with `deleted` or `revoked`, FR-8.24).
    RelayEnded,
    /// Too many attempts (`rate-limited`).
    RateLimited,
    /// The P2P mode leaves the relay out.
    Mode,
    /// The relay only serves registered users and this client is a guest (#601).
    NotRegistered,
    /// The relay only serves members of certain groups (#601).
    GroupNotAllowed,
    /// Only people in the same channel can see each other's camera (#601).
    NotSameChannel,
    /// This client is in no channel the bot can see (#601).
    NotInChannel,
    /// The bot doesn't know this client yet (e.g. it restarted); reconnecting fixes it (#601).
    NoHello,
    /// No client certificate to bind with and sign the key exchange.
    NoCertificate,
}

impl CameraFailure {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotConnected => "not-connected",
            Self::Busy => "busy",
            Self::Camera(CameraError::NoCamera) => "no-camera",
            Self::Camera(CameraError::PermissionDenied) => "permission-denied",
            Self::Camera(CameraError::DeviceGone) => "device-gone",
            Self::Camera(CameraError::Unreadable) => "unreadable",
            Self::Camera(CameraError::Backend(_)) => "camera-failed",
            Self::RoomFull => "room-full",
            Self::RelayFull => "relay-full",
            Self::UseNotAllowed => "use-not-allowed",
            Self::Refused => "denied",
            Self::RelayDown => "relay-down",
            Self::RelayEnded => "relay-ended",
            Self::RateLimited => "rate-limited",
            Self::Mode => "mode",
            Self::NoCertificate => "no-certificate",
            Self::NotRegistered => "not-registered",
            Self::GroupNotAllowed => "group-not-allowed",
            Self::NotSameChannel => "not-same-channel",
            Self::NotInChannel => "not-in-channel",
            Self::NoHello => "no-hello",
        }
    }

    fn from_reason(reason: BroadcastErrorReason) -> Self {
        match reason {
            BroadcastErrorReason::RoomFull => Self::RoomFull,
            BroadcastErrorReason::RelayFull | BroadcastErrorReason::Full => Self::RelayFull,
            BroadcastErrorReason::UseNotAllowed => Self::UseNotAllowed,
            BroadcastErrorReason::Denied => Self::Refused,
            BroadcastErrorReason::NotSameChannel => Self::NotSameChannel,
            BroadcastErrorReason::NotRegistered => Self::NotRegistered,
            BroadcastErrorReason::GroupNotAllowed => Self::GroupNotAllowed,
            BroadcastErrorReason::NotInChannel => Self::NotInChannel,
            BroadcastErrorReason::NoHello => Self::NoHello,
            BroadcastErrorReason::RelayDown => Self::RelayDown,
            BroadcastErrorReason::RateLimited => Self::RateLimited,
        }
    }

    fn from_relay(relay: RelayUnavailable) -> Self {
        match relay {
            RelayUnavailable::NotConnected => Self::NotConnected,
            RelayUnavailable::UseNotAllowed(_) => Self::UseNotAllowed,
            RelayUnavailable::RelayFull => Self::RelayFull,
            RelayUnavailable::RateLimited => Self::RateLimited,
            RelayUnavailable::Denied | RelayUnavailable::Refused(_) => Self::Refused,
            RelayUnavailable::NotRegistered => Self::NotRegistered,
            RelayUnavailable::GroupNotAllowed => Self::GroupNotAllowed,
            RelayUnavailable::NoHello => Self::NoHello,
            _ => Self::RelayDown,
        }
    }

    fn from_fanout(fanout: FanoutUnavailable) -> Self {
        match fanout {
            FanoutUnavailable::Relay(relay) => Self::from_relay(relay),
            FanoutUnavailable::Refused(reason) => Self::from_reason(reason),
            FanoutUnavailable::NotOffered => Self::UseNotAllowed,
            FanoutUnavailable::Mode => Self::Mode,
            FanoutUnavailable::NoCertificate => Self::NoCertificate,
            FanoutUnavailable::Lost => Self::RelayDown,
        }
    }

    fn from_link(e: &P2pError) -> Self {
        match e {
            P2pError::RelayRefused(RelayRefusal::UseNotAllowed) => Self::UseNotAllowed,
            P2pError::RelayRefused(_) => Self::Refused,
            _ => Self::RelayDown,
        }
    }
}

/// The own camera as Settings, the preview dialog, the tray and the media area see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraView {
    pub server_id: ServerId,
    pub state: CameraState,
    /// Why the last Turn on failed; shown in the preview until the next try (FR-8.13).
    pub error: Option<CameraFailure>,
    /// The broadcast id (16 hex) while on air.
    pub broadcast: Option<String>,
    /// The admitted viewers' sessions.
    pub viewers: Vec<u32>,
}

impl CameraView {
    pub fn off(server_id: ServerId) -> Self {
        Self {
            server_id,
            state: CameraState::Off,
            error: None,
            broadcast: None,
            viewers: Vec::new(),
        }
    }
}

/// A step of the session (section 6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Menu, user panel button or hotkey.
    Open,
    /// **Turn on camera** in the preview.
    TurnOn,
    /// `bcast-err` or a failed link.
    Refused,
    /// The broadcast exists and the publisher is bound.
    Ready,
    /// Cancel / Esc in the preview, or any stop rule.
    TurnOff,
}

/// The state after `step`, or `None` when the step does not apply in `state`.
pub(crate) fn next_state(state: CameraState, step: Step) -> Option<CameraState> {
    use CameraState::{Off, OnAir, Preview, Requesting};
    match (state, step) {
        (Off, Step::Open) => Some(Preview),
        (Preview, Step::TurnOn) => Some(Requesting),
        (Requesting, Step::Refused) => Some(Preview),
        (Requesting, Step::Ready) => Some(OnAir),
        (Preview | Requesting | OnAir, Step::TurnOff) => Some(Off),
        _ => None,
    }
}

/// The broadcast id as `mumcord.camera` carries it.
pub fn broadcast_hex(id: BroadcastId) -> String {
    format!("{:016x}", id.0)
}

struct Live {
    id: BroadcastId,
    publisher: Publisher,
    viewers: CameraViewers,
    /// The broadcast uses the Bovnar media profile.
    media: bool,
    /// One `STREAM_CAMERA` per admitted viewer that reads the media profile (FR-8.92).
    direct: super::direct::DirectViewers,
}

pub(super) struct Own {
    server_id: ServerId,
    pub(super) state: CameraState,
    /// Tells a Turn on apart from an earlier one that was cancelled meanwhile.
    attempt: u64,
    error: Option<CameraFailure>,
    pub(super) send: CameraSend,
    live: Option<Live>,
}

impl Own {
    fn view(&self) -> CameraView {
        CameraView {
            server_id: self.server_id,
            state: self.state,
            error: self.error.clone(),
            broadcast: self.live.as_ref().map(|l| broadcast_hex(l.id)),
            viewers: self
                .live
                .as_ref()
                .map(|l| l.viewers.sessions())
                .unwrap_or_default(),
        }
    }

    fn live_on(&mut self, server_id: ServerId) -> Option<&mut Live> {
        if self.server_id != server_id || self.state != CameraState::OnAir {
            return None;
        }
        self.live.as_mut()
    }
}

/// The camera side of the engine: the own camera and the camera tokens viewers wait for.
#[derive(Default)]
pub(crate) struct Cameras {
    pub(crate) engine: Option<WeakEngine>,
    pub(super) own: Option<Own>,
    /// The Settings → Camera preview (FR-8.30): the camera runs locally without a server. The
    /// preview dialog takes it over (FR-8.29).
    pub(super) local: Option<CameraSend>,
    /// The background of the running camera, kept for a device change.
    pub(super) background: Option<mumcord_effects::Background>,
    /// Where the built-in background images are (the app's resources).
    pub(super) builtin_backgrounds: Option<std::path::PathBuf>,
    /// The own picture for the preview and the self-view, shared by every camera session.
    pub(super) preview: Arc<PreviewTap>,
    attempts: u64,
    /// The OS camera API, or the test camera; set once.
    backend: Option<Arc<dyn CameraBackend>>,
    /// Viewers waiting for the bot's `bcast-tok` of a camera, by server and camera owner.
    waiting: HashMap<(ServerId, u32), oneshot::Sender<(BroadcastId, RelayToken)>>,
    /// Other people's cameras (the viewer side).
    pub(super) recv: super::recv::Receiving,
    /// Broadcast ends kept open by the test viewer.
    #[cfg(feature = "test-backend")]
    test_views: Vec<crate::screen_share::broadcast::link::Subscriber>,
}

pub(crate) type ServerCameras = std::sync::Mutex<Cameras>;

impl Cameras {
    pub(super) fn backend(&mut self) -> Option<Arc<dyn CameraBackend>> {
        if self.backend.is_none() {
            self.backend = TestCameraMode::from_env()
                .map(|mode| Arc::new(TestCamera::new(mode)) as Arc<dyn CameraBackend>)
                .or_else(os_backend);
        }
        self.backend.clone()
    }

    pub(super) fn engine(&self) -> Option<Engine> {
        self.engine.as_ref().and_then(WeakEngine::upgrade)
    }
}

/// The camera API of this OS.
fn os_backend() -> Option<Arc<dyn CameraBackend>> {
    #[cfg(target_os = "linux")]
    let backend: Option<Arc<dyn CameraBackend>> =
        Some(Arc::new(mumcord_camera::LinuxCamera::new()));
    #[cfg(target_os = "macos")]
    let backend: Option<Arc<dyn CameraBackend>> = Some(Arc::new(mumcord_camera::MacCamera::new()));
    #[cfg(windows)]
    let backend: Option<Arc<dyn CameraBackend>> =
        Some(Arc::new(mumcord_camera::WindowsCamera::new()));
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    let backend = None;
    backend
}

/// The screen-share prefs whose encoder selection and self-test the camera uses (FR-8.43):
/// 720p30 (or 15) natural video.
pub(super) fn encoder_prefs(ceiling: SendCeiling) -> QualityPrefs {
    QualityPrefs {
        resolution: ScreenResolution::P720,
        fps: if ceiling.fps <= 15 {
            ScreenFps::Fps15
        } else {
            ScreenFps::Fps30
        },
        optimize: ScreenOptimize::Motion,
        ..QualityPrefs::default()
    }
}

pub(super) fn cert_hash(ctx: &SessionCtx, session: u32) -> Option<String> {
    ctx.handle
        .with_state(|s| s.users.get(&session).and_then(|u| u.cert_hash.clone()))
}

/// Whether `session` is in the own channel.
fn in_own_channel(ctx: &SessionCtx, session: u32) -> bool {
    ctx.handle.with_state(|s| {
        let own = s.users.get(&s.session?)?.channel_id;
        Some(s.users.get(&session)?.channel_id == own)
    }) == Some(true)
}

/// The opened publisher side of a camera broadcast, with its viewer limit.
type Opened = (BroadcastId, CertKey, mumcord_p2p::BroadcastLink, u16);

impl Engine {
    /// Uses `backend` for the camera instead of the default one (the per-OS backends and the
    /// test camera are wired in here).
    pub fn camera_set_backend(&self, backend: Arc<dyn CameraBackend>) {
        lock(self.camera_state()).backend = Some(backend);
    }

    /// The own camera, `None` while it is off.
    pub fn camera_view(&self) -> Option<CameraView> {
        lock(self.camera_state()).own.as_ref().map(Own::view)
    }

    fn emit_camera(&self, server_id: ServerId, view: Option<CameraView>) {
        let view = view.unwrap_or_else(|| CameraView::off(server_id));
        self.events()
            .for_server(server_id)
            .emit(EngineEvent::CameraChanged(view));
    }

    /// Opens the preview (Off → Preview, FR-8.10): the camera runs with the default background
    /// (FR-8.76), nothing is sent. A running Settings preview moves to it (FR-8.29).
    pub fn camera_preview_start(&self, server_id: ServerId) -> Result<(), CameraFailure> {
        self.session_ctx_for(server_id)
            .map_err(|_| CameraFailure::NotConnected)?;
        super::stop::watch_screen_lock(self);
        let settings = self.encoder_settings();
        let background = self.camera_default_background(&settings);
        let view = {
            let mut cameras = lock(self.camera_state());
            if let Some(own) = &cameras.own {
                return if own.server_id == server_id && own.state == CameraState::Preview {
                    Ok(())
                } else {
                    Err(CameraFailure::Busy)
                };
            }
            let send = match cameras.local.take() {
                Some(send) => send,
                None => self.camera_send_start(&mut cameras, &settings, None, background)?,
            };
            let Some(state) = next_state(CameraState::Off, Step::Open) else {
                return Err(CameraFailure::Busy);
            };
            let own = Own {
                server_id,
                state,
                attempt: 0,
                error: None,
                send,
                live: None,
            };
            let view = own.view();
            cameras.own = Some(own);
            view
        };
        self.emit_camera(server_id, Some(view));
        Ok(())
    }

    /// **Turn on camera** (Preview → Requesting → OnAir, FR-8.13): returns once the broadcast is
    /// live, or with the reason it is not (the camera stays in the preview).
    pub async fn camera_turn_on(&self, server_id: ServerId) -> Result<(), CameraFailure> {
        let (attempt, view) = {
            let mut cameras = lock(self.camera_state());
            cameras.attempts += 1;
            let attempt = cameras.attempts;
            let own = cameras
                .own
                .as_mut()
                .filter(|o| o.server_id == server_id)
                .ok_or(CameraFailure::Busy)?;
            own.state = next_state(own.state, Step::TurnOn).ok_or(CameraFailure::Busy)?;
            own.attempt = attempt;
            own.error = None;
            (attempt, own.view())
        };
        self.emit_camera(server_id, Some(view));
        let opened = self.open_camera_broadcast(server_id).await;
        self.camera_opened(server_id, attempt, opened)
    }

    async fn open_camera_broadcast(&self, server_id: ServerId) -> Result<Opened, CameraFailure> {
        self.relays()
            .availability_for(server_id, RelayUse::Camera)
            .map_err(CameraFailure::from_relay)?;
        if !self.relay_mode_allows_broadcast(server_id) {
            return Err(CameraFailure::Mode);
        }
        // A relay without broadcasts can't carry a camera.
        let limit = self
            .relays()
            .broadcast_announcement_for(server_id, RelayUse::Camera)
            .map_err(CameraFailure::from_relay)?
            .broadcast
            .filter(|max| *max > 0)
            .ok_or(CameraFailure::UseNotAllowed)?
            .min(MAX_VIEWERS);
        let (p2p, identity) = self
            .p2p_with_identity()
            .await
            .ok_or(CameraFailure::NoCertificate)?;
        let me = cert_key(&identity).ok_or(CameraFailure::NoCertificate)?;
        let (ann, id, token) = self
            .relay_broadcast_request(server_id, RelayUse::Camera, limit, false)
            .await
            .map_err(CameraFailure::from_fanout)?;
        let link = p2p
            .broadcast(&ann, &token, BindRole::Publisher, false)
            .await
            .map_err(|e| {
                tracing::info!(error = %e, "camera broadcast link not opened");
                CameraFailure::from_link(&e)
            })?;
        Ok((id, me, link, limit))
    }

    fn camera_opened(
        &self,
        server_id: ServerId,
        attempt: u64,
        opened: Result<Opened, CameraFailure>,
    ) -> Result<(), CameraFailure> {
        let rt = self.runtime().clone();
        let weak = self.downgrade();
        let (result, view, announce) = {
            let mut cameras = lock(self.camera_state());
            let Some(own) = cameras.own.as_mut().filter(|o| {
                o.server_id == server_id
                    && o.attempt == attempt
                    && o.state == CameraState::Requesting
            }) else {
                // Turned off meanwhile; dropping the link ends the broadcast.
                return Err(CameraFailure::Busy);
            };
            let started = opened.and_then(|(id, me, link, limit)| {
                let frames = own.send.take_frames().ok_or(CameraFailure::Busy)?;
                let media = link.is_media();
                let on_end: OnEnd = Box::new(move |end| {
                    tracing::info!(?end, "camera broadcast ended");
                    if let Some(engine) = weak.upgrade() {
                        engine.camera_broadcast_ended(server_id, id, end);
                    }
                });
                frames.pump(&rt);
                let direct = super::direct::DirectViewers::new(broadcast_hex(id), frames.clone());
                let publisher = Publisher::start(&rt, id, me, link, frames.relay(), on_end)
                    .map_err(|e| {
                        tracing::warn!(error = %e, "camera broadcast not started");
                        CameraFailure::RelayDown
                    })?;
                Ok(Live {
                    id,
                    publisher,
                    viewers: CameraViewers::new(limit),
                    media,
                    direct,
                })
            });
            match started {
                Ok(live) => {
                    let id = live.id;
                    own.live = Some(live);
                    own.state = next_state(own.state, Step::Ready).unwrap_or(CameraState::OnAir);
                    own.send.set_on_air(true);
                    tracing::info!("camera on air");
                    crate::crash::set_context("camera", "on air");
                    (Ok(()), own.view(), Some(id))
                }
                Err(failure) => {
                    tracing::info!(reason = failure.code(), "camera not turned on");
                    own.state =
                        next_state(own.state, Step::Refused).unwrap_or(CameraState::Preview);
                    own.error = Some(failure.clone());
                    (Err(failure), own.view(), None)
                }
            }
        };
        if let (Some(id), Ok(ctx)) = (announce, self.session_ctx_for(server_id)) {
            let peers = signal::channel_peers(&ctx, self.mumcord_state());
            signal::send(
                &ctx,
                &peers,
                &CameraMessage::On {
                    id: broadcast_hex(id),
                },
            );
        }
        self.emit_camera(server_id, Some(view));
        result
    }

    /// Turns the camera off from any state (Cancel in the preview, the camera button): announces
    /// `off` when it was on air, ends the broadcast and releases the device. The stop rules of
    /// FR-8.21 – FR-8.24 take the same path with their reason ([`Self::camera_stop`]).
    pub fn camera_turn_off(&self, server_id: ServerId) {
        self.camera_stop(server_id, StopReason::TurnedOff);
    }

    /// Ends the own camera on `server_id` if it runs there; `error` is the "Camera stopped"
    /// notice the off view carries (FR-8.23, FR-8.24).
    pub(super) fn camera_end(&self, server_id: ServerId, error: Option<CameraFailure>) {
        let own = {
            let mut cameras = lock(self.camera_state());
            let applies = cameras.own.as_ref().is_some_and(|o| {
                o.server_id == server_id && next_state(o.state, Step::TurnOff).is_some()
            });
            if !applies {
                return;
            }
            cameras.own.take()
        };
        let Some(own) = own else {
            return;
        };
        if own.state == CameraState::OnAir
            && let Ok(ctx) = self.session_ctx_for(server_id)
        {
            let peers = signal::channel_peers(&ctx, self.mumcord_state());
            signal::send(&ctx, &peers, &CameraMessage::Off);
        }
        // The publisher's drop closes the link (the relay ends the broadcast), the capture's
        // drop turns the camera light off.
        drop(own);
        tracing::info!(
            reason = error.as_ref().map(CameraFailure::code),
            "camera off"
        );
        crate::crash::clear_context("camera");
        self.emit_camera(
            server_id,
            Some(CameraView {
                error,
                ..CameraView::off(server_id)
            }),
        );
    }

    /// The relay link of the own camera ended by itself (FR-8.24).
    fn camera_broadcast_ended(&self, server_id: ServerId, id: BroadcastId, end: BroadcastEnd) {
        let same = lock(self.camera_state())
            .own
            .as_ref()
            .and_then(|o| o.live.as_ref().filter(|_| o.server_id == server_id))
            .is_some_and(|l| l.id == id);
        if same {
            self.camera_stop(server_id, StopReason::from_end(end));
        }
    }

    /// The own camera's server and state, if it runs.
    pub(super) fn camera_own(&self) -> Option<(ServerId, CameraState)> {
        lock(self.camera_state())
            .own
            .as_ref()
            .map(|o| (o.server_id, o.state))
    }

    /// A viewer asked to join the own camera `id` (FR-8.47). Same channel, not blocked and
    /// the join rate were checked on arrival.
    fn camera_join(&self, ctx: &SessionCtx, viewer: u32, id: &str, codecs: &[Codec], media: bool) {
        let Some(hash) = cert_hash(ctx, viewer) else {
            return;
        };
        let needs_update = {
            let cameras = lock(self.camera_state());
            cameras
                .own
                .as_ref()
                .and_then(|own| own.live.as_ref())
                .filter(|live| broadcast_hex(live.id) == id)
                .is_some_and(|live| live.media && !media)
        };
        if needs_update {
            signal::send(ctx, &[viewer], &CameraMessage::Needs { id: id.to_owned() });
            return;
        }
        let (broadcast, direct) = {
            let mut cameras = lock(self.camera_state());
            let Some(own) = cameras.own.as_mut() else {
                return;
            };
            let send = &own.send;
            let Some(live) = own
                .live
                .as_mut()
                .filter(|l| broadcast_hex(l.id) == id && own.server_id == ctx.server_id)
            else {
                return;
            };
            match live.viewers.join(viewer, &hash, codecs) {
                Join::Full => (None, false),
                Join::Admit => {
                    live.publisher.admit(&hash);
                    send.set_vp9_only(live.viewers.needs_vp9());
                    // A viewer that reads the media profile gets its own direct stream while the
                    // relay broadcast cannot carry that profile (FR-8.92, FR-8.94).
                    let wanted = media && !live.media && live.direct.wants(viewer);
                    (Some(live.id), wanted)
                }
            }
        };
        let Some(broadcast) = broadcast else {
            tracing::debug!("camera viewer not admitted: the broadcast is full");
            // Told, so the viewer shows "Too many people are watching" instead of waiting for a
            // token that never comes (FR-8.52).
            signal::send(ctx, &[viewer], &CameraMessage::Full { id: id.to_owned() });
            return;
        };
        if direct {
            self.camera_direct_open(ctx.server_id, viewer, id);
        }
        let engine = self.clone();
        let server_id = ctx.server_id;
        self.runtime().spawn(async move {
            let Err(reason) = engine
                .relay_broadcast_admit(server_id, RelayUse::Camera, broadcast, viewer)
                .await
            else {
                engine.emit_camera(server_id, engine.camera_view());
                return;
            };
            tracing::info!(
                reason = reason.as_str(),
                "camera viewer not admitted by the bot"
            );
            if reason == BroadcastErrorReason::Full
                && let Ok(ctx) = engine.session_ctx_for(server_id)
            {
                let full = CameraMessage::Full {
                    id: broadcast_hex(broadcast),
                };
                signal::send(&ctx, &[viewer], &full);
            }
            engine.camera_remove_viewer(server_id, broadcast, viewer, false);
        });
    }

    /// Opens the direct stream to an admitted viewer that reads the media profile (FR-8.92). The
    /// connection is made in the background: a camera viewer that cannot connect directly keeps
    /// watching through the relay broadcast.
    fn camera_direct_open(&self, server_id: ServerId, viewer: u32, id: &str) {
        let engine = self.clone();
        let id = id.to_owned();
        self.runtime().spawn(async move {
            let opened = engine
                .p2p_open_stream(
                    server_id,
                    viewer,
                    crate::p2p::STREAM_CAMERA,
                    crate::p2p::ConsentPrompt::Allowed,
                )
                .await;
            engine.camera_direct_opened(server_id, viewer, &id, opened);
        });
    }

    /// The stream to `viewer` was opened: it starts sending only if that viewer is still admitted
    /// to this very camera session; a stream for an older session or a viewer that left meanwhile
    /// is dropped, which closes it (FR-8.92).
    fn camera_direct_opened(
        &self,
        server_id: ServerId,
        viewer: u32,
        id: &str,
        opened: Result<(SendStream, RecvStream), DirectError>,
    ) {
        let mut cameras = lock(self.camera_state());
        let Some(own) = cameras.own.as_mut().filter(|o| o.server_id == server_id) else {
            return;
        };
        let on_air = own.state == CameraState::OnAir;
        let Some(live) = own
            .live
            .as_mut()
            .filter(|l| on_air && broadcast_hex(l.id) == id)
        else {
            return;
        };
        if !live.viewers.contains(viewer) {
            return;
        }
        match opened {
            Ok((send, recv)) => {
                live.direct.add(self.runtime(), viewer, send, recv);
                tracing::info!(session = viewer, "camera direct stream opened");
            }
            Err(e) => tracing::debug!(error = %e, "no direct stream to a camera viewer"),
        }
    }

    /// Ends `viewer`'s admission to the own camera `id`: it gets no keys any more, the key
    /// rotates (FR-7.44), and with `kick` the bot is told (`bcast-kick`, FR-8.49).
    fn camera_remove_viewer(&self, server_id: ServerId, id: BroadcastId, viewer: u32, kick: bool) {
        let removed = {
            let mut cameras = lock(self.camera_state());
            let Some(own) = cameras.own.as_mut() else {
                return;
            };
            let on_air = own.server_id == server_id && own.state == CameraState::OnAir;
            let send = &own.send;
            match own.live.as_mut().filter(|l| on_air && l.id == id) {
                Some(live) => {
                    // Its direct stream, if any, closes with the admission (FR-8.92).
                    live.direct.remove(viewer);
                    let was_full = live.viewers.is_full();
                    match live.viewers.leave(viewer) {
                        Some(hash) => {
                            live.publisher.remove(&hash);
                            send.set_vp9_only(live.viewers.needs_vp9());
                            Some(was_full)
                        }
                        None => None,
                    }
                }
                None => None,
            }
        };
        let Some(was_full) = removed else {
            return;
        };
        // A freed slot is announced again, so viewers turned away as full ask anew (FR-8.52).
        if was_full && let Ok(ctx) = self.session_ctx_for(server_id) {
            let peers = signal::channel_peers(&ctx, self.mumcord_state());
            signal::send(
                &ctx,
                &peers,
                &CameraMessage::On {
                    id: broadcast_hex(id),
                },
            );
        }
        if kick {
            self.relay_broadcast_kick(server_id, RelayUse::Camera, id, viewer);
        }
        self.emit_camera(server_id, self.camera_view());
    }

    /// The own camera's broadcast on `server_id` with `viewer` admitted, if any.
    fn camera_watched_by(&self, server_id: ServerId, viewer: u32) -> Option<BroadcastId> {
        let mut cameras = lock(self.camera_state());
        let live = cameras.own.as_mut()?.live_on(server_id)?;
        live.viewers.contains(viewer).then_some(live.id)
    }

    /// Nobody this client blocks keeps a direct camera stream (FR-8.92); the relay broadcast of
    /// the camera is left as it is.
    pub(super) fn camera_close_blocked_direct(&self, ctx: &SessionCtx) {
        let blocked = self.camera_blocked(ctx);
        if blocked.is_empty() {
            return;
        }
        let mut cameras = lock(self.camera_state());
        let Some(own) = cameras
            .own
            .as_mut()
            .filter(|o| o.server_id == ctx.server_id)
        else {
            return;
        };
        let Some(live) = own.live.as_mut() else {
            return;
        };
        for viewer in blocked {
            live.direct.remove(viewer);
        }
    }

    /// The bot's `bcast-tok` for a camera (`k = cam`), already checked to come from the pinned
    /// bot usable for cameras: taken only while this client waits to join `sharer`'s camera.
    pub(crate) fn camera_broadcast_token(
        &self,
        ctx: &SessionCtx,
        sharer: u32,
        id: BroadcastId,
        token: RelayToken,
    ) {
        let waiter = lock(self.camera_state())
            .waiting
            .remove(&(ctx.server_id, sharer));
        match waiter {
            Some(waiter) => {
                let _ = waiter.send((id, token));
            }
            None => tracing::debug!("ignoring a camera token nobody waits for"),
        }
    }

    /// Waits for the bot's token to `sharer`'s camera on `server_id` (the viewer side, C-8):
    /// register before sending `cam-join`. A later call replaces an earlier one.
    pub(crate) fn camera_await_token(
        &self,
        server_id: ServerId,
        sharer: u32,
    ) -> oneshot::Receiver<(BroadcastId, RelayToken)> {
        let (tx, rx) = oneshot::channel();
        lock(self.camera_state())
            .waiting
            .insert((server_id, sharer), tx);
        rx
    }

    /// Test viewer (until C-8's viewer exists): asks `sharer` to admit this client to camera
    /// `id`, binds as a subscriber and returns the decrypted frames.
    #[cfg(feature = "test-backend")]
    pub async fn camera_test_watch(
        &self,
        server_id: ServerId,
        sharer: u32,
        id: &str,
    ) -> crate::EngineResult<tokio::sync::mpsc::Receiver<crate::screen_share::wire::Frame>> {
        use crate::session::{EngineError, ErrorKind};
        let failed = |what: &str| EngineError::new(ErrorKind::Internal, what.to_owned());
        let ctx = self.session_ctx_for(server_id)?;
        let token = self.camera_await_token(server_id, sharer);
        let join = CameraMessage::CamJoin {
            id: id.to_owned(),
            codecs: vec![Codec::Av1, Codec::Vp9],
            media: true,
        };
        if !signal::send(&ctx, &[sharer], &join) {
            return Err(failed("cam-join not sent"));
        }
        let (bid, token) = tokio::time::timeout(std::time::Duration::from_secs(30), token)
            .await
            .map_err(|_| failed("no camera token"))?
            .map_err(|_| failed("token wait dropped"))?;
        let ann = self
            .relays()
            .broadcast_announcement_for(server_id, RelayUse::Camera)
            .map_err(|_| failed("no relay for cameras"))?;
        let hash = cert_hash(&ctx, sharer).ok_or_else(|| failed("no certificate"))?;
        let (p2p, identity) = self
            .p2p_with_identity()
            .await
            .ok_or_else(|| failed("no identity"))?;
        let me = cert_key(&identity).ok_or_else(|| failed("no key"))?;
        let link = p2p
            .broadcast(&ann, &token, BindRole::Subscriber, false)
            .await
            .map_err(|e| failed(&e.to_string()))?;
        let (subscriber, frames, _units) = crate::screen_share::broadcast::link::Subscriber::start(
            self.runtime(),
            bid,
            me,
            hash,
            link,
            Box::new(|_| {}),
        );
        lock(self.camera_state()).test_views.push(subscriber);
        Ok(frames)
    }
}

/// `mumcord.camera` data from the session task (data from blocked people never gets here).
pub(crate) fn on_plugin_data(
    state: &ServerCameras,
    signals: &CameraSignals,
    ctx: &SessionCtx,
    data: &PluginData,
) {
    let Some(event) = signals.on_plugin_data(ctx, data) else {
        return;
    };
    let Some(engine) = lock(state).engine() else {
        return;
    };
    match event.message {
        CameraMessage::CamJoin {
            id, codecs, media, ..
        } => {
            engine.camera_join(ctx, event.sender, &id, &codecs, media);
        }
        CameraMessage::CamLeave { id } => {
            if let Some(live) = engine.camera_watched_by(ctx.server_id, event.sender)
                && broadcast_hex(live) == id
            {
                engine.camera_remove_viewer(ctx.server_id, live, event.sender, true);
            }
        }
        CameraMessage::On { id } => super::recv::on_camera_on(&engine, ctx, event.sender, id),
        CameraMessage::Full { id } => super::recv::on_camera_full(&engine, ctx, event.sender, &id),
        CameraMessage::Needs { id } => {
            super::recv::on_camera_needs(&engine, ctx, event.sender, &id)
        }
        CameraMessage::Off => super::recv::on_camera_gone(&engine, ctx, event.sender),
        CameraMessage::Needs { .. } => {}
    }
}

/// Session tick: a camera whose pipeline stopped on its own (unplugged, unreadable, no encoder)
/// turns off (FR-8.21 "device lost"; C-9 adds the notice).
pub(crate) fn on_tick(state: &ServerCameras, ctx: &SessionCtx) {
    let (ended, engine) = {
        let cameras = lock(state);
        let ended = cameras
            .own
            .as_ref()
            .filter(|o| o.server_id == ctx.server_id)
            .and_then(|o| o.send.ended());
        (ended, cameras.engine())
    };
    let Some(engine) = engine else {
        return;
    };
    if let Some(reason) = ended {
        tracing::info!(?reason, "camera stopped on its own");
        engine.camera_stop(ctx.server_id, StopReason::DeviceLost(reason));
    }
    super::recv::on_tick(&engine, ctx);
}

/// A person told this client its features: a newcomer in the channel gets the own `on`
/// (FR-8.8, FR-2.75).
pub(crate) fn on_peer_known(
    state: &ServerCameras,
    peers: &ServerPeers,
    ctx: &SessionCtx,
    session: u32,
) {
    let id = {
        let mut cameras = lock(state);
        let Some(live) = cameras.own.as_mut().and_then(|o| o.live_on(ctx.server_id)) else {
            return;
        };
        live.id
    };
    signal::announce_to_newcomer(
        ctx,
        peers,
        session,
        &CameraMessage::On {
            id: broadcast_hex(id),
        },
    );
}

/// A person joined the server or moved: a viewer who left the own channel is removed and the
/// key rotates (FR-8.49); someone who came in and is known gets the own `on`.
pub(crate) fn on_user_moved(
    state: &ServerCameras,
    peers: &ServerPeers,
    ctx: &SessionCtx,
    session: u32,
) {
    let Some(engine) = lock(state).engine() else {
        return;
    };
    if ctx.handle.with_state(|s| s.session) == Some(session) {
        // Leaving or being moved out of the channel ends the own camera (FR-8.21, FR-8.50).
        engine.camera_stop(ctx.server_id, StopReason::LeftChannel);
        super::recv::on_own_channel_changed(&engine, ctx);
    }
    if in_own_channel(ctx, session) {
        on_peer_known(state, peers, ctx, session);
        return;
    }
    super::recv::on_camera_gone(&engine, ctx, session);
    if let Some(id) = engine.camera_watched_by(ctx.server_id, session) {
        engine.camera_remove_viewer(ctx.server_id, id, session, true);
    }
}

/// A person left the server: their join limit is forgotten, and a viewer is removed from the own
/// camera with a rekey (the bot removes them too, FR-8.49).
pub(crate) fn on_user_removed(
    state: &ServerCameras,
    signals: &CameraSignals,
    ctx: &SessionCtx,
    session: u32,
) {
    signals.on_user_removed(ctx.server_id, session);
    let Some(engine) = lock(state).engine() else {
        return;
    };
    super::recv::on_camera_gone(&engine, ctx, session);
    if let Some(id) = engine.camera_watched_by(ctx.server_id, session) {
        engine.camera_remove_viewer(ctx.server_id, id, session, true);
    }
}

/// The connection to `server_id` ended or started afresh: the own camera there turns off (the
/// broadcast and the channel are gone), and waiting viewers give up. True when the own camera
/// was on that server.
pub(crate) fn on_connection_reset(
    state: &ServerCameras,
    signals: &CameraSignals,
    server_id: ServerId,
) -> bool {
    signals.on_connection_reset(server_id);
    let own = {
        let mut cameras = lock(state);
        cameras
            .waiting
            .retain(|(server, _), _| *server != server_id);
        #[cfg(feature = "test-backend")]
        cameras.test_views.clear();
        if cameras
            .own
            .as_ref()
            .is_some_and(|o| o.server_id == server_id)
        {
            cameras.own.take()
        } else {
            None
        }
    };
    let was_on = own.is_some();
    if was_on {
        tracing::info!("camera off: the connection ended");
        crate::crash::clear_context("camera");
    }
    drop(own);
    was_on
}

#[cfg(test)]
mod tests {
    use super::*;
    use CameraState::{Off, OnAir, Preview, Requesting};

    #[test]
    fn the_session_follows_the_state_diagram() {
        assert_eq!(next_state(Off, Step::Open), Some(Preview));
        assert_eq!(next_state(Preview, Step::TurnOn), Some(Requesting));
        assert_eq!(next_state(Requesting, Step::Refused), Some(Preview));
        assert_eq!(next_state(Requesting, Step::Ready), Some(OnAir));
        for state in [Preview, Requesting, OnAir] {
            assert_eq!(next_state(state, Step::TurnOff), Some(Off));
        }
    }

    #[test]
    fn nothing_skips_the_preview_or_the_request() {
        // The preview can't be skipped (FR-8.10), and only a granted request goes on air.
        assert_eq!(next_state(Off, Step::TurnOn), None);
        assert_eq!(next_state(Off, Step::Ready), None);
        assert_eq!(next_state(Preview, Step::Ready), None);
        assert_eq!(next_state(OnAir, Step::Open), None);
        assert_eq!(next_state(OnAir, Step::TurnOn), None);
        assert_eq!(next_state(OnAir, Step::Refused), None);
        assert_eq!(next_state(Off, Step::TurnOff), None);
    }

    #[test]
    fn bot_errors_name_the_preview_texts() {
        let cases = [
            (BroadcastErrorReason::RoomFull, "room-full"),
            (BroadcastErrorReason::RelayFull, "relay-full"),
            (BroadcastErrorReason::UseNotAllowed, "use-not-allowed"),
            (BroadcastErrorReason::NotSameChannel, "not-same-channel"),
            (BroadcastErrorReason::Denied, "denied"),
            // #601: each refusal keeps its own reason.
            (BroadcastErrorReason::NotRegistered, "not-registered"),
            (BroadcastErrorReason::GroupNotAllowed, "group-not-allowed"),
            (BroadcastErrorReason::NotInChannel, "not-in-channel"),
            (BroadcastErrorReason::NoHello, "no-hello"),
            (BroadcastErrorReason::RelayDown, "relay-down"),
            (BroadcastErrorReason::RateLimited, "rate-limited"),
        ];
        for (reason, code) in cases {
            assert_eq!(CameraFailure::from_reason(reason).code(), code);
            assert_eq!(
                CameraFailure::from_fanout(FanoutUnavailable::Refused(reason)).code(),
                code
            );
        }
        assert_eq!(
            CameraFailure::from_relay(RelayUnavailable::UseNotAllowed(RelayUse::Camera)),
            CameraFailure::UseNotAllowed
        );
        assert_eq!(
            CameraFailure::from_relay(RelayUnavailable::BotNoAnswer),
            CameraFailure::RelayDown
        );
        assert_eq!(
            CameraFailure::from_link(&P2pError::RelayRefused(RelayRefusal::UseNotAllowed)),
            CameraFailure::UseNotAllowed
        );
    }

    #[test]
    fn broadcast_ids_are_sixteen_hex_digits() {
        assert_eq!(broadcast_hex(BroadcastId(0xab)), "00000000000000ab");
        assert_eq!(broadcast_hex(BroadcastId(u64::MAX)), "ffffffffffffffff");
    }

    #[test]
    fn the_camera_uses_the_720p_natural_video_self_test() {
        let prefs = encoder_prefs(SendCeiling::from_setting(Default::default()));
        let (height, fps) = crate::screen_share::encoder::select::ceiling(prefs);
        assert_eq!(
            (height, fps, prefs.optimize),
            crate::screen_share::encoder::CAMERA_SELFTEST
        );
    }
}
