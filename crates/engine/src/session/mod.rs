//! The engine object and the connection lifecycle (FR-1.9, 1.10, 1.13, 1.14, 1.19, 1.22,
//! 1.24–1.26).
//!
//! One server is connected at a time. A supervisor task per `connect` runs the state machine
//! of spec §7: connect → sync → event pump → (on loss) reconnect with backoff. Each `connect`
//! or `disconnect` bumps a generation number; tasks of an older generation can no longer
//! change state, so cancelling is race-free without waiting for tasks to end.
//!
//! Lock order: `core` before `store`. Neither lock is held across an `.await`.

mod batch;
pub mod convert;
mod description;
pub mod extension;
mod local;
mod machine;
mod types;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
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
use mumcord_store::{Identity, OwnVoice, SecretStore, ServerId, Store};
use tokio::runtime::Handle as Runtime;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub use machine::MAX_RECONNECT_ATTEMPTS;
pub use types::*;

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

/// Event output of the engine; cheap to clone. Extensions (B14) emit through it too.
#[derive(Clone)]
pub struct Events(mpsc::UnboundedSender<EngineEvent>, Arc<SoundHub>);

impl Events {
    /// Notification sounds (B26); also reachable for `Sound::Message` from chat code.
    pub fn sounds(&self) -> &Arc<SoundHub> {
        &self.1
    }

    pub fn emit(&self, event: EngineEvent) {
        self.1.observe(&event);
        // A closed receiver means the app is shutting down; nobody is left to inform.
        let _ = self.0.send(event);
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
    core: Mutex<Core>,
    /// Link previews and video cards for chat links (FR-1.49, FR-1.50); also the image cache
    /// of full-size chat images.
    previews: Arc<crate::web::LinkPreviews>,
    /// Full-size image transfers with other Mumcord clients (FR-1.60c).
    full_images: Arc<crate::full_image::transfer::Transfers>,
    /// Shared "now playing" status (FR-1.58).
    now_playing: Mutex<crate::now_playing::NowPlaying>,
    /// Shared "now gaming" status (FR-1.59).
    now_gaming: Mutex<crate::now_gaming::NowGaming>,
    /// Other Mumcord users on the server (FR-1.61).
    mumcord: Mutex<crate::mumcord::Peers>,
    /// History sync with other Mumcord users (FR-1.60).
    history_sync: Arc<Mutex<crate::history_sync::HistorySync>>,
}

#[derive(Default)]
struct Core {
    machine: Machine,
    generation: u64,
    supervisor: Option<JoinHandle<()>>,
    session: Option<Session>,
    pending_trust: Option<PendingTrust>,
    self_voice: SelfVoice,
    people: People,
    audio: Option<Audio>,
    pipeline: Option<VoicePipeline>,
    mic_test: Option<MicTest>,
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
    server_id: ServerId,
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
    Failure(Failure),
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
        E::SecureStorage(_) | E::CorruptStoredIdentity => ErrorKind::SecureStorageUnavailable,
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
    ) -> EngineResult<(Self, mpsc::UnboundedReceiver<EngineEvent>)> {
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
        let events = Events(tx, SoundHub::from_store(store.clone()));
        let controls = Arc::new(DspControls::default());
        local::apply_dsp(&controls, &settings);
        let meter = Arc::new(Meter::default());
        voice::spawn_meter(&rt, controls.clone(), meter.clone(), events.clone());
        let inner = Arc::new(Inner {
            rt,
            data_dir: data_dir.to_path_buf(),
            store,
            secrets: Arc::new(secrets),
            identity: Mutex::new(None),
            events,
            controls,
            meter,
            voice: Arc::new(VoiceShared::default()),
            reconnects: AtomicU32::new(0),
            rejoin_offers: Mutex::new(rejoin_offers),
            previews: Arc::new(crate::web::LinkPreviews::new(
                Some(data_dir.join("link-previews")),
                crate::web::LinkPreviews::app_policy(),
                crate::web::fetch::Timeouts::default(),
            )),
            full_images: Arc::new(crate::full_image::new_transfers()),
            core: Mutex::new(Core::default()),
            now_playing: Mutex::new(crate::now_playing::NowPlaying::new(settings.now_playing)),
            now_gaming: Mutex::new(crate::now_gaming::NowGaming::new(settings.now_gaming)),
            mumcord: Mutex::default(),
            history_sync: Arc::default(),
        });
        let engine = Self { inner };
        engine
            .link_previews()
            .set_max_age_days(settings.preview_cache_days);
        engine.start_maintenance();
        Ok((engine, rx))
    }

    /// Shared DSP switches; push-to-talk key state goes to `set_ptt_pressed`.
    pub fn dsp_controls(&self) -> Arc<DspControls> {
        self.inner.controls.clone()
    }

    /// Context of the connected session, for extensions (B14).
    pub fn session_ctx(&self) -> EngineResult<SessionCtx> {
        let core = lock(&self.inner.core);
        core.session
            .as_ref()
            .map(|s| self.inner.ctx(s))
            .ok_or_else(EngineError::not_connected)
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

    pub(crate) fn full_image_transfers(&self) -> &Arc<crate::full_image::transfer::Transfers> {
        &self.inner.full_images
    }

    pub(crate) fn now_playing_state(&self) -> &Mutex<crate::now_playing::NowPlaying> {
        &self.inner.now_playing
    }

    pub(crate) fn mumcord_state(&self) -> &Mutex<crate::mumcord::Peers> {
        &self.inner.mumcord
    }

    pub(crate) fn now_gaming_state(&self) -> &Mutex<crate::now_gaming::NowGaming> {
        &self.inner.now_gaming
    }

    pub(crate) fn history_sync_state(&self) -> &Mutex<crate::history_sync::HistorySync> {
        &self.inner.history_sync
    }

    /// The store, also while disconnected (history commands, B14).
    pub(crate) fn store_handle(&self) -> Arc<Mutex<Store>> {
        self.inner.store.clone()
    }

    pub fn connection_state(&self) -> ConnState {
        lock(&self.inner.core).machine.state().clone()
    }

    /// Connects to a saved server, replacing any current connection (FR-1.9).
    /// Lands read-only (self-muted and deafened) where the server puts the user; a
    /// `channel_path` (from a link) counts as an explicit join instead.
    pub fn connect(
        &self,
        server_id: ServerId,
        channel_path: Option<Vec<String>>,
    ) -> EngineResult<()> {
        if lock(&self.inner.store)
            .server(server_id)
            .map_err(store_error)?
            .is_none()
        {
            return Err(EngineError::new(ErrorKind::NotFound, "unknown server"));
        }
        // Taken even when a link joins a channel: the offer is for the first connect only.
        let rejoin = lock(&self.inner.rejoin_offers).remove(&server_id);
        let rejoin = rejoin.filter(|_| channel_path.is_none());
        // Switching servers ends the current connection on the user's request.
        self.inner.hang_up(&mut lock(&self.inner.core));
        self.inner.start(server_id, channel_path, rejoin);
        Ok(())
    }

    /// Disconnects and cancels reconnecting or a pending trust question.
    pub fn disconnect(&self) {
        self.inner.hang_up(&mut lock(&self.inner.core));
        self.shutdown();
        self.inner.forget_voice_channel();
    }

    /// Disconnects for quitting or restarting the app. Unlike [`Engine::disconnect`] it keeps
    /// the voice channel, so the next start offers to rejoin it (QA.9).
    pub fn shutdown(&self) {
        let mut core = lock(&self.inner.core);
        self.inner.reset(&mut core);
        core.machine.cancel();
        self.inner.emit_state(&mut core);
    }

    /// Stops reconnecting; same as disconnecting.
    pub fn reconnect_cancel(&self) {
        self.disconnect();
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
            match core.pending_trust.take() {
                Some(p) if p.server_id == server_id && p.fingerprint == fingerprint => p,
                other => {
                    core.pending_trust = other;
                    return Err(EngineError::invalid("no such certificate question"));
                }
            }
        };
        if !trust {
            let mut core = lock(&self.inner.core);
            self.inner.reset(&mut core);
            core.machine.end(Some(EngineError::new(
                declined_kind(pending.changed),
                "server certificate not trusted",
            )));
            self.inner.emit_state(&mut core);
            return Ok(());
        }
        lock(&self.inner.store)
            .set_trusted_cert(server_id, &pending.fingerprint)
            .map_err(store_error)?;
        self.inner
            .start(server_id, pending.channel_path, pending.rejoin);
        Ok(())
    }

    pub fn snapshot(&self) -> Option<Snapshot> {
        let core = lock(&self.inner.core);
        self.inner.snapshot(&core)
    }

    pub fn set_self_mute(&self, mute: bool) -> EngineResult<()> {
        self.inner.change_self_voice(|v| v.set_mute(mute))
    }

    pub fn set_self_deaf(&self, deaf: bool) -> EngineResult<()> {
        self.inner.change_self_voice(|v| v.set_deaf(deaf))
    }

    /// Explicit join: moves there (unless already there) and turns voice on with the user's last
    /// own mute/deafen choice (QA.7).
    pub fn join_channel(&self, channel_id: u32) -> EngineResult<()> {
        let ctx = self.session_ctx()?;
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
            let mut voice = lock(&self.inner.core).self_voice;
            let joins = joins_voice(!here, voice);
            voice.join(own);
            joins.then_some(voice)
        };
        // Before the voice change is sent, so the hub knows its echo when it arrives.
        if let Some(voice) = joined {
            self.inner
                .events
                .sounds()
                .voice_join(voice.mute, voice.deaf);
        }
        self.inner.change_self_voice(|v| v.join(own))
    }

    pub fn person_set_volume(&self, session: u32, volume: u16) -> EngineResult<()> {
        if volume > 200 {
            return Err(EngineError::invalid("volume must be 0–200"));
        }
        self.inner.change_person(session, |s| s.volume = volume)
    }

    pub fn person_set_local_mute(&self, session: u32, muted: bool) -> EngineResult<()> {
        self.inner.change_person(session, |s| s.local_mute = muted)
    }

    /// Saves the private note about a person (FR-1.63); an empty note removes it.
    pub fn person_set_note(&self, session: u32, note: &str) -> EngineResult<()> {
        let mut core = lock(&self.inner.core);
        let (server_id, user) = Inner::person_in_session(&core, session)?;
        let store = lock(&self.inner.store);
        core.people.save_note(&store, server_id, &user, note)
    }

    /// Asks the server for a channel description announced only by its hash (FR-1.48b). The
    /// text arrives as a `ChannelUpserted` event; asking again for the same hash is a no-op.
    pub fn channel_description_request(&self, channel_id: u32) -> EngineResult<()> {
        let mut core = lock(&self.inner.core);
        let session = core
            .session
            .as_mut()
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
    pub async fn person_profile(&self, session: u32) -> EngineResult<ProfileView> {
        let ctx = self.session_ctx()?;
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
            profile.note = core
                .people
                .note(&lock(&self.inner.store), ctx.server_id, &current);
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

    /// Lists devices. Without a connection or mic test the devices are opened only for the
    /// listing and closed again.
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
                let settings = lock(&inner.store).settings().map_err(store_error)?;
                let audio = Audio::start(
                    settings.input_device,
                    settings.output_device,
                    inner.events.clone(),
                )?;
                audio.devices()
            })
            .await
            .map_err(|e| EngineError::internal(e.to_string()))?
    }

    /// Live link figures of the current connection, `None` when not connected (AC-1.23).
    pub fn connection_stats(&self) -> Option<ConnectionStatsView> {
        let core = lock(&self.inner.core);
        let session = core.session.as_ref()?;
        let link = session.transport.as_ref().map(|t| t.stats());
        let voice = &self.inner.voice;
        Some(ConnectionStatsView {
            transport: link.map(|l| match l.transport {
                Transport::Udp => VoiceTransportKind::Udp,
                Transport::Tcp => VoiceTransportKind::Tcp,
            }),
            rtt_ms: link.and_then(|l| l.rtt).map(|d| d.as_secs_f32() * 1000.0),
            loss_percent: link.map_or(0.0, |l| l.loss.clamp(0.0, 1.0) * 100.0),
            send_bitrate_bps: voice.bitrate_bps.load(Ordering::Relaxed),
            server_max_bandwidth_bps: session.handle.with_state(|s| s.server.max_bandwidth),
            packets_sent: voice.packets_sent.load(Ordering::Relaxed),
            packets_received: voice.packets_received.load(Ordering::Relaxed),
        })
    }

    /// Devices and connection counters for the diagnostic file (no names or addresses
    /// beyond device names, which the diagnostics module documents).
    pub fn diagnostic_info(&self, app_version: &str) -> crate::diag::DiagnosticInfo {
        let mut info = crate::diag::DiagnosticInfo::new(app_version);
        let core = lock(&self.inner.core);
        if let Some(audio) = &core.audio {
            for (direction, is_input) in [
                (mumcord_audio::io::Direction::Input, true),
                (mumcord_audio::io::Direction::Output, false),
            ] {
                for d in audio.io().devices(direction).unwrap_or_default() {
                    info.audio_devices.push(crate::diag::DiagAudioDevice {
                        name: d.name,
                        is_input,
                        is_default: d.is_default,
                    });
                }
            }
        }
        info.connection.reconnect_count = self.inner.reconnects.load(Ordering::Relaxed);
        let crypt = core
            .session
            .as_ref()
            .and_then(|s| s.transport.as_ref())
            .map(|t| t.stats().crypt);
        drop(core);
        if let Some(stats) = self.connection_stats() {
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
            events: self.events.clone(),
        }
    }

    /// Reports the state; once disconnected, also closes the audio devices unless the mic
    /// test uses them (devices are open only while connected or testing).
    fn emit_state(&self, core: &mut Core) {
        if matches!(core.machine.state(), ConnState::Disconnected { .. }) {
            self.release_audio_if_idle(core);
        }
        self.events
            .emit(EngineEvent::StateChanged(core.machine.state().clone()));
    }

    /// The user ends the connection (disconnect or server switch, not quitting): plays the
    /// hang-up sound (QA.19) if voice is on. The pipeline stops first and the sound runs on its
    /// own short-lived player, so tearing down voice and closing the devices cannot cut it off.
    fn hang_up(&self, core: &mut Core) {
        if core.session.is_none() {
            return;
        }
        let Some(samples) = self.events.sounds().hang_up() else {
            return;
        };
        self.stop_pipeline(core);
        if let Some(audio) = core.audio.as_ref() {
            audio.play_tail(&self.rt, &samples);
        }
    }

    fn release_audio_if_idle(&self, core: &mut Core) {
        if core.session.is_none() && core.mic_test.is_none() && core.pipeline.is_none() {
            core.audio = None;
        }
    }

    /// Starts a fresh connection, replacing any current one. Voice of the previous connection
    /// ends here, so no voice channel is remembered until the user joins again.
    fn start(
        self: &Arc<Self>,
        server_id: ServerId,
        channel_path: Option<Vec<String>>,
        rejoin: Option<String>,
    ) {
        self.forget_voice_channel();
        let mut core = lock(&self.core);
        let generation = self.reset(&mut core);
        core.machine.connect(server_id);
        self.emit_state(&mut core);
        let inner = self.clone();
        core.supervisor = Some(self.rt.spawn(async move {
            inner
                .supervise(generation, server_id, channel_path, rejoin)
                .await
        }));
    }

    /// Ends the current connection and its tasks; returns the new generation.
    fn reset(&self, core: &mut Core) -> u64 {
        core.generation += 1;
        if let Some(task) = core.supervisor.take() {
            task.abort();
        }
        core.pending_trust = None;
        self.stop_pipeline(core);
        if let Some(session) = core.session.take() {
            crate::now_playing::on_disconnect(&self.now_playing, &session.handle);
            crate::now_gaming::on_disconnect(&self.now_gaming, &session.handle);
            crate::mumcord::on_disconnect(&self.mumcord);
            crate::history_sync::on_disconnect(&self.history_sync);
            self.full_images.reset();
            session.handle.disconnect();
        }
        self.meter.voice.store(false, Ordering::Relaxed);
        core.generation
    }

    /// Runs a state transition if `generation` is still current and reports the new state.
    fn transition<R>(&self, generation: u64, f: impl FnOnce(&mut Core) -> R) -> Option<R> {
        let mut core = lock(&self.core);
        if core.generation != generation {
            return None;
        }
        let result = f(&mut core);
        self.emit_state(&mut core);
        Some(result)
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
                            self.transition(generation, |c| c.machine.lost())
                        }
                        Ended::Final(error) => {
                            if self
                                .transition(generation, |c| c.machine.end(error))
                                .is_some()
                            {
                                self.forget_voice_channel();
                            }
                            return;
                        }
                        Ended::Stale => return,
                    }
                }
                Err(AttemptError::Trust {
                    fingerprint,
                    previous,
                }) => {
                    let current = self.transition(generation, |c| {
                        c.machine.connect(server_id);
                        c.pending_trust = Some(PendingTrust {
                            server_id,
                            fingerprint: fingerprint.clone(),
                            changed: previous.is_some(),
                            channel_path: path.take(),
                            rejoin: rejoin.take(),
                        });
                    });
                    if current.is_some() {
                        tracing::info!(
                            server_id,
                            changed = previous.is_some(),
                            "server certificate needs a trust decision"
                        );
                        self.events.emit(EngineEvent::CertTrustRequired {
                            server_id,
                            fingerprint,
                            previous_fingerprint: previous,
                        });
                    }
                    return;
                }
                Err(AttemptError::Failure(failure)) => {
                    tracing::info!(server_id, "connection attempt failed");
                    self.transition(generation, |c| c.machine.failed(failure))
                }
            };
            match step {
                Some(Step::Retry(delay)) => {
                    self.reconnects.fetch_add(1, Ordering::Relaxed);
                    tracing::info!(server_id, delay_secs = delay.as_secs(), "reconnecting");
                    tokio::time::sleep(delay).await;
                }
                // Gave up: voice ended without the app quitting.
                Some(Step::Stop) => {
                    self.forget_voice_channel();
                    return;
                }
                None => return,
            }
        }
    }

    /// One connection attempt including the certificate check.
    async fn attempt(
        &self,
        server_id: ServerId,
    ) -> Result<(Connection, String, u16), AttemptError> {
        let fatal = |e: EngineError| AttemptError::Failure(Failure::Fatal(e));
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
        let secrets = self.secrets.clone();
        let password = self
            .rt
            .spawn_blocking(move || secrets.server_password(server_id))
            .await
            .map_err(|e| fatal(EngineError::internal(e.to_string())))?
            .map_err(store_error)
            .map_err(fatal)?;
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
        match mumcord_protocol::connect(config).await {
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
            Err(e) => Err(AttemptError::Failure(convert::connect_failure(&e))),
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
        self.voice.reset_link();
        let (pump_tx, mut pump_rx) = mpsc::unbounded_channel();
        let default_channel_id = handle.with_state(|s| s.own_user().map(|u| u.channel_id));
        let link_join = matches!(arrival, Arrival::Fresh(Some(_)));
        let (followed, rejoin_offer) = self.post_sync(server_id, &handle, arrival);

        let registered = self.transition(generation, |core| {
            core.people.clear();
            core.session = Some(Session {
                server_id,
                handle: handle.clone(),
                transport: transport.clone(),
                pump: pump_tx,
                rejoin_offer,
                default_channel_id,
                description_requests: description::DescriptionRequests::default(),
            });
            self.start_pipeline(core);
            core.machine.synced();
            self.remember_voice_channel(core);
        });
        if registered.is_none() {
            handle.disconnect();
            return Ended::Stale;
        }
        tracing::info!(server_id, "connected");
        self.events
            .emit(EngineEvent::FollowedChannelsChanged(followed));
        let ctx = SessionCtx {
            server_id,
            handle: handle.clone(),
            store: self.store.clone(),
            events: self.events.clone(),
        };
        crate::now_playing::on_synced(&self.now_playing);
        crate::now_gaming::on_synced(&self.now_gaming);
        crate::mumcord::on_synced(&self.mumcord, &ctx);
        crate::history_sync::on_synced(&self.history_sync, &ctx, &self.rt);
        self.full_images.reset();
        if let Some(mut snapshot) = self.snapshot(&lock(&self.core)) {
            extension::on_synced(&ctx, &mut snapshot);
            self.events
                .emit(EngineEvent::ServerSynced(Box::new(snapshot)));
        }
        // A link naming a channel is an explicit join (QA.23); after `ServerSynced`, which
        // resets the sound hub's view of the own state.
        if link_join {
            let voice = lock(&self.core).self_voice;
            self.events.sounds().voice_join(voice.mute, voice.deaf);
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
        bitrate.set_music_quality(self.controls.music_quality());
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
                    VoiceEvent::Packet(packet) => {
                        self.voice.packets_received.fetch_add(1, Ordering::Relaxed);
                        if !self.voice.deaf.load(Ordering::Relaxed)
                            && let (Some(tx), Some(p)) = (packets.as_mut(), voice::playback_packet(&packet))
                        {
                            tx.push(p);
                        }
                    }
                    VoiceEvent::TransportChanged(t) => {
                        tracing::info!(transport = ?t, "voice transport changed");
                        bitrate.set_transport(match t {
                            Transport::Udp => BitrateTransport::Udp,
                            Transport::Tcp => BitrateTransport::TcpTunnel,
                        });
                        self.events.emit(EngineEvent::TransportChanged(match t {
                            Transport::Udp => VoiceTransportKind::Udp,
                            Transport::Tcp => VoiceTransportKind::Tcp,
                        }));
                    }
                },
                Some(PumpCmd::Packets(sender)) = pump_rx.recv() => packets = sender,
                _ = tick.tick() => {
                    bitrate.set_music_quality(self.controls.music_quality());
                    if let Some(t) = &transport {
                        let stats = t.stats();
                        let target = bitrate.update(
                            BitrateStats {
                                loss_fraction: stats.loss,
                                rtt_ms: stats.rtt.map(|d| d.as_millis() as u32),
                            },
                            epoch.elapsed().as_millis() as u64,
                        );
                        self.voice.bitrate_bps.store(target.bitrate_bps, Ordering::Relaxed);
                        self.voice.loss_percent.store(target.loss_percent.into(), Ordering::Relaxed);
                    }
                }
            }
        }
    }

    fn end_connection(
        &self,
        generation: u64,
        server_id: ServerId,
        reason: DisconnectReason,
    ) -> Ended {
        let mut core = lock(&self.core);
        if core.generation != generation {
            return Ended::Stale;
        }
        self.stop_pipeline(&mut core);
        core.session = None;
        self.meter.voice.store(false, Ordering::Relaxed);
        drop(core);
        crate::history_sync::on_disconnect(&self.history_sync);
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
    /// server's burst. Returns the followed channel ids and the channel to offer for
    /// rejoining (QA.9).
    fn post_sync(
        &self,
        server_id: ServerId,
        handle: &Handle,
        arrival: Arrival,
    ) -> (Vec<u32>, Option<u32>) {
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
        let landing_voice = || self.landing_voice();
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
        let voice = {
            let mut core = lock(&self.core);
            if let Some(voice) = voice {
                core.self_voice = voice;
                self.voice.muted.store(voice.mute, Ordering::Relaxed);
                self.voice.deaf.store(voice.deaf, Ordering::Relaxed);
            }
            core.self_voice
        };
        // A fresh login starts unmuted on the server, so only a set state needs sending.
        let sent = if voice.deaf {
            handle.set_self_deaf(true)
        } else if voice.mute {
            handle.set_self_mute(true)
        } else {
            Ok(())
        };
        if let Err(e) = sent {
            tracing::warn!(error = %e, "restoring mute state failed");
        }
        (followed, offer)
    }

    fn on_event(&self, ctx: &SessionCtx, transport: Option<&VoiceTransport>, event: Event) {
        match &event {
            Event::ChannelUpdated(channel) => {
                let view = ctx.handle.with_state(|s| channel_view(s, channel));
                self.events.emit(EngineEvent::ChannelUpserted(view));
            }
            Event::ChannelRemoved(channel) => {
                self.events.emit(EngineEvent::ChannelRemoved(channel.id));
            }
            Event::UserUpdated(change) => {
                let user = &change.user;
                let mut core = lock(&self.core);
                let setting = core.people.setting(&lock(&self.store), ctx.server_id, user);
                if change.joined
                    && !people::is_default(setting)
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
                if own && change.moved_from.is_some() {
                    self.remember_channel(ctx, user.channel_id);
                }
                self.events
                    .emit(EngineEvent::UserUpserted(self.user_view_of(user, setting)));
            }
            Event::UserRemoved { user, .. } => {
                let mut core = lock(&self.core);
                core.people.forget(user.session);
                if let Some(pipeline) = core.pipeline.as_mut() {
                    pipeline.forget(user.session);
                }
                drop(core);
                crate::now_playing::on_user_removed(&self.now_playing, user.session);
                crate::now_gaming::on_user_removed(&self.now_gaming, user.session);
                crate::mumcord::on_user_removed(&self.mumcord, user.session);
                crate::history_sync::on_user_removed(&self.history_sync, user.session);
                self.full_images.forget(user.session);
                self.events.emit(EngineEvent::UserRemoved(user.session));
            }
            Event::PermissionDenied(denied) => {
                self.events.emit(EngineEvent::ServerDenied {
                    error: convert::denial_error(denied),
                    channel_id: denied.channel_id,
                });
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
                }
                crate::history_sync::on_plugin_data(&self.history_sync, ctx, data);
                crate::full_image::on_plugin_data(&self.full_images, &self.previews, ctx, data);
            }
            Event::TextMessage(_) | Event::ServerConfigUpdated(_) | Event::Disconnected(_) => {}
        }
        extension::on_event(ctx, &event);
    }

    fn user_view_of(
        &self,
        user: &mumcord_protocol::User,
        setting: mumcord_store::PersonSetting,
    ) -> UserView {
        user_view(user, setting, lock(&self.mumcord).features(user.session))
    }

    /// Emits the person again, e.g. after they were recognised as a Mumcord user.
    fn republish_user(&self, ctx: &SessionCtx, session: u32) {
        let Some(user) = ctx.handle.with_state(|s| s.users.get(&session).cloned()) else {
            return;
        };
        let setting = {
            // Lock order (see module docs): taking `store` first deadlocks against `snapshot`.
            let core = lock(&self.core);
            core.people
                .setting(&lock(&self.store), ctx.server_id, &user)
        };
        self.events
            .emit(EngineEvent::UserUpserted(self.user_view_of(&user, setting)));
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
            self.events.emit(EngineEvent::ChannelsUpserted(views));
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

    /// While voice is on, remembers the own channel so a restart can offer to rejoin it (QA.9).
    /// Unlike `last_channel` it is never the read-only channel of a fresh connect.
    fn remember_voice_channel(&self, core: &Core) {
        let Some(session) = &core.session else {
            return;
        };
        if core.self_voice.own().is_none() {
            return;
        }
        let Some(path) = session
            .handle
            .with_state(|s| s.own_user().and_then(|u| channel_path(s, u.channel_id)))
        else {
            return;
        };
        if let Err(e) = lock(&self.store).set_voice_channel(session.server_id, &join_path(&path)) {
            tracing::warn!(error = %e, "saving the voice channel failed");
        }
    }

    /// Voice ended other than by quitting the app: nothing to offer on the next start.
    fn forget_voice_channel(&self) {
        if let Err(e) = lock(&self.store).clear_voice_channels() {
            tracing::warn!(error = %e, "clearing the voice channel failed");
        }
    }

    fn snapshot(&self, core: &Core) -> Option<Snapshot> {
        let session = core.session.as_ref()?;
        let store = lock(&self.store);
        session.handle.with_state(|s: &ServerState| {
            Some(Snapshot {
                server_id: session.server_id,
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
                        let mut view =
                            self.user_view_of(u, core.people.setting(&store, session.server_id, u));
                        // The server's echo of the mute/deafen just sent may still be on its way.
                        if Some(u.session) == s.session {
                            view.self_mute = core.self_voice.mute;
                            view.self_deaf = core.self_voice.deaf;
                        }
                        view
                    })
                    .collect(),
                followed_channel_ids: followed_ids(&store, session.server_id, s),
                unread: Vec::new(),
                read_only: core.self_voice.own().is_none(),
                rejoin_channel_id: session.rejoin_offer,
            })
        })
    }

    /// Voice state of a fresh connect without a link: read-only, or (setting "Connect muted
    /// until I join a channel" off, QA.22) the last own mute/deafen choice right away.
    fn landing_voice(&self) -> SelfVoice {
        let connect_muted = lock(&self.store)
            .settings()
            .map(|s| s.connect_muted)
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "reading the settings failed");
                true
            });
        fresh_voice(connect_muted, self.own_voice())
    }

    /// The remembered own mute/deafen choice; neither if it cannot be read.
    fn own_voice(&self) -> OwnVoice {
        lock(&self.store).own_voice().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "reading the remembered mute state failed");
            OwnVoice::default()
        })
    }

    fn change_self_voice(&self, change: impl FnOnce(&mut SelfVoice)) -> EngineResult<()> {
        let mut core = lock(&self.core);
        let before = core.self_voice;
        change(&mut core.self_voice);
        let after = core.self_voice;
        if before.own().is_none() && after.own().is_some() {
            if let Some(session) = core.session.as_mut() {
                session.rejoin_offer = None;
            }
            self.remember_voice_channel(&core);
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
        self.voice.muted.store(after.mute, Ordering::Relaxed);
        self.voice.deaf.store(after.deaf, Ordering::Relaxed);
        let Some(session) = &core.session else {
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
        session: u32,
        change: impl FnOnce(&mut mumcord_store::PersonSetting),
    ) -> EngineResult<()> {
        let mut core = lock(&self.core);
        let (server_id, user) = Self::person_in_session(&core, session)?;
        let setting = {
            let store = lock(&self.store);
            let mut setting = core.people.setting(&store, server_id, &user);
            change(&mut setting);
            core.people.save(&store, server_id, &user, setting)?;
            setting
        };
        if let Some(pipeline) = core.pipeline.as_mut() {
            pipeline.apply_person(session, setting.volume, setting.local_mute);
        }
        drop(core);
        self.events
            .emit(EngineEvent::UserUpserted(self.user_view_of(&user, setting)));
        Ok(())
    }

    /// The server and the person with this session on it.
    fn person_in_session(
        core: &Core,
        session: u32,
    ) -> EngineResult<(ServerId, mumcord_protocol::User)> {
        let s = core
            .session
            .as_ref()
            .ok_or_else(EngineError::not_connected)?;
        let user = s
            .handle
            .with_state(|st| st.users.get(&session).cloned())
            .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown person"))?;
        Ok((s.server_id, user))
    }

    /// Starts the audio devices on first use.
    async fn ensure_audio(self: &Arc<Self>) -> EngineResult<()> {
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
        let started = core.session.is_some() && core.pipeline.is_none();
        if started {
            self.start_pipeline(&mut core);
        }
        Ok(())
    }

    /// Starts voice for the current session if devices are free; otherwise it starts once the
    /// devices are up or the mic test is over.
    fn start_pipeline(self: &Arc<Self>, core: &mut Core) {
        if core.pipeline.is_some() || core.mic_test.is_some() {
            return;
        }
        let Some(session) = core.session.as_ref() else {
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
            self.events.clone(),
            own,
        ) {
            Ok((mut pipeline, packets)) => {
                let _ = session.pump.send(PumpCmd::Packets(Some(packets)));
                let server_id = session.server_id;
                let store = lock(&self.store);
                let users = session
                    .handle
                    .with_state(|s| s.users.values().cloned().collect::<Vec<_>>());
                for user in users {
                    let setting = core.people.setting(&store, server_id, &user);
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
        if let Some(session) = &core.session {
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
