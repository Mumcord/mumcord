//! Screen sharing to Mumcord users in the same channel (FR-2.42 – FR-2.46, Beta).
//!
//! Who shares and who watches is agreed over plugin data ([`signal`]); the media itself only
//! travels over a direct connection, as frames on a [`crate::p2p::STREAM_SCREEN`] stream
//! ([`wire`]) that the viewer opens to the sharer once accepted. The sharer captures and encodes
//! once ([`capture`], `media`) and fans the frames out to every viewer (`fanout`); a viewer hands
//! them to the [`ScreenFrameSink`] (`watch`).
//!
//! Only the user's own channel is tracked. A sharer announces its share to everyone there and
//! again to each Mumcord user who comes in later, to them twice in case the server dropped the
//! first message; leaving the channel (or the server) ends the
//! own share and everything watched, and the people still there drop the share and the viewer
//! themselves when they see the move.

pub mod broadcast;
pub mod capture;
#[cfg(any(target_os = "macos", test))]
pub mod capture_macos;
#[cfg(target_os = "linux")]
pub mod capture_wayland;
#[cfg(windows)]
pub mod capture_windows;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub mod capture_x11;
mod convert;
pub(crate) mod encoder;
mod fanout;
mod media;
mod preview;
mod quality;
pub mod signal;
pub mod sound;
mod start;
mod state;
mod thumbnail;
pub(crate) mod watch;
mod windows_rules;
pub mod wire;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mumble_client_protocol::{Handle, PluginData};
use mumcord_p2p::DeclineReason;
use mumcord_p2p::quinn::{RecvStream, SendStream};
use tokio::sync::oneshot;

use crate::ServerId;
use crate::inspector::MediaTapSink;
use crate::mumcord::{FEATURE_SCREEN_BROADCAST, ServerPeers};
use crate::p2p::DirectError;
use crate::peer::{RelayUnavailable, RelayUse};
use crate::session::{
    Engine, EngineError, EngineEvent, EngineResult, ErrorKind, SessionCtx, WeakEngine, lock,
};
use broadcast::Fanout;
use broadcast::admit::{Decision, Path};
use broadcast::link::Received;
use mumcord_p2p::{BroadcastId, P2pError};
use signal::{DATA_ID, FEATURE, Signal, decode, encode};
use state::{Admit, Announced, Answer, OwnShare, Shares, Watching};

pub use broadcast::{FanoutInfo, FanoutUnavailable, FanoutView, RelayViewer};
pub use capture::{CaptureSource, SoundSource, TestPattern, TestTone};
pub use encoder::monitor::LoadDetails;
pub use encoder::{ActiveEncoder, Av1Skipped, EncoderStatus, GpuVendor, SoftwareLimit};
pub(crate) use fanout::ViewerQueue;
pub(crate) use media::EncoderFactory;
pub use media::{EncoderSwitch, ScreenMediaStats, SwitchReason, ViewerMediaStats, ViewerPath};
pub use mumcord_audio::mixer::share::ShareSoundStats;
/// Why the bot refused a broadcast, part of [`FanoutUnavailable::Refused`].
pub use mumcord_p2p::BroadcastErrorReason;
pub use mumcord_screenshare::I420Frame;
pub use preview::{CpuCores, SharePath, SharePreview};
pub use quality::{LoweredReason, QualityPrefs};
pub use signal::ShareKind;
pub use sound::{
    AppIcon, AppName, NoSoundReason, ShareSoundMode, SoundApp, SoundOutcome, SoundRequest,
    SoundStatus, SoundUnavailable,
};
pub use start::StartOutcome;
pub use thumbnail::ShareThumbnail;
pub use watch::ScreenFrameSink;

/// How long a watch request waits for the sharer's answer.
const ANSWER_WAIT: Duration = Duration::from_secs(10);
/// When an unanswered watch request is sent again, counted from the first. The server drops
/// plugin data beyond its rate limit (about 4 per second, burst 15), so when many people click
/// "Watch" at once some requests never arrive.
const WATCH_RESEND: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(3),
    Duration::from_secs(7),
];

/// Why the user cannot watch a share (FR-2.46).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchBlock {
    /// "Peer-to-peer is off" for this server (FR-4.3).
    P2pOff,
    /// The user said no to direct connections and the relay cannot be used either.
    ConsentOff,
    /// The relay was needed but cannot be used, with the reason (FR-4.4, FR-4.11, FR-4.14).
    Relay(RelayUnavailable),
    /// The sharer's client cannot or does not want to take direct connections.
    PeerUnsupported,
    /// No direct path was found (no relay exists, e.g. behind CGNAT).
    NoDirectPath,
}

impl WatchBlock {
    /// The reason as the `message` of a [`ErrorKind::DirectUnavailable`] error.
    pub fn code(self) -> &'static str {
        match self {
            Self::P2pOff => "p2pOff",
            Self::ConsentOff => "consentOff",
            Self::Relay(_) => "relayUnavailable",
            Self::PeerUnsupported => "peerUnsupported",
            Self::NoDirectPath => "noDirectPath",
        }
    }
}

/// One share in the user's channel as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenShareView {
    /// The sharing person; the own session for the user's share.
    pub session: u32,
    pub paused: bool,
    pub sound: bool,
    /// What the own share's sound is or why it has none (FR-7.84, FR-7.85); the default (no
    /// sound, no reason) for the shares of others.
    pub sound_status: SoundStatus,
    /// Who is watching (at most [`signal::MAX_VIEWERS`]), sorted.
    pub viewers: Vec<u32>,
    /// Why "Watch" is not possible; always `None` for the own share.
    pub blocked: Option<WatchBlock>,
    /// What the sharer picked, when the platform can tell (issue #163).
    pub kind: Option<ShareKind>,
    /// The picked window's title, only for the own share (issue #558); never sent anywhere.
    pub title: Option<String>,
    /// How long the share has been running (issue #163): the own one from its start, another's
    /// from the time their signal arrived, so no clock of another machine is trusted.
    pub elapsed_ms: u32,
}

/// Shares per connected server, and where watched frames go.
#[derive(Default)]
pub(crate) struct ScreenShares {
    servers: HashMap<ServerId, Shares>,
    sink: Option<Arc<dyn ScreenFrameSink>>,
    /// Reaches the relay and P2P settings for what blocks watching; set once at start.
    pub(crate) engine: Option<WeakEngine>,
    /// What watch requests claim this client can play instead of every codec (test hook).
    #[cfg(feature = "test-backend")]
    playable: Option<Vec<signal::CodecName>>,
}

impl WatchBlock {
    /// The block for a P2P connection that cannot be made; `None` for errors that are no
    /// block (the server connection is gone).
    fn of(e: &DirectError) -> Option<Self> {
        Some(match e {
            DirectError::NotConnected => return None,
            DirectError::Off => Self::P2pOff,
            DirectError::NotAllowed => Self::ConsentOff,
            DirectError::Relay(reason) => Self::Relay(*reason),
            DirectError::Unsupported
            | DirectError::P2p(P2pError::Declined(DeclineReason::Declined)) => {
                Self::PeerUnsupported
            }
            _ => Self::NoDirectPath,
        })
    }
}

pub(crate) type ServerShares = Mutex<ScreenShares>;

/// The own session and channel, with the other people in that channel.
struct Here {
    own: u32,
    others: Vec<u32>,
}

fn here(handle: &Handle) -> Option<Here> {
    handle.with_state(|s| {
        let own = s.session?;
        let channel_id = s.users.get(&own)?.channel_id;
        let others = s
            .users
            .values()
            .filter(|u| u.session != own && u.channel_id == channel_id)
            .map(|u| u.session)
            .collect();
        Some(Here { own, others })
    })
}

/// The people in `sessions` that announced `feature` on this server.
fn capable(peers: &ServerPeers, server_id: ServerId, sessions: &[u32], feature: &str) -> Vec<u32> {
    let servers = lock(peers);
    let Some(known) = servers.get(&server_id) else {
        return Vec::new();
    };
    sessions
        .iter()
        .copied()
        .filter(|s| {
            known
                .features(*s)
                .is_some_and(|f| f.iter().any(|x| x == feature))
        })
        .collect()
}

fn send(handle: &Handle, receivers: &[u32], signal: &Signal) {
    if receivers.is_empty() {
        return;
    }
    if let Err(e) = handle.send_plugin_data(receivers, DATA_ID, encode(signal)) {
        tracing::debug!(error = %e, "screen share message not sent");
    }
}

/// Sends the own share as it is now (`live: false` without one) to the capable `receivers`.
fn announce(state: &ServerShares, peers: &ServerPeers, ctx: &SessionCtx, receivers: &[u32]) {
    let signal = match lock(state)
        .servers
        .get(&ctx.server_id)
        .and_then(|s| s.own.as_ref())
    {
        Some(own) => Signal::Share {
            live: true,
            paused: own.paused,
            sound: own.sound,
            kind: own.kind,
            elapsed_ms: own.elapsed_ms(),
        },
        None => Signal::Share {
            live: false,
            paused: false,
            sound: false,
            kind: None,
            elapsed_ms: 0,
        },
    };
    let receivers = capable(peers, ctx.server_id, receivers, FEATURE);
    if !receivers.is_empty() {
        tracing::debug!(?receivers, ?signal, "screen share announced");
    }
    send(&ctx.handle, &receivers, &signal);
}

/// Tells the channel who watches the own share.
fn send_viewers(state: &ServerShares, peers: &ServerPeers, ctx: &SessionCtx, others: &[u32]) {
    let list = lock(state)
        .servers
        .get(&ctx.server_id)
        .and_then(|s| s.own.as_ref())
        .map(OwnShare::viewer_list);
    if let Some(list) = list {
        send(
            &ctx.handle,
            &capable(peers, ctx.server_id, others, FEATURE),
            &Signal::Viewers { list },
        );
    }
}

/// What blocks watching anyone on the server before trying: mode, relay, consent (FR-4.3,
/// FR-4.4).
fn static_block(state: &ServerShares, ctx: &SessionCtx) -> Option<WatchBlock> {
    let engine = lock(state).engine.as_ref().and_then(WeakEngine::upgrade)?;
    engine
        .p2p_unavailable(ctx.server_id, RelayUse::ScreenShare)
        .as_ref()
        .and_then(WatchBlock::of)
}

/// The shares in the user's channel with what blocks watching them.
fn views(state: &ServerShares, peers: &ServerPeers, ctx: &SessionCtx) -> Vec<ScreenShareView> {
    let Some(own) = ctx.handle.with_state(|s| s.session) else {
        return Vec::new();
    };
    let static_block = static_block(state, ctx);
    let mut views = lock(state)
        .servers
        .get(&ctx.server_id)
        .map(|s| s.views(own))
        .unwrap_or_default();
    let sharers: Vec<u32> = views.iter().map(|v| v.session).collect();
    let direct = capable(peers, ctx.server_id, &sharers, crate::p2p::FEATURE);
    for view in views.iter_mut().filter(|v| v.session != own) {
        if let Some(block) = static_block {
            view.blocked = Some(block);
        } else if !direct.contains(&view.session) {
            view.blocked = Some(WatchBlock::PeerUnsupported);
        }
    }
    views
}

fn emit(state: &ServerShares, peers: &ServerPeers, ctx: &SessionCtx) {
    ctx.events
        .emit(EngineEvent::ScreenSharesChanged(views(state, peers, ctx)));
}

/// Drops everything of `server_id` (reconnect, disconnect); true when the UI showed something.
pub(crate) fn reset(state: &ServerShares, server_id: ServerId) -> bool {
    lock(state)
        .servers
        .remove(&server_id)
        .is_some_and(|s| !s.is_empty())
}

/// A screen share signal arrived (session task).
pub(crate) fn on_plugin_data(
    state: &ServerShares,
    peers: &ServerPeers,
    ctx: &SessionCtx,
    data: &PluginData,
) {
    if data.data_id != DATA_ID {
        return;
    }
    let Some(sender) = data.sender else {
        return;
    };
    let Some(here) = here(&ctx.handle) else {
        return;
    };
    // Only people in the own channel share and watch; anything else is stale or forged.
    if sender == here.own || !here.others.contains(&sender) {
        return;
    }
    let Some(signal) = decode(&data.data) else {
        tracing::debug!("ignoring a malformed screen share message");
        return;
    };
    if let Signal::Share { live, .. } = signal {
        tracing::debug!(sender, live, "screen share announcement received");
    }
    // What a Watch needs to know before the state is locked.
    let (broadcast_viewer, viewer_hash, viewer_media) = match signal {
        Signal::Watch { bc, mv, .. } => (
            bc == 1
                && !capable(peers, ctx.server_id, &[sender], FEATURE_SCREEN_BROADCAST).is_empty(),
            cert_hash(&ctx.handle, sender),
            mv,
        ),
        _ => (false, None, 0),
    };
    let mut reply = None;
    let mut own_viewers = false;
    let mut admit = None;
    let mut kick = None;
    let mut relink = false;
    let changed = {
        let mut all = lock(state);
        let shares = all.servers.entry(ctx.server_id).or_default();
        match signal {
            Signal::Share {
                live: true,
                paused,
                sound,
                kind,
                elapsed_ms,
            } => shares.share_live(
                sender,
                &Announced {
                    paused,
                    sound,
                    kind,
                    elapsed_ms,
                },
                Instant::now(),
            ),
            Signal::Share { live: false, .. } => shares.share_ended(sender),
            Signal::Watch { codecs, .. } => {
                let caps = state::ViewerCaps::from_watch(&codecs, viewer_media);
                let fanout = shares.own.as_mut().map(|own| {
                    on_watch(
                        own,
                        sender,
                        caps,
                        broadcast_viewer,
                        viewer_hash.as_deref(),
                        Instant::now(),
                    )
                });
                match fanout {
                    Some(OnWatch::Reply(signal, new)) => {
                        reply = Some(signal);
                        own_viewers = new;
                    }
                    Some(OnWatch::Admit(id, new)) => {
                        admit = Some(id);
                        own_viewers = new;
                    }
                    Some(OnWatch::Quiet) => {}
                    // Nothing to watch: the request came after the share ended.
                    None => {
                        reply = Some(Signal::Share {
                            live: false,
                            paused: false,
                            sound: false,
                            kind: None,
                            elapsed_ms: 0,
                        });
                    }
                }
                own_viewers
            }
            Signal::Accept => {
                // A share watched through the relay continues on a direct copy once the sharer
                // lost the relay (FR-7.49).
                relink = watched_via_relay(shares, sender);
                shares.answered(sender, Answer::Accept);
                false
            }
            Signal::Full => {
                if watched_via_relay(shares, sender) {
                    // The relay is gone and the direct copies are taken (FR-7.49).
                    shares.watching.remove(&sender);
                    true
                } else {
                    shares.answered(sender, Answer::Full);
                    false
                }
            }
            Signal::NeedsUpdate => {
                shares.answered(sender, Answer::NeedsUpdate);
                false
            }
            Signal::Unwatch => {
                let left = shares.own.as_mut().and_then(|own| leave(own, sender));
                own_viewers = left.is_some();
                kick = left.flatten();
                own_viewers
            }
            Signal::Report { fps, slow } => {
                if let Some(Fanout::Live {
                    roster, reports, ..
                }) = shares.own.as_mut().and_then(|own| own.fanout.as_mut())
                    && roster.path(sender) == Some(Path::Relay)
                {
                    reports.insert(sender, (Received { fps, slow }, Instant::now()));
                }
                false
            }
            Signal::Viewers { list } => {
                let list = list
                    .into_iter()
                    .filter(|v| *v == here.own || here.others.contains(v))
                    .collect();
                shares.set_viewers(sender, list)
            }
        }
    };
    if let Some(reply) = reply {
        send(&ctx.handle, &[sender], &reply);
    }
    let engine = lock(state).engine.as_ref().and_then(WeakEngine::upgrade);
    if let Some(engine) = engine {
        if let Some(id) = admit {
            engine.admit_to_broadcast(ctx.server_id, id, sender);
        }
        if let Some(id) = kick {
            engine.relay_broadcast_kick(ctx.server_id, RelayUse::ScreenShare, id, sender);
        }
        if relink {
            engine.screen_share_relink(ctx.server_id, sender);
        }
    }
    if own_viewers {
        send_viewers(state, peers, ctx, &here.others);
    }
    if changed {
        emit(state, peers, ctx);
    }
}

/// The certificate hash the Mumble server reports for `session`.
fn cert_hash(handle: &Handle, session: u32) -> Option<String> {
    handle.with_state(|s| s.users.get(&session).and_then(|u| u.cert_hash.clone()))
}

/// Whether the user watches `sharer`'s share through the relay broadcast.
fn watched_via_relay(shares: &Shares, sharer: u32) -> bool {
    matches!(shares.watching.get(&sharer), Some(Watching::Open(link)) if link.broadcast().is_some())
}

/// What to do about a Watch for the own share.
enum OnWatch {
    /// Send this; `true` when the viewer is new.
    Reply(Signal, bool),
    /// Ask the bot to admit the viewer to this broadcast; `true` when the viewer is new.
    Admit(BroadcastId, bool),
    /// Nothing: the viewer asks again (the broadcast is starting, or its token is on the way).
    Quiet,
}

/// Decides about a Watch from `viewer`: through the relay broadcast when it is up and the
/// viewer's Mumcord can join it, else a direct copy as before (FR-7.42, FR-7.48).
fn on_watch(
    own: &mut OwnShare,
    viewer: u32,
    caps: state::ViewerCaps,
    broadcast: bool,
    hash: Option<&str>,
    now: Instant,
) -> OnWatch {
    // A viewer without a certificate hash can't do the key exchange.
    let broadcast = broadcast && hash.is_some();
    let decided = match own.fanout.as_mut() {
        Some(Fanout::Starting) if broadcast => return OnWatch::Quiet,
        Some(Fanout::Live {
            publisher,
            roster,
            hashes,
            ..
        }) => {
            let new = roster.path(viewer).is_none();
            Some(match roster.decide(viewer, broadcast, caps.media, now) {
                Decision::Relay => {
                    if let Some(hash) = hash {
                        publisher.admit(hash);
                        hashes.insert(viewer, hash.to_owned());
                    }
                    OnWatch::Admit(publisher.id(), new)
                }
                Decision::Direct => {
                    own.viewers.entry(viewer).or_insert(None);
                    OnWatch::Reply(Signal::Accept, new)
                }
                Decision::Wait => OnWatch::Quiet,
                Decision::Full => OnWatch::Reply(Signal::Full, false),
                Decision::NeedsUpdate => OnWatch::Reply(Signal::NeedsUpdate, false),
            })
        }
        _ => None,
    };
    if let Some(decided) = decided {
        if matches!(
            decided,
            OnWatch::Admit(..) | OnWatch::Reply(Signal::Accept, _)
        ) {
            // The share's one encoder has to suit relay viewers too (FR-7.58).
            own.set_caps(viewer, caps);
        }
        return decided;
    }
    match own.admit(viewer, caps) {
        Admit::New => OnWatch::Reply(Signal::Accept, true),
        Admit::Already => OnWatch::Reply(Signal::Accept, false),
        Admit::Full => OnWatch::Reply(Signal::Full, false),
    }
}

/// `viewer` stopped watching the own share or left. `None` when it was no viewer; else the
/// broadcast to remove it from at the relay, if it watched through one. A relay viewer leaving
/// makes the share switch keys (FR-7.44).
fn leave(own: &mut OwnShare, viewer: u32) -> Option<Option<BroadcastId>> {
    let direct = own.remove_viewer(viewer);
    if let Some(Fanout::Live {
        publisher,
        roster,
        hashes,
        reports,
        ..
    }) = own.fanout.as_mut()
    {
        reports.remove(&viewer);
        match roster.remove(viewer) {
            Some(Path::Relay) => {
                if let Some(hash) = hashes.remove(&viewer) {
                    publisher.remove(&hash);
                }
                return Some(Some(publisher.id()));
            }
            Some(Path::Direct) => return Some(None),
            None => {}
        }
    }
    direct.then_some(None)
}

/// Someone left the server.
pub(crate) fn on_user_removed(
    state: &ServerShares,
    peers: &ServerPeers,
    ctx: &SessionCtx,
    session: u32,
) {
    forget(state, peers, ctx, session);
}

fn forget(state: &ServerShares, peers: &ServerPeers, ctx: &SessionCtx, session: u32) {
    let (forgot, kick, engine) = {
        let mut all = lock(state);
        let engine = all.engine.as_ref().and_then(WeakEngine::upgrade);
        let Some(shares) = all.servers.get_mut(&ctx.server_id) else {
            return;
        };
        let left = shares.own.as_mut().and_then(|own| leave(own, session));
        let mut forgot = shares.forget(session);
        forgot.own_viewers |= left.is_some();
        forgot.changed |= left.is_some();
        (forgot, left.flatten(), engine)
    };
    if let (Some(id), Some(engine)) = (kick, engine) {
        engine.relay_broadcast_kick(ctx.server_id, RelayUse::ScreenShare, id, session);
    }
    if forgot.own_viewers
        && let Some(here) = here(&ctx.handle)
    {
        send_viewers(state, peers, ctx, &here.others);
    }
    if forgot.changed {
        emit(state, peers, ctx);
    }
}

/// Someone (or the user) joined the server or moved. The user changing channel ends the own
/// share and all watching; another person leaving the channel ends theirs; a Mumcord user
/// coming into the channel gets the own share.
pub(crate) fn on_user_moved(
    state: &ServerShares,
    peers: &ServerPeers,
    ctx: &SessionCtx,
    session: u32,
) {
    let Some(here) = here(&ctx.handle) else {
        return;
    };
    if session == here.own {
        // The sharers in the new channel announce themselves when they see the move.
        if reset(state, ctx.server_id) {
            emit(state, peers, ctx);
        }
    } else if here.others.contains(&session) {
        offer_share(state, peers, ctx, session);
    } else {
        forget(state, peers, ctx, session);
    }
}

/// Another client was recognised as a Mumcord user: if they are in the channel, they may have
/// missed the own share (their hello came after their move). Their share, if it came before
/// their hello, may no longer be blocked.
pub(crate) fn on_peer_known(
    state: &ServerShares,
    peers: &ServerPeers,
    ctx: &SessionCtx,
    session: u32,
) {
    if !here(&ctx.handle).is_some_and(|h| h.others.contains(&session)) {
        return;
    }
    offer_share(state, peers, ctx, session);
    let shares = lock(state)
        .servers
        .get(&ctx.server_id)
        .is_some_and(|s| s.others.contains_key(&session));
    if shares {
        emit(state, peers, ctx);
    }
}

fn offer_share(state: &ServerShares, peers: &ServerPeers, ctx: &SessionCtx, session: u32) {
    let sharing = lock(state)
        .servers
        .get(&ctx.server_id)
        .is_some_and(|s| s.own.is_some());
    if !sharing {
        return;
    }
    announce(state, peers, ctx, &[session]);
    if let Some(own) = lock(state)
        .servers
        .get_mut(&ctx.server_id)
        .and_then(|s| s.own.as_mut())
    {
        own.announced_to(session, Instant::now());
    }
}

/// Sends the own share once more to the newcomers due for it, and reports what arrives from
/// shares watched through the relay (session tick).
pub(crate) fn on_tick(state: &ServerShares, peers: &ServerPeers, ctx: &SessionCtx) {
    broadcast::send_reports(state, ctx);
    let due = lock(state)
        .servers
        .get_mut(&ctx.server_id)
        .and_then(|s| s.own.as_mut())
        .map(|own| own.take_reannounce(Instant::now()))
        .unwrap_or_default();
    if due.is_empty() {
        return;
    }
    let Some(here) = here(&ctx.handle) else {
        return;
    };
    let due: Vec<u32> = due
        .into_iter()
        .filter(|s| here.others.contains(s))
        .collect();
    announce(state, peers, ctx, &due);
}

/// Serves a viewer's screen stream. Only an accepted viewer of the own share gets one; frames
/// go to it until they stop watching or the share ends.
pub(crate) async fn serve_direct(
    engine: &Engine,
    server_id: ServerId,
    session: u32,
    send: SendStream,
    recv: RecvStream,
) {
    let mut all = lock(engine.screen_share_state());
    let slot = all
        .servers
        .get_mut(&server_id)
        .and_then(|s| s.own.as_mut())
        .and_then(|own| {
            let media = own.media.as_ref()?;
            // An accepted viewer opens a stream again when the connection to them was
            // replaced (a file moved it to the relay); the old stream died with it.
            let caps = own.viewer_caps(session);
            let slot = own.viewers.get_mut(&session)?;
            Some((media, slot, caps))
        });
    match slot {
        Some((media, slot, caps)) => {
            // The old link leaves the hub before the new one joins under the same session.
            drop(slot.take());
            *slot = Some(media.add_viewer(engine.runtime(), session, caps, send, recv));
        }
        None => tracing::debug!(
            ?server_id,
            session,
            "screen stream from someone not accepted; closing it"
        ),
    }
}

/// Sends the watch request, again while it is unanswered, and waits for the answer; `None` when
/// none came in time.
async fn ask_to_watch(
    handle: &Handle,
    sharer: u32,
    request: Signal,
    mut answer: oneshot::Receiver<Answer>,
) -> Option<Result<Answer, oneshot::error::RecvError>> {
    let start = tokio::time::Instant::now();
    send(handle, &[sharer], &request);
    for at in WATCH_RESEND {
        tokio::select! {
            answered = &mut answer => return Some(answered),
            () = tokio::time::sleep_until(start + at) => send(handle, &[sharer], &request),
        }
    }
    tokio::time::timeout_at(start + ANSWER_WAIT, answer)
        .await
        .ok()
}

fn not_sharing() -> EngineError {
    EngineError::new(ErrorKind::NotFound, "no screen share")
}

fn blocked(block: WatchBlock) -> EngineError {
    EngineError::new(ErrorKind::DirectUnavailable, block.code())
}

impl Engine {
    /// Why nobody could watch a share of the user on `server_id`, so starting one makes no
    /// sense: the mode is off, or neither direct (consent) nor the relay may be used (FR-4.3 –
    /// FR-4.5, AC-4.9). `None` when a share may start; a consent not asked yet is no block.
    pub fn screen_share_block(&self, server_id: ServerId) -> Option<WatchBlock> {
        self.p2p_unavailable(server_id, RelayUse::ScreenShare)
            .as_ref()
            .and_then(WatchBlock::of)
    }

    /// Starts sharing in the user's channel on `server_id` and tells the Mumcord users there
    /// (FR-2.42). Captures the whole screen on X11 and lets the user pick on Wayland; closing
    /// that picker gives [`StartOutcome::Cancelled`]. The share stops by itself when the capture
    /// ends (the desktop's stop button). Errors with [`ErrorKind::DirectUnavailable`] and a
    /// [`WatchBlock::code`] when [`Engine::screen_share_block`] blocks it.
    ///
    /// `prefs` and `codec` are the start dialog's picks for this one share; `None` takes the
    /// Settings value. Settings themselves are never changed here.
    pub async fn screen_share_start(
        &self,
        server_id: ServerId,
        sound: bool,
        prefs: Option<QualityPrefs>,
        codec: Option<mumcord_store::ScreenCodec>,
    ) -> EngineResult<StartOutcome> {
        let ctx = self.session_ctx_for(server_id)?;
        let here = here(&ctx.handle).ok_or_else(EngineError::not_connected)?;
        if let Some(block) = self.screen_share_block(server_id) {
            return Err(blocked(block));
        }
        if lock(self.screen_share_state())
            .servers
            .get(&server_id)
            .is_some_and(|s| s.own.is_some())
        {
            return Err(EngineError::invalid("already sharing on this server"));
        }
        let Some(mut source) = start::into_outcome(start::open_source().await)? else {
            return Ok(StartOutcome::Cancelled);
        };
        let (app_sound, mut sound_status) =
            sound::status::take(source.as_mut(), sound::unavailable());
        // With the switch off nobody asked for sound, so "Which app's sound?" is not shown.
        if !sound {
            sound_status.choice = None;
        }
        let settings = ctx.store().settings().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "screen share settings not read; using the defaults");
            mumcord_store::Settings::default()
        });
        let (settings, prefs) = with_picks(settings, prefs, codec);
        let selection = self.screen_encoders().select(&settings, prefs);
        let token = {
            let mut all = lock(self.screen_share_state());
            let shares = all.servers.entry(server_id).or_default();
            // The picker was open for a while; another start may have won meanwhile.
            if shares.own.is_some() {
                return Err(EngineError::invalid("already sharing on this server"));
            }
            let mut own = OwnShare::new(sound, sound_status);
            own.sound_status.log(own.sound);
            own.kind = source.kind();
            own.title = source.title();
            let media = media::Media::start(
                source,
                selection.prefs,
                encoder::factory(selection.candidates, own.encoder.clone()),
            );
            media.set_media_tap(MediaTapSink::new(&self.inspector(), server_id));
            if let Some(app_sound) = app_sound {
                media.start_sound(app_sound);
            }
            media.set_sound(own.sound);
            let token = media.token();
            own.media = Some(media);
            shares.own = Some(own);
            token
        };
        self.share_changed(&ctx, &here);
        self.watch_for_end(server_id, token);
        self.start_fanout(server_id, token);
        Ok(StartOutcome::Started)
    }

    /// Stops the share once its capture ended on its own, unless it was replaced meanwhile, and
    /// shows a broken sound capture once.
    fn watch_for_end(&self, server_id: ServerId, token: usize) {
        let engine = self.clone();
        self.runtime().spawn(async move {
            let mut sound_failed_seen = false;
            loop {
                tokio::time::sleep(start::ENDED_POLL).await;
                let state = {
                    let all = lock(engine.screen_share_state());
                    all.servers
                        .get(&server_id)
                        .and_then(|s| s.own.as_ref())
                        .and_then(|o| o.media.as_ref())
                        .map(|m| (m.token(), m.ended(), m.sound_failed()))
                };
                match state {
                    Some((t, false, failed)) if t == token => {
                        if failed && !sound_failed_seen {
                            sound_failed_seen = true;
                            engine.share_sound_failed(server_id);
                        }
                    }
                    Some((t, true, _)) if t == token => {
                        if let Err(e) = engine.screen_share_stop(server_id) {
                            tracing::debug!(error = %e, "ended screen share could not be stopped");
                        }
                        return;
                    }
                    _ => return,
                }
            }
        });
    }

    /// The own share's sound capture broke (FR-7.84): the video goes on, the live bar shows
    /// "No sound" with the reason.
    fn share_sound_failed(&self, server_id: ServerId) {
        let updated = self.update_own_share(server_id, |own| {
            own.sound_failed();
            own.sound_status.log(own.sound);
            if let Some(media) = &own.media {
                media.set_sound(false);
            }
        });
        if let Err(e) = updated {
            tracing::debug!(error = %e, "screen share sound failure not shown");
        }
    }

    /// Stops the own share; its viewers' streams close. Nothing happens without one.
    pub fn screen_share_stop(&self, server_id: ServerId) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let here = here(&ctx.handle).ok_or_else(EngineError::not_connected)?;
        let stopped = lock(self.screen_share_state())
            .servers
            .get_mut(&server_id)
            .and_then(|s| s.own.take())
            .is_some();
        if stopped {
            self.share_changed(&ctx, &here);
        }
        Ok(())
    }

    /// Pauses or resumes the own share (FR-2.45).
    pub fn screen_share_set_paused(&self, server_id: ServerId, paused: bool) -> EngineResult<()> {
        self.update_own_share(server_id, |own| {
            own.paused = paused;
            if let Some(media) = &own.media {
                media.set_paused(paused);
            }
        })
    }

    /// Turns the own share's sound on or off (FR-2.45).
    pub fn screen_share_set_sound(&self, server_id: ServerId, sound: bool) -> EngineResult<()> {
        self.update_own_share(server_id, |own| {
            own.sound = sound && own.sound_status.available();
            own.sound_status.off = !sound;
            own.sound_status.log(own.sound);
            if let Some(media) = &own.media {
                media.set_sound(own.sound);
            }
        })
    }

    /// The apps playing sound now, for "Which app's sound?" while the own share waits for the
    /// user's pick (Linux, FR-7.83); call it again to refresh the open list. Empty elsewhere.
    pub fn screen_share_sound_apps(&self) -> EngineResult<Vec<SoundApp>> {
        #[cfg(target_os = "linux")]
        {
            sound::pick::list().map_err(|reason| EngineError::invalid(reason.code()))
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(Vec::new())
        }
    }

    /// The answer to "Which app's sound?" (FR-7.83): `Some(id)` (a [`SoundApp::id`]) shares
    /// that app's sound and turns sound on; `None` ("No sound" or Cancel) keeps the share
    /// silent. Errors when nothing is asked, or when the app stopped playing meanwhile (the
    /// list stays open and should be refreshed).
    pub fn screen_share_choose_sound(
        &self,
        server_id: ServerId,
        app: Option<&str>,
    ) -> EngineResult<()> {
        let asked = lock(self.screen_share_state())
            .servers
            .get(&server_id)
            .and_then(|s| s.own.as_ref())
            .is_some_and(|own| own.sound_status.choice.is_some());
        if !asked {
            return Err(EngineError::invalid("no sound choice is asked"));
        }
        let outcome = match app {
            None => SoundOutcome::NoSound,
            Some(id) => chosen_sound(id)?,
        };
        let (source, mut status) = outcome.into_parts();
        status.declined = app.is_none();
        let mut source = Some(source);
        self.update_own_share(server_id, |own| {
            // Another answer may have come first.
            if own.sound_status.choice.is_none() {
                return;
            }
            let Some(media) = &own.media else { return };
            own.sound_status = status;
            own.sound = false;
            if let Some(source) = source.take().flatten() {
                media.start_sound(source);
                own.sound = true;
            }
            media.set_sound(own.sound);
            own.sound_status.log(own.sound);
        })
    }

    /// Why this system cannot share sound (FR-7.84): the start dialog then shows the Sound
    /// switch off and disabled with the matching note; `None` where it can. Windows needs 10
    /// version 2004 (process loopback), macOS 13 (ScreenCaptureKit), Linux PipeWire.
    pub fn screen_share_sound_unavailable(&self) -> Option<SoundUnavailable> {
        sound::unavailable()
    }

    /// Sends `source` as the own share's sound instead of OS capture, e.g. a [`TestTone`]
    /// (test hook). The share then counts as having sound, like a capture that came with one,
    /// and sound is switched on; the sound on/off switch still applies.
    #[cfg(feature = "test-backend")]
    pub fn screen_share_set_sound_source(
        &self,
        server_id: ServerId,
        source: Box<dyn SoundSource>,
    ) -> EngineResult<()> {
        self.update_own_share(server_id, |own| {
            if let Some(media) = &own.media {
                media.start_sound(source);
                media.set_sound(true);
                own.sound_status = SoundStatus::test_source();
                own.sound = true;
                own.sound_status.log(own.sound);
            }
        })
    }

    /// Level of every watched share's sound, 0.0–1.0 (auto-ducking, FR-2.76): the playback
    /// thread fades to it and it carries over to the next voice connection.
    pub fn screen_share_set_sound_level(&self, level: f32) {
        self.voice_shared().share_gain.set(level);
    }

    /// Plays the sound of `sharer`'s share at `percent`, if it is being watched.
    pub(crate) fn screen_share_apply_volume(&self, server_id: ServerId, sharer: u32, percent: u16) {
        if let Some(Watching::Open(link)) = lock(self.screen_share_state())
            .servers
            .get(&server_id)
            .and_then(|s| s.watching.get(&sharer))
        {
            link.set_volume(percent);
        }
    }

    /// How the sound of `sharer`'s share plays here; `None` while not watching it.
    pub fn screen_share_sound_stats(
        &self,
        server_id: ServerId,
        sharer: u32,
    ) -> Option<ShareSoundStats> {
        match lock(self.screen_share_state())
            .servers
            .get(&server_id)
            .and_then(|s| s.watching.get(&sharer))
        {
            Some(Watching::Open(link)) => Some(link.sound_stats()),
            _ => None,
        }
    }

    fn update_own_share(
        &self,
        server_id: ServerId,
        update: impl FnOnce(&mut OwnShare),
    ) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let here = here(&ctx.handle).ok_or_else(EngineError::not_connected)?;
        {
            let mut all = lock(self.screen_share_state());
            let own = all
                .servers
                .get_mut(&server_id)
                .and_then(|s| s.own.as_mut())
                .ok_or_else(not_sharing)?;
            update(own);
        }
        self.share_changed(&ctx, &here);
        Ok(())
    }

    fn share_changed(&self, ctx: &SessionCtx, here: &Here) {
        announce(
            self.screen_share_state(),
            self.mumcord_state(),
            ctx,
            &here.others,
        );
        emit(self.screen_share_state(), self.mumcord_state(), ctx);
    }

    /// Starts watching `sharer`'s share (FR-2.43, FR-2.46): asks the sharer, then opens the
    /// screen stream over a direct connection. Errors: [`ErrorKind::ShareFull`] when
    /// [`signal::MAX_VIEWERS`] people already watch, [`ErrorKind::DirectUnavailable`] with a
    /// [`WatchBlock::code`] as message when no direct connection can be made.
    pub async fn screen_share_watch(&self, server_id: ServerId, sharer: u32) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let here = here(&ctx.handle).ok_or_else(EngineError::not_connected)?;
        if !here.others.contains(&sharer) {
            return Err(EngineError::invalid("that person is not in your channel"));
        }
        if let Some(block) = self
            .p2p_unavailable(server_id, RelayUse::ScreenShare)
            .as_ref()
            .and_then(WatchBlock::of)
        {
            return Err(blocked(block));
        }
        if !self.mumcord_user_has_feature(server_id, sharer, crate::p2p::FEATURE) {
            return Err(blocked(WatchBlock::PeerUnsupported));
        }
        let (attempt, answer) = {
            let mut all = lock(self.screen_share_state());
            let shares = all
                .servers
                .get_mut(&server_id)
                .filter(|s| s.others.contains_key(&sharer))
                .ok_or_else(not_sharing)?;
            let Some(begun) = shares.begin_watch(sharer) else {
                return Ok(());
            };
            begun
        };
        let request = self.watch_request(server_id);
        match ask_to_watch(&ctx.handle, sharer, request, answer).await {
            Some(Ok(Answer::Accept)) => {}
            Some(Ok(Answer::Broadcast(id, token, media))) => {
                return self
                    .watch_broadcast(&ctx, sharer, attempt, id, token, media)
                    .await;
            }
            Some(Ok(Answer::Full)) => {
                return Err(EngineError::new(ErrorKind::ShareFull, "this share is full"));
            }
            Some(Ok(Answer::NeedsUpdate)) => {
                return Err(EngineError::new(
                    ErrorKind::NeedsUpdate,
                    "this share needs Mumcord 0.9.0",
                ));
            }
            // The share ended, the user left the channel or a newer attempt replaced this one.
            Some(Err(_)) => return Err(not_sharing()),
            None => {
                if let Some(shares) = lock(self.screen_share_state()).servers.get_mut(&server_id) {
                    shares.end_attempt(sharer, attempt);
                }
                return Err(EngineError::new(
                    ErrorKind::NotFound,
                    "the sharer did not answer",
                ));
            }
        }
        let opened = self
            .p2p_open_stream(
                server_id,
                sharer,
                crate::p2p::STREAM_SCREEN,
                crate::p2p::ConsentPrompt::Allowed,
            )
            .await;
        let volume = self.person_share_volume(server_id, sharer);
        // A newer attempt replaced this one meanwhile: leave its state and the sharer alone.
        let mut superseded = false;
        let result = {
            let mut all = lock(self.screen_share_state());
            let sink = all.sink.clone();
            let shares = all.servers.get_mut(&server_id);
            match opened {
                Ok((send, recv)) => {
                    if let Some(shares) = shares.filter(|s| s.is_opening(sharer, attempt)) {
                        shares.failed.remove(&sharer);
                        if let Some(watching) = shares.watching.get_mut(&sharer) {
                            *watching = Watching::Open(watch::WatchLink::start(
                                self.runtime(),
                                server_id,
                                sharer,
                                sink,
                                sound::ShareSound::new(self.voice_shared().clone(), volume),
                                send,
                                recv,
                                Some(MediaTapSink::new(&self.inspector(), server_id)),
                            ));
                        }
                    }
                    Ok(())
                }
                Err(e) => {
                    tracing::info!(error = %e, "no direct connection for a screen share");
                    let Some(block) = WatchBlock::of(&e) else {
                        return Err(EngineError::not_connected());
                    };
                    if let Some(shares) = shares
                        && shares.end_attempt(sharer, attempt)
                    {
                        shares.failed.insert(sharer, block);
                    } else {
                        superseded = true;
                    }
                    Err(blocked(block))
                }
            }
        };
        if result.is_err() && !superseded {
            send(&ctx.handle, &[sharer], &Signal::Unwatch);
        }
        emit(self.screen_share_state(), self.mumcord_state(), &ctx);
        result
    }

    /// The P2P connection to `peer` was replaced by a new one (a file moved it to the relay):
    /// a share being watched from them continues on the new connection with a fresh stream,
    /// starting at a keyframe. Where the new connection cannot carry it (the relay does not
    /// allow screen sharing), the watch ends with that reason as if it had failed to open.
    pub(crate) fn screen_share_relink(&self, server_id: ServerId, peer: u32) {
        let watching = lock(self.screen_share_state())
            .servers
            .get(&server_id)
            .is_some_and(|s| matches!(s.watching.get(&peer), Some(Watching::Open(_))));
        if !watching {
            return;
        }
        let engine = self.clone();
        self.runtime().spawn(async move {
            let opened = engine
                .p2p_open_stream(server_id, peer, crate::p2p::STREAM_SCREEN, crate::p2p::ConsentPrompt::Allowed)
                .await;
            let volume = engine.person_share_volume(server_id, peer);
            let unwatch = {
                let mut all = lock(engine.screen_share_state());
                let sink = all.sink.clone();
                let Some(shares) = all.servers.get_mut(&server_id) else {
                    return;
                };
                // Stopped watching meanwhile.
                let Some(Watching::Open(link)) = shares.watching.get_mut(&peer) else {
                    return;
                };
                match opened {
                    Ok((send, recv)) => {
                        let decode_codecs = link.decode_codecs();
                        let max_fps = link.max_fps();
                        *link = watch::WatchLink::start(
                            engine.runtime(),
                            server_id,
                            peer,
                            sink,
                            sound::ShareSound::new(engine.voice_shared().clone(), volume),
                            send,
                            recv,
                            Some(MediaTapSink::new(&engine.inspector(), server_id)),
                        );
                        link.set_decode_codecs(&decode_codecs);
                        link.set_max_fps(max_fps);
                        link.request_keyframe();
                        false
                    }
                    Err(e) => {
                        tracing::info!(error = %e, "screen share not continued on the new connection");
                        shares.watching.remove(&peer);
                        if let Some(block) = WatchBlock::of(&e) {
                            shares.failed.insert(peer, block);
                        }
                        true
                    }
                }
            };
            if let Ok(ctx) = engine.session_ctx_for(server_id) {
                if unwatch {
                    send(&ctx.handle, &[peer], &Signal::Unwatch);
                }
                emit(engine.screen_share_state(), engine.mumcord_state(), &ctx);
            }
        });
    }

    /// Stops watching `sharer`'s share; its stream closes.
    pub fn screen_share_unwatch(&self, server_id: ServerId, sharer: u32) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let watched = lock(self.screen_share_state())
            .servers
            .get_mut(&server_id)
            .and_then(|s| s.watching.remove(&sharer))
            .is_some();
        if watched {
            send(&ctx.handle, &[sharer], &Signal::Unwatch);
        }
        Ok(())
    }

    /// Where the frames of every share the user watches go (FR-2.44). Applies to watches that
    /// start afterwards; without a sink, frames are read and dropped.
    pub fn set_screen_frame_sink(&self, sink: Arc<dyn ScreenFrameSink>) {
        lock(self.screen_share_state()).sink = Some(sink);
    }

    /// Decodes `sharer`'s frames of `codecs` here (dav1d for AV1, libvpx for VP9) and hands
    /// pictures to [`ScreenFrameSink::on_decoded`] instead of encoded frames, for the codecs the
    /// webview's WebCodecs cannot decode (FR-7.59). Replaces the earlier list.
    pub fn screen_share_set_decode_codecs(
        &self,
        server_id: ServerId,
        sharer: u32,
        codecs: &[wire::Codec],
    ) -> EngineResult<()> {
        self.with_watch(server_id, sharer, |link| link.set_decode_codecs(codecs))
    }

    /// The viewer's frame-rate choice for `sharer`'s share (FR-7.51), on either path: at most
    /// about `max_fps` frames per second, by dropping temporal layers here and, through the
    /// relay broadcast, at the relay too (FR-7.45); `None` is Auto.
    pub fn screen_share_set_viewer_fps(
        &self,
        server_id: ServerId,
        sharer: u32,
        max_fps: Option<u32>,
    ) -> EngineResult<()> {
        self.with_watch(server_id, sharer, |link| link.set_max_fps(max_fps))
    }

    /// Asks `sharer` for a keyframe, e.g. after the UI's decoder failed on a frame.
    pub fn screen_share_request_keyframe(
        &self,
        server_id: ServerId,
        sharer: u32,
    ) -> EngineResult<()> {
        self.with_watch(server_id, sharer, watch::WatchLink::request_keyframe)
    }

    fn with_watch(
        &self,
        server_id: ServerId,
        sharer: u32,
        act: impl FnOnce(&watch::WatchLink),
    ) -> EngineResult<()> {
        match lock(self.screen_share_state())
            .servers
            .get(&server_id)
            .and_then(|s| s.watching.get(&sharer))
        {
            Some(Watching::Open(link)) => {
                act(link);
                Ok(())
            }
            _ => Err(EngineError::new(
                ErrorKind::NotFound,
                "not watching that share",
            )),
        }
    }

    /// How the own share on `server_id` is sent right now; `None` without one.
    pub fn screen_share_media_stats(&self, server_id: ServerId) -> Option<ScreenMediaStats> {
        let (mut stats, encoder, relay_viewers) = {
            let all = lock(self.screen_share_state());
            let own = all.servers.get(&server_id)?.own.as_ref()?;
            let relay_viewers = own
                .fanout
                .as_ref()
                .map(|f| f.info(Instant::now()).relay_viewers)
                .unwrap_or_default();
            (
                own.media.as_ref()?.stats(),
                own.active_encoder(),
                relay_viewers,
            )
        };
        // Read after the share state is released: the P2P links have their own lock.
        stats.add_viewer_paths(encoder, |session| {
            match self.p2p_link(server_id, session).map(|l| l.path) {
                Some(mumcord_p2p::ConnectionPath::Relay(_)) => ViewerPath::Relay,
                Some(mumcord_p2p::ConnectionPath::Direct) | None => ViewerPath::Direct,
            }
        });
        stats.add_broadcast_viewers(encoder, &relay_viewers);
        Some(stats)
    }

    /// The encoder the own share on `server_id` runs on (codec badge, FR-7.39); `None` without
    /// a share or before its encoder opened.
    pub fn screen_share_encoder(&self, server_id: ServerId) -> Option<ActiveEncoder> {
        lock(self.screen_share_state())
            .servers
            .get(&server_id)?
            .own
            .as_ref()?
            .active_encoder()
    }

    /// The start dialog's "Viewers get" line for these picks (FR-7.36): the encoder, rate and
    /// path a share started now would get. Starts no self-test ([`Self::screen_encoder_prepare`]
    /// does).
    pub fn screen_share_preview(
        &self,
        server_id: ServerId,
        prefs: QualityPrefs,
        codec: mumcord_store::ScreenCodec,
    ) -> SharePreview {
        let mut settings = self.encoder_settings();
        settings.screen_codec = codec;
        let selection = self.screen_encoders().preview(&settings, prefs);
        let av1_blocked = lock(self.screen_share_state())
            .servers
            .get(&server_id)
            .and_then(|s| s.own.as_ref())
            .is_some_and(|own| own.av1_blocker().is_some());
        preview::preview(
            &selection,
            av1_blocked,
            self.screen_broadcast_max(server_id),
        )
    }

    /// How many viewers a relay broadcast takes on `server_id` (FR-7.40); `None` without
    /// fan-out. A running share answers with its own broadcast, or `None` once it is on
    /// direct copies.
    pub fn screen_broadcast_max(&self, server_id: ServerId) -> Option<u8> {
        let running = lock(self.screen_share_state())
            .servers
            .get(&server_id)
            .and_then(|s| s.own.as_ref())
            .and_then(|own| own.fanout.as_ref())
            .map(Fanout::view);
        let limit = match running {
            Some(FanoutView::Relay { limit }) => limit,
            Some(FanoutView::Direct { .. }) => return None,
            Some(FanoutView::Starting) | None => self.broadcast_limit(server_id)?,
        };
        Some(u8::try_from(limit).unwrap_or(u8::MAX))
    }

    /// The own share's current picks, so the Quality menu checks what the share really uses,
    /// including a pick made only for this share in the start dialog; `None` without a share.
    pub fn screen_share_choice(&self, server_id: ServerId) -> Option<QualityPrefs> {
        lock(self.screen_share_state())
            .servers
            .get(&server_id)?
            .own
            .as_ref()?
            .media
            .as_ref()
            .map(media::Media::choice)
    }

    /// Changes the own share's resolution and frame rate ceiling, "Optimize for" and upload
    /// limit while it runs (Quality menu, FR-7.39); applies within a frame or two.
    pub fn screen_share_set_quality(
        &self,
        server_id: ServerId,
        prefs: QualityPrefs,
    ) -> EngineResult<()> {
        let all = lock(self.screen_share_state());
        let own = all
            .servers
            .get(&server_id)
            .and_then(|s| s.own.as_ref())
            .ok_or_else(not_sharing)?;
        // Software AV1 stays at 1080p30 at most (FR-7.37).
        let software_av1 = lock(&own.encoder)
            .is_some_and(|e| e.codec == mumcord_screenshare::Codec::Av1 && e.gpu.is_none());
        let prefs = if software_av1 {
            encoder::select::cap_for_software_av1(prefs)
        } else {
            prefs
        };
        own.media.as_ref().ok_or_else(not_sharing)?.set_prefs(prefs);
        Ok(())
    }

    /// "Undo" on "Switched to VP9 to reduce load": back to the encoder before the fallback
    /// (FR-7.57). Fails when the share offers no Undo (any more).
    pub fn screen_share_undo_encoder_switch(&self, server_id: ServerId) -> EngineResult<()> {
        let all = lock(self.screen_share_state());
        let media = all
            .servers
            .get(&server_id)
            .and_then(|s| s.own.as_ref())
            .and_then(|own| own.media.as_ref())
            .ok_or_else(not_sharing)?;
        if media.undo_encoder_switch() {
            Ok(())
        } else {
            Err(EngineError::invalid("the share offers no undo"))
        }
    }

    /// The picture the sharer's control bar shows for the own share (issue #163, FR-3.127).
    pub fn screen_share_thumbnail(&self, server_id: ServerId) -> Option<ShareThumbnail> {
        lock(self.screen_share_state())
            .servers
            .get(&server_id)
            .and_then(|s| s.own.as_ref())
            .and_then(|own| own.media.as_ref())
            .and_then(media::Media::thumbnail)
    }

    /// Makes this client's watch requests list only `codecs`, like a viewer that cannot play
    /// the others (test hook, FR-7.58); `None` lists every codec again.
    #[cfg(feature = "test-backend")]
    pub fn screen_share_set_playable_codecs(&self, codecs: Option<&[wire::Codec]>) {
        lock(self.screen_share_state()).playable = codecs.map(|codecs| {
            codecs
                .iter()
                .map(|codec| match codec {
                    wire::Codec::Vp9 => signal::CodecName::Vp9,
                    wire::Codec::Av1 => signal::CodecName::Av1,
                })
                .collect()
        });
    }

    /// The watch request for a share on `server_id`: through the relay broadcast when this
    /// side can join one (FR-7.42).
    fn watch_request(&self, server_id: ServerId) -> Signal {
        #[cfg_attr(
            not(feature = "test-backend"),
            expect(unused_mut, reason = "test hook")
        )]
        let mut request = signal::watch_request(self.broadcast_limit(server_id).is_some());
        #[cfg(feature = "test-backend")]
        if let (Some(playable), Signal::Watch { codecs, .. }) = (
            lock(self.screen_share_state()).playable.clone(),
            &mut request,
        ) {
            *codecs = playable;
        }
        request
    }

    /// Throttles the own share's upload in-process, to `viewer` or (with `None`) to all viewers
    /// together, since network-level throttling needs root; `kbps: None` lifts it (AC-2.29).
    #[cfg(feature = "test-backend")]
    pub fn screen_share_set_upload_limit(
        &self,
        server_id: ServerId,
        viewer: Option<u32>,
        kbps: Option<u32>,
    ) -> EngineResult<()> {
        let all = lock(self.screen_share_state());
        let media = all
            .servers
            .get(&server_id)
            .and_then(|s| s.own.as_ref())
            .and_then(|own| own.media.as_ref())
            .ok_or_else(not_sharing)?;
        media.set_upload_limit(viewer, kbps);
        Ok(())
    }

    /// The shares in the user's channel on `server_id` (empty when not connected).
    pub fn screen_shares(&self, server_id: ServerId) -> Vec<ScreenShareView> {
        self.session_ctx_for(server_id).map_or_else(
            |_| Vec::new(),
            |ctx| views(self.screen_share_state(), self.mumcord_state(), &ctx),
        )
    }
}
/// `settings` with this share's picks applied, and the quality prefs to start with. Only the
/// returned copy changes; the stored Settings stay as they are.
fn with_picks(
    mut settings: mumcord_store::Settings,
    prefs: Option<QualityPrefs>,
    codec: Option<mumcord_store::ScreenCodec>,
) -> (mumcord_store::Settings, QualityPrefs) {
    if let Some(codec) = codec {
        settings.screen_codec = codec;
    }
    let prefs = prefs.unwrap_or_else(|| QualityPrefs::from_settings(&settings));
    (settings, prefs)
}

/// The capture of the app the user chose in "Which app's sound?"; an error when it stopped
/// playing meanwhile.
fn chosen_sound(id: &str) -> EngineResult<SoundOutcome> {
    #[cfg(target_os = "linux")]
    {
        match sound::pick::start_chosen(id) {
            Ok(Some(outcome)) => Ok(outcome),
            Ok(None) => Err(EngineError::new(
                ErrorKind::NotFound,
                "that app no longer plays sound",
            )),
            Err(reason) => Ok(SoundOutcome::Unavailable(reason)),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = id;
        Err(EngineError::invalid("no sound choice on this system"))
    }
}

#[cfg(test)]
mod pick_tests {
    use mumcord_store::{ScreenCodec, ScreenFps, ScreenResolution, Settings};

    use super::*;

    #[test]
    fn a_per_share_pick_overrides_the_settings_copy_only() {
        let stored = Settings {
            screen_codec: ScreenCodec::Auto,
            screen_fps: ScreenFps::Fps30,
            ..Settings::default()
        };
        let picks = QualityPrefs {
            resolution: ScreenResolution::P720,
            fps: ScreenFps::Fps60,
            ..QualityPrefs::default()
        };
        let (used, prefs) = with_picks(stored.clone(), Some(picks), Some(ScreenCodec::Vp9));
        assert_eq!(used.screen_codec, ScreenCodec::Vp9);
        assert_eq!(prefs, picks);
        assert_eq!(
            stored.screen_codec,
            ScreenCodec::Auto,
            "Settings stay as saved"
        );
        assert_eq!(stored.screen_fps, ScreenFps::Fps30);
    }

    #[test]
    fn without_picks_the_settings_apply() {
        let stored = Settings::default();
        let (used, prefs) = with_picks(stored.clone(), None, None);
        assert_eq!(used.screen_codec, stored.screen_codec);
        assert_eq!(prefs, QualityPrefs::from_settings(&stored));
    }
}
