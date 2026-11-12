//! The own camera (spec 08, section 6.4): Off → Preview → Requesting → OnAir.
//!
//! - **Preview**: the camera runs locally, nothing is sent (FR-8.10): no `bcast-req`, no
//!   broadcast, no announcement.
//! - **Requesting**: **Turn on camera** opens the paths of FR-8.94. A new relay (one that carries
//!   `bc`, `cam` and the media profile `mv`) gets one broadcast asked for with `k = cam`
//!   (FR-8.13, FR-8.42) that this client binds as its publisher; an old relay keeps its old-format
//!   broadcast for 0.8.x viewers. With no usable relay — or in `direct_only` mode — no broadcast
//!   is created at all and the camera runs on the direct path alone (FR-8.94, FR-8.96). A refusal
//!   goes back to Preview with the reason (FR-8.51).
//! - **OnAir**: the channel gets `mumcord.camera on`, newcomers get it too (FR-8.8), viewers are
//!   admitted on `cam-join` — through the bot for the broadcast, locally for a direct stream
//!   (FR-8.47) — and removed with a rekey or a closed stream (FR-8.48 – FR-8.49), and the encoder
//!   follows their `want` reports (FR-8.45). A 0.9.0 viewer goes direct when the broadcast is old
//!   or missing, and a viewer that cannot read the new media profile gets `needs` instead of a
//!   stream (FR-8.95).
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
use mumcord_store::{Consent, P2pMode, ScreenFps, ScreenOptimize, ScreenResolution};
use tokio::sync::oneshot;

use super::admit::{CameraPath, CameraViewer, CameraViewers, Join, MAX_VIEWERS};
use super::avail::Paths;
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
    /// The relay broadcast id (16 hex) while on air; `None` for a camera that runs without one
    /// (FR-8.94).
    pub broadcast: Option<String>,
    /// The admitted viewers with the path each of them gets (FR-8.62).
    pub viewers: Vec<CameraViewer>,
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

/// The bot's answer to a `cam-join`: the broadcast, the token for it and its media format.
type CameraToken = (BroadcastId, RelayToken, bool);

/// The broadcast id as `mumcord.camera` carries it.
pub fn broadcast_hex(id: BroadcastId) -> String {
    format!("{:016x}", id.0)
}

/// Which media format the camera sends (FR-7.96, FR-8.95).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    /// No relay broadcast: every 0.9.0 viewer gets a direct stream (FR-8.96).
    Direct,
    /// The old relay broadcast; it serves 0.8.x viewers as today (FR-7.98).
    Old,
    /// A relay broadcast with the Bovnar media profile (FR-7.96).
    New,
}

/// How a `cam-join` is answered (FR-8.94, FR-8.95).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    /// Through the relay broadcast.
    Relay,
    /// Through its own `STREAM_CAMERA`.
    Direct,
    /// It cannot be served at all: it gets `needs` (FR-8.95).
    Needs,
}

/// The matrix of FR-8.94/FR-8.95 for one viewer.
fn route(format: Format, direct_ok: bool, media: bool) -> Route {
    match (format, media) {
        // A 0.8.x viewer has no direct camera path and cannot read the new profile (FR-8.95).
        (Format::New | Format::Direct, false) => Route::Needs,
        (Format::New, true) => Route::Relay,
        (Format::Direct, true) => Route::Direct,
        // The old broadcast serves 0.8.x viewers as today; a 0.9.0 viewer gets its own stream
        // unless the mode forbids direct, and then the old broadcast stays the fallback it has
        // always been for 0.9.0 clients (FR-8.94).
        (Format::Old, false) => Route::Relay,
        (Format::Old, true) if direct_ok => Route::Direct,
        (Format::Old, true) => Route::Relay,
    }
}

/// What a `cam-join` gets from the matrix, before the viewer list's own seat count (FR-8.94,
/// FR-8.95).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    /// Served over the relay broadcast.
    Relay,
    /// Opened a `STREAM_CAMERA` of its own.
    Direct,
    /// The direct path has no seat left (FR-8.93): it gets `full`.
    Full,
    /// It cannot read the camera's format and gets nothing but `needs` (FR-8.95).
    Needs,
}

/// The matrix of FR-8.94/FR-8.95 for one `cam-join`: `direct_room` is a free seat on the direct
/// path (8 viewers, FR-8.93).
fn answer(format: Format, direct_ok: bool, media: bool, direct_room: bool) -> Answer {
    match route(format, direct_ok, media) {
        Route::Needs => Answer::Needs,
        Route::Relay => Answer::Relay,
        Route::Direct if direct_room => Answer::Direct,
        Route::Direct => Answer::Full,
    }
}

/// The format a camera carries: without a broadcast every stream uses the new profile, with one
/// the relay's capability picks it and it stays fixed for that broadcast (FR-7.97, FR-8.94).
fn format_of(broadcast: bool, media: bool) -> Format {
    match (broadcast, media) {
        (false, _) => Format::Direct,
        (true, true) => Format::New,
        (true, false) => Format::Old,
    }
}

struct Broadcast {
    id: BroadcastId,
    publisher: Publisher,
}
struct Live {
    /// The camera session id: the `on` id, which is also the `STREAM_CAMERA` description id
    /// (FR-8.92). It is the relay broadcast id when there is one.
    session: String,
    /// The format every viewer gets at least one of (FR-8.95).
    format: Format,
    /// The relay broadcast; `None` when the camera runs without a relay (FR-8.94).
    broadcast: Option<Broadcast>,
    viewers: CameraViewers,
    /// One `STREAM_CAMERA` per admitted viewer that reads the media profile (FR-8.92).
    direct: super::direct::DirectViewers,
    /// The mode leaves the direct path open; `relay_only` does not (FR-8.94).
    direct_ok: bool,
}

impl Live {
    /// Whether the camera sends the Bovnar media profile, which the `on` message names in `mv`
    /// (FR-8.95): true for a new-format broadcast and for a camera that runs without one.
    fn sends_media(&self) -> bool {
        matches!(self.format, Format::New | Format::Direct)
    }

    /// Decides about a `cam-join` that passed the channel, blocking and rate checks (FR-8.47);
    /// `media` says whether the viewer reads the Bovnar media profile (FR-8.95).
    fn admit(
        &mut self,
        viewer: u32,
        hash: &str,
        codecs: &[Codec],
        media: bool,
    ) -> Result<Admit, CameraMessage> {
        let id = self.session.clone();
        let path = match answer(
            self.format,
            self.direct_ok,
            media,
            self.direct.has_room(viewer),
        ) {
            Answer::Needs => return Err(CameraMessage::Needs { id }),
            // The direct path serves 8 viewers per camera (FR-8.93, FR-8.52).
            Answer::Full => return Err(CameraMessage::Full { id }),
            Answer::Relay => CameraPath::Relay,
            Answer::Direct => CameraPath::Direct,
        };
        match self.viewers.join(viewer, hash, codecs, path) {
            Join::Full => Err(CameraMessage::Full { id }),
            Join::Admit => match (path, &mut self.broadcast) {
                (CameraPath::Relay, Some(broadcast)) => {
                    broadcast.publisher.admit(hash);
                    Ok(Admit::Relay(broadcast.id))
                }
                // A direct viewer is admitted locally; no bot is involved (FR-8.47).
                _ => Ok(Admit::Direct),
            },
        }
    }
}

/// How an admitted viewer is served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Admit {
    /// Ask the bot to admit it to this relay broadcast, then it does the key exchange.
    Relay(BroadcastId),
    /// Open a `STREAM_CAMERA` to it.
    Direct,
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
            broadcast: self
                .live
                .as_ref()
                .and_then(|l| l.broadcast.as_ref())
                .map(|b| broadcast_hex(b.id)),
            viewers: self
                .live
                .as_ref()
                .map(|l| l.viewers.entries())
                .unwrap_or_default(),
        }
    }

    fn live_on(&mut self, server_id: ServerId) -> Option<&mut Live> {
        if self.server_id != server_id || self.state != CameraState::OnAir {
            return None;
        }
        self.live.as_mut()
    }

    /// The camera is on air here and reaches its viewers through a relay broadcast, so losing the
    /// relay ends it (FR-8.7).
    pub(super) fn on_relay(&self, server_id: ServerId) -> bool {
        self.server_id == server_id
            && self.state == CameraState::OnAir
            && self.live.as_ref().is_some_and(|l| l.broadcast.is_some())
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
    waiting: HashMap<(ServerId, u32), oneshot::Sender<CameraToken>>,
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

    /// **Turn on camera** (Preview → Requesting → OnAir, FR-8.13): returns once the camera is
    /// on air over the paths of FR-8.94, or with the reason it is not (the camera stays in the
    /// preview).
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
        let opened = self.open_camera(server_id).await;
        self.camera_opened(server_id, attempt, opened)
    }

    /// The paths of FR-8.94 on `server_id`, or why no path is left.
    fn camera_paths(&self, server_id: ServerId) -> Result<Paths, CameraFailure> {
        let relay = self.relays().info(server_id);
        let (mode, consent) = self.p2p_mode_and_consent_of(server_id);
        let direct_allowed = consent != Consent::No;
        // No path is left in `off` mode, in `relay_only` without a relay that carries cameras and
        // with direct connections turned off while only the direct path remains (FR-8.4);
        // availability already refused the entries with that reason.
        Paths::of(&relay, mode, direct_allowed).ok_or_else(|| {
            // The mode `off` and turned-off direct connections are one place in the settings, so
            // both answer with the mode's text; the availability reason tells them apart in the
            // UI, which disables the entries before this step is reached (FR-8.4).
            if mode == P2pMode::Off || !direct_allowed {
                CameraFailure::Mode
            } else {
                CameraFailure::from_relay(
                    self.relays()
                        .availability_for(server_id, RelayUse::Camera)
                        .err()
                        .unwrap_or(RelayUnavailable::UseNotAllowed(RelayUse::Camera)),
                )
            }
        })
    }

    /// Opens what the camera needs before it goes on air: a relay broadcast when the paths allow
    /// one, nothing at all on the direct path alone (FR-8.42, FR-8.94).
    async fn open_camera(
        &self,
        server_id: ServerId,
    ) -> Result<(Paths, Option<Opened>), CameraFailure> {
        let paths = self.camera_paths(server_id)?;
        if !paths.relay {
            return Ok((paths, None));
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
            .relay_broadcast_request(server_id, RelayUse::Camera, limit, paths.media)
            .await
            .map_err(CameraFailure::from_fanout)?;
        let link = p2p
            .broadcast(&ann, &token, BindRole::Publisher, paths.media)
            .await
            .map_err(|e| {
                tracing::info!(error = %e, "camera broadcast link not opened");
                CameraFailure::from_link(&e)
            })?;
        Ok((paths, Some((id, me, link, limit))))
    }

    fn camera_opened(
        &self,
        server_id: ServerId,
        attempt: u64,
        opened: Result<(Paths, Option<Opened>), CameraFailure>,
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
            let started = opened.and_then(|(paths, relay_opened)| {
                let frames = own.send.take_frames().ok_or(CameraFailure::Busy)?;
                // Without a broadcast the session id is its own: the `on` id and the stream
                // description id are the same 16 hex characters either way (FR-8.92, §6.5).
                let session = match relay_opened {
                    Some((id, ..)) => broadcast_hex(id),
                    None => crate::files::prepare::random_hex(8),
                };
                let on_end_id = relay_opened.as_ref().map(|(id, ..)| *id);
                let direct = super::direct::DirectViewers::new(session.clone(), frames.clone());
                frames.pump(&rt);
                let limit = relay_opened
                    .as_ref()
                    .map_or(MAX_VIEWERS, |(_, _, _, limit)| *limit);
                let broadcast = match relay_opened {
                    Some((id, me, link, _)) => {
                        let on_end: OnEnd = Box::new(move |end| {
                            tracing::info!(?end, "camera broadcast ended");
                            if let (Some(engine), Some(id)) = (weak.upgrade(), on_end_id) {
                                engine.camera_broadcast_ended(server_id, id, end);
                            }
                        });
                        let publisher =
                            Publisher::start(&rt, id, me, link, frames.relay(), None, on_end)
                                .map_err(|e| {
                                    tracing::warn!(error = %e, "camera broadcast not started");
                                    CameraFailure::RelayDown
                                })?;
                        Some(Broadcast { id, publisher })
                    }
                    None => None,
                };
                // The broadcast's format is fixed for it by the publisher and the relay's
                // capability (FR-7.97); without a broadcast every viewer gets the new one.
                let format = format_of(broadcast.is_some(), paths.media);
                Ok(Live {
                    session,
                    format,
                    broadcast,
                    viewers: CameraViewers::new(limit),
                    direct,
                    direct_ok: paths.direct,
                })
            });
            match started {
                Ok(live) => {
                    let session = live.session.clone();
                    let media = live.sends_media();
                    own.live = Some(live);
                    own.state = next_state(own.state, Step::Ready).unwrap_or(CameraState::OnAir);
                    own.send.set_on_air(true);
                    tracing::info!("camera on air");
                    crate::crash::set_context("camera", "on air");
                    (Ok(()), own.view(), Some((session, media)))
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
        if let (Some((session, media)), Ok(ctx)) = (announce, self.session_ctx_for(server_id)) {
            let peers = signal::channel_peers(&ctx, self.mumcord_state());
            signal::send(&ctx, &peers, &CameraMessage::On { id: session, media });
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
            .filter(|o| o.server_id == server_id)
            .and_then(|o| o.live.as_ref())
            .and_then(|l| l.broadcast.as_ref())
            .is_some_and(|broadcast| broadcast.id == id);
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
    /// the join rate were checked on arrival; `media` says whether it reads the Bovnar media
    /// profile (FR-8.95).
    fn camera_join(&self, ctx: &SessionCtx, viewer: u32, id: &str, codecs: &[Codec], media: bool) {
        let Some(hash) = cert_hash(ctx, viewer) else {
            return;
        };
        let answer = {
            let mut cameras = lock(self.camera_state());
            let Some(own) = cameras.own.as_mut() else {
                return;
            };
            let send = &own.send;
            let Some(live) = own
                .live
                .as_mut()
                .filter(|l| l.session == id && own.server_id == ctx.server_id)
            else {
                return;
            };
            let answer = live.admit(viewer, &hash, codecs, media);
            if answer.is_ok() {
                send.set_vp9_only(live.viewers.needs_vp9());
            }
            answer
        };
        match answer {
            Err(refused) => {
                // Told, so the viewer shows "Too many people are watching" or the "needs
                // 0.9.0" text instead of waiting for a token that never comes (FR-8.52,
                // FR-8.95).
                signal::send(ctx, &[viewer], &refused);
            }
            Ok(Admit::Direct) => self.camera_direct_open(ctx.server_id, viewer, id),
            Ok(Admit::Relay(broadcast)) => {
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
                    engine.camera_remove_viewer(
                        server_id,
                        &broadcast_hex(broadcast),
                        viewer,
                        false,
                    );
                });
            }
        }
    }

    /// Opens the direct stream to an admitted viewer that reads the media profile (FR-8.92). The
    /// connection is made in the background; a viewer whose stream does not come up asks again
    /// with its next `cam-join` (FR-8.52).
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
        let Some(live) = own.live.as_mut().filter(|l| on_air && l.session == id) else {
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

    /// Ends `viewer`'s admission to the own camera `session`: it gets no keys any more, the key
    /// rotates for a relay viewer (FR-7.44), its direct stream closes, and with `kick` the bot is
    /// told (`bcast-kick`, FR-8.49).
    fn camera_remove_viewer(&self, server_id: ServerId, session: &str, viewer: u32, kick: bool) {
        let removed = {
            let mut cameras = lock(self.camera_state());
            let Some(own) = cameras.own.as_mut() else {
                return;
            };
            let on_air = own.server_id == server_id && own.state == CameraState::OnAir;
            let send = &own.send;
            match own.live.as_mut().filter(|l| on_air && l.session == session) {
                Some(live) => {
                    // Its direct stream, if any, closes with the admission (FR-8.92).
                    live.direct.remove(viewer);
                    let was_full = live.viewers.is_full();
                    match live.viewers.leave(viewer) {
                        Some((hash, path)) => {
                            // Only a relay viewer holds the broadcast keys (FR-8.48).
                            if path == CameraPath::Relay
                                && let Some(broadcast) = &mut live.broadcast
                            {
                                broadcast.publisher.remove(&hash);
                            }
                            send.set_vp9_only(live.viewers.needs_vp9());
                            Some((
                                was_full,
                                live.broadcast.as_ref().map(|b| b.id),
                                live.sends_media(),
                            ))
                        }
                        None => None,
                    }
                }
                None => None,
            }
        };
        let Some((was_full, broadcast, media)) = removed else {
            return;
        };
        // A freed slot is announced again, so viewers turned away as full ask anew (FR-8.52).
        if was_full && let Ok(ctx) = self.session_ctx_for(server_id) {
            let peers = signal::channel_peers(&ctx, self.mumcord_state());
            signal::send(
                &ctx,
                &peers,
                &CameraMessage::On {
                    id: session.to_owned(),
                    media,
                },
            );
        }
        if kick && let Some(broadcast) = broadcast {
            self.relay_broadcast_kick(server_id, RelayUse::Camera, broadcast, viewer);
        }
        self.emit_camera(server_id, self.camera_view());
    }

    /// The own camera's session id on `server_id` with `viewer` admitted, if any.
    fn camera_watched_by(&self, server_id: ServerId, viewer: u32) -> Option<String> {
        let mut cameras = lock(self.camera_state());
        let live = cameras.own.as_mut()?.live_on(server_id)?;
        live.viewers.contains(viewer).then(|| live.session.clone())
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
        media: bool,
    ) {
        let waiter = lock(self.camera_state())
            .waiting
            .remove(&(ctx.server_id, sharer));
        match waiter {
            Some(waiter) => {
                let _ = waiter.send((id, token, media));
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
    ) -> oneshot::Receiver<CameraToken> {
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
        let (bid, token, media) = tokio::time::timeout(std::time::Duration::from_secs(30), token)
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
            .broadcast(&ann, &token, BindRole::Subscriber, media)
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
            if engine
                .camera_watched_by(ctx.server_id, event.sender)
                .as_deref()
                == Some(id.as_str())
            {
                engine.camera_remove_viewer(ctx.server_id, &id, event.sender, true);
            }
        }
        // A 0.9.0 client reads both formats, so `mv` changes nothing it does; a 0.8.x viewer of a
        // new-format camera learns from it that it needs 0.9.0 (FR-8.95).
        CameraMessage::On { id, .. } => super::recv::on_camera_on(&engine, ctx, event.sender, id),
        CameraMessage::Full { id } => super::recv::on_camera_full(&engine, ctx, event.sender, &id),
        CameraMessage::Needs { id } => {
            super::recv::on_camera_needs(&engine, ctx, event.sender, &id)
        }
        CameraMessage::Off => super::recv::on_camera_gone(&engine, ctx, event.sender),
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
    let (camera, media) = {
        let mut cameras = lock(state);
        let Some(live) = cameras.own.as_mut().and_then(|o| o.live_on(ctx.server_id)) else {
            return;
        };
        (live.session.clone(), live.sends_media())
    };
    signal::announce_to_newcomer(
        ctx,
        peers,
        session,
        &CameraMessage::On { id: camera, media },
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
        engine.camera_remove_viewer(ctx.server_id, &id, session, true);
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
        engine.camera_remove_viewer(ctx.server_id, &id, session, true);
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
    use Answer::{Direct as DirectAnswer, Full, Needs as NeedsAnswer, Relay as RelayAnswer};
    use CameraState::{Off, OnAir, Preview, Requesting};

    #[test]
    fn every_cell_of_the_fallback_table_picks_the_path_per_viewer() {
        // FR-8.95 with a 0.9.0 sharer, cell by cell. `media` is the viewer's `mv: 2` (its hello
        // has `media-v2`), `direct_ok` the mode (`false` in `relay_only` or with direct
        // connections off) and `direct_room` a free seat on the direct path (FR-8.93).
        let cells = [
            // 0.9.0 viewer, new relay: the new-format relay broadcast. A full direct path does not
            // change that; the broadcast is the first path (FR-8.94).
            (Format::New, true, true, true, RelayAnswer, "new relay"),
            (
                Format::New,
                true,
                true,
                false,
                RelayAnswer,
                "new relay, direct full",
            ),
            (
                Format::New,
                false,
                true,
                true,
                RelayAnswer,
                "new relay, relay_only",
            ),
            // 0.9.0 viewer, old relay or none: a `STREAM_CAMERA` of its own.
            (Format::Old, true, true, true, DirectAnswer, "old relay"),
            (Format::Direct, true, true, true, DirectAnswer, "no relay"),
            // `relay_only` never opens a stream, so the 0.9.0 viewer stays on the broadcast in
            // whatever format it has, the old one included (FR-8.94).
            (
                Format::Old,
                false,
                true,
                true,
                RelayAnswer,
                "old relay, relay_only",
            ),
            // The direct path's 8 viewers are taken (FR-8.93).
            (
                Format::Old,
                true,
                true,
                false,
                Full,
                "old relay, direct full",
            ),
            (
                Format::Direct,
                true,
                true,
                false,
                Full,
                "no relay, direct full",
            ),
            // 0.8.x viewer, old relay: the old-format broadcast serves it as today.
            (
                Format::Old,
                true,
                false,
                true,
                RelayAnswer,
                "old relay, 0.8.x viewer",
            ),
            (
                Format::Old,
                false,
                false,
                true,
                RelayAnswer,
                "old relay, relay_only, 0.8.x viewer",
            ),
            // 0.8.x viewer of a new-format camera: `needs`, and no stream and no second broadcast
            // are opened for it (FR-8.95).
            (
                Format::New,
                true,
                false,
                true,
                NeedsAnswer,
                "new relay, 0.8.x viewer",
            ),
            (
                Format::New,
                false,
                false,
                true,
                NeedsAnswer,
                "new relay, relay_only, 0.8.x viewer",
            ),
            (
                Format::Direct,
                true,
                false,
                true,
                NeedsAnswer,
                "no relay, 0.8.x viewer",
            ),
            // A format refusal outranks `full`: the seat count is not why it is refused.
            (
                Format::New,
                true,
                false,
                false,
                NeedsAnswer,
                "new relay, 0.8.x viewer, direct full",
            ),
            (
                Format::Direct,
                true,
                false,
                false,
                NeedsAnswer,
                "no relay, 0.8.x viewer, direct full",
            ),
        ];
        for (format, direct_ok, media, direct_room, want, why) in cells {
            assert_eq!(
                answer(format, direct_ok, media, direct_room),
                want,
                "{why} ({format:?}, direct_ok: {direct_ok}, media: {media})"
            );
        }
    }

    #[test]
    fn the_matrix_never_mixes_up_a_refusal_and_a_path() {
        // Every combination of the FR-8.95 inputs: only an old-format broadcast serves a 0.8.x
        // viewer, a 0.8.x viewer of a new-format camera is never served on any path, and a 0.9.0
        // viewer (its `cam-join` has `mv: 2`) never gets `needs`.
        for format in [Format::New, Format::Old, Format::Direct] {
            for direct_ok in [true, false] {
                for media in [true, false] {
                    for direct_room in [true, false] {
                        let got = answer(format, direct_ok, media, direct_room);
                        let why = format!(
                            "{format:?}, direct_ok: {direct_ok}, media: {media}, room: {direct_room}"
                        );
                        match (format, media) {
                            (Format::Old, false) => assert_eq!(got, RelayAnswer, "{why}"),
                            (Format::New | Format::Direct, false) => {
                                assert_eq!(got, NeedsAnswer, "{why}")
                            }
                            (_, true) => {
                                assert!(matches!(got, RelayAnswer | DirectAnswer | Full), "{why}")
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_broadcast_format_is_fixed_per_broadcast() {
        // FR-7.97, FR-8.94: the relay's capability picks the format once, when the camera goes on
        // air. Without a broadcast every stream carries the new profile (FR-8.92), so a
        // `direct_only` camera never claims a relay format it does not have.
        assert_eq!(format_of(true, true), Format::New);
        assert_eq!(format_of(true, false), Format::Old);
        assert_eq!(format_of(false, false), Format::Direct);
        assert_eq!(format_of(false, true), Format::Direct);
        // A new relay is the new format, an old one the old: the camera never runs a new-format
        // broadcast that a 0.8.x viewer could be served from (FR-8.95).
        assert_ne!(format_of(true, true), format_of(true, false));
    }

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
