//! The engine object and the connection lifecycle (FR-1.10, 1.13, 1.14, 1.19, 1.22,
//! 1.24–1.26, FR-2.1–2.3, 2.6–2.9).
//!
//! Several servers are connected at a time, each in its own slot. A supervisor task per
//! connect runs the slot's state machine of spec §7: connect → sync → event pump → (on loss)
//! reconnect with backoff. Each start of a slot takes a new generation number; tasks of an
//! older generation can no longer change state, so cancelling is race-free without waiting
//! for tasks to end. Voice runs on at most one server, the voice server; the others are
//! read-only.
//!
//! Lock order: `core` before `store`. Neither lock is held across an `.await`.

pub(crate) mod batch;
mod channel_password;
pub mod convert;
mod description;
pub mod extension;
mod local;
mod machine;
mod types;

use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use mumcord_audio::bitrate::{
    BitrateController, LinkStats as BitrateStats, Transport as BitrateTransport,
};
use mumcord_audio::dsp::{DspControls, MicTest, MicTestPhase as DspPhase};
use mumcord_audio::mixer::PacketSender;
use mumcord_protocol::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use mumcord_protocol::{
    ClientIdentity, ConnectConfig, ConnectError, Connection, DisconnectReason, Event, Handle,
    ServerState, Transport, VoiceEvent, VoiceTransport,
};
use mumcord_store::{Identity, OwnVoice, SavedServer, SecretStore, ServerId, Store};
use tokio::runtime::Handle as Runtime;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub use channel_password::{ChannelPasswordChannel, ChannelPasswordState, ChannelPasswordView};
pub use machine::MAX_RECONNECT_ATTEMPTS;
pub use types::*;

use crate::audio_rating::AudioRatingView;
use crate::friends::FriendHub;
use crate::people::{self, People};
use crate::sounds::SoundHub;
use crate::voice::{self, Audio, Meter, VoicePipeline, VoiceShared};
use convert::{channel_path, channel_view, join_path, resolve_path, split_path, user_view};
use machine::{CertCheck, Failure, Machine, SelfVoice, Step, check_cert, format_fingerprint};

/// Upper bound for TCP connect + TLS + login of one attempt.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(15);
/// How often link statistics feed the bitrate controller (FR-1.24).
const BITRATE_INTERVAL: Duration = Duration::from_secs(1);
/// How long `person_profile` waits for requested avatar/comment blobs.
const PROFILE_WAIT: Duration = Duration::from_secs(3);
/// Longest wait on quit for the connections to send what is still queued (they drain in parallel,
/// each bounded by the protocol's own drain timeout), so quitting never hangs.
const SHUTDOWN_DRAIN: Duration = Duration::from_secs(2);

/// An engine event with the server it belongs to (`None` for app-wide events).
pub type TaggedEvent = (Option<ServerId>, EngineEvent);

/// The engine's event stream, returned by [`Engine::new`].
pub struct EventReceiver(mpsc::UnboundedReceiver<TaggedEvent>);

impl EventReceiver {
    /// The next event without its server.
    pub async fn recv(&mut self) -> Option<EngineEvent> {
        self.0.recv().await.map(|(_, event)| event)
    }

    /// The next event with the server it belongs to.
    pub async fn recv_tagged(&mut self) -> Option<TaggedEvent> {
        self.0.recv().await
    }

    pub fn try_recv(&mut self) -> Result<EngineEvent, mpsc::error::TryRecvError> {
        self.0.try_recv().map(|(_, event)| event)
    }
}

/// Which servers the hubs follow. Saved server ids start at 1, so 0 means none.
#[derive(Default)]
struct Routing {
    /// The server the voice pipeline is bound to (see `Core::audio_server`).
    audio: AtomicI64,
}

fn load_server(slot: &AtomicI64) -> Option<ServerId> {
    Some(slot.load(Ordering::Relaxed)).filter(|id| *id != 0)
}

/// Event output of the engine; cheap to clone. Extensions (B14) emit through it too. The
/// output of a session ([`SessionCtx::events`]) tags its events with that session's server.
#[derive(Clone)]
pub struct Events {
    tx: mpsc::UnboundedSender<TaggedEvent>,
    sounds: Arc<SoundHub>,
    friends: Arc<FriendHub>,
    routing: Arc<Routing>,
    server: Option<ServerId>,
}

impl Events {
    /// Notification sounds (B26); also reachable for `Sound::Message` from chat code.
    pub fn sounds(&self) -> &Arc<SoundHub> {
        &self.sounds
    }

    /// Friends and their online state (FR-1.64).
    pub fn friends(&self) -> &Arc<FriendHub> {
        &self.friends
    }

    /// The same output, tagging its events with `server_id`.
    fn for_server(&self, server_id: ServerId) -> Self {
        Self {
            server: Some(server_id),
            ..self.clone()
        }
    }

    pub fn emit(&self, event: EngineEvent) {
        let server = self.server;
        // The sound hub follows the server with the voice pipeline: sounds are about voice.
        if server.is_none() || server == load_server(&self.routing.audio) {
            self.sounds.observe(&event);
        }
        let friends = self.friends.observe(server, &event);
        // A closed receiver means the app is shutting down; nobody is left to inform.
        let _ = self.tx.send((server, event));
        if let Some(friends) = friends {
            let _ = self.tx.send((None, EngineEvent::FriendsChanged(friends)));
        }
    }
}

/// What a connected session offers to extensions (chat, follow, history, profile – B14).
#[derive(Clone)]
pub struct SessionCtx {
    pub server_id: ServerId,
    pub handle: Handle,
    pub store: Arc<Mutex<Store>>,
    pub events: Events,
}

impl SessionCtx {
    pub fn store(&self) -> MutexGuard<'_, Store> {
        lock(&self.store)
    }

    /// Ids of the stored followed channels that exist on the server (empty on servers that
    /// cannot listen).
    pub fn followed_channel_ids(&self) -> Vec<u32> {
        let store = self.store();
        self.handle
            .with_state(|s| followed_ids(&store, self.server_id, s))
    }

    /// Channel path below the root, e.g. `["Raid", "Team 1"]` (store key of follows/history).
    pub fn channel_path(&self, channel_id: u32) -> Option<Vec<String>> {
        self.handle.with_state(|s| channel_path(s, channel_id))
    }

    /// Channel id of a path below the root on this server.
    pub fn resolve_channel_path(&self, path: &[String]) -> Option<u32> {
        self.handle.with_state(|s| resolve_path(s, path))
    }
}

/// Followed channels of `server_id` that exist in `state`.
fn followed_ids(store: &Store, server_id: ServerId, state: &ServerState) -> Vec<u32> {
    if !state.server.supports_listening() {
        return Vec::new();
    }
    store
        .followed_channels(server_id)
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "reading followed channels failed");
            Vec::new()
        })
        .iter()
        .filter_map(|f| resolve_path(state, &split_path(&f.channel_path)))
        .collect()
}

/// The engine: owns store, identity, the connection and the audio pipeline. Cheap to clone.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

struct Inner {
    rt: Runtime,
    data_dir: PathBuf,
    store: Arc<Mutex<Store>>,
    secrets: Arc<SecretStore>,
    /// Held while the audio devices start, so concurrent first uses open the microphone once
    /// (every open can show the OS microphone prompt).
    audio_start: tokio::sync::Mutex<()>,
    /// `None` until loaded from secure storage; then `Some(None)` before the wizard ran.
    identity: Mutex<Option<Option<Identity>>>,
    events: Events,
    controls: Arc<DspControls>,
    meter: Arc<Meter>,
    voice: Arc<VoiceShared>,
    reconnects: AtomicU32,
    /// Voice channels the user was joined to when the app last quit, per server. Each is
    /// offered for rejoining once, on the first fresh connect to its server (QA.9).
    rejoin_offers: Mutex<HashMap<ServerId, String>>,
    /// One-off passwords typed in the wrong-password dialog (FR-2.59), used instead of the
    /// saved one until the server's connection ends. Never logged.
    password_overrides: Mutex<HashMap<ServerId, String>>,
    /// Joins retried with a channel password, until the server answers (FR-2.78).
    password_attempts: Mutex<channel_password::Attempts>,
    core: Mutex<Core>,
    /// Link previews and video cards for chat links (FR-1.49, FR-1.50); also the image cache
    /// of full-size chat images.
    previews: Arc<crate::web::LinkPreviews>,
    /// Full-size image transfers with other Mumcord clients (FR-1.60c).
    full_images: crate::full_image::ServerTransfers,
    /// Direct connections to other Mumcord users (FR-2.35).
    direct: crate::p2p::Direct,
    /// Shared "now playing" status (FR-1.58).
    now_playing: Mutex<crate::now_playing::NowPlaying>,
    /// Shared "now gaming" status (FR-1.59).
    now_gaming: Mutex<crate::now_gaming::NowGaming>,
    user_stats: Mutex<crate::stats::UserStatsHub>,
    /// Other Mumcord users on each server (FR-1.61).
    mumcord: crate::mumcord::ServerPeers,
    /// History sync with other Mumcord users (FR-1.60).
    history_sync: Arc<Mutex<crate::history_sync::HistorySync>>,
    /// Positional audio (FR-1.41).
    positional: crate::positional::Positional,
    /// Watch parties in the user's channels (FR-2.39).
    watch_parties: crate::watch_party::ServerParties,
    /// Screen shares in the user's channels (FR-2.42).
    screen_shares: crate::screen_share::ServerShares,
    /// Packet inspector capture, shared by every connection (issue #60).
    inspector: Arc<crate::inspector::Inspector>,
}

#[derive(Default)]
struct Core {
    /// Every server that is connecting, connected, reconnecting or waiting for a trust
    /// decision. A server leaves its slot when it is disconnected.
    slots: HashMap<ServerId, Slot>,
    /// Last generation handed out; never reused, so tasks of a removed slot stay stale.
    generations: u64,
    /// The server that carries voice (FR-2.2); `None` while every server is read-only.
    voice_server: Option<ServerId>,
    /// The server the voice pipeline is bound to: the voice server, else the earliest
    /// connected server (read-only there, as with a single server).
    audio_server: Option<ServerId>,
    audio: Option<Audio>,
    pipeline: Option<VoicePipeline>,
    mic_test: Option<MicTest>,
}

/// One server's connection.
struct Slot {
    machine: Machine,
    generation: u64,
    supervisor: Option<JoinHandle<()>>,
    session: Option<Session>,
    pending_trust: Option<PendingTrust>,
    self_voice: SelfVoice,
    people: People,
}

impl Core {
    /// Glitch counters of the running voice pipeline; `None` while it is not running.
    fn audio_health(&self, voice: &VoiceShared) -> Option<AudioHealthView> {
        let pipeline = self.pipeline.as_ref()?;
        let io = self.audio.as_ref()?.io().stats();
        let base = pipeline.io_base();
        let playback = pipeline.playback_stats();
        let mut view = AudioHealthView {
            device_xruns: io.device_xruns.saturating_sub(base.device_xruns),
            stream_restarts: io.stream_restarts.saturating_sub(base.stream_restarts),
            capture_overruns: io.capture_overruns.saturating_sub(base.capture_overruns),
            playback_underruns: io
                .playback_underruns
                .saturating_sub(base.playback_underruns),
            capture_late_frames: voice.capture_late_frames.load(Ordering::Relaxed),
            playback_late_wakeups: playback.late_wakeups,
            concealed_frames: playback.concealed_frames,
            late_packets: playback.late_packets,
            jitter_delay_ms: playback.jitter_delay_ms,
            capture_time_avg_us: voice.capture_time.avg_us(),
            capture_time_max_us: voice.capture_time.max_us(),
            mix_time_avg_us: playback.mix_time_avg_us,
            mix_time_max_us: playback.mix_time_max_us,
            realtime_threads: voice.realtime_threads.load(Ordering::Relaxed),
            received_frames: playback.received_frames,
            rating: AudioRatingView::default(),
        };
        view.rating = pipeline.health().rate(Instant::now(), view.totals());
        Some(view)
    }

    /// Adds the current counters to the rating's history; called about once a second.
    fn sample_audio_health(&mut self, voice: &VoiceShared) {
        let Some(totals) = self.audio_health(voice).map(|a| a.totals()) else {
            return;
        };
        let Some(pipeline) = self.pipeline.as_mut() else {
            return;
        };
        let processing_max_us = voice
            .capture_time
            .take_recent_max_us()
            .max(pipeline.take_recent_mix_max_us());
        pipeline
            .health_mut()
            .sample(Instant::now(), totals, processing_max_us);
    }

    fn session(&self, server_id: ServerId) -> EngineResult<&Session> {
        self.slots
            .get(&server_id)
            .and_then(|slot| slot.session.as_ref())
            .ok_or_else(EngineError::not_connected)
    }

    /// The connected server that was started first (lowest generation).
    fn earliest_connected(&self) -> Option<ServerId> {
        self.slots
            .iter()
            .filter(|(_, slot)| slot.session.is_some())
            .min_by_key(|(_, slot)| slot.generation)
            .map(|(id, _)| *id)
    }
}

/// See [`Engine::downgrade`].
pub(crate) struct WeakEngine(std::sync::Weak<Inner>);

impl WeakEngine {
    pub fn upgrade(&self) -> Option<Engine> {
        self.0.upgrade().map(|inner| Engine { inner })
    }
}

/// A live connection.
struct Session {
    server_id: ServerId,
    handle: Handle,
    transport: Option<Arc<VoiceTransport>>,
    pump: mpsc::UnboundedSender<PumpCmd>,
    /// Channel offered for rejoining until the user turns voice on (QA.9).
    rejoin_offer: Option<u32>,
    /// Where the server put the user on connect, before any rejoin (FR-1.48b).
    default_channel_id: Option<u32>,
    description_requests: description::DescriptionRequests,
}

/// Commands to the session task.
enum PumpCmd {
    /// New packet input of the mixer (`None` while the mic test borrows the devices).
    Packets(Option<PacketSender>),
}

struct PendingTrust {
    fingerprint: String,
    /// The server presented a different certificate than the one trusted before.
    changed: bool,
    channel_path: Option<Vec<String>>,
    rejoin: Option<String>,
}

/// The error kind for a declined trust question.
fn declined_kind(changed: bool) -> ErrorKind {
    if changed {
        ErrorKind::CertificateChanged
    } else {
        ErrorKind::CertificateRejected
    }
}

/// How a connection attempt ended.
enum AttemptError {
    Trust {
        fingerprint: String,
        previous: Option<String>,
    },
    /// `reason` is the log-safe cause from [`convert::failure_reason`] or the error kind.
    Failure { failure: Failure, reason: String },
}

/// How a synced connection came about.
enum Arrival {
    /// The user connected (server click, app start, link with an optional channel path).
    Fresh(Option<Vec<String>>),
    /// First fresh connect to the server since app start, which quit while joined to this
    /// channel path for voice: offer to rejoin it (QA.9).
    FreshWithRejoin(String),
    /// Automatic reconnect after the connection was lost.
    Reconnect,
}

/// How an established connection ended.
enum Ended {
    Lost,
    Final(Option<EngineError>),
    /// Superseded by a newer connect/disconnect.
    Stale,
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Maps store errors to engine errors.
pub(crate) fn store_error(err: mumcord_store::Error) -> EngineError {
    use mumcord_store::Error as E;
    let kind = match &err {
        E::SecureStorage(_) | E::CorruptStoredIdentity | E::SecretUnavailable => {
            ErrorKind::SecureStorageUnavailable
        }
        E::Pkcs12(_) | E::Pkcs12NoIdentity | E::EmptyIdentityName => ErrorKind::InvalidInput,
        E::InvalidVolume(_) | E::InvalidPort => ErrorKind::InvalidInput,
        E::UnknownServer(_) => ErrorKind::NotFound,
        _ => ErrorKind::Internal,
    };
    EngineError::new(kind, err.to_string())
}

impl Engine {
    /// Opens the store in `data_dir` and applies the saved settings. Background tasks run on
    /// `rt`. Returns the engine and its event stream.
    pub fn new(
        data_dir: &Path,
        secrets: SecretStore,
        rt: Runtime,
    ) -> EngineResult<(Self, EventReceiver)> {
        std::fs::create_dir_all(data_dir).map_err(|e| {
            EngineError::new(
                ErrorKind::Io,
                format!("creating the data folder failed: {e}"),
            )
        })?;
        let store = Store::open(data_dir).map_err(store_error)?;
        crate::profile::backfill_server_avatars(&store, data_dir);
        let settings = store.settings().map_err(store_error)?;
        let rejoin_offers = store
            .voice_channels()
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "reading the voice channels to rejoin failed");
                Vec::new()
            })
            .into_iter()
            .collect();
        let store = Arc::new(Mutex::new(store));
        let (tx, rx) = mpsc::unbounded_channel();
        let events = Events {
            tx,
            sounds: SoundHub::from_store(store.clone()),
            friends: FriendHub::new(store.clone(), rt.clone()),
            routing: Arc::default(),
            server: None,
        };
        let controls = Arc::new(DspControls::default());
        local::apply_dsp(&controls, &settings);
        let meter = Arc::new(Meter::default());
        voice::spawn_meter(&rt, controls.clone(), meter.clone(), events.clone());
        let voice = Arc::new(VoiceShared::default());
        let inner = Arc::new(Inner {
            rt,
            data_dir: data_dir.to_path_buf(),
            store,
            secrets: Arc::new(secrets),
            audio_start: tokio::sync::Mutex::new(()),
            identity: Mutex::new(None),
            events,
            controls,
            meter,
            voice: voice.clone(),
            reconnects: AtomicU32::new(0),
            rejoin_offers: Mutex::new(rejoin_offers),
            password_overrides: Mutex::new(HashMap::new()),
            password_attempts: Mutex::default(),
            previews: Arc::new(crate::web::LinkPreviews::new(
                Some(data_dir.join("link-previews")),
                crate::web::LinkPreviews::app_policy(),
                crate::web::fetch::Timeouts::default(),
            )),
            full_images: crate::full_image::ServerTransfers::default(),
            direct: crate::p2p::Direct::default(),
            core: Mutex::new(Core::default()),
            now_playing: Mutex::new(crate::now_playing::NowPlaying::new(settings.now_playing)),
            now_gaming: Mutex::new(crate::now_gaming::NowGaming::new(settings.now_gaming)),
            user_stats: Mutex::default(),
            mumcord: Mutex::default(),
            history_sync: Arc::default(),
            watch_parties: Mutex::default(),
            screen_shares: Mutex::default(),
            inspector: Arc::default(),
            positional: crate::positional::Positional::new(
                voice.positional.clone(),
                &settings.positional,
            ),
        });
        let engine = Self { inner };
        engine.inner.direct.bind(engine.downgrade());
        engine.positional_poll_restart();
        engine
            .link_previews()
            .set_max_age_days(settings.preview_cache_days);
        engine.start_maintenance();
        Ok((engine, EventReceiver(rx)))
    }

    /// Shared DSP switches; push-to-talk key state goes to `set_ptt_pressed`.
    pub fn dsp_controls(&self) -> Arc<DspControls> {
        self.inner.controls.clone()
    }

    /// Context of the connected session of `server_id`.
    pub fn session_ctx_for(&self, server_id: ServerId) -> EngineResult<SessionCtx> {
        let core = lock(&self.inner.core);
        core.session(server_id).map(|s| self.inner.ctx(s))
    }

    /// Contexts of every connected session, for the features shared on all servers.
    pub(crate) fn session_ctxs(&self) -> Vec<SessionCtx> {
        let core = lock(&self.inner.core);
        core.slots
            .values()
            .filter_map(|slot| slot.session.as_ref())
            .map(|s| self.inner.ctx(s))
            .collect()
    }

    /// Context of the session the voice pipeline is bound to (positional audio, FR-2.2).
    pub(crate) fn voice_ctx(&self) -> Option<SessionCtx> {
        let core = lock(&self.inner.core);
        let server_id = core.audio_server?;
        core.session(server_id).ok().map(|s| self.inner.ctx(s))
    }

    /// Preview card for a link in a message (FR-1.49, FR-1.50); `None` when previews are
    /// switched off in the settings or there is nothing to show. Nothing is fetched while the
    /// switch is off.
    pub async fn link_preview(&self, url: &str) -> Option<crate::web::LinkPreview> {
        match self.settings() {
            Ok(settings) if settings.link_previews => self.inner.previews.preview(url).await,
            Ok(_) => None,
            Err(e) => {
                tracing::warn!(error = %e, "reading the settings for a link preview failed");
                None
            }
        }
    }

    /// An external image or image link from a message, ready to show inline (FR-1.55); `None`
    /// when previews are switched off in the settings (then nothing is fetched) or the image
    /// cannot be shown.
    pub async fn inline_image(&self, url: &str) -> Option<crate::web::InlineImage> {
        match self.settings() {
            Ok(settings) if settings.link_previews => self.inner.previews.inline_image(url).await,
            Ok(_) => None,
            Err(e) => {
                tracing::warn!(error = %e, "reading the settings for an inline image failed");
                None
            }
        }
    }

    /// The app data directory given to `Engine::new`.
    pub(crate) fn data_dir(&self) -> &Path {
        &self.inner.data_dir
    }

    /// Background task runtime.
    pub(crate) fn runtime(&self) -> &Runtime {
        &self.inner.rt
    }

    /// The guarded web fetcher and preview cache (also used for now-playing covers).
    pub(crate) fn link_previews(&self) -> &crate::web::LinkPreviews {
        &self.inner.previews
    }

    pub(crate) fn link_previews_arc(&self) -> Arc<crate::web::LinkPreviews> {
        self.inner.previews.clone()
    }

    pub(crate) fn full_image_transfers(&self) -> &crate::full_image::ServerTransfers {
        &self.inner.full_images
    }

    pub(crate) fn now_playing_state(&self) -> &Mutex<crate::now_playing::NowPlaying> {
        &self.inner.now_playing
    }

    pub(crate) fn direct(&self) -> &crate::p2p::Direct {
        &self.inner.direct
    }

    /// The identity from secure storage; `None` before the wizard created one.
    pub(crate) async fn stored_identity(&self) -> EngineResult<Option<Identity>> {
        self.inner.identity().await
    }

    pub(crate) fn mumcord_state(&self) -> &crate::mumcord::ServerPeers {
        &self.inner.mumcord
    }

    pub(crate) fn watch_party_state(&self) -> &crate::watch_party::ServerParties {
        &self.inner.watch_parties
    }

    pub(crate) fn screen_share_state(&self) -> &crate::screen_share::ServerShares {
        &self.inner.screen_shares
    }

    pub(crate) fn voice_shared(&self) -> &Arc<VoiceShared> {
        &self.inner.voice
    }

    pub(crate) fn now_gaming_state(&self) -> &Mutex<crate::now_gaming::NowGaming> {
        &self.inner.now_gaming
    }

    pub(crate) fn user_stats_state(&self) -> &Mutex<crate::stats::UserStatsHub> {
        &self.inner.user_stats
    }

    pub(crate) fn history_sync_state(&self) -> &Mutex<crate::history_sync::HistorySync> {
        &self.inner.history_sync
    }

    pub(crate) fn positional(&self) -> &crate::positional::Positional {
        &self.inner.positional
    }

    pub(crate) fn events(&self) -> &Events {
        &self.inner.events
    }

    /// A handle for background tasks that must not keep the engine alive.
    pub(crate) fn downgrade(&self) -> WeakEngine {
        WeakEngine(Arc::downgrade(&self.inner))
    }

    /// The store, also while disconnected (history commands, B14).
    pub(crate) fn store_handle(&self) -> Arc<Mutex<Store>> {
        self.inner.store.clone()
    }

    /// State of the voice server, else of the server the voice pipeline is bound to, else of
    /// any server in a slot; `Disconnected` when no server is.
    pub fn connection_state(&self) -> ConnState {
        let core = lock(&self.inner.core);
        core.voice_server
            .or(core.audio_server)
            .and_then(|id| core.slots.get(&id))
            .or_else(|| core.slots.values().min_by_key(|s| s.generation))
            .map_or(ConnState::Disconnected { error: None }, |slot| {
                slot.machine.state().clone()
            })
    }

    /// State of one server; `Disconnected` when it is in no slot.
    pub fn connection_state_of(&self, server_id: ServerId) -> ConnState {
        lock(&self.inner.core)
            .slots
            .get(&server_id)
            .map_or(ConnState::Disconnected { error: None }, |slot| {
                slot.machine.state().clone()
            })
    }

    /// States of every server that is not disconnected, in connect order.
    pub fn connections(&self) -> Vec<ConnState> {
        let core = lock(&self.inner.core);
        let mut slots: Vec<&Slot> = core.slots.values().collect();
        slots.sort_by_key(|s| s.generation);
        slots.iter().map(|s| s.machine.state().clone()).collect()
    }

    /// The server that carries voice (FR-2.2), `None` while every server is read-only.
    pub fn voice_server(&self) -> Option<ServerId> {
        lock(&self.inner.core).voice_server
    }

    /// Connects to a saved server next to the connected ones (FR-2.1, FR-2.6), within the
    /// limit of the settings. Lands read-only (self-muted and deafened) where the server puts
    /// the user; a `channel_path` (from a link) counts as an explicit join instead and moves
    /// voice there. On a server that is already connected only the `channel_path` is joined.
    pub fn connect(
        &self,
        server_id: ServerId,
        channel_path: Option<Vec<String>>,
    ) -> EngineResult<()> {
        self.connect_with_password(server_id, channel_path, None)
    }

    /// As [`Engine::connect`], with a one-off `password` replacing the saved one for this
    /// connection (retry after a wrong password, FR-2.59). Nothing is saved.
    pub fn connect_with_password(
        &self,
        server_id: ServerId,
        channel_path: Option<Vec<String>>,
        password: Option<String>,
    ) -> EngineResult<()> {
        let max = {
            let store = lock(&self.inner.store);
            if store.server(server_id).map_err(store_error)?.is_none() {
                return Err(EngineError::new(ErrorKind::NotFound, "unknown server"));
            }
            store.settings().map_err(store_error)?.max_connected_servers
        };
        let joined = {
            let core = lock(&self.inner.core);
            if let Some(slot) = core.slots.get(&server_id) {
                let target = slot
                    .session
                    .as_ref()
                    .zip(channel_path.as_ref())
                    .map(|(session, path)| session.handle.with_state(|s| resolve_path(s, path)));
                Some(target.flatten())
            } else {
                if core.slots.len() >= usize::from(max) {
                    return Err(limit_error(max));
                }
                None
            }
        };
        if let Some(target) = joined {
            return match target {
                Some(channel_id) => self.join_channel(server_id, channel_id),
                None => Ok(()),
            };
        }
        match password {
            Some(password) => {
                lock(&self.inner.password_overrides).insert(server_id, password);
            }
            None => {
                lock(&self.inner.password_overrides).remove(&server_id);
            }
        }
        // Taken even when a link joins a channel: the offer is for the first connect only.
        let rejoin = lock(&self.inner.rejoin_offers).remove(&server_id);
        if rejoin.is_some() && lock(&self.inner.core).voice_server.is_none() {
            // Offered once; the stored channel must not offer it again after the next quit.
            self.inner.forget_voice_channel();
        }
        let rejoin = rejoin.filter(|_| channel_path.is_none());
        self.inner.start(server_id, channel_path, rejoin);
        Ok(())
    }

    /// Connects at app start (FR-2.7): servers marked "Always connect at start" first, then
    /// the servers connected when the app last quit, most recent first (the voice server
    /// before the others, then the saved order), up to the limit. Every connect is read-only.
    /// Nothing happens before the first-run wizard created an identity.
    pub async fn connect_at_start(&self) -> EngineResult<()> {
        if self.inner.identity().await?.is_none() {
            return Ok(());
        }
        let (servers, max) = {
            let store = lock(&self.inner.store);
            let servers = store.servers().map_err(store_error)?;
            (
                servers,
                store.settings().map_err(store_error)?.max_connected_servers,
            )
        };
        for server_id in start_servers(&servers, max) {
            if let Err(e) = self.connect(server_id, None) {
                tracing::warn!(server_id, error = %e, "connecting at start failed");
            }
        }
        Ok(())
    }

    /// Disconnects one server and cancels its reconnecting or pending trust question
    /// (FR-2.9). A server that is not connected is left alone.
    pub fn disconnect(&self, server_id: ServerId) {
        let mut core = lock(&self.inner.core);
        if !core.slots.contains_key(&server_id) {
            return;
        }
        if core.audio_server == Some(server_id) {
            self.inner.hang_up(&mut core);
        }
        self.inner.end_slot(&mut core, server_id, None);
    }

    /// Disconnects every server for quitting or restarting the app (FR-2.9). Unlike
    /// [`Engine::disconnect`] it keeps the voice channel, so the next start offers to rejoin
    /// it (QA.9), and records which servers were connected for the next start (FR-2.7). The
    /// returned future resolves once the connections have sent what was still queued (chat,
    /// status, plugin data), at most [`SHUTDOWN_DRAIN`] later; await it before exiting.
    pub fn shutdown(&self) -> impl Future<Output = ()> + Send + use<> {
        let mut core = lock(&self.inner.core);
        let mut slots: Vec<(ServerId, u64)> = core
            .slots
            .iter()
            .map(|(id, s)| (*id, s.generation))
            .collect();
        slots.sort_by_key(|(_, generation)| *generation);
        let connected: Vec<ServerId> = slots.iter().map(|(id, _)| *id).collect();
        if let Err(e) = lock(&self.inner.store).set_quit_state(
            &connected,
            core.voice_server,
            crate::chat::now_millis(),
        ) {
            tracing::warn!(error = %e, "saving the connected servers failed");
        }
        let closed: Vec<_> = core
            .slots
            .values()
            .filter_map(|slot| slot.session.as_ref().map(|s| s.handle.closed()))
            .collect();
        for server_id in connected {
            self.inner.close_slot(&mut core, server_id, None);
        }
        // The connections drain in their own tasks, in parallel; this only waits for the last.
        let all_closed = async move {
            for closed in closed {
                closed.await;
            }
        };
        async move {
            if tokio::time::timeout(SHUTDOWN_DRAIN, all_closed)
                .await
                .is_err()
            {
                tracing::warn!("connections did not close in time on shutdown");
            }
        }
    }

    /// Stops reconnecting; same as disconnecting.
    pub fn reconnect_cancel(&self, server_id: ServerId) {
        self.disconnect(server_id);
    }

    /// Answers a `CertTrustRequired` event.
    pub fn cert_trust_decide(
        &self,
        server_id: ServerId,
        fingerprint: &str,
        trust: bool,
    ) -> EngineResult<()> {
        let pending = {
            let mut core = lock(&self.inner.core);
            let slot = core
                .slots
                .get_mut(&server_id)
                .filter(|slot| {
                    slot.pending_trust
                        .as_ref()
                        .is_some_and(|p| p.fingerprint == fingerprint)
                })
                .ok_or_else(|| EngineError::invalid("no such certificate question"))?;
            let pending = slot.pending_trust.take().expect("checked above");
            if !trust {
                let error = EngineError::new(
                    declined_kind(pending.changed),
                    "server certificate not trusted",
                );
                self.inner.end_slot(&mut core, server_id, Some(error));
                return Ok(());
            }
            pending
        };
        lock(&self.inner.store)
            .set_trusted_cert(server_id, &pending.fingerprint)
            .map_err(store_error)?;
        self.inner
            .start(server_id, pending.channel_path, pending.rejoin);
        Ok(())
    }

    /// Snapshot of the server the voice pipeline is bound to (in-game overlay, FR-2.2).
    pub fn snapshot(&self) -> Option<Snapshot> {
        let core = lock(&self.inner.core);
        self.inner.snapshot(&core, core.audio_server?)
    }

    pub fn snapshot_of(&self, server_id: ServerId) -> Option<Snapshot> {
        let core = lock(&self.inner.core);
        self.inner.snapshot(&core, server_id)
    }

    /// Mutes the voice server. Without one, it applies to the only connected server (leaving
    /// read-only there, as with a single server); with several read-only servers only the
    /// choice is remembered for the next join.
    pub fn set_self_mute(&self, mute: bool) -> EngineResult<()> {
        self.inner.change_own_voice(|v| v.set_mute(mute))
    }

    /// Deafens like [`Engine::set_self_mute`] mutes.
    pub fn set_self_deaf(&self, deaf: bool) -> EngineResult<()> {
        self.inner.change_own_voice(|v| v.set_deaf(deaf))
    }

    /// Explicit join: moves there (unless already there) and turns voice on with the user's last
    /// own mute/deafen choice (QA.7). On a server without voice it moves voice there; the
    /// previous voice server becomes read-only (FR-2.2).
    pub fn join_channel(&self, server_id: ServerId, channel_id: u32) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let (known, here) = ctx.handle.with_state(|s| {
            (
                s.channels.contains_key(&channel_id),
                s.own_user().map(|u| u.channel_id) == Some(channel_id),
            )
        });
        if !known {
            return Err(EngineError::new(ErrorKind::NotFound, "unknown channel"));
        }
        if !here {
            ctx.handle
                .join_channel(channel_id)
                .map_err(convert::command_error)?;
        }
        let own = self.inner.own_voice();
        let joined = {
            let mut core = lock(&self.inner.core);
            self.inner.take_voice(&mut core, server_id);
            let mut voice = core
                .slots
                .get(&server_id)
                .map(|s| s.self_voice)
                .ok_or_else(EngineError::not_connected)?;
            let joins = joins_voice(!here, voice);
            voice.join(own);
            joins.then_some(voice)
        };
        // Before the voice change is sent, so the hub knows its echo when it arrives.
        if let Some(voice) = joined {
            self.inner.events.sounds().voice_join(
                (!here).then_some(channel_id),
                voice.mute,
                voice.deaf,
            );
        }
        self.inner.change_self_voice(server_id, |v| v.join(own))?;
        // The server's echo of the move may come too late, e.g. when the app quits right away.
        self.inner.remember_voice_channel_at(server_id, channel_id);
        Ok(())
    }

    pub fn person_set_volume(
        &self,
        server_id: ServerId,
        session: u32,
        volume: u16,
    ) -> EngineResult<()> {
        if volume > 200 {
            return Err(EngineError::invalid("volume must be 0–200"));
        }
        self.inner
            .change_person(server_id, session, |s| s.volume = volume)
    }

    pub fn person_set_local_mute(
        &self,
        server_id: ServerId,
        session: u32,
        muted: bool,
    ) -> EngineResult<()> {
        self.inner
            .change_person(server_id, session, |s| s.local_mute = muted)
    }

    /// Saves the private note about a person (FR-1.63); an empty note removes it.
    pub fn person_set_note(
        &self,
        server_id: ServerId,
        session: u32,
        note: &str,
    ) -> EngineResult<()> {
        let mut core = lock(&self.inner.core);
        let user = Inner::person_in_session(&core, server_id, session)?;
        let store = lock(&self.inner.store);
        let slot = core
            .slots
            .get_mut(&server_id)
            .ok_or_else(EngineError::not_connected)?;
        slot.people.save_note(&store, server_id, &user, note)
    }

    /// Friends with their online state, online first (FR-1.64).
    pub fn friends(&self) -> Vec<crate::friends::FriendView> {
        self.inner.events.friends().views()
    }

    /// Adds a person on a connected server as a friend (FR-1.64); friends are known by
    /// certificate on every server. People without a certificate cannot be friends; neither
    /// can the user.
    pub fn friend_add(&self, server_id: ServerId, session: u32) -> EngineResult<()> {
        let friend = {
            let core = lock(&self.inner.core);
            let user = Inner::person_in_session(&core, server_id, session)?;
            let own = core.session(server_id)?.handle.with_state(|st| st.session);
            if own == Some(session) {
                return Err(EngineError::invalid("you cannot be your own friend"));
            }
            let cert_hash = user.cert_hash.ok_or_else(|| {
                EngineError::invalid("people without a certificate cannot be friends")
            })?;
            let now = crate::chat::now_millis();
            lock(&self.inner.store)
                .add_friend(&cert_hash, &user.name, now)
                .map_err(store_error)?;
            mumcord_store::Friend {
                cert_hash,
                name: user.name,
                added_at: now,
                last_seen_at: None,
            }
        };
        let friends = self.inner.events.friends().added(friend);
        self.inner.events.emit(EngineEvent::FriendsChanged(friends));
        Ok(())
    }

    /// Removes a friend by certificate hash; unknown hashes are a no-op (FR-1.64).
    pub fn friend_remove(&self, cert_hash: &str) -> EngineResult<()> {
        lock(&self.inner.store)
            .remove_friend(cert_hash)
            .map_err(store_error)?;
        let friends = self.inner.events.friends().removed(cert_hash);
        self.inner.events.emit(EngineEvent::FriendsChanged(friends));
        Ok(())
    }

    /// Asks the server for a channel description announced only by its hash (FR-1.48b). The
    /// text arrives as a `ChannelUpserted` event; asking again for the same hash is a no-op.
    pub fn channel_description_request(
        &self,
        server_id: ServerId,
        channel_id: u32,
    ) -> EngineResult<()> {
        let mut core = lock(&self.inner.core);
        let session = core
            .slots
            .get_mut(&server_id)
            .and_then(|slot| slot.session.as_mut())
            .ok_or_else(EngineError::not_connected)?;
        let channel = session
            .handle
            .with_state(|s| s.channels.get(&channel_id).cloned())
            .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown channel"))?;
        if session.description_requests.should_request(&channel) {
            session
                .handle
                .request_blobs(&[], &[], &[channel_id])
                .map_err(convert::command_error)?;
        }
        Ok(())
    }

    /// Avatar and comment of a person, fetched from the server if only announced by hash.
    pub async fn person_profile(
        &self,
        server_id: ServerId,
        session: u32,
    ) -> EngineResult<ProfileView> {
        let ctx = self.session_ctx_for(server_id)?;
        let user = |ctx: &SessionCtx| {
            ctx.handle
                .with_state(|s| s.users.get(&session).cloned())
                .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown person"))
        };
        let mut current = user(&ctx)?;
        let (texture, comment) = people::missing_blobs(&current);
        if texture || comment {
            let pick = |want: bool| if want { vec![session] } else { Vec::new() };
            ctx.handle
                .request_blobs(&pick(texture), &pick(comment), &[])
                .map_err(convert::command_error)?;
            let deadline = Instant::now() + PROFILE_WAIT;
            while people::missing_blobs(&current) != (false, false) && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(100)).await;
                current = user(&ctx)?;
            }
        }
        let mut profile = people::profile(&current);
        {
            let core = lock(&self.inner.core);
            if let Some(slot) = core.slots.get(&server_id) {
                profile.note = slot
                    .people
                    .note(&lock(&self.inner.store), server_id, &current);
            }
        }
        // The own profile is edited from here; the status lines are not part of it.
        if ctx.handle.with_state(|s| s.session) == Some(session) {
            profile.comment = profile
                .comment
                .map(|c| crate::now_playing::comment::strip(&c).to_owned())
                .filter(|c| !c.is_empty());
        }
        Ok(profile)
    }

    /// Microphone test: record up to 5 s, then play back (FR-1.4). Voice pauses meanwhile.
    pub async fn mic_test_start(&self) -> EngineResult<()> {
        self.inner.ensure_audio().await?;
        let mut core = lock(&self.inner.core);
        if core.mic_test.is_some() {
            return Ok(());
        }
        self.inner.stop_pipeline(&mut core);
        let audio = core
            .audio
            .as_mut()
            .ok_or_else(|| EngineError::internal("audio not running"))?;
        let test = match voice::start_mic_test(audio, self.inner.controls.clone()) {
            Ok(test) => test,
            Err(error) => {
                self.inner.events.emit(EngineEvent::MicTestPhaseChanged(
                    types::MicTestPhase::Failed,
                ));
                return Err(error);
            }
        };
        core.mic_test = Some(test);
        self.inner.events.emit(EngineEvent::MicTestPhaseChanged(
            types::MicTestPhase::Recording,
        ));
        self.inner.meter.test.store(true, Ordering::Relaxed);
        let inner = self.inner.clone();
        self.inner
            .rt
            .spawn(async move { inner.watch_mic_test().await });
        Ok(())
    }

    /// Ends the recording early; playback of what was recorded follows.
    pub fn mic_test_stop_recording(&self) {
        if let Some(test) = &lock(&self.inner.core).mic_test {
            test.stop_recording();
        }
    }

    /// Ends the mic test wherever it is; voice of a connected session resumes.
    pub fn mic_test_stop(&self) {
        let mut core = lock(&self.inner.core);
        self.inner.finish_mic_test(&mut core);
    }

    /// Highest bitrate in bit/s the bitrate slider can reach on the voice server (FR-2.64);
    /// `None` while no voice server is connected.
    pub fn bitrate_max(&self) -> Option<u32> {
        Some(self.inner.voice.bitrate_max_bps.load(Ordering::Relaxed)).filter(|&b| b > 0)
    }

    /// Lists devices. Without a connection or mic test no stream is opened for it: opening the
    /// microphone just to list it made macOS ask for microphone access on every device-picker
    /// screen.
    pub async fn audio_devices(&self) -> EngineResult<AudioDeviceList> {
        let inner = self.inner.clone();
        self.inner
            .rt
            .spawn_blocking(move || {
                {
                    let core = lock(&inner.core);
                    if let Some(audio) = &core.audio {
                        return audio.devices();
                    }
                }
                voice::idle_device_list()
            })
            .await
            .map_err(|e| EngineError::internal(e.to_string()))?
    }

    /// Starts or stops the packet inspector's capture (issue #60); stopping drops what it holds.
    pub fn set_packet_capture(&self, enabled: bool) {
        self.inner.inspector.set_enabled(enabled);
    }

    /// Drops the captured packets and resets the inspector's counters.
    pub fn clear_packet_capture(&self) {
        self.inner.inspector.clear();
    }

    /// Captured packets with an id above `after`, the counters and the voice packet counts.
    pub fn poll_packet_capture(&self, after: Option<u64>) -> crate::inspector::InspectorPoll {
        let voice = {
            let core = lock(&self.inner.core);
            core.slots
                .keys()
                .filter_map(|&server_id| {
                    let stats = core.session(server_id).ok()?.transport.as_ref()?.stats();
                    let rating = (core.audio_server == Some(server_id))
                        .then(|| core.audio_health(&self.inner.voice))
                        .flatten()
                        .map(|a| (a.rating.rating, a.rating.issue));
                    Some(crate::inspector::VoiceCount {
                        server_id,
                        sent: stats.audio_sent,
                        received: stats.audio_received,
                        rating,
                    })
                })
                .collect()
        };
        self.inner.inspector.poll(after, voice)
    }

    /// Live link figures of a server's connection, `None` when not connected (AC-1.23).
    /// Bitrate and packet counters belong to the voice pipeline's server; elsewhere they are 0.
    pub fn connection_stats(&self, server_id: ServerId) -> Option<ConnectionStatsView> {
        let core = lock(&self.inner.core);
        let session = core.session(server_id).ok()?;
        let link = session.transport.as_ref().map(|t| t.stats());
        let voice = &self.inner.voice;
        let on_voice = core.audio_server == Some(server_id);
        let count = |n: &std::sync::atomic::AtomicU64| {
            if on_voice {
                n.load(Ordering::Relaxed)
            } else {
                0
            }
        };
        Some(ConnectionStatsView {
            transport: link.map(|l| match l.transport {
                Transport::Udp => VoiceTransportKind::Udp,
                Transport::Tcp => VoiceTransportKind::Tcp,
            }),
            rtt_ms: link.and_then(|l| l.rtt).map(|d| d.as_secs_f32() * 1000.0),
            loss_percent: link.map_or(0.0, |l| l.loss.clamp(0.0, 1.0) * 100.0),
            send_bitrate_bps: if on_voice {
                voice.bitrate_bps.load(Ordering::Relaxed)
            } else {
                0
            },
            server_max_bandwidth_bps: session.handle.with_state(|s| s.server.max_bandwidth),
            packets_sent: count(&voice.packets_sent),
            packets_received: count(&voice.packets_received),
            audio: on_voice.then(|| core.audio_health(voice)).flatten(),
        })
    }

    /// Devices and connection counters for the diagnostic file (no names or addresses
    /// beyond device names, which the diagnostics module documents).
    pub fn diagnostic_info(&self, app_version: &str) -> crate::diag::DiagnosticInfo {
        use mumcord_audio::io::{AudioIo, Direction};
        let mut info = crate::diag::DiagnosticInfo::new(app_version);
        for (direction, is_input) in [(Direction::Input, true), (Direction::Output, false)] {
            // Without a connection or mic test no audio runs; the list is still wanted then.
            let running = lock(&self.inner.core)
                .audio
                .as_ref()
                .map(|audio| audio.io().devices(direction));
            let devices = running.unwrap_or_else(|| AudioIo::list_devices(direction));
            match devices {
                Ok(devices) => {
                    info.audio_devices.extend(devices.into_iter().map(|d| {
                        crate::diag::DiagAudioDevice {
                            name: d.name,
                            is_input,
                            is_default: d.is_default,
                        }
                    }));
                }
                Err(e) => tracing::warn!(?direction, "listing audio devices failed: {e}"),
            }
        }
        let core = lock(&self.inner.core);
        info.connection.reconnect_count = self.inner.reconnects.load(Ordering::Relaxed);
        let audio_server = core.audio_server;
        let crypt = audio_server
            .and_then(|id| core.session(id).ok())
            .and_then(|s| s.transport.as_ref())
            .map(|t| t.stats().crypt);
        drop(core);
        if let Some(stats) = audio_server.and_then(|id| self.connection_stats(id)) {
            let c = &mut info.connection;
            c.connected = true;
            c.transport = stats.transport.map(|t| {
                match t {
                    VoiceTransportKind::Udp => "udp",
                    VoiceTransportKind::Tcp => "tcp",
                }
                .to_owned()
            });
            c.ping_ms = stats.rtt_ms;
            c.loss_percent = stats.loss_percent;
            c.send_bitrate_kbps = Some(stats.send_bitrate_bps / 1000);
            c.server_max_bandwidth_kbps = stats.server_max_bandwidth_bps.map(|b| b / 1000);
            c.packets_sent = stats.packets_sent;
            c.packets_received = stats.packets_received;
            c.audio = stats.audio;
        }
        if let Some(crypt) = crypt {
            info.connection.packets_good = crypt.good.into();
            info.connection.packets_late = crypt.late.into();
            info.connection.packets_lost = crypt.lost.into();
        }
        info
    }
}

impl Inner {
    fn ctx(&self, session: &Session) -> SessionCtx {
        SessionCtx {
            server_id: session.server_id,
            handle: session.handle.clone(),
            store: self.store.clone(),
            events: self.events.for_server(session.server_id),
        }
    }

    /// The user's own bitrate cap in bits per second; `None` is "Max" (FR-2.64).
    fn user_cap_bps(&self) -> Option<u32> {
        self.controls.bitrate_cap_kbps().map(|k| k * 1000)
    }

    /// Stores the slider maximum for `bitrate_max` and reports a change.
    fn publish_bitrate_max(&self, max_bps: Option<u32>) {
        let new = max_bps.unwrap_or(0);
        if self.voice.bitrate_max_bps.swap(new, Ordering::Relaxed) != new {
            self.events.emit(EngineEvent::BitrateMaxChanged(max_bps));
        }
    }

    fn is_audio_server(&self, server_id: ServerId) -> bool {
        load_server(&self.events.routing.audio) == Some(server_id)
    }

    /// Reports a server's state (`Disconnected` once it left its slot) and closes the audio
    /// devices when nothing needs them any more.
    fn emit_state(&self, core: &mut Core, server_id: ServerId, error: Option<EngineError>) {
        let state = core
            .slots
            .get(&server_id)
            .map_or(ConnState::Disconnected { error }, |slot| {
                slot.machine.state().clone()
            });
        self.release_audio_if_idle(core);
        self.events
            .for_server(server_id)
            .emit(EngineEvent::StateChanged(state));
    }

    /// The user ends the voice pipeline's connection (disconnect, not quitting): plays the
    /// hang-up sound (QA.19) if voice is on. The pipeline stops first and the sound runs on its
    /// own short-lived player, so tearing down voice and closing the devices cannot cut it off.
    fn hang_up(&self, core: &mut Core) {
        let Some(samples) = self.events.sounds().hang_up() else {
            return;
        };
        self.stop_pipeline(core);
        if let Some(audio) = core.audio.as_ref() {
            audio.play_tail(&self.rt, &samples);
        }
    }

    /// Devices are open only while a server is in a slot or the mic test runs.
    fn release_audio_if_idle(&self, core: &mut Core) {
        if core.slots.is_empty() && core.mic_test.is_none() && core.pipeline.is_none() {
            core.audio = None;
        }
    }

    /// Starts a fresh connection in a new slot (or in the slot waiting for a trust decision).
    fn start(
        self: &Arc<Self>,
        server_id: ServerId,
        channel_path: Option<Vec<String>>,
        rejoin: Option<String>,
    ) {
        let mut core = lock(&self.core);
        core.generations += 1;
        let generation = core.generations;
        let mut machine = Machine::default();
        machine.connect(server_id);
        let slot = Slot {
            machine,
            generation,
            supervisor: None,
            session: None,
            pending_trust: None,
            self_voice: SelfVoice::read_only(),
            people: People::default(),
        };
        if let Some(task) = core
            .slots
            .insert(server_id, slot)
            .and_then(|mut previous| previous.supervisor.take())
        {
            task.abort();
        }
        self.emit_state(&mut core, server_id, None);
        let inner = self.clone();
        let task = self.rt.spawn(async move {
            inner
                .supervise(generation, server_id, channel_path, rejoin)
                .await
        });
        if let Some(slot) = core.slots.get_mut(&server_id) {
            slot.supervisor = Some(task);
        }
    }

    /// Ends a server for good on the user's request or after its connection ended: as
    /// [`Inner::close_slot`], and the voice channel is forgotten if it had voice, so the next
    /// start offers nothing to rejoin.
    fn end_slot(
        self: &Arc<Self>,
        core: &mut Core,
        server_id: ServerId,
        error: Option<EngineError>,
    ) {
        if core.voice_server == Some(server_id) {
            self.forget_voice_channel();
        }
        self.close_slot(core, server_id, error);
    }

    /// Ends a server's connection and its tasks and reports it disconnected. Voice moves to
    /// no server; the pipeline moves to another connected server (read-only there).
    fn close_slot(
        self: &Arc<Self>,
        core: &mut Core,
        server_id: ServerId,
        error: Option<EngineError>,
    ) {
        if core.audio_server == Some(server_id) {
            self.stop_pipeline(core);
        }
        lock(&self.password_overrides).remove(&server_id);
        let Some(mut slot) = core.slots.remove(&server_id) else {
            return;
        };
        if let Some(task) = slot.supervisor.take() {
            task.abort();
        }
        if core.voice_server == Some(server_id) {
            core.voice_server = None;
            self.events.emit(EngineEvent::VoiceServerChanged(None));
        }
        if let Some(session) = slot.session.take() {
            crate::now_playing::on_disconnect(&self.now_playing, server_id, &session.handle);
            crate::now_gaming::on_disconnect(&self.now_gaming, server_id, &session.handle);
            crate::mumcord::on_disconnect(&self.mumcord, server_id);
            crate::history_sync::on_disconnect(&self.history_sync, server_id);
            self.forget_password_attempt(server_id);
            self.full_images.reset(server_id);
            self.direct.reset(server_id);
            if crate::watch_party::reset(&self.watch_parties, server_id) {
                self.events
                    .for_server(server_id)
                    .emit(EngineEvent::WatchPartyChanged(None));
            }
            if crate::screen_share::reset(&self.screen_shares, server_id) {
                self.events
                    .for_server(server_id)
                    .emit(EngineEvent::ScreenSharesChanged(Vec::new()));
            }
            session.handle.disconnect();
        }
        self.rebind_audio(core);
        self.emit_state(core, server_id, error);
    }

    /// The Mumcord features start afresh on `ctx`'s (re)synced session.
    fn features_synced(&self, ctx: &SessionCtx) {
        crate::now_playing::on_synced(&self.now_playing, ctx.server_id);
        crate::now_gaming::on_synced(&self.now_gaming, ctx.server_id);
        crate::mumcord::on_synced(&self.mumcord, ctx);
        crate::history_sync::on_synced(&self.history_sync, ctx, &self.rt);
        self.full_images.reset(ctx.server_id);
        self.direct.reset(ctx.server_id);
        if crate::watch_party::reset(&self.watch_parties, ctx.server_id) {
            ctx.events.emit(EngineEvent::WatchPartyChanged(None));
        }
        if crate::screen_share::reset(&self.screen_shares, ctx.server_id) {
            ctx.events
                .emit(EngineEvent::ScreenSharesChanged(Vec::new()));
        }
    }

    /// Binds the voice pipeline to the voice server, else to the earliest connected server.
    /// Moving it stops the pipeline and starts a new one on the other connection, the same
    /// hand-over as the mic test; the audio threads see only the exchanged queues.
    fn rebind_audio(self: &Arc<Self>, core: &mut Core) {
        let target = core.voice_server.or_else(|| core.earliest_connected());
        if core.audio_server != target {
            self.stop_pipeline(core);
            if core.audio_server.is_some() {
                self.positional.on_disconnect();
            }
            core.audio_server = target;
            self.events
                .routing
                .audio
                .store(target.unwrap_or(0), Ordering::Relaxed);
            self.voice.reset_link();
            self.events.emit(EngineEvent::BitrateMaxChanged(None));
            let voice = target
                .and_then(|id| core.slots.get(&id))
                .map_or(SelfVoice::read_only(), |slot| slot.self_voice);
            self.voice.muted.store(voice.mute, Ordering::Relaxed);
            self.voice.deaf.store(voice.deaf, Ordering::Relaxed);
            if let Some(id) = target {
                self.positional.on_synced();
                // The sound hub learns the people of its new server, as on a sync.
                if let Some(snapshot) = self.snapshot(core, id) {
                    self.events
                        .sounds()
                        .observe(&EngineEvent::ServerSynced(Box::new(snapshot)));
                }
            }
        }
        self.start_pipeline(core);
    }

    /// Makes `server_id` the voice server (FR-2.2). The previous voice server becomes
    /// read-only again: self-deafened and muted, the user stays in their channel.
    fn take_voice(self: &Arc<Self>, core: &mut Core, server_id: ServerId) {
        if core.voice_server == Some(server_id) {
            return;
        }
        if let Some(previous) = core.voice_server.take()
            && let Some(slot) = core.slots.get_mut(&previous)
        {
            slot.self_voice = SelfVoice::read_only();
            // Deafening on the server mutes too.
            if let Some(session) = &slot.session
                && let Err(e) = session.handle.set_self_deaf(true)
            {
                tracing::warn!(error = %e, "making the previous voice server read-only failed");
            }
        }
        core.voice_server = Some(server_id);
        // Only the voice server's channel is offered for rejoining after a restart (FR-2.3).
        self.forget_voice_channel();
        self.events
            .emit(EngineEvent::VoiceServerChanged(Some(server_id)));
        self.rebind_audio(core);
    }

    /// Runs a state transition if `generation` is still the slot's and reports the new state.
    fn transition<R>(
        &self,
        server_id: ServerId,
        generation: u64,
        f: impl FnOnce(&mut Core) -> R,
    ) -> Option<R> {
        let mut core = lock(&self.core);
        if core.slots.get(&server_id)?.generation != generation {
            return None;
        }
        let result = f(&mut core);
        self.emit_state(&mut core, server_id, None);
        Some(result)
    }

    /// Runs a machine step if `generation` is still the slot's; giving up ends the slot.
    fn step(
        self: &Arc<Self>,
        server_id: ServerId,
        generation: u64,
        f: impl FnOnce(&mut Machine) -> Step,
    ) -> Option<Step> {
        let mut core = lock(&self.core);
        let slot = core
            .slots
            .get_mut(&server_id)
            .filter(|slot| slot.generation == generation)?;
        let step = f(&mut slot.machine);
        match step {
            Step::Retry(_) => self.emit_state(&mut core, server_id, None),
            // Gave up: voice ended without the app quitting.
            Step::Stop => {
                let error = match slot.machine.state() {
                    ConnState::Disconnected { error } => error.clone(),
                    _ => None,
                };
                self.end_slot(&mut core, server_id, error);
            }
        }
        Some(step)
    }

    /// The connection ended for good (kicked, banned or closed on request).
    fn finish(self: &Arc<Self>, server_id: ServerId, generation: u64, error: Option<EngineError>) {
        let mut core = lock(&self.core);
        if core
            .slots
            .get(&server_id)
            .is_some_and(|slot| slot.generation == generation)
        {
            self.end_slot(&mut core, server_id, error);
        }
    }

    async fn supervise(
        self: Arc<Self>,
        generation: u64,
        server_id: ServerId,
        mut path: Option<Vec<String>>,
        mut rejoin: Option<String>,
    ) {
        let mut synced_before = false;
        loop {
            let step = match self.attempt(server_id).await {
                Ok((connection, host, port)) => {
                    let arrival = if synced_before {
                        Arrival::Reconnect
                    } else if let Some(rejoin) = rejoin.take() {
                        Arrival::FreshWithRejoin(rejoin)
                    } else {
                        Arrival::Fresh(path.take())
                    };
                    match self
                        .run_connection(generation, server_id, connection, &host, port, arrival)
                        .await
                    {
                        Ended::Lost => {
                            synced_before = true;
                            self.step(server_id, generation, Machine::lost)
                        }
                        Ended::Final(error) => {
                            self.finish(server_id, generation, error);
                            return;
                        }
                        Ended::Stale => return,
                    }
                }
                Err(AttemptError::Trust {
                    fingerprint,
                    previous,
                }) => {
                    let current = self.transition(server_id, generation, |c| {
                        if let Some(slot) = c.slots.get_mut(&server_id) {
                            slot.machine.connect(server_id);
                            slot.pending_trust = Some(PendingTrust {
                                fingerprint: fingerprint.clone(),
                                changed: previous.is_some(),
                                channel_path: path.take(),
                                rejoin: rejoin.take(),
                            });
                        }
                    });
                    if current.is_some() {
                        tracing::info!(
                            server_id,
                            changed = previous.is_some(),
                            "server certificate needs a trust decision"
                        );
                        self.events
                            .for_server(server_id)
                            .emit(EngineEvent::CertTrustRequired {
                                server_id,
                                fingerprint,
                                previous_fingerprint: previous,
                            });
                    }
                    return;
                }
                Err(AttemptError::Failure { failure, reason }) => {
                    tracing::info!(server_id, %reason, "connection attempt failed");
                    self.step(server_id, generation, |m| m.failed(failure))
                }
            };
            match step {
                Some(Step::Retry(delay)) => {
                    self.reconnects.fetch_add(1, Ordering::Relaxed);
                    tracing::info!(server_id, delay_secs = delay.as_secs(), "reconnecting");
                    tokio::time::sleep(delay).await;
                }
                Some(Step::Stop) | None => return,
            }
        }
    }

    /// One connection attempt including the certificate check.
    async fn attempt(
        &self,
        server_id: ServerId,
    ) -> Result<(Connection, String, u16), AttemptError> {
        let fatal = |e: EngineError| AttemptError::Failure {
            reason: format!("{:?}", e.kind),
            failure: Failure::Fatal(e),
        };
        let (server, trusted) = {
            let store = lock(&self.store);
            let server = store
                .server(server_id)
                .map_err(store_error)
                .and_then(|s| {
                    s.ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown server"))
                })
                .map_err(fatal)?;
            let trusted = store
                .trusted_cert(server_id)
                .map_err(store_error)
                .map_err(fatal)?;
            (server, trusted)
        };
        let overridden = lock(&self.password_overrides).get(&server_id).cloned();
        let secrets = self.secrets.clone();
        let has_override = overridden.is_some();
        let (saved, tokens) = self
            .rt
            .spawn_blocking(move || {
                let password = if has_override {
                    None
                } else {
                    secrets.server_password(server_id)?
                };
                Ok::<_, mumcord_store::Error>((password, secrets.access_tokens(server_id)?))
            })
            .await
            .map_err(|e| fatal(EngineError::internal(e.to_string())))?
            .map_err(store_error)
            .map_err(fatal)?;
        let password = overridden.or(saved);
        let identity = self
            .identity()
            .await
            .map_err(fatal)?
            .ok_or_else(|| fatal(EngineError::new(ErrorKind::NotFound, "no identity yet")))?;

        let expected = trusted.clone();
        let config = ConnectConfig {
            host: server.address.clone(),
            port: server.port,
            username: server.username.clone(),
            password,
            tokens,
            identity: Some(ClientIdentity {
                chain: vec![CertificateDer::from(identity.cert_der().to_vec())],
                key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                    identity.key_pkcs8_der().to_vec(),
                )),
            }),
            verify_cert: Arc::new(move |cert| {
                check_cert(expected.as_deref(), &format_fingerprint(&cert.sha256_hex))
                    == CertCheck::Trusted
            }),
            login_timeout: LOGIN_TIMEOUT,
        };
        let events = self.events.for_server(server_id);
        let progress: mumcord_protocol::SyncProgress = Arc::new(move |channels| {
            events.emit(EngineEvent::SyncProgress {
                channels: u32::try_from(channels).unwrap_or(u32::MAX),
            });
        });
        let tap = Some(self.inspector.tap(server_id));
        match mumcord_protocol::connect_with_tap(config, Some(progress), tap).await {
            Ok(connection) => Ok((connection, server.address, server.port)),
            Err(ConnectError::CertificateRejected(cert)) => {
                let fingerprint = format_fingerprint(&cert.sha256_hex);
                match check_cert(trusted.as_deref(), &fingerprint) {
                    CertCheck::Unknown => Err(AttemptError::Trust {
                        fingerprint,
                        previous: None,
                    }),
                    CertCheck::Changed { previous } => {
                        tracing::warn!(server_id, "server certificate changed");
                        Err(AttemptError::Trust {
                            fingerprint,
                            previous: Some(previous),
                        })
                    }
                    CertCheck::Trusted => Err(fatal(EngineError::new(
                        ErrorKind::CertificateRejected,
                        "certificate rejected",
                    ))),
                }
            }
            Err(e) => Err(AttemptError::Failure {
                failure: convert::connect_failure(&e),
                reason: convert::failure_reason(&e),
            }),
        }
    }

    /// Runs a synced connection until it ends.
    async fn run_connection(
        self: &Arc<Self>,
        generation: u64,
        server_id: ServerId,
        connection: Connection,
        host: &str,
        port: u16,
        arrival: Arrival,
    ) -> Ended {
        let Connection { handle, mut events } = connection;
        let (transport, mut voice_rx) = match start_voice(&handle, host, port).await {
            Some((transport, rx)) => (Some(Arc::new(transport)), Some(rx)),
            None => (None, None),
        };
        let (pump_tx, mut pump_rx) = mpsc::unbounded_channel();
        let default_channel_id = handle.with_state(|s| s.own_user().map(|u| u.channel_id));
        let link_join = matches!(arrival, Arrival::Fresh(Some(_)));
        let (followed, rejoin_offer, voice) = self.post_sync(server_id, &handle, arrival);

        let registered = self.transition(server_id, generation, |core| {
            if let Some(slot) = core.slots.get_mut(&server_id) {
                slot.people.clear();
                if let Some(voice) = voice {
                    slot.self_voice = voice;
                }
                slot.session = Some(Session {
                    server_id,
                    handle: handle.clone(),
                    transport: transport.clone(),
                    pump: pump_tx,
                    rejoin_offer,
                    default_channel_id,
                    description_requests: description::DescriptionRequests::default(),
                });
                slot.machine.synced();
            }
            if core
                .slots
                .get(&server_id)
                .is_some_and(|slot| slot.self_voice.own().is_some())
            {
                self.take_voice(core, server_id);
            }
            self.rebind_audio(core);
            self.remember_voice_channel(core);
            core.audio_server == Some(server_id)
        });
        let Some(audio) = registered else {
            handle.disconnect();
            return Ended::Stale;
        };
        tracing::info!(server_id, "connected");
        let ctx = SessionCtx {
            server_id,
            handle: handle.clone(),
            store: self.store.clone(),
            events: self.events.for_server(server_id),
        };
        ctx.events
            .emit(EngineEvent::FollowedChannelsChanged(followed));
        self.features_synced(&ctx);
        if audio {
            // A reconnect of the pipeline's server: fresh counters and game context.
            self.voice.reset_link();
            self.positional.on_synced();
        }
        let snapshot = self.snapshot(&lock(&self.core), server_id);
        if let Some(mut snapshot) = snapshot {
            extension::on_synced(&ctx, &mut snapshot);
            ctx.events
                .emit(EngineEvent::ServerSynced(Box::new(snapshot)));
        }
        // A link naming a channel is an explicit join (QA.23); after `ServerSynced`, which
        // resets the sound hub's view of the own state.
        if link_join
            && let Some(voice) = lock(&self.core)
                .slots
                .get(&server_id)
                .map(|slot| slot.self_voice)
        {
            self.events
                .sounds()
                .voice_join(None, voice.mute, voice.deaf);
        }
        // On every sync, reconnects included: the server forgets the avatar of an
        // unregistered user whenever the connection ends (QA.10). In the background:
        // encoding and the server's confirmation must not hold up the event pump.
        self.rt.spawn(crate::profile::restore_avatar(
            ctx.clone(),
            self.data_dir.join(crate::profile::DEFAULT_AVATAR_FILE),
        ));
        // Effective permissions decide which channels the UI offers to join (FR-1.16).
        for id in handle.with_state(|s| s.channels.keys().copied().collect::<Vec<_>>()) {
            let _ = handle.query_permissions(id);
        }

        let mut bitrate = BitrateController::new();
        let mut oversized_seen = 0;
        bitrate.set_user_cap(self.user_cap_bps());
        bitrate.set_server_max_bandwidth(handle.with_state(|s| s.server.max_bandwidth));
        let epoch = Instant::now();
        let mut tick = tokio::time::interval(BITRATE_INTERVAL);
        let mut packets: Option<PacketSender> = None;
        let mut permissions = batch::PermissionBatch::default();
        loop {
            let permissions_due = permissions.deadline();
            let voice_event = async {
                match voice_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                event = events.recv() => {
                    let event = match event {
                        Some(Event::Disconnected(reason)) => {
                            return self.end_connection(generation, server_id, reason);
                        }
                        None => return self.end_connection(generation, server_id, DisconnectReason::Closed),
                        Some(event) => event,
                    };
                    match &event {
                        Event::ServerConfigUpdated(info) => {
                            bitrate.set_server_max_bandwidth(info.max_bandwidth);
                        }
                        Event::PermissionsUpdated { channel_id } => {
                            permissions.add(*channel_id, tokio::time::Instant::now());
                        }
                        _ => {}
                    }
                    self.on_event(&ctx, transport.as_deref(), event);
                }
                _ = tokio::time::sleep_until(permissions_due.unwrap_or_else(tokio::time::Instant::now)),
                    if permissions_due.is_some() =>
                {
                    if let Some(channels) = permissions.take() {
                        self.emit_permissions(&ctx, &channels);
                    }
                }
                Some(event) = voice_event => match event {
                    // Only the pipeline's server plays voice; read-only servers send none.
                    VoiceEvent::Packet(packet) if self.is_audio_server(server_id) => {
                        self.voice.packets_received.fetch_add(1, Ordering::Relaxed);
                        if let Some(sending) = self.positional.on_voice_packet(
                            packet.session,
                            packet.positional.is_some(),
                            Instant::now(),
                        ) {
                            ctx.events.emit(EngineEvent::PositionalSendingChanged {
                                session: packet.session,
                                sending,
                            });
                        }
                        if !self.voice.deaf.load(Ordering::Relaxed)
                            && let (Some(tx), Some(p)) = (packets.as_mut(), voice::playback_packet(&packet))
                        {
                            tx.push(p);
                        }
                    }
                    VoiceEvent::Packet(_) => {}
                    VoiceEvent::TransportChanged(t) => {
                        tracing::info!(transport = ?t, "voice transport changed");
                        bitrate.set_transport(match t {
                            Transport::Udp => BitrateTransport::Udp,
                            Transport::Tcp => BitrateTransport::TcpTunnel,
                        });
                        ctx.events.emit(EngineEvent::TransportChanged(match t {
                            Transport::Udp => VoiceTransportKind::Udp,
                            Transport::Tcp => VoiceTransportKind::Tcp,
                        }));
                    }
                },
                Some(PumpCmd::Packets(sender)) = pump_rx.recv() => packets = sender,
                _ = tick.tick() => {
                    crate::watch_party::on_tick(&self.watch_parties, &ctx);
                    crate::screen_share::on_tick(&self.screen_shares, &self.mumcord, &ctx);
                    let audio = self.is_audio_server(server_id);
                    if audio {
                        lock(&self.core).sample_audio_health(&self.voice);
                        for session in self.positional.expire(Instant::now()) {
                            ctx.events.emit(EngineEvent::PositionalSendingChanged {
                                session,
                                sending: false,
                            });
                        }
                    }
                                bitrate.set_user_cap(self.user_cap_bps());
                    if audio {
                        self.publish_bitrate_max(Some(bitrate.max_possible()));
                    }
                    if let Some(t) = &transport {
                        let stats = t.stats();
                        handle.report_voice_link(
                            stats.rtt.map(|rtt| mumcord_protocol::UdpPing {
                                avg_ms: rtt.as_secs_f32() * 1000.0,
                                packets: stats.crypt.good,
                            }),
                            stats.crypt,
                        );
                        bitrate.on_oversized_packets(stats.oversized.saturating_sub(oversized_seen));
                        oversized_seen = stats.oversized;
                        let target = bitrate.update(
                            BitrateStats {
                                loss_fraction: stats.loss,
                                rtt_ms: stats.rtt.map(|d| d.as_millis() as u32),
                            },
                            epoch.elapsed().as_millis() as u64,
                        );
                        if audio {
                            self.voice.bitrate_bps.store(target.bitrate_bps, Ordering::Relaxed);
                            self.voice.loss_percent.store(target.loss_percent.into(), Ordering::Relaxed);
                        }
                    }
                }
            }
        }
    }

    fn end_connection(
        self: &Arc<Self>,
        generation: u64,
        server_id: ServerId,
        reason: DisconnectReason,
    ) -> Ended {
        let mut core = lock(&self.core);
        if core
            .slots
            .get(&server_id)
            .is_none_or(|slot| slot.generation != generation)
        {
            return Ended::Stale;
        }
        if core.audio_server == Some(server_id) {
            self.stop_pipeline(&mut core);
        }
        if let Some(slot) = core.slots.get_mut(&server_id) {
            slot.session = None;
        }
        // A read-only server's pipeline moves on; the voice server keeps voice (FR-2.8).
        self.rebind_audio(&mut core);
        drop(core);
        crate::history_sync::on_disconnect(&self.history_sync, server_id);
        self.forget_password_attempt(server_id);
        match reason {
            DisconnectReason::Requested => Ended::Final(None),
            DisconnectReason::Kicked { reason, ban, .. } => {
                tracing::info!(server_id, ban, "removed by the server");
                let what = if ban { "banned" } else { "kicked" };
                Ended::Final(Some(EngineError::new(
                    ErrorKind::PermissionDenied,
                    format!("{what}: {}", reason.unwrap_or_default()),
                )))
            }
            DisconnectReason::Closed | DisconnectReason::Timeout | DisconnectReason::Error(_) => {
                tracing::info!(server_id, "connection lost");
                Ended::Lost
            }
        }
    }

    /// Restores followed channels. A fresh connect stays where the server put the user,
    /// read-only (self-muted and deafened), unless a link names a channel: that is an explicit
    /// join with the user's last own mute/deafen choice. A reconnect rejoins the last channel and restores the own
    /// mute/deafen state (continuity). At most three rate-limited messages, inside the
    /// server's burst. Returns the followed channel ids, the channel to offer for
    /// rejoining (QA.9) and the new voice state (`None`: the slot's stays).
    fn post_sync(
        &self,
        server_id: ServerId,
        handle: &Handle,
        arrival: Arrival,
    ) -> (Vec<u32>, Option<u32>, Option<SelfVoice>) {
        let (last, followed_paths) = {
            let store = lock(&self.store);
            let last = store
                .server(server_id)
                .ok()
                .flatten()
                .and_then(|s| s.last_channel);
            let followed = store.followed_channels(server_id).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "reading followed channels failed");
                Vec::new()
            });
            (last, followed)
        };
        let mut offer_path = None;
        let landing_voice = || self.landing_voice(server_id);
        let (target, voice) = match arrival {
            Arrival::Fresh(Some(path)) => {
                let mut voice = SelfVoice::read_only();
                voice.join(self.own_voice());
                (Some(path), Some(voice))
            }
            Arrival::Fresh(None) => (None, Some(landing_voice())),
            Arrival::FreshWithRejoin(path) => {
                offer_path = Some(split_path(&path));
                (None, Some(landing_voice()))
            }
            Arrival::Reconnect => (last.as_deref().map(split_path), None),
        };
        let (join, offer, followed) = handle.with_state(|s| {
            let enterable = |id: &u32| {
                s.channels
                    .get(id)
                    .is_some_and(|c| c.can_enter != Some(false))
            };
            let own = s.own_user().map(|u| u.channel_id);
            let join = target
                .as_deref()
                .and_then(|p| resolve_path(s, p))
                .filter(|id| Some(*id) != own)
                .filter(enterable);
            let offer = offer_path
                .as_deref()
                .and_then(|p| resolve_path(s, p))
                .filter(enterable);
            let followed: Vec<u32> = followed_paths
                .iter()
                .filter_map(|f| resolve_path(s, &split_path(&f.channel_path)))
                .collect();
            (join, offer, followed)
        });
        if let Some(id) = join
            && let Err(e) = handle.join_channel(id)
        {
            tracing::warn!(error = %e, "joining the channel failed");
        }
        let followed =
            if !followed.is_empty() && handle.with_state(|s| s.server.supports_listening()) {
                match handle.listen(&followed) {
                    Ok(()) => followed,
                    Err(e) => {
                        tracing::warn!(error = %e, "restoring followed channels failed");
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };
        let sending = voice.unwrap_or_else(|| {
            lock(&self.core)
                .slots
                .get(&server_id)
                .map_or(SelfVoice::read_only(), |slot| slot.self_voice)
        });
        // A fresh login starts unmuted on the server, so only a set state needs sending.
        let sent = if sending.deaf {
            handle.set_self_deaf(true)
        } else if sending.mute {
            handle.set_self_mute(true)
        } else {
            Ok(())
        };
        if let Err(e) = sent {
            tracing::warn!(error = %e, "restoring mute state failed");
        }
        (followed, offer, voice)
    }

    fn on_event(&self, ctx: &SessionCtx, transport: Option<&VoiceTransport>, event: Event) {
        let server_id = ctx.server_id;
        match &event {
            Event::ChannelUpdated(channel) => {
                let view = ctx.handle.with_state(|s| channel_view(s, channel));
                ctx.events.emit(EngineEvent::ChannelUpserted(view));
            }
            Event::ChannelRemoved(channel) => {
                ctx.events.emit(EngineEvent::ChannelRemoved(channel.id));
            }
            Event::UserUpdated(change) => {
                let user = &change.user;
                let mut core = lock(&self.core);
                let audio = core.audio_server == Some(server_id);
                let setting = core
                    .slots
                    .get(&server_id)
                    .map(|slot| slot.people.setting(&lock(&self.store), server_id, user));
                if change.joined
                    && audio
                    && let Some(setting) = setting.filter(|s| !people::is_default(*s))
                    && let Some(pipeline) = core.pipeline.as_mut()
                {
                    pipeline.apply_person(user.session, setting.volume, setting.local_mute);
                }
                drop(core);
                let own = ctx.handle.with_state(|s| s.session) == Some(user.session);
                if change.joined {
                    crate::now_playing::on_user_joined(&self.now_playing, ctx, user.session);
                    crate::now_gaming::on_user_joined(&self.now_gaming, ctx, user.session);
                    crate::mumcord::on_user_joined(&self.mumcord, ctx, user.session);
                }
                crate::history_sync::on_user(&self.history_sync, ctx, user);
                if change.joined || change.moved_from.is_some() {
                    crate::watch_party::on_user_moved(
                        &self.watch_parties,
                        &self.mumcord,
                        ctx,
                        user.session,
                    );
                    crate::screen_share::on_user_moved(
                        &self.screen_shares,
                        &self.mumcord,
                        ctx,
                        user.session,
                    );
                }
                if own && change.moved_from.is_some() {
                    self.remember_channel(ctx, user.channel_id);
                    self.on_own_channel(ctx, user.channel_id);
                }
                if let Some(setting) = setting {
                    ctx.events.emit(EngineEvent::UserUpserted(
                        self.user_view_of(server_id, user, setting),
                    ));
                }
            }
            Event::UserRemoved { user, .. } => {
                let mut core = lock(&self.core);
                if let Some(slot) = core.slots.get_mut(&server_id) {
                    slot.people.forget(user.session);
                }
                if core.audio_server == Some(server_id)
                    && let Some(pipeline) = core.pipeline.as_mut()
                {
                    pipeline.forget(user.session);
                }
                let audio = core.audio_server == Some(server_id);
                drop(core);
                crate::now_playing::on_user_removed(&self.now_playing, server_id, user.session);
                crate::now_gaming::on_user_removed(&self.now_gaming, server_id, user.session);
                crate::mumcord::on_user_removed(&self.mumcord, server_id, user.session);
                crate::history_sync::on_user_removed(&self.history_sync, server_id, user.session);
                crate::watch_party::on_user_removed(&self.watch_parties, ctx, user.session);
                crate::screen_share::on_user_removed(
                    &self.screen_shares,
                    &self.mumcord,
                    ctx,
                    user.session,
                );
                self.full_images.forget(server_id, user.session);
                self.direct.forget(server_id, user.session);
                if audio {
                    self.positional.on_user_removed(user.session);
                }
                crate::stats::on_user_removed(&self.user_stats, server_id, user.session);
                ctx.events.emit(EngineEvent::UserRemoved(user.session));
            }
            Event::PermissionDenied(denied) => {
                ctx.events.emit(EngineEvent::ServerDenied {
                    error: convert::denial_error(denied),
                    channel_id: denied.channel_id,
                });
                self.on_channel_denied(ctx, denied);
            }
            // Collected by the session loop and emitted in batches (`emit_permissions`).
            Event::PermissionsUpdated { .. } => {}
            Event::CryptSetup(setup) => {
                if let Some(t) = transport {
                    t.on_crypt_setup(setup.clone());
                }
            }
            Event::UdpTunnel(packet) => {
                if let Some(t) = transport {
                    t.on_tunnel(packet.clone());
                }
            }
            Event::PluginData(data) => {
                crate::now_playing::on_plugin_data(&self.now_playing, ctx, data);
                crate::now_gaming::on_plugin_data(&self.now_gaming, ctx, data);
                if let Some(session) = crate::mumcord::on_plugin_data(&self.mumcord, ctx, data) {
                    self.republish_user(ctx, session);
                    crate::watch_party::on_peer_known(
                        &self.watch_parties,
                        &self.mumcord,
                        ctx,
                        session,
                    );
                    crate::screen_share::on_peer_known(
                        &self.screen_shares,
                        &self.mumcord,
                        ctx,
                        session,
                    );
                }
                crate::watch_party::on_plugin_data(&self.watch_parties, ctx, data);
                crate::screen_share::on_plugin_data(&self.screen_shares, &self.mumcord, ctx, data);
                crate::history_sync::on_plugin_data(&self.history_sync, ctx, data);
                crate::full_image::on_plugin_data(&self.full_images, &self.previews, ctx, data);
                self.direct.on_plugin_data(ctx, data);
            }
            Event::UserStats(stats) => crate::stats::on_stats(&self.user_stats, ctx, stats),
            Event::TextMessage(_) | Event::ServerConfigUpdated(_) | Event::Disconnected(_) => {}
        }
        extension::on_event(ctx, &event);
    }

    /// The person as the UI shows them, with the Mumcord features they announced there.
    fn user_view_of(
        &self,
        server_id: ServerId,
        user: &mumcord_protocol::User,
        setting: mumcord_store::PersonSetting,
    ) -> UserView {
        let peers = lock(&self.mumcord);
        let features = peers.get(&server_id).and_then(|p| p.features(user.session));
        user_view(user, setting, features)
    }

    /// Emits the person again, e.g. after they were recognised as a Mumcord user.
    fn republish_user(&self, ctx: &SessionCtx, session: u32) {
        let Some(user) = ctx.handle.with_state(|s| s.users.get(&session).cloned()) else {
            return;
        };
        let setting = {
            // Lock order (see module docs): taking `store` first deadlocks against `snapshot`.
            let core = lock(&self.core);
            let Some(slot) = core.slots.get(&ctx.server_id) else {
                return;
            };
            slot.people
                .setting(&lock(&self.store), ctx.server_id, &user)
        };
        ctx.events.emit(EngineEvent::UserUpserted(self.user_view_of(
            ctx.server_id,
            &user,
            setting,
        )));
    }

    /// One `ChannelsUpserted` for a settled burst of permission answers (see `batch`).
    fn emit_permissions(&self, ctx: &SessionCtx, channels: &batch::Channels) {
        let views = ctx.handle.with_state(|s| {
            s.channels
                .values()
                .filter(|c| channels.contains(c.id))
                .map(|c| channel_view(s, c))
                .collect::<Vec<_>>()
        });
        if !views.is_empty() {
            ctx.events.emit(EngineEvent::ChannelsUpserted(views));
        }
    }

    /// Remembers the own channel for the next connection (FR-1.14).
    fn remember_channel(&self, ctx: &SessionCtx, channel_id: u32) {
        let Some(path) = ctx.handle.with_state(|s| channel_path(s, channel_id)) else {
            return;
        };
        if let Err(e) = lock(&self.store).set_last_channel(ctx.server_id, Some(&join_path(&path))) {
            tracing::warn!(error = %e, "saving the last channel failed");
        }
        self.remember_voice_channel(&lock(&self.core));
    }

    /// While voice is on, remembers the voice server's channel so a restart can offer to
    /// rejoin it (QA.9). Unlike `last_channel` it is never the read-only channel of a fresh
    /// connect.
    fn remember_voice_channel(&self, core: &Core) {
        let Some(server_id) = core.voice_server else {
            return;
        };
        let Some(slot) = core.slots.get(&server_id) else {
            return;
        };
        let Some(session) = &slot.session else {
            return;
        };
        if slot.self_voice.own().is_none() {
            return;
        }
        let Some(path) = session
            .handle
            .with_state(|s| s.own_user().and_then(|u| channel_path(s, u.channel_id)))
        else {
            return;
        };
        if let Err(e) = lock(&self.store).set_voice_channel(server_id, &join_path(&path)) {
            tracing::warn!(error = %e, "saving the voice channel failed");
        }
    }

    /// Remembers `channel_id` as the voice channel if `server_id` has voice (QA.9).
    fn remember_voice_channel_at(&self, server_id: ServerId, channel_id: u32) {
        let core = lock(&self.core);
        if core.voice_server != Some(server_id) {
            return;
        }
        let Some(path) = core
            .session(server_id)
            .ok()
            .and_then(|s| s.handle.with_state(|st| channel_path(st, channel_id)))
        else {
            return;
        };
        if let Err(e) = lock(&self.store).set_voice_channel(server_id, &join_path(&path)) {
            tracing::warn!(error = %e, "saving the voice channel failed");
        }
    }

    /// Voice ended other than by quitting the app: nothing to offer on the next start.
    fn forget_voice_channel(&self) {
        if let Err(e) = lock(&self.store).clear_voice_channels() {
            tracing::warn!(error = %e, "clearing the voice channel failed");
        }
    }

    fn snapshot(&self, core: &Core, server_id: ServerId) -> Option<Snapshot> {
        let slot = core.slots.get(&server_id)?;
        let session = slot.session.as_ref()?;
        let store = lock(&self.store);
        session.handle.with_state(|s: &ServerState| {
            Some(Snapshot {
                server_id,
                self_session: s.session?,
                info: ServerInfoView {
                    default_channel_id: session.default_channel_id,
                    ..convert::server_info_view(s)
                },
                channels: s.channels.values().map(|c| channel_view(s, c)).collect(),
                users: s
                    .users
                    .values()
                    .map(|u| {
                        let mut view = self.user_view_of(
                            server_id,
                            u,
                            slot.people.setting(&store, server_id, u),
                        );
                        // The server's echo of the mute/deafen just sent may still be on its way.
                        if Some(u.session) == s.session {
                            view.self_mute = slot.self_voice.mute;
                            view.self_deaf = slot.self_voice.deaf;
                        }
                        view
                    })
                    .collect(),
                followed_channel_ids: followed_ids(&store, server_id, s),
                unread: Vec::new(),
                read_only: slot.self_voice.own().is_none(),
                rejoin_channel_id: session.rejoin_offer,
            })
        })
    }

    /// Voice state of a fresh connect without a link: read-only, or (setting "Connect muted
    /// until I join a channel" off, QA.22) the last own mute/deafen choice right away, unless
    /// another server already has voice (FR-2.2).
    fn landing_voice(&self, server_id: ServerId) -> SelfVoice {
        let connect_muted = lock(&self.store)
            .settings()
            .map(|s| s.connect_muted)
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "reading the settings failed");
                true
            });
        let voice_elsewhere = lock(&self.core)
            .voice_server
            .is_some_and(|id| id != server_id);
        fresh_voice(connect_muted || voice_elsewhere, self.own_voice())
    }

    /// The remembered own mute/deafen choice; neither if it cannot be read.
    fn own_voice(&self) -> OwnVoice {
        lock(&self.store).own_voice().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "reading the remembered mute state failed");
            OwnVoice::default()
        })
    }

    /// Mute/deafen without a server (user panel, tray, shortcuts): the voice server, else the
    /// only connected server; with several read-only servers only the choice is remembered.
    fn change_own_voice(self: &Arc<Self>, change: impl FnOnce(&mut SelfVoice)) -> EngineResult<()> {
        let target = {
            let core = lock(&self.core);
            core.voice_server.or_else(|| {
                let mut connected = core
                    .slots
                    .iter()
                    .filter(|(_, slot)| slot.session.is_some())
                    .map(|(id, _)| *id);
                connected.next().filter(|_| connected.next().is_none())
            })
        };
        if let Some(server_id) = target {
            return self.change_self_voice(server_id, change);
        }
        let mut voice = SelfVoice::read_only();
        voice.join(self.own_voice());
        change(&mut voice);
        if let Some(own) = voice.own() {
            lock(&self.store).save_own_voice(own).map_err(store_error)?;
        }
        Ok(())
    }

    fn change_self_voice(
        self: &Arc<Self>,
        server_id: ServerId,
        change: impl FnOnce(&mut SelfVoice),
    ) -> EngineResult<()> {
        let mut core = lock(&self.core);
        let slot = core
            .slots
            .get_mut(&server_id)
            .ok_or_else(EngineError::not_connected)?;
        let before = slot.self_voice;
        change(&mut slot.self_voice);
        let after = slot.self_voice;
        let leaves_read_only = before.own().is_none() && after.own().is_some();
        if leaves_read_only && let Some(session) = slot.session.as_mut() {
            session.rejoin_offer = None;
        }
        // The user's own choice while voice is on is remembered, never the forced read-only
        // state (QA.7). Leaving read-only by undeafening or unmuting is such a choice too:
        // otherwise the next Join would restore an older, possibly deafened, setting.
        if let Some(own) = after.own()
            && before.own() != Some(own)
            && let Err(e) = lock(&self.store).save_own_voice(own)
        {
            tracing::warn!(error = %e, "remembering the mute state failed");
        }
        if leaves_read_only {
            self.take_voice(&mut core, server_id);
            self.remember_voice_channel(&core);
        }
        if core.audio_server == Some(server_id) {
            self.voice.muted.store(after.mute, Ordering::Relaxed);
            self.voice.deaf.store(after.deaf, Ordering::Relaxed);
        }
        let Ok(session) = core.session(server_id) else {
            return Ok(());
        };
        let handle = &session.handle;
        let result = if before.deaf != after.deaf {
            // The server keeps self-mute when undeafening; unmute explicitly if needed.
            handle.set_self_deaf(after.deaf).and_then(|()| {
                if !after.deaf && !after.mute {
                    handle.set_self_mute(false)
                } else {
                    Ok(())
                }
            })
        } else if before.mute != after.mute {
            handle.set_self_mute(after.mute)
        } else {
            Ok(())
        };
        result.map_err(convert::command_error)
    }

    fn change_person(
        &self,
        server_id: ServerId,
        session: u32,
        change: impl FnOnce(&mut mumcord_store::PersonSetting),
    ) -> EngineResult<()> {
        let mut core = lock(&self.core);
        let user = Self::person_in_session(&core, server_id, session)?;
        let setting = {
            let store = lock(&self.store);
            let slot = core
                .slots
                .get_mut(&server_id)
                .ok_or_else(EngineError::not_connected)?;
            let mut setting = slot.people.setting(&store, server_id, &user);
            change(&mut setting);
            slot.people.save(&store, server_id, &user, setting)?;
            setting
        };
        if core.audio_server == Some(server_id)
            && let Some(pipeline) = core.pipeline.as_mut()
        {
            pipeline.apply_person(session, setting.volume, setting.local_mute);
        }
        drop(core);
        self.events
            .for_server(server_id)
            .emit(EngineEvent::UserUpserted(
                self.user_view_of(server_id, &user, setting),
            ));
        Ok(())
    }

    /// The person with this session on the server.
    fn person_in_session(
        core: &Core,
        server_id: ServerId,
        session: u32,
    ) -> EngineResult<mumcord_protocol::User> {
        core.session(server_id)?
            .handle
            .with_state(|st| st.users.get(&session).cloned())
            .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown person"))
    }

    /// Starts the audio devices on first use.
    async fn ensure_audio(self: &Arc<Self>) -> EngineResult<()> {
        let _starting = self.audio_start.lock().await;
        if lock(&self.core).audio.is_some() {
            return Ok(());
        }
        let settings = lock(&self.store).settings().map_err(store_error)?;
        let events = self.events.clone();
        let audio = self
            .rt
            .spawn_blocking(move || {
                Audio::start(settings.input_device, settings.output_device, events)
            })
            .await
            .map_err(|e| EngineError::internal(e.to_string()))??;
        let mut core = lock(&self.core);
        if core.audio.is_none() {
            core.audio = Some(audio);
        }
        self.start_pipeline(&mut core);
        Ok(())
    }

    /// Starts voice for the pipeline's server if devices are free; otherwise it starts once
    /// the devices are up or the mic test is over.
    fn start_pipeline(self: &Arc<Self>, core: &mut Core) {
        if core.pipeline.is_some() || core.mic_test.is_some() {
            return;
        }
        let Some(server_id) = core.audio_server else {
            return;
        };
        let Some(slot) = core.slots.get(&server_id) else {
            return;
        };
        let Some(session) = slot.session.as_ref() else {
            return;
        };
        let Some(transport) = session.transport.clone() else {
            return;
        };
        let Some(own) = session.handle.with_state(|s| s.session) else {
            return;
        };
        let Some(audio) = core.audio.as_mut() else {
            // Devices are started off the async thread; this call comes back here.
            let inner = self.clone();
            self.rt.spawn(async move {
                if let Err(e) = inner.ensure_audio().await {
                    tracing::error!(error = %e, "starting audio failed");
                }
            });
            return;
        };
        match VoicePipeline::start(
            &self.rt,
            audio,
            self.controls.clone(),
            transport,
            self.voice.clone(),
            self.events.for_server(server_id),
            own,
        ) {
            Ok((mut pipeline, packets)) => {
                let _ = session.pump.send(PumpCmd::Packets(Some(packets)));
                let store = lock(&self.store);
                let users = session
                    .handle
                    .with_state(|s| s.users.values().cloned().collect::<Vec<_>>());
                for user in users {
                    let setting = slot.people.setting(&store, server_id, &user);
                    if !people::is_default(setting) {
                        pipeline.apply_person(user.session, setting.volume, setting.local_mute);
                    }
                }
                drop(store);
                core.pipeline = Some(pipeline);
                self.meter.voice.store(true, Ordering::Relaxed);
            }
            Err(e) => tracing::error!(error = %e, "starting voice failed"),
        }
    }

    fn stop_pipeline(&self, core: &mut Core) {
        if let Some(session) = core.audio_server.and_then(|id| core.session(id).ok()) {
            let _ = session.pump.send(PumpCmd::Packets(None));
        }
        if let (Some(pipeline), Some(audio)) = (core.pipeline.take(), core.audio.as_mut()) {
            pipeline.stop(audio);
        }
        self.meter.voice.store(false, Ordering::Relaxed);
    }

    async fn watch_mic_test(self: Arc<Self>) {
        let mut announced = types::MicTestPhase::Recording;
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut core = lock(&self.core);
            match core.mic_test.as_ref().map(MicTest::phase) {
                None => return,
                Some(DspPhase::Recording) => {}
                Some(DspPhase::Playing) => {
                    self.meter.test.store(false, Ordering::Relaxed);
                    if announced != types::MicTestPhase::Playing {
                        announced = types::MicTestPhase::Playing;
                        self.events
                            .emit(EngineEvent::MicTestPhaseChanged(announced));
                    }
                }
                Some(DspPhase::Done) => {
                    self.finish_mic_test(&mut core);
                    return;
                }
            }
        }
    }

    fn finish_mic_test(self: &Arc<Self>, core: &mut Core) {
        let Some(test) = core.mic_test.take() else {
            return;
        };
        self.events
            .emit(EngineEvent::MicTestPhaseChanged(types::MicTestPhase::Done));
        self.meter.test.store(false, Ordering::Relaxed);
        match (test.finish(), core.audio.as_mut()) {
            (Some((source, sink)), Some(audio)) => audio.return_devices(source, sink),
            _ => tracing::error!("mic test thread panicked; audio devices lost until restart"),
        }
        self.start_pipeline(core);
        self.release_audio_if_idle(core);
    }

    /// The identity from secure storage, cached after the first read.
    async fn identity(&self) -> EngineResult<Option<Identity>> {
        if let Some(cached) = lock(&self.identity).clone() {
            return Ok(cached);
        }
        let secrets = self.secrets.clone();
        let loaded = self
            .rt
            .spawn_blocking(move || secrets.load_identity())
            .await
            .map_err(|e| EngineError::internal(e.to_string()))?
            .map_err(store_error)?;
        *lock(&self.identity) = Some(loaded.clone());
        Ok(loaded)
    }
}

/// Opens the UDP voice transport; voice is unavailable (not fatal) if this fails.
async fn start_voice(
    handle: &Handle,
    host: &str,
    port: u16,
) -> Option<(VoiceTransport, mpsc::UnboundedReceiver<VoiceEvent>)> {
    let addr = match tokio::net::lookup_host((host, port)).await {
        Ok(mut addrs) => addrs.next()?,
        Err(e) => {
            tracing::warn!(error = %e, "resolving the voice address failed");
            return None;
        }
    };
    match VoiceTransport::start(handle.clone(), addr).await {
        Ok(started) => Some(started),
        Err(e) => {
            tracing::error!(error = %e, "starting the voice transport failed");
            None
        }
    }
}

/// The refusal for connecting (or marking) more servers than the limit allows (FR-2.1).
pub(crate) fn limit_error(max: u8) -> EngineError {
    EngineError::new(
        ErrorKind::ServerLimitReached,
        format!(
            "at most {max} servers can be connected at the same time; \
             change the limit in Settings → Servers"
        ),
    )
}

/// Servers to connect at app start (FR-2.7): "Always connect at start" first, then the
/// servers connected at the last quit, most recent first (the voice server before the others,
/// then the saved order), up to `max`.
fn start_servers(servers: &[SavedServer], max: u8) -> Vec<ServerId> {
    let mut previous: Vec<&SavedServer> = servers
        .iter()
        .filter(|s| !s.always_connect_at_start && s.connected_at_quit.is_some())
        .collect();
    // Stable: equal servers keep the saved order.
    previous.sort_by_key(|s| (Reverse(s.connected_at_quit), !s.was_voice_server_at_quit));
    servers
        .iter()
        .filter(|s| s.always_connect_at_start)
        .chain(previous)
        .map(|s| s.id)
        .take(usize::from(max))
        .collect()
}

/// Whether an explicit join turns voice on in a new way and so gets the join sound (QA.23):
/// moving to another channel, or leaving read-only in the current one. Re-joining the channel
/// one already talks in changes nothing audible.
fn joins_voice(moves: bool, voice: SelfVoice) -> bool {
    moves || voice.own().is_none()
}

/// The voice state a fresh connect without a link starts with (QA.22).
fn fresh_voice(connect_muted: bool, own: OwnVoice) -> SelfVoice {
    let mut voice = SelfVoice::read_only();
    if !connect_muted {
        voice.join(own);
    }
    voice
}

#[cfg(test)]
mod declined_kind_tests {
    use super::*;

    #[test]
    fn fresh_connect_is_read_only_unless_the_setting_is_off() {
        let own = OwnVoice {
            mute: true,
            deaf: false,
        };
        let on = fresh_voice(true, own);
        assert!(on.mute && on.deaf && on.own().is_none());
        let off = fresh_voice(false, own);
        assert_eq!(off.own(), Some(own), "last own choice, no read-only hint");
        assert!(off.mute && !off.deaf);
    }

    fn saved(id: ServerId, always: bool, quit_at: Option<i64>, voice: bool) -> SavedServer {
        SavedServer {
            id,
            label: format!("s{id}"),
            address: "localhost".into(),
            port: 64738,
            username: "me".into(),
            last_channel: None,
            save_history: true,
            sort_order: id,
            always_connect_at_start: always,
            connected_at_quit: quit_at,
            was_voice_server_at_quit: voice,
            notifications_muted: false,
            theme_id: None,
        }
    }

    #[test]
    fn start_connects_marked_servers_first_then_the_most_recent_up_to_the_limit() {
        let servers = [
            saved(1, false, Some(100), false),
            saved(2, false, Some(100), true),
            saved(3, true, None, false),
            saved(4, false, None, false),
            saved(5, false, Some(50), false),
        ];
        assert_eq!(start_servers(&servers, 5), [3, 2, 1, 5]);
        assert_eq!(
            start_servers(&servers, 2),
            [3, 2],
            "AC-2.6: C plus the most recent"
        );
        assert_eq!(start_servers(&servers, 1), [3]);
    }

    #[test]
    fn limit_error_names_limit_and_setting() {
        let error = limit_error(2);
        assert_eq!(error.kind, ErrorKind::ServerLimitReached);
        assert!(error.message.contains('2') && error.message.contains("Settings → Servers"));
    }

    #[test]
    fn declined_changed_cert_keeps_its_kind() {
        assert_eq!(declined_kind(true), ErrorKind::CertificateChanged);
        assert_eq!(declined_kind(false), ErrorKind::CertificateRejected);
    }

    #[test]
    fn join_sound_only_when_voice_changes() {
        let read_only = SelfVoice::read_only();
        let mut talking = SelfVoice::read_only();
        talking.join(OwnVoice::default());
        assert!(joins_voice(true, read_only), "joining another channel");
        assert!(
            joins_voice(false, read_only),
            "leaving read-only by joining here"
        );
        assert!(
            joins_voice(true, talking),
            "moving to another channel with voice on"
        );
        assert!(
            !joins_voice(false, talking),
            "re-joining the channel one talks in"
        );
    }
}

#[cfg(test)]
mod password_override_tests {
    use super::*;
    use crate::{PasswordChange, SecretStore, ServerFields};

    // Single-threaded runtime: the connect task cannot run (and close the slot) before the
    // assertions, so the override is observable right after the call.
    #[tokio::test]
    async fn retry_password_is_kept_for_this_connect_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (engine, _rx) = Engine::new(
            dir.path(),
            SecretStore::in_memory(),
            tokio::runtime::Handle::current(),
        )
        .expect("engine");
        engine
            .identity_create("Tester".into())
            .await
            .expect("identity");
        let server = engine
            .server_add(ServerFields {
                label: "s".into(),
                address: "127.0.0.1".into(),
                port: 1,
                username: "me".into(),
                password: PasswordChange::Set("saved".into()),
            })
            .await
            .expect("server");

        engine
            .connect_with_password(server.id, None, Some("typed".into()))
            .expect("connect");
        assert_eq!(
            lock(&engine.inner.password_overrides).get(&server.id),
            Some(&"typed".to_owned())
        );

        // The slot closing (disconnect or fatal failure) drops the override: the next plain
        // connect uses the saved password again.
        engine
            .inner
            .close_slot(&mut lock(&engine.inner.core), server.id, None);
        assert!(lock(&engine.inner.password_overrides).is_empty());
        engine.connect(server.id, None).expect("plain connect");
        assert!(lock(&engine.inner.password_overrides).is_empty());
    }
}
