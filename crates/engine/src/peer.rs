//! Relay discovery and trust (spec 04, FR-4.7 … FR-4.12, FR-4.20) and relay token requests
//! (FR-4.14).
//!
//! A relay bot (mumbot `peer`) announces the community relay of a server with plugin data
//! [`PEER_DATA_ID`]. Only announcements of registered users with a certificate are accepted;
//! the first accepted one pins the pair (bot certificate hash, relay fingerprint) per server
//! entry and identity (trust on first use). A different pair is not used until the user trusts
//! it. Two announcing bots at once cancel each other out. The relay counts as available while
//! the pinned announcement is `up` and its bot is on the server; a bot that leaves gets
//! [`RelayTimings::bot_grace`] to come back.
//!
//! Tokens are credentials and relay addresses personal data: neither is logged.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use mumcord_p2p::{
    BroadcastErrorReason, BroadcastId, BroadcastKind, MAX_PEER_BYTES, PEER_DATA_ID, PeerMessage,
    RequestId, TokenErrorReason,
};
pub use mumcord_p2p::{RelayAnnouncement, RelayRefusal, RelayState, RelayToken, RelayUse};
use mumcord_protocol::PluginData;
use mumcord_store::{IdentityId, RelayPin, ServerId, Store, UnixMillis};
use tokio::sync::oneshot;
use tokio::time::Instant;

use crate::camera::avail::CameraUnavailable;
use crate::inspector::Inspector;
use crate::screen_share::broadcast::FanoutUnavailable;
use crate::session::{
    Engine, EngineError, EngineEvent, EngineResult, Events, SessionCtx, WeakEngine, lock,
    store_error,
};

/// Why the relay cannot be used. The UI maps each code to its text (FR-4.4, FR-4.11, FR-4.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayUnavailable {
    /// "This server has no relay" (FR-4.4, FR-4.12).
    NoRelay,
    /// The server's connection is not up.
    NotConnected,
    /// The announced relay differs from the pinned one and is not trusted (FR-4.9).
    Untrusted,
    /// Two relays are announced; none is used (FR-4.10).
    Conflict,
    /// "The relay bot left the server." (FR-4.11)
    BotLeft,
    /// "The relay is not reachable right now (reported by the relay bot)." (FR-4.11; also the
    /// bot's `relay-down` answer)
    RelayDown,
    /// "The relay bot did not answer." (FR-4.11, FR-4.14)
    BotNoAnswer,
    /// "The relay refused the connection: {reason}" (FR-4.11, spec section 6.5).
    Refused(RelayRefusal),
    /// "The relay could not be reached over UDP or TCP 443." (FR-4.11)
    Unreachable,
    /// "The other person can't use the relay (older Mumcord or relay turned off)." (FR-4.14;
    /// also the bot's `peer-not-mumcord` answer)
    PeerCannotRelay,
    /// The bot refused the token (`denied`).
    Denied,
    /// This client is not registered on the server, and the relay only serves registered users
    /// (#406): "This server's relay is only available to registered users. …". The bot's
    /// `not-registered` answer, or its `denied` while this client is a guest (`denied` is then
    /// almost always the default `registered_only` rule).
    NotRegistered,
    /// The relay only serves members of certain groups, and this client is in none (#601).
    GroupNotAllowed,
    /// The bot has not seen this client's hello, e.g. after the bot restarted; reconnecting
    /// sends it again (#601).
    NoHello,
    /// The other person left the server before the bot answered (`peer-gone`).
    PeerGone,
    /// Too many token requests (`rate-limited`).
    RateLimited,
    /// The relay has no room for another pair (`relay-full`).
    RelayFull,
    /// The relay's owner turned this use off (FR-7.7): "This server's relay doesn't allow …".
    /// Announced in `u`, or the bot's `use-not-allowed` answer.
    UseNotAllowed(RelayUse),
}

impl RelayUnavailable {
    /// The bot's `token-err` for a token request made for `relay_use`; `registered` tells
    /// whether this client is registered on the server.
    fn of_token_error(reason: TokenErrorReason, relay_use: RelayUse, registered: bool) -> Self {
        match reason {
            TokenErrorReason::Denied if !registered => Self::NotRegistered,
            TokenErrorReason::Denied => Self::Denied,
            TokenErrorReason::NotRegistered => Self::NotRegistered,
            TokenErrorReason::GroupNotAllowed => Self::GroupNotAllowed,
            TokenErrorReason::NoHello => Self::NoHello,
            TokenErrorReason::UseNotAllowed => Self::UseNotAllowed(relay_use),
            TokenErrorReason::PeerGone => Self::PeerGone,
            TokenErrorReason::PeerNotMumcord => Self::PeerCannotRelay,
            TokenErrorReason::RateLimited => Self::RateLimited,
            TokenErrorReason::RelayDown => Self::RelayDown,
            TokenErrorReason::RelayFull => Self::RelayFull,
        }
    }
}

/// Every use, in the order the server info panel lists them (FR-7.7).
const ALL_USES: [RelayUse; 5] = [
    RelayUse::Images,
    RelayUse::Files,
    RelayUse::ScreenShare,
    RelayUse::Camera,
    RelayUse::PluginData,
];

/// State of the server's relay for the server info panel (FR-4.20).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RelayStatus {
    /// No relay announced.
    #[default]
    None,
    Available,
    Unavailable(RelayUnavailable),
    /// "The relay of this server changed" (FR-4.9); `kept_blocked` once the user chose
    /// "Keep blocked".
    Untrusted {
        kept_blocked: bool,
    },
    /// "Two relays are announced on this server; none is used." (FR-4.10)
    Conflict,
}

/// What the server info panel shows about the relay of a connected server (FR-4.20).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RelayInfo {
    /// User names of the registered users announcing a relay ("Mumbot: Yes"); two on a
    /// conflict, none without an announcement.
    pub bots: Vec<String>,
    /// Host of the announced relay.
    pub host: Option<String>,
    pub status: RelayStatus,
    /// When the pinned relay was first accepted.
    pub first_seen: Option<UnixMillis>,
    /// What the announced relay may be used for ("Relay allows: …", FR-7.7); all uses when
    /// the announcement has no `u` (older bot, FR-7.6), none without an announced relay.
    pub uses: Vec<RelayUse>,
    /// Most viewers per broadcast (`bc`, FR-7.3); `None` when the relay offers no broadcasts.
    pub broadcast_max: Option<u16>,
    /// The relay only serves registered users (announced `reg`, #601).
    pub registered_only: bool,
    /// The relay speaks the Bovnar media profile (announced `mv`, FR-7.96).
    pub media: bool,
    /// The announced relay's certificate fingerprint.
    pub fingerprint: Option<String>,
    /// The trusted relay's fingerprint while a changed relay waits for trust (#600), so the user
    /// can compare the two before trusting the new one.
    pub previous_fingerprint: Option<String>,
}

/// Waiting times of the relay logic; shorter in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayTimings {
    /// How long a relay stays available after its bot left (FR-4.11).
    pub bot_grace: Duration,
    /// How long a token request waits for the bot (FR-4.14).
    pub token_wait: Duration,
}

impl Default for RelayTimings {
    fn default() -> Self {
        Self {
            bot_grace: Duration::from_secs(15),
            token_wait: Duration::from_secs(5),
        }
    }
}

/// Whether this client is registered on the server of `ctx` (has a user id).
pub(crate) fn self_registered(ctx: &SessionCtx) -> bool {
    ctx.handle.with_state(|s| {
        s.session
            .and_then(|own| s.users.get(&own))
            .is_some_and(|u| u.user_id.is_some())
    })
}

/// Inspector note for announcements of unregistered users (FR-4.8).
const NOT_REGISTERED: &str = "sender not registered";

/// The pair pinned on first use: bot certificate hash and relay fingerprint, lowercase hex.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pair {
    bot: String,
    fp: String,
}

#[derive(Debug, Clone)]
struct Announcer {
    session: u32,
    name: String,
    pair: Pair,
    ann: RelayAnnouncement,
}

impl Announcer {
    /// Whether the announcement allows `relay_use`. Cameras are the exception to "no `u` allows
    /// all" (FR-7.6): a bot without `u` predates them and can't handle camera broadcasts
    /// (FR-8.2).
    fn allows(&self, relay_use: RelayUse) -> bool {
        if relay_use == RelayUse::Camera && self.ann.uses.is_none() {
            return false;
        }
        self.ann.allows(relay_use)
    }
}

/// What became of an announcement.
#[derive(Debug, PartialEq, Eq)]
enum Announced {
    /// Older `seq` than the last one of the same sender.
    Stale,
    /// Used; `pin` is set when it is the first one and has to be pinned.
    Accepted { pin: Option<Pair> },
}

/// The relay of one connected server (spec 04 section 7.2).
#[derive(Default)]
struct ServerRelay {
    pin: Option<RelayPin>,
    /// Registered senders announcing a relay, by session.
    announcers: BTreeMap<u32, Announcer>,
    /// The only announcer left the server; it stays usable until the timer of this
    /// generation expires.
    gone: Option<(Announcer, u64)>,
    /// The trusted bot left and did not come back in time.
    left: bool,
    /// The untrusted pair the user chose to keep blocked.
    kept_blocked: Option<Pair>,
}

impl ServerRelay {
    fn trusted(&self, pair: &Pair) -> bool {
        self.pin
            .as_ref()
            .is_some_and(|p| p.bot_cert_hash == pair.bot && p.relay_fp == pair.fp)
    }

    fn announce(&mut self, from: Announcer) -> Announced {
        if self
            .announcers
            .get(&from.session)
            .is_some_and(|last| from.ann.seq < last.ann.seq)
        {
            return Announced::Stale;
        }
        self.gone = None;
        self.left = false;
        let first = self.pin.is_none() && self.announcers.keys().all(|s| *s == from.session);
        let pin = first.then(|| from.pair.clone());
        self.announcers.insert(from.session, from);
        Announced::Accepted { pin }
    }

    /// `session` left; returns `true` when it was the only announcer and the grace time of
    /// generation `generation` starts.
    fn leave(&mut self, session: u32, generation: u64) -> bool {
        let Some(announcer) = self.announcers.remove(&session) else {
            return false;
        };
        // Leaving a conflict re-evaluates the other announcer at once (section 7.2).
        if !self.announcers.is_empty() {
            return false;
        }
        self.gone = Some((announcer, generation));
        true
    }

    /// The grace time of `generation` passed; `true` if that ended the relay.
    fn expire(&mut self, generation: u64) -> bool {
        let Some((announcer, _)) = self.gone.take_if(|(_, g)| *g == generation) else {
            return false;
        };
        self.left = self.trusted(&announcer.pair);
        true
    }

    /// The one announcer in use, present or within its grace time; `None` on a conflict.
    fn single(&self) -> Option<&Announcer> {
        match self.announcers.len() {
            0 => self.gone.as_ref().map(|(a, _)| a),
            1 => self.announcers.values().next(),
            _ => None,
        }
    }

    fn info(&self) -> RelayInfo {
        let first_seen = self.pin.as_ref().map(|p| p.first_seen);
        if self.announcers.len() > 1 {
            return RelayInfo {
                bots: self.announcers.values().map(|a| a.name.clone()).collect(),
                status: RelayStatus::Conflict,
                first_seen,
                ..RelayInfo::default()
            };
        }
        let Some(announcer) = self.single() else {
            let status = if self.left {
                RelayStatus::Unavailable(RelayUnavailable::BotLeft)
            } else {
                RelayStatus::None
            };
            return RelayInfo {
                status,
                first_seen,
                ..RelayInfo::default()
            };
        };
        let untrusted = !self.trusted(&announcer.pair);
        let status = if untrusted {
            RelayStatus::Untrusted {
                kept_blocked: self.kept_blocked.as_ref() == Some(&announcer.pair),
            }
        } else if announcer.ann.state == RelayState::Down {
            RelayStatus::Unavailable(RelayUnavailable::RelayDown)
        } else {
            RelayStatus::Available
        };
        RelayInfo {
            bots: vec![announcer.name.clone()],
            host: Some(announcer.ann.host.clone()),
            status,
            first_seen,
            uses: ALL_USES
                .into_iter()
                .filter(|u| announcer.allows(*u))
                .collect(),
            broadcast_max: announcer.ann.broadcast,
            registered_only: announcer.ann.registered_only,
            media: announcer.ann.media,
            fingerprint: Some(announcer.pair.fp.clone()),
            previous_fingerprint: if untrusted {
                self.pin.as_ref().map(|p| p.relay_fp.clone())
            } else {
                None
            },
        }
    }

    /// The bot to ask for a token, present on the server, with a trusted relay that is up.
    fn usable(&self) -> Result<&Announcer, RelayUnavailable> {
        if self.announcers.len() > 1 {
            return Err(RelayUnavailable::Conflict);
        }
        let Some(announcer) = self.announcers.values().next() else {
            return Err(if self.left || self.gone.is_some() {
                RelayUnavailable::BotLeft
            } else {
                RelayUnavailable::NoRelay
            });
        };
        if !self.trusted(&announcer.pair) {
            return Err(RelayUnavailable::Untrusted);
        }
        if announcer.ann.state == RelayState::Down {
            return Err(RelayUnavailable::RelayDown);
        }
        Ok(announcer)
    }

    /// [`Self::usable`] for `relay_use`: the announcement must allow it (FR-7.6).
    fn usable_for(&self, relay_use: RelayUse) -> Result<&Announcer, RelayUnavailable> {
        let announcer = self.usable()?;
        if !announcer.allows(relay_use) {
            return Err(RelayUnavailable::UseNotAllowed(relay_use));
        }
        Ok(announcer)
    }

    /// The pair the user trusts instead of the pinned one ("Trust new relay").
    fn untrusted_pair(&self) -> Option<Pair> {
        self.single()
            .filter(|a| !self.trusted(&a.pair))
            .map(|a| a.pair.clone())
    }
}

/// Who a received `token` is for.
#[derive(Debug, PartialEq, Eq)]
enum TokenRoute {
    /// This client asked for it.
    Requester,
    /// This client is the peer of `requester`.
    Peer {
        requester: u32,
    },
    Ignore,
}

/// A token is accepted only from the pinned bot and only if this client is `a` or `b`
/// (section 6.4).
fn route_token(own: Option<u32>, bot: Option<u32>, sender: u32, a: u32, b: u32) -> TokenRoute {
    if bot != Some(sender) || own.is_none() {
        TokenRoute::Ignore
    } else if own == Some(a) {
        TokenRoute::Requester
    } else if own == Some(b) {
        TokenRoute::Peer { requester: a }
    } else {
        TokenRoute::Ignore
    }
}

type TokenReply = Result<RelayToken, RelayUnavailable>;

/// The bot's answer to `bcast-req` or `bcast-admit` (spec 07, section 6.6).
type BroadcastReply = Result<Option<(BroadcastId, RelayToken)>, BroadcastErrorReason>;

/// Where a `bcast-tok` goes. A sender can run a broadcast of each kind at once, so the token is
/// handed only to the side of the kind the bot named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BroadcastTokenRoute {
    ScreenShare,
    PluginData,
    Camera,
    /// No side takes it; only the reason is logged.
    Ignore(&'static str),
}

/// How long the sharer listens for a `bcast-err` after `bcast-admit`; the bot answers a
/// successful admission only to the viewer, so silence means admitted.
const ADMIT_ERROR_WAIT: Duration = Duration::from_secs(5);

struct BroadcastPending {
    bot: u32,
    reply: oneshot::Sender<BroadcastReply>,
}

struct Pending {
    bot: u32,
    peer: u32,
    /// What the token was asked for; names the use a `use-not-allowed` answer refers to.
    relay_use: RelayUse,
    reply: oneshot::Sender<TokenReply>,
}

#[derive(Default)]
struct State {
    servers: HashMap<ServerId, ServerRelay>,
    /// Own token requests waiting for the bot.
    requests: HashMap<(ServerId, RequestId), Pending>,
    /// Tokens for which this client is the peer, by requester, until the P2P step takes them.
    arrived: HashMap<(ServerId, u32), (RelayToken, Instant)>,
    /// The P2P step waiting for the token of a requester.
    waiting: HashMap<(ServerId, u32), oneshot::Sender<RelayToken>>,
    /// Own `bcast-req` and `bcast-admit` waiting for the bot.
    broadcasts: HashMap<(ServerId, RequestId), BroadcastPending>,
    /// Generation of the bot-gone timers; unique across servers and reconnects.
    generation: u64,
}

/// Relay state of the engine.
#[derive(Default)]
pub(crate) struct Relays {
    engine: OnceLock<WeakEngine>,
    state: Mutex<State>,
    /// The camera availability last reported per server (FR-8.2), to emit only changes.
    camera: Mutex<HashMap<ServerId, Option<CameraUnavailable>>>,
    timings: Mutex<RelayTimings>,
}

impl Relays {
    pub(crate) fn bind(&self, engine: WeakEngine) {
        if self.engine.set(engine).is_err() {
            tracing::warn!("relay state bound twice");
        }
    }

    fn timings(&self) -> RelayTimings {
        *lock(&self.timings)
    }

    pub(crate) fn info(&self, server_id: ServerId) -> RelayInfo {
        lock(&self.state)
            .servers
            .get(&server_id)
            .map(ServerRelay::info)
            .unwrap_or_default()
    }

    /// Whether the relay of `server_id` can be used right now, else why not (FR-4.11).
    pub(crate) fn availability(&self, server_id: ServerId) -> Result<(), RelayUnavailable> {
        lock(&self.state)
            .servers
            .get(&server_id)
            .ok_or(RelayUnavailable::NoRelay)?
            .usable()
            .map(|_| ())
    }

    /// [`Self::availability`] for one use (FR-7.6): the relay must also allow `relay_use`.
    pub(crate) fn availability_for(
        &self,
        server_id: ServerId,
        relay_use: RelayUse,
    ) -> Result<(), RelayUnavailable> {
        lock(&self.state)
            .servers
            .get(&server_id)
            .ok_or(RelayUnavailable::NoRelay)?
            .usable_for(relay_use)
            .map(|_| ())
    }

    /// The relay's per-sender cap for plugin data (`pk`), when it announces one.
    pub(crate) fn plugin_data_kbps(&self, server_id: ServerId) -> Option<u32> {
        lock(&self.state)
            .servers
            .get(&server_id)
            .and_then(ServerRelay::single)
            .filter(|a| a.ann.allows(RelayUse::PluginData))
            .and_then(|a| a.ann.plugin_data_kbps)
    }

    /// Whether the relay of `server_id` may carry `relay_use` by its current announcement. Only
    /// a relay that is in use can carry anything, so without one this is `true`: there is no
    /// relayed connection to refuse.
    pub(crate) fn allows(&self, server_id: ServerId, relay_use: RelayUse) -> bool {
        lock(&self.state)
            .servers
            .get(&server_id)
            .and_then(ServerRelay::single)
            .is_none_or(|a| a.ann.allows(relay_use))
    }

    /// Reports the camera availability of `server_id` after the relay changed.
    fn refresh_camera(&self, server_id: ServerId) {
        if let Some(engine) = self.engine.get().and_then(WeakEngine::upgrade) {
            engine.camera_availability_refresh(server_id);
        }
    }

    /// Remembers `reason` as the camera availability of `server_id`; whether it differs from the
    /// one reported before (the first report always counts).
    pub(crate) fn camera_changed(
        &self,
        server_id: ServerId,
        reason: Option<CameraUnavailable>,
    ) -> bool {
        lock(&self.camera).insert(server_id, reason) != Some(reason)
    }

    /// Runs `change` on the server's relay; returns the new info if the panel changes.
    fn change(
        &self,
        server_id: ServerId,
        change: impl FnOnce(&mut ServerRelay),
    ) -> Option<RelayInfo> {
        let mut state = lock(&self.state);
        let relay = state.servers.entry(server_id).or_default();
        let before = relay.info();
        change(relay);
        let after = relay.info();
        (after != before).then_some(after)
    }

    /// `mumcord.peer` plugin data from the session task.
    pub(crate) fn on_plugin_data(
        &self,
        ctx: &SessionCtx,
        data: &PluginData,
        inspector: &Inspector,
    ) {
        if data.data_id != PEER_DATA_ID {
            return;
        }
        // Only Mumble user sessions count as senders (FR-4.7).
        let Some(sender) = data.sender else {
            return;
        };
        if data.data.len() > MAX_PEER_BYTES {
            tracing::debug!("ignoring an oversized relay message");
            return;
        }
        let (own, user) = ctx.handle.with_state(|s| {
            (
                s.session,
                s.users
                    .get(&sender)
                    .map(|u| (u.name.clone(), u.user_id, u.cert_hash.clone())),
            )
        });
        let Some((name, user_id, cert_hash)) = user else {
            return;
        };
        if own == Some(sender) {
            return;
        }
        let message = match PeerMessage::decode(&data.data) {
            Ok(message) => message,
            Err(e) => {
                tracing::debug!(error = %e, "ignoring a relay message");
                return;
            }
        };
        match message {
            PeerMessage::Relay(ann) => {
                let (Some(_), Some(cert_hash)) = (user_id, cert_hash) else {
                    inspector.count_ignored(ctx.server_id, PEER_DATA_ID, NOT_REGISTERED);
                    return;
                };
                let announcer = Announcer {
                    session: sender,
                    name,
                    pair: Pair {
                        bot: cert_hash.to_ascii_lowercase(),
                        fp: hex(&ann.fingerprint),
                    },
                    ann,
                };
                self.on_announcement(ctx, announcer);
            }
            PeerMessage::Token {
                rid,
                a,
                b,
                token,
                ttl,
            } => {
                self.on_token(ctx.server_id, own, sender, rid, a, b, token, ttl);
            }
            PeerMessage::TokenError { rid, reason } => {
                let registered = self_registered(ctx);
                self.on_token_error(ctx.server_id, sender, rid, reason, registered);
            }
            PeerMessage::Broadcast { rid, id, token, .. } => {
                self.on_broadcast_reply(ctx.server_id, sender, rid, Ok(Some((id, token))));
            }
            PeerMessage::BroadcastError { rid, reason } => {
                self.on_broadcast_reply(ctx.server_id, sender, rid, Err(reason));
            }
            PeerMessage::BroadcastToken {
                id,
                token,
                sharer,
                kind,
                media,
                ..
            } => self.on_broadcast_token(ctx, sender, sharer, kind, id, token, media),
            // Only relay bots receive token requests and the sharer's broadcast messages.
            PeerMessage::TokenRequest { .. }
            | PeerMessage::BroadcastRequest { .. }
            | PeerMessage::BroadcastAdmit { .. }
            | PeerMessage::BroadcastKick { .. } => {}
        }
    }

    fn on_announcement(&self, ctx: &SessionCtx, announcer: Announcer) {
        let (identity, pin) = {
            let store = ctx.store();
            match pin_identity(&store, ctx.server_id).and_then(|identity| {
                let pin = match identity {
                    Some(identity) => store.relay_pin(ctx.server_id, identity)?,
                    None => None,
                };
                Ok((identity, pin))
            }) {
                Ok(found) => found,
                Err(e) => {
                    tracing::warn!(error = %e, "reading the relay pin failed");
                    return;
                }
            }
        };
        let mut announced = Announced::Stale;
        let mut first_seen = 0;
        let changed = self.change(ctx.server_id, |relay| {
            relay.pin = pin;
            announced = relay.announce(announcer);
            if let Announced::Accepted { pin: Some(pair) } = &announced {
                first_seen = crate::chat::now_millis();
                relay.pin = Some(RelayPin {
                    bot_cert_hash: pair.bot.clone(),
                    relay_fp: pair.fp.clone(),
                    first_seen,
                });
            }
        });
        match announced {
            Announced::Stale => tracing::debug!("ignoring a stale relay announcement"),
            Announced::Accepted { pin: Some(pair) } => {
                tracing::info!("relay pinned on first use");
                if let Some(identity) = identity {
                    write_pin(&ctx.store(), ctx.server_id, identity, &pair, first_seen);
                }
            }
            Announced::Accepted { pin: None } => {}
        }
        if let Some(info) = changed {
            ctx.events.emit(EngineEvent::RelayChanged(info));
            self.refresh_camera(ctx.server_id);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn on_token(
        &self,
        server_id: ServerId,
        own: Option<u32>,
        sender: u32,
        rid: RequestId,
        a: u32,
        b: u32,
        token: RelayToken,
        ttl: u16,
    ) {
        let mut state = lock(&self.state);
        let bot = state
            .servers
            .get(&server_id)
            .and_then(|r| r.usable().ok())
            .map(|a| a.session);
        match route_token(own, bot, sender, a, b) {
            TokenRoute::Requester => {
                let key = (server_id, rid);
                if state.requests.get(&key).is_some_and(|p| p.peer == b)
                    && let Some(pending) = state.requests.remove(&key)
                {
                    // The requester may have given up already.
                    let _ = pending.reply.send(Ok(token));
                }
            }
            TokenRoute::Peer { requester } => {
                let key = (server_id, requester);
                let token = match state.waiting.remove(&key) {
                    Some(waiter) => match waiter.send(token) {
                        Ok(()) => return,
                        Err(token) => token,
                    },
                    None => token,
                };
                let expires = Instant::now() + Duration::from_secs(ttl.into());
                state.arrived.insert(key, (token, expires));
            }
            TokenRoute::Ignore => tracing::debug!("ignoring a relay token not meant for us"),
        }
    }

    /// `bcast` or `bcast-err` for an own request; only the bot it went to may answer.
    fn on_broadcast_reply(
        &self,
        server_id: ServerId,
        sender: u32,
        rid: RequestId,
        reply: BroadcastReply,
    ) {
        let mut state = lock(&self.state);
        let key = (server_id, rid);
        if state.broadcasts.get(&key).is_some_and(|p| p.bot == sender)
            && let Some(pending) = state.broadcasts.remove(&key)
        {
            let _ = pending.reply.send(reply);
        }
    }

    /// `bcast-tok` (FR-7.42): handed to the side of its broadcast kind; each side checks that
    /// this client waits for that sharer.
    #[allow(clippy::too_many_arguments)]
    fn on_broadcast_token(
        &self,
        ctx: &SessionCtx,
        sender: u32,
        sharer: u32,
        kind: BroadcastKind,
        id: BroadcastId,
        token: RelayToken,
        media: bool,
    ) {
        let route = self.broadcast_token_route(ctx.server_id, sender, kind);
        let engine = || self.engine.get().and_then(WeakEngine::upgrade);
        match route {
            BroadcastTokenRoute::ScreenShare => {
                if let Some(engine) = engine() {
                    engine.screen_share_broadcast_token(ctx, sharer, id, token, media);
                }
            }
            BroadcastTokenRoute::PluginData => {
                if let Some(engine) = engine() {
                    engine.plugin_relay_broadcast_token(ctx, sharer, id, token);
                }
            }
            BroadcastTokenRoute::Camera => {
                if let Some(engine) = engine() {
                    engine.camera_broadcast_token(ctx, sharer, id, token);
                }
            }
            BroadcastTokenRoute::Ignore(reason) => {
                tracing::debug!("ignoring a broadcast token: {reason}");
            }
        }
    }

    /// Only the pinned bot that is usable for the token's kind may hand out its tokens.
    fn broadcast_token_route(
        &self,
        server_id: ServerId,
        sender: u32,
        kind: BroadcastKind,
    ) -> BroadcastTokenRoute {
        let pinned = lock(&self.state)
            .servers
            .get(&server_id)
            .and_then(|r| r.usable_for(kind.relay_use()).ok())
            .is_some_and(|a| a.session == sender);
        if !pinned {
            return BroadcastTokenRoute::Ignore("not from the pinned relay bot for its kind");
        }
        match kind {
            BroadcastKind::ScreenShare => BroadcastTokenRoute::ScreenShare,
            BroadcastKind::PluginData => BroadcastTokenRoute::PluginData,
            BroadcastKind::Camera => BroadcastTokenRoute::Camera,
        }
    }

    /// The announcement of the relay usable for broadcasts of `relay_use` (screen sharing or
    /// plugin data), with the bot to ask.
    fn broadcast_bot(
        &self,
        server_id: ServerId,
        relay_use: RelayUse,
    ) -> Result<(u32, RelayAnnouncement), RelayUnavailable> {
        lock(&self.state)
            .servers
            .get(&server_id)
            .ok_or(RelayUnavailable::NoRelay)?
            .usable_for(relay_use)
            .map(|a| (a.session, a.ann.clone()))
    }

    /// The relay announcement for joining a screen-share broadcast as a viewer.
    pub(crate) fn broadcast_announcement(
        &self,
        server_id: ServerId,
    ) -> Result<RelayAnnouncement, RelayUnavailable> {
        self.broadcast_announcement_for(server_id, RelayUse::ScreenShare)
    }

    /// The relay announcement for a `relay_use` broadcast (camera sharing uses `Camera`).
    pub(crate) fn broadcast_announcement_for(
        &self,
        server_id: ServerId,
        relay_use: RelayUse,
    ) -> Result<RelayAnnouncement, RelayUnavailable> {
        self.broadcast_bot(server_id, relay_use).map(|(_, ann)| ann)
    }

    /// The relay announcement for joining a plugin-data broadcast as a receiver.
    pub(crate) fn plugin_broadcast_announcement(
        &self,
        server_id: ServerId,
    ) -> Result<RelayAnnouncement, RelayUnavailable> {
        self.broadcast_bot(server_id, RelayUse::PluginData)
            .map(|(_, ann)| ann)
    }

    /// Sends a broadcast message to `bot` and registers `rid` for the answer.
    fn send_broadcast(
        &self,
        ctx: &SessionCtx,
        bot: u32,
        rid: RequestId,
        message: &PeerMessage,
    ) -> Option<oneshot::Receiver<BroadcastReply>> {
        let (reply, rx) = oneshot::channel();
        lock(&self.state)
            .broadcasts
            .insert((ctx.server_id, rid), BroadcastPending { bot, reply });
        let sent = message
            .encode()
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                ctx.handle
                    .send_plugin_data(&[bot], PEER_DATA_ID, bytes)
                    .map_err(|e| e.to_string())
            });
        if let Err(e) = sent {
            tracing::debug!(error = %e, "relay broadcast message not sent");
            lock(&self.state).broadcasts.remove(&(ctx.server_id, rid));
            return None;
        }
        Some(rx)
    }

    /// Waits up to `wait` for the answer to `rid`; `Ok(None)` when none came.
    async fn finish_broadcast(
        &self,
        server_id: ServerId,
        rid: RequestId,
        rx: oneshot::Receiver<BroadcastReply>,
        wait: Duration,
    ) -> Option<BroadcastReply> {
        let answer = tokio::time::timeout(wait, rx).await;
        lock(&self.state).broadcasts.remove(&(server_id, rid));
        match answer {
            Ok(Ok(reply)) => Some(reply),
            // The bot left (the request was dropped) or the server connection ended.
            Ok(Err(_)) => Some(Err(BroadcastErrorReason::RelayDown)),
            Err(_) => None,
        }
    }

    fn on_token_error(
        &self,
        server_id: ServerId,
        sender: u32,
        rid: RequestId,
        reason: TokenErrorReason,
        registered: bool,
    ) {
        let mut state = lock(&self.state);
        let key = (server_id, rid);
        if state.requests.get(&key).is_some_and(|p| p.bot == sender)
            && let Some(pending) = state.requests.remove(&key)
        {
            let _ = pending.reply.send(Err(RelayUnavailable::of_token_error(
                reason,
                pending.relay_use,
                registered,
            )));
        }
    }

    /// A person left the server: their token exchanges end, and an announcing bot starts its
    /// grace time.
    pub(crate) fn on_user_removed(&self, ctx: &SessionCtx, session: u32) {
        let server_id = ctx.server_id;
        let mut grace = None;
        let changed = {
            let mut state = lock(&self.state);
            state.generation += 1;
            let generation = state.generation;
            state.arrived.remove(&(server_id, session));
            state.waiting.remove(&(server_id, session));
            let gone: Vec<_> = state
                .requests
                .iter()
                .filter(|((id, _), p)| *id == server_id && (p.peer == session || p.bot == session))
                .map(|(key, _)| *key)
                .collect();
            state
                .broadcasts
                .retain(|(id, _), p| *id != server_id || p.bot != session);
            for key in gone {
                if let Some(pending) = state.requests.remove(&key) {
                    let reason = if pending.bot == session {
                        RelayUnavailable::BotLeft
                    } else {
                        RelayUnavailable::PeerGone
                    };
                    let _ = pending.reply.send(Err(reason));
                }
            }
            drop(state);
            self.change(server_id, |relay| {
                if relay.leave(session, generation) {
                    grace = Some(generation);
                }
            })
        };
        if let Some(generation) = grace {
            self.start_grace(server_id, generation, ctx.events.clone());
        }
        if let Some(info) = changed {
            ctx.events.emit(EngineEvent::RelayChanged(info));
            self.refresh_camera(ctx.server_id);
        }
    }

    fn start_grace(&self, server_id: ServerId, generation: u64, events: Events) {
        let Some(engine) = self.engine.get().and_then(WeakEngine::upgrade) else {
            return;
        };
        let wait = self.timings().bot_grace;
        let weak = engine.downgrade();
        engine.runtime().spawn(async move {
            tokio::time::sleep(wait).await;
            let Some(engine) = weak.upgrade() else {
                return;
            };
            if let Some(info) = engine.relays().expire(server_id, generation) {
                events.emit(EngineEvent::RelayChanged(info));
                engine.camera_availability_refresh(server_id);
            }
        });
    }

    /// The bot-gone timer of `generation` fired; the new info if the relay ended.
    fn expire(&self, server_id: ServerId, generation: u64) -> Option<RelayInfo> {
        let mut state = lock(&self.state);
        let relay = state.servers.get_mut(&server_id)?;
        relay.expire(generation).then(|| relay.info())
    }

    /// The connection to `server_id` (re)synced or ended; returns `true` if the panel had
    /// something to show.
    pub(crate) fn reset(&self, server_id: ServerId) -> bool {
        let mut state = lock(&self.state);
        let shown = state
            .servers
            .remove(&server_id)
            .is_some_and(|r| r.info() != RelayInfo::default());
        let ended: Vec<_> = state
            .requests
            .keys()
            .filter(|(id, _)| *id == server_id)
            .copied()
            .collect();
        for key in ended {
            if let Some(pending) = state.requests.remove(&key) {
                let _ = pending.reply.send(Err(RelayUnavailable::NotConnected));
            }
        }
        state.arrived.retain(|(id, _), _| *id != server_id);
        state.waiting.retain(|(id, _), _| *id != server_id);
        shown
    }

    /// Registers a token request; returns the bot to send it to.
    fn begin_request(
        &self,
        server_id: ServerId,
        peer: u32,
        rid: RequestId,
        relay_use: RelayUse,
    ) -> Result<(u32, RelayAnnouncement, oneshot::Receiver<TokenReply>), RelayUnavailable> {
        let mut state = lock(&self.state);
        let (bot, ann) = state
            .servers
            .get(&server_id)
            .ok_or(RelayUnavailable::NoRelay)?
            .usable_for(relay_use)
            .map(|a| (a.session, a.ann.clone()))?;
        let (reply, rx) = oneshot::channel();
        state.requests.insert(
            (server_id, rid),
            Pending {
                bot,
                peer,
                relay_use,
                reply,
            },
        );
        Ok((bot, ann, rx))
    }

    /// Waits for the bot's answer to the request `rid` (FR-4.14).
    async fn finish_request(
        &self,
        server_id: ServerId,
        rid: RequestId,
        rx: oneshot::Receiver<TokenReply>,
    ) -> TokenReply {
        let answer = tokio::time::timeout(self.timings().token_wait, rx).await;
        lock(&self.state).requests.remove(&(server_id, rid));
        match answer {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => Err(RelayUnavailable::NotConnected),
            Err(_) => Err(RelayUnavailable::BotNoAnswer),
        }
    }

    /// Waits up to `wait` for the token the bot sends this client as the peer of `requester`.
    async fn await_token(
        &self,
        server_id: ServerId,
        requester: u32,
        wait: Duration,
    ) -> Result<(RelayAnnouncement, RelayToken), RelayUnavailable> {
        let key = (server_id, requester);
        let (ann, rx) = {
            let mut state = lock(&self.state);
            let ann = state
                .servers
                .get(&server_id)
                .ok_or(RelayUnavailable::NoRelay)?
                .usable()?
                .ann
                .clone();
            if let Some((token, expires)) = state.arrived.remove(&key)
                && Instant::now() < expires
            {
                return Ok((ann, token));
            }
            let (tx, rx) = oneshot::channel();
            state.waiting.insert(key, tx);
            (ann, rx)
        };
        let token = tokio::time::timeout(wait, rx).await;
        let mut state = lock(&self.state);
        if state
            .waiting
            .get(&key)
            .is_some_and(oneshot::Sender::is_closed)
        {
            state.waiting.remove(&key);
        }
        match token {
            Ok(Ok(token)) => Ok((ann, token)),
            Ok(Err(_)) => Err(RelayUnavailable::NotConnected),
            Err(_) => Err(RelayUnavailable::BotNoAnswer),
        }
    }
}

/// The identity a server entry connects with: its own if it still exists, else the default.
pub(crate) fn pin_identity(
    store: &Store,
    server_id: ServerId,
) -> mumcord_store::Result<Option<IdentityId>> {
    if let Some(id) = store.server(server_id)?.and_then(|s| s.identity_id)
        && store.identity(id)?.is_some()
    {
        return Ok(Some(id));
    }
    Ok(store.default_identity()?.map(|record| record.id))
}

fn write_pin(
    store: &Store,
    server_id: ServerId,
    identity: IdentityId,
    pair: &Pair,
    at: UnixMillis,
) {
    let pin = RelayPin {
        bot_cert_hash: pair.bot.clone(),
        relay_fp: pair.fp.clone(),
        first_seen: at,
    };
    if let Err(e) = store.set_relay_pin(server_id, identity, &pin) {
        tracing::warn!(error = %e, "storing the relay pin failed");
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // Writing to a String cannot fail.
        let _ = write!(out, "{b:02x}");
    }
    out
}

impl Engine {
    /// The relay of `server_id` as the server info panel shows it (FR-4.20); empty when not
    /// connected. Changes arrive as [`EngineEvent::RelayChanged`].
    pub fn relay_info(&self, server_id: ServerId) -> RelayInfo {
        self.relays().info(server_id)
    }

    /// "Trust new relay" (FR-4.9): pins the changed relay announced on `server_id`.
    pub fn relay_trust_new(&self, server_id: ServerId) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let identity = pin_identity(&ctx.store(), server_id).map_err(store_error)?;
        let Some(identity) = identity else {
            return Err(EngineError::invalid("no identity to pin the relay for"));
        };
        let first_seen = crate::chat::now_millis();
        let mut pair = None;
        let changed = self.relays().change(server_id, |relay| {
            pair = relay.untrusted_pair();
            if let Some(pair) = &pair {
                relay.pin = Some(RelayPin {
                    bot_cert_hash: pair.bot.clone(),
                    relay_fp: pair.fp.clone(),
                    first_seen,
                });
                relay.kept_blocked = None;
            }
        });
        let Some(pair) = pair else {
            return Err(EngineError::invalid("no changed relay to trust"));
        };
        let pin = RelayPin {
            bot_cert_hash: pair.bot,
            relay_fp: pair.fp,
            first_seen,
        };
        ctx.store()
            .set_relay_pin(server_id, identity, &pin)
            .map_err(store_error)?;
        tracing::info!("changed relay trusted by the user");
        if let Some(info) = changed {
            ctx.events.emit(EngineEvent::RelayChanged(info));
            self.camera_availability_refresh(ctx.server_id);
        }
        Ok(())
    }

    /// "Keep blocked" (FR-4.9): the changed relay stays unused and the panel stops asking.
    pub fn relay_keep_blocked(&self, server_id: ServerId) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let changed = self.relays().change(server_id, |relay| {
            if let Some(pair) = relay.untrusted_pair() {
                relay.kept_blocked = Some(pair);
            }
        });
        if let Some(info) = changed {
            ctx.events.emit(EngineEvent::RelayChanged(info));
            self.camera_availability_refresh(ctx.server_id);
        }
        Ok(())
    }

    /// "Forget relay" (FR-4.9): removes the pin of `server_id`. The next announcement is
    /// pinned afresh; until then the server shows no relay.
    pub fn relay_forget(&self, server_id: ServerId) -> EngineResult<()> {
        let identity = {
            let store = self.store_handle();
            let store = lock(&store);
            let identity = pin_identity(&store, server_id).map_err(store_error)?;
            if let Some(identity) = identity {
                store
                    .forget_relay_pin(server_id, identity)
                    .map_err(store_error)?;
            }
            identity
        };
        if identity.is_none() {
            return Ok(());
        }
        let changed = self
            .relays()
            .change(server_id, |relay| *relay = ServerRelay::default());
        if let (Some(info), Ok(ctx)) = (changed, self.session_ctx_for(server_id)) {
            ctx.events.emit(EngineEvent::RelayChanged(info));
            self.camera_availability_refresh(server_id);
        }
        Ok(())
    }

    /// Asks the pinned relay bot of `server_id` for a token for this client and `peer` (the
    /// offering side, FR-4.14), for the feature's `relay_use` (FR-7.5). Returns the announcement
    /// to connect to and the token. A use the relay does not allow is not asked for (FR-7.6).
    pub async fn relay_token_request(
        &self,
        server_id: ServerId,
        peer: u32,
        relay_use: RelayUse,
    ) -> Result<(RelayAnnouncement, RelayToken), RelayUnavailable> {
        let ctx = self
            .session_ctx_for(server_id)
            .map_err(|_| RelayUnavailable::NotConnected)?;
        let rid = RequestId::random();
        let (bot, ann, rx) = self
            .relays()
            .begin_request(server_id, peer, rid, relay_use)?;
        // Always named: the relay refuses a pair without `use` unless every use is on.
        let message = PeerMessage::TokenRequest {
            rid,
            peer,
            relay_use: Some(relay_use),
        };
        let sent = message
            .encode()
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                ctx.handle
                    .send_plugin_data(&[bot], PEER_DATA_ID, bytes)
                    .map_err(|e| e.to_string())
            });
        if let Err(e) = sent {
            tracing::debug!(error = %e, "relay token request not sent");
            lock(&self.relays().state)
                .requests
                .remove(&(server_id, rid));
            return Err(RelayUnavailable::NotConnected);
        }
        let token = self.relays().finish_request(server_id, rid, rx).await?;
        Ok((ann, token))
    }

    /// Asks the relay bot for a broadcast of the own screen share (`relay_use` `ScreenShare`,
    /// sent without `k` as before spec 08) or camera (`Camera`, `k = cam`, FR-8.42) for up to
    /// `max` viewers (`bcast-req`, FR-7.40). Returns the relay to bind to, the broadcast and the
    /// publisher token, or why fan-out is not available.
    pub(crate) async fn relay_broadcast_request(
        &self,
        server_id: ServerId,
        relay_use: RelayUse,
        max: u16,
        media: bool,
    ) -> Result<(RelayAnnouncement, BroadcastId, RelayToken), FanoutUnavailable> {
        let ctx = self
            .session_ctx_for(server_id)
            .map_err(|_| FanoutUnavailable::Relay(RelayUnavailable::NotConnected))?;
        let (bot, ann) = self
            .relays()
            .broadcast_bot(server_id, relay_use)
            .map_err(FanoutUnavailable::Relay)?;
        // The publisher's format is fixed for the broadcast and needs the relay's capability.
        let media = media && ann.media;
        let rid = RequestId::random();
        // Screen shares leave `k` out, which bots from before plugin data understand.
        let message = PeerMessage::BroadcastRequest {
            rid,
            max,
            kind: (relay_use != RelayUse::ScreenShare).then_some(relay_use),
            media,
        };
        let rx = self
            .relays()
            .send_broadcast(&ctx, bot, rid, &message)
            .ok_or(FanoutUnavailable::Relay(RelayUnavailable::NotConnected))?;
        let wait = self.relays().timings().token_wait;
        match self
            .relays()
            .finish_broadcast(server_id, rid, rx, wait)
            .await
        {
            Some(Ok(Some((id, token)))) => Ok((ann, id, token)),
            Some(Ok(None)) => Err(FanoutUnavailable::Relay(RelayUnavailable::BotNoAnswer)),
            Some(Err(reason)) => Err(FanoutUnavailable::Refused(reason)),
            None => Err(FanoutUnavailable::Relay(RelayUnavailable::BotNoAnswer)),
        }
    }

    /// Asks the relay bot to admit `viewer` to broadcast `id` (`bcast-admit`, FR-7.42). The bot
    /// answers only a refusal; `Ok` after a short wait without one.
    pub(crate) async fn relay_broadcast_admit(
        &self,
        server_id: ServerId,
        relay_use: RelayUse,
        id: BroadcastId,
        viewer: u32,
    ) -> Result<(), BroadcastErrorReason> {
        let ctx = self
            .session_ctx_for(server_id)
            .map_err(|_| BroadcastErrorReason::RelayDown)?;
        let (bot, _) = self
            .relays()
            .broadcast_bot(server_id, relay_use)
            .map_err(|_| BroadcastErrorReason::RelayDown)?;
        let rid = RequestId::random();
        let message = PeerMessage::BroadcastAdmit { rid, id, viewer };
        let rx = self
            .relays()
            .send_broadcast(&ctx, bot, rid, &message)
            .ok_or(BroadcastErrorReason::RelayDown)?;
        match self
            .relays()
            .finish_broadcast(server_id, rid, rx, ADMIT_ERROR_WAIT)
            .await
        {
            Some(Err(reason)) => Err(reason),
            Some(Ok(_)) | None => Ok(()),
        }
    }

    /// Tells the relay bot that `viewer` left broadcast `id` (`bcast-kick`); no answer.
    pub(crate) fn relay_broadcast_kick(
        &self,
        server_id: ServerId,
        relay_use: RelayUse,
        id: BroadcastId,
        viewer: u32,
    ) {
        let Ok(ctx) = self.session_ctx_for(server_id) else {
            return;
        };
        let Ok((bot, _)) = self.relays().broadcast_bot(server_id, relay_use) else {
            return;
        };
        let sent = PeerMessage::BroadcastKick { id, viewer }
            .encode()
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                ctx.handle
                    .send_plugin_data(&[bot], PEER_DATA_ID, bytes)
                    .map_err(|e| e.to_string())
            });
        if let Err(e) = sent {
            tracing::debug!(error = %e, "relay broadcast kick not sent");
        }
    }

    /// Waits up to `wait` for the token the pinned bot sends this client as the peer of
    /// `requester` (the answering side, FR-4.14). A token that arrived before the call is kept
    /// until its `ttl` ends.
    pub async fn relay_token_await(
        &self,
        server_id: ServerId,
        requester: u32,
        wait: Duration,
    ) -> Result<(RelayAnnouncement, RelayToken), RelayUnavailable> {
        self.relays().await_token(server_id, requester, wait).await
    }

    /// Changes the relay waiting times (tests use short ones).
    #[doc(hidden)]
    pub fn relay_set_timings(&self, timings: RelayTimings) {
        *lock(&self.relays().timings) = timings;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER_BOT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn ann(fp: u8, state: RelayState, seq: u32) -> RelayAnnouncement {
        RelayAnnouncement {
            host: "relay.test".to_owned(),
            udp: 4443,
            tcp: 443,
            fingerprint: [fp; 32],
            state,
            seq,
            uses: None,
            broadcast: None,
            plugin_data_kbps: None,
            registered_only: false,
            media: false,
        }
    }

    fn announcer(session: u32, bot: &str, ann: RelayAnnouncement) -> Announcer {
        Announcer {
            session,
            name: format!("bot{session}"),
            pair: Pair {
                bot: bot.to_owned(),
                fp: hex(&ann.fingerprint),
            },
            ann,
        }
    }

    fn pinned(bot: &str, fp: u8) -> RelayPin {
        RelayPin {
            bot_cert_hash: bot.to_owned(),
            relay_fp: hex(&[fp; 32]),
            first_seen: 7,
        }
    }

    /// A server relay that accepted `announcer` as its first, pinned announcement.
    fn trusted_relay(announcer: Announcer) -> ServerRelay {
        let mut relay = ServerRelay {
            pin: Some(RelayPin {
                bot_cert_hash: announcer.pair.bot.clone(),
                relay_fp: announcer.pair.fp.clone(),
                first_seen: 7,
            }),
            ..ServerRelay::default()
        };
        relay.announce(announcer);
        relay
    }

    #[test]
    fn the_first_announcement_is_pinned_and_available() {
        let mut relay = ServerRelay::default();
        let first = announcer(5, BOT, ann(1, RelayState::Up, 1));
        let pair = first.pair.clone();
        assert_eq!(
            relay.announce(first),
            Announced::Accepted {
                pin: Some(pair.clone())
            }
        );
        relay.pin = Some(pinned(BOT, 1));
        let info = relay.info();
        assert_eq!(info.status, RelayStatus::Available);
        assert_eq!(info.bots, ["bot5"]);
        assert_eq!(info.host.as_deref(), Some("relay.test"));
        assert_eq!(info.first_seen, Some(7));
        assert_eq!(relay.usable().map(|a| a.session), Ok(5));
        // Re-announcing the same pair pins nothing more.
        assert_eq!(
            relay.announce(announcer(5, BOT, ann(1, RelayState::Up, 2))),
            Announced::Accepted { pin: None }
        );
    }

    #[test]
    fn a_changed_bot_or_fingerprint_is_not_trusted() {
        for changed in [
            announcer(5, OTHER_BOT, ann(1, RelayState::Up, 1)),
            announcer(5, BOT, ann(2, RelayState::Up, 1)),
        ] {
            let mut relay = ServerRelay {
                pin: Some(pinned(BOT, 1)),
                ..ServerRelay::default()
            };
            assert_eq!(relay.announce(changed), Announced::Accepted { pin: None });
            assert_eq!(
                relay.info().status,
                RelayStatus::Untrusted {
                    kept_blocked: false
                }
            );
            assert_eq!(
                relay.usable().map(|a| a.session),
                Err(RelayUnavailable::Untrusted)
            );
            let pair = relay.untrusted_pair().expect("untrusted pair");
            relay.kept_blocked = Some(pair.clone());
            assert_eq!(
                relay.info().status,
                RelayStatus::Untrusted { kept_blocked: true }
            );
            // "Trust new relay" replaces the pin.
            relay.pin = Some(RelayPin {
                bot_cert_hash: pair.bot,
                relay_fp: pair.fp,
                first_seen: 9,
            });
            assert_eq!(relay.info().status, RelayStatus::Available);
            assert_eq!(relay.untrusted_pair(), None);
        }
    }

    #[test]
    fn address_changes_with_the_same_pin_are_accepted() {
        let mut relay = trusted_relay(announcer(5, BOT, ann(1, RelayState::Up, 1)));
        let mut moved = ann(1, RelayState::Up, 2);
        moved.host = "10.0.0.9".to_owned();
        moved.udp = 9000;
        relay.announce(announcer(5, BOT, moved));
        let info = relay.info();
        assert_eq!(info.status, RelayStatus::Available);
        assert_eq!(info.host.as_deref(), Some("10.0.0.9"));
        assert_eq!(relay.usable().map(|a| a.ann.udp), Ok(9000));
    }

    #[test]
    fn stale_announcements_are_ignored() {
        let mut relay = trusted_relay(announcer(5, BOT, ann(1, RelayState::Up, 4)));
        assert_eq!(
            relay.announce(announcer(5, BOT, ann(1, RelayState::Down, 3))),
            Announced::Stale
        );
        assert_eq!(relay.info().status, RelayStatus::Available);
        // The same seq again (a re-announcement to one client) is fine.
        assert_eq!(
            relay.announce(announcer(5, BOT, ann(1, RelayState::Up, 4))),
            Announced::Accepted { pin: None }
        );
    }

    #[test]
    fn down_and_up_switch_availability_at_once() {
        let mut relay = trusted_relay(announcer(5, BOT, ann(1, RelayState::Up, 1)));
        relay.announce(announcer(5, BOT, ann(1, RelayState::Down, 2)));
        assert_eq!(
            relay.info().status,
            RelayStatus::Unavailable(RelayUnavailable::RelayDown)
        );
        assert_eq!(
            relay.usable().map(|a| a.session),
            Err(RelayUnavailable::RelayDown)
        );
        relay.announce(announcer(5, BOT, ann(1, RelayState::Up, 3)));
        assert_eq!(relay.info().status, RelayStatus::Available);
    }

    #[test]
    fn two_announcing_bots_cancel_out_until_one_leaves() {
        let mut relay = trusted_relay(announcer(5, BOT, ann(1, RelayState::Up, 1)));
        assert_eq!(
            relay.announce(announcer(6, OTHER_BOT, ann(2, RelayState::Up, 1))),
            Announced::Accepted { pin: None }
        );
        let info = relay.info();
        assert_eq!(info.status, RelayStatus::Conflict);
        assert_eq!(info.bots, ["bot5", "bot6"]);
        assert_eq!(info.host, None);
        assert_eq!(
            relay.usable().map(|a| a.session),
            Err(RelayUnavailable::Conflict)
        );
        // The intruder leaves: the pinned one is used again at once, without a grace time.
        assert!(!relay.leave(6, 1));
        assert_eq!(relay.info().status, RelayStatus::Available);
        // The pinned one leaves instead: the other is evaluated and not trusted.
        relay.announce(announcer(6, OTHER_BOT, ann(2, RelayState::Up, 1)));
        assert!(!relay.leave(5, 2));
        assert_eq!(
            relay.info().status,
            RelayStatus::Untrusted {
                kept_blocked: false
            }
        );
    }

    #[test]
    fn a_leaving_bot_ends_the_relay_after_the_grace_time() {
        let mut relay = trusted_relay(announcer(5, BOT, ann(1, RelayState::Up, 1)));
        assert!(relay.leave(5, 1));
        // Within the grace time the panel still shows the relay, but nobody can be asked.
        assert_eq!(relay.info().status, RelayStatus::Available);
        assert_eq!(
            relay.usable().map(|a| a.session),
            Err(RelayUnavailable::BotLeft)
        );
        // A timer of an older generation changes nothing.
        assert!(!relay.expire(0));
        assert!(relay.expire(1));
        let info = relay.info();
        assert_eq!(
            info.status,
            RelayStatus::Unavailable(RelayUnavailable::BotLeft)
        );
        assert!(info.bots.is_empty());
        assert_eq!(
            relay.usable().map(|a| a.session),
            Err(RelayUnavailable::BotLeft)
        );
    }

    #[test]
    fn a_reannouncement_within_the_grace_time_keeps_the_relay() {
        let mut relay = trusted_relay(announcer(5, BOT, ann(1, RelayState::Up, 1)));
        assert!(relay.leave(5, 1));
        // The bot reconnected with a new session.
        relay.announce(announcer(9, BOT, ann(1, RelayState::Up, 1)));
        assert!(!relay.expire(1));
        assert_eq!(relay.info().status, RelayStatus::Available);
        assert_eq!(relay.usable().map(|a| a.session), Ok(9));
    }

    #[test]
    fn no_announcement_means_no_relay() {
        let relay = ServerRelay::default();
        assert_eq!(relay.info(), RelayInfo::default());
        assert_eq!(
            relay.usable().map(|a| a.session),
            Err(RelayUnavailable::NoRelay)
        );
    }

    #[test]
    fn tokens_are_accepted_only_from_the_pinned_bot_for_own_pairs() {
        // (own, bot, sender, a, b)
        assert_eq!(
            route_token(Some(1), Some(5), 5, 1, 2),
            TokenRoute::Requester
        );
        assert_eq!(
            route_token(Some(2), Some(5), 5, 1, 2),
            TokenRoute::Peer { requester: 1 }
        );
        assert_eq!(route_token(Some(3), Some(5), 5, 1, 2), TokenRoute::Ignore);
        assert_eq!(route_token(Some(1), Some(5), 6, 1, 2), TokenRoute::Ignore);
        assert_eq!(route_token(Some(1), None, 5, 1, 2), TokenRoute::Ignore);
        assert_eq!(route_token(None, Some(5), 5, 1, 2), TokenRoute::Ignore);
    }

    #[test]
    fn token_errors_map_to_reasons() {
        use TokenErrorReason as R;
        let mapped: Vec<RelayUnavailable> = [
            R::Denied,
            R::PeerGone,
            R::PeerNotMumcord,
            R::RateLimited,
            R::RelayDown,
            R::RelayFull,
            R::UseNotAllowed,
            R::NotRegistered,
        ]
        .into_iter()
        .map(|r| RelayUnavailable::of_token_error(r, RelayUse::Files, true))
        .collect();
        assert_eq!(
            mapped,
            [
                RelayUnavailable::Denied,
                RelayUnavailable::PeerGone,
                RelayUnavailable::PeerCannotRelay,
                RelayUnavailable::RateLimited,
                RelayUnavailable::RelayDown,
                RelayUnavailable::RelayFull,
                RelayUnavailable::UseNotAllowed(RelayUse::Files),
                RelayUnavailable::NotRegistered,
            ]
        );
    }

    #[test]
    fn a_guest_denied_a_token_is_told_to_register() {
        let guest = |r| RelayUnavailable::of_token_error(r, RelayUse::Files, false);
        assert_eq!(
            guest(TokenErrorReason::Denied),
            RelayUnavailable::NotRegistered
        );
        assert_eq!(
            guest(TokenErrorReason::NotRegistered),
            RelayUnavailable::NotRegistered
        );
        assert_eq!(
            guest(TokenErrorReason::RelayFull),
            RelayUnavailable::RelayFull
        );
    }

    /// Relays with a trusted bot (session 5) on server 1.
    fn relays() -> Relays {
        let relays = Relays::default();
        relays.change(1, |relay| {
            *relay = trusted_relay(announcer(5, BOT, ann(1, RelayState::Up, 1)));
        });
        relays
    }

    fn token(c: char) -> RelayToken {
        RelayToken::parse(&c.to_string().repeat(43)).expect("token")
    }

    #[tokio::test(start_paused = true)]
    async fn a_token_request_times_out_without_an_answer() {
        let relays = relays();
        let rid = RequestId(1);
        let (bot, _, rx) = relays
            .begin_request(1, 2, rid, RelayUse::Images)
            .expect("usable");
        assert_eq!(bot, 5);
        assert_eq!(
            relays.finish_request(1, rid, rx).await,
            Err(RelayUnavailable::BotNoAnswer)
        );
        assert!(lock(&relays.state).requests.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_token_request_gets_the_token_or_the_mapped_error() {
        let relays = relays();
        let (_, _, rx) = relays
            .begin_request(1, 2, RequestId(1), RelayUse::Images)
            .expect("usable");
        // Wrong sender, wrong peer, and someone else's pair are ignored.
        relays.on_token(1, Some(1), 6, RequestId(1), 1, 2, token('x'), 30);
        relays.on_token(1, Some(1), 5, RequestId(1), 1, 3, token('x'), 30);
        relays.on_token(1, Some(1), 5, RequestId(1), 4, 2, token('x'), 30);
        relays.on_token(1, Some(1), 5, RequestId(1), 1, 2, token('a'), 30);
        assert_eq!(
            relays.finish_request(1, RequestId(1), rx).await,
            Ok(token('a'))
        );

        let (_, _, rx) = relays
            .begin_request(1, 2, RequestId(2), RelayUse::Images)
            .expect("usable");
        relays.on_token_error(1, 6, RequestId(2), TokenErrorReason::RelayFull, true);
        relays.on_token_error(1, 5, RequestId(2), TokenErrorReason::PeerNotMumcord, true);
        assert_eq!(
            relays.finish_request(1, RequestId(2), rx).await,
            Err(RelayUnavailable::PeerCannotRelay)
        );
    }

    #[test]
    fn a_token_request_needs_a_usable_relay() {
        let relays = Relays::default();
        assert_eq!(
            relays
                .begin_request(1, 2, RequestId(1), RelayUse::Images)
                .err(),
            Some(RelayUnavailable::NoRelay)
        );
        let relays = relays_with_down();
        assert_eq!(
            relays
                .begin_request(1, 2, RequestId(1), RelayUse::Images)
                .err(),
            Some(RelayUnavailable::RelayDown)
        );
    }

    fn relays_with_down() -> Relays {
        let relays = relays();
        relays.change(1, |relay| {
            relay.announce(announcer(5, BOT, ann(1, RelayState::Down, 2)));
        });
        relays
    }

    #[tokio::test(start_paused = true)]
    async fn the_peer_gets_its_token_before_or_while_waiting() {
        let relays = relays();
        // Arrives before the P2P step asks for it.
        relays.on_token(1, Some(2), 5, RequestId(1), 1, 2, token('a'), 30);
        let (ann, got) = relays
            .await_token(1, 1, Duration::from_secs(5))
            .await
            .expect("token");
        assert_eq!(got, token('a'));
        assert_eq!(ann.host, "relay.test");

        // Arrives while waiting; one from another bot is ignored.
        let waiting = relays.await_token(1, 3, Duration::from_secs(5));
        tokio::pin!(waiting);
        assert!(futures_poll(&mut waiting).is_none());
        relays.on_token(1, Some(2), 6, RequestId(2), 3, 2, token('b'), 30);
        relays.on_token(1, Some(2), 5, RequestId(2), 3, 2, token('c'), 30);
        assert_eq!(waiting.await.map(|(_, t)| t), Ok(token('c')));

        // Nothing arrives.
        assert_eq!(
            relays
                .await_token(1, 4, Duration::from_secs(5))
                .await
                .map(|(_, t)| t),
            Err(RelayUnavailable::BotNoAnswer)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_early_token_expires_after_its_ttl() {
        let relays = relays();
        relays.on_token(1, Some(2), 5, RequestId(1), 1, 2, token('a'), 1);
        tokio::time::advance(Duration::from_secs(2)).await;
        assert_eq!(
            relays
                .await_token(1, 1, Duration::from_millis(10))
                .await
                .map(|(_, t)| t),
            Err(RelayUnavailable::BotNoAnswer)
        );
    }

    /// Polls `future` once.
    fn futures_poll<F: std::future::Future + Unpin>(future: &mut F) -> Option<F::Output> {
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        match std::pin::Pin::new(future).poll(&mut cx) {
            std::task::Poll::Ready(out) => Some(out),
            std::task::Poll::Pending => None,
        }
    }

    #[test]
    fn reset_fails_waiting_requests_and_clears_the_server() {
        let relays = relays();
        let (_, _, mut rx) = relays
            .begin_request(1, 2, RequestId(1), RelayUse::Images)
            .expect("usable");
        assert!(relays.reset(1));
        assert_eq!(rx.try_recv(), Ok(Err(RelayUnavailable::NotConnected)));
        assert_eq!(relays.info(1), RelayInfo::default());
        assert!(!relays.reset(1));
    }

    #[test]
    fn the_grace_timer_of_another_generation_does_nothing() {
        let relays = relays();
        relays.change(1, |relay| {
            relay.leave(5, 3);
        });
        assert_eq!(relays.expire(1, 2), None);
        assert_eq!(
            relays.expire(1, 3).map(|i| i.status),
            Some(RelayStatus::Unavailable(RelayUnavailable::BotLeft))
        );
    }

    /// Relays on server 1 whose trusted bot announces `uses` and `bc`.
    fn relays_with_uses(uses: Option<Vec<RelayUse>>, broadcast: Option<u16>) -> Relays {
        let relays = Relays::default();
        relays.change(1, |relay| {
            *relay = trusted_relay(announcer(
                5,
                BOT,
                RelayAnnouncement {
                    uses,
                    broadcast,
                    ..ann(1, RelayState::Up, 1)
                },
            ));
        });
        relays
    }

    #[test]
    fn plugin_data_needs_its_use_and_reports_the_cap() {
        let relays = Relays::default();
        relays.change(1, |relay| {
            *relay = trusted_relay(announcer(
                5,
                BOT,
                RelayAnnouncement {
                    uses: Some(vec![RelayUse::PluginData]),
                    plugin_data_kbps: Some(512),
                    ..ann(1, RelayState::Up, 1)
                },
            ));
        });
        assert_eq!(relays.availability_for(1, RelayUse::PluginData), Ok(()));
        assert_eq!(relays.plugin_data_kbps(1), Some(512));
        assert_eq!(relays.info(1).uses, [RelayUse::PluginData]);
    }

    #[test]
    fn broadcast_tokens_go_to_the_side_of_their_kind() {
        let relays = relays_with_uses(
            Some(vec![
                RelayUse::ScreenShare,
                RelayUse::PluginData,
                RelayUse::Camera,
            ]),
            Some(10),
        );
        // A sender with a screen share, plugin data and a camera broadcast at once: each token
        // reaches only its own side.
        assert_eq!(
            relays.broadcast_token_route(1, 5, BroadcastKind::ScreenShare),
            BroadcastTokenRoute::ScreenShare
        );
        assert_eq!(
            relays.broadcast_token_route(1, 5, BroadcastKind::PluginData),
            BroadcastTokenRoute::PluginData
        );
        assert_eq!(
            relays.broadcast_token_route(1, 5, BroadcastKind::Camera),
            BroadcastTokenRoute::Camera
        );
    }

    #[test]
    fn an_old_bot_without_kind_reaches_screen_sharing_only() {
        let relays = relays_with_uses(None, Some(10));
        let old_bot = format!(
            r#"{{"v":1,"t":"bcast-tok","id":"0000000000000001","tok":"{}","ttl":60,"sharer":8}}"#,
            "A".repeat(43)
        );
        let Ok(PeerMessage::BroadcastToken { kind, .. }) = PeerMessage::decode(old_bot.as_bytes())
        else {
            panic!("an old bot's token must decode");
        };
        assert_eq!(
            relays.broadcast_token_route(1, 5, kind),
            BroadcastTokenRoute::ScreenShare
        );
    }

    #[test]
    fn broadcast_tokens_need_the_pinned_bot_usable_for_their_kind() {
        let relays = relays_with_uses(Some(vec![RelayUse::ScreenShare]), Some(10));
        assert!(matches!(
            relays.broadcast_token_route(1, 5, BroadcastKind::PluginData),
            BroadcastTokenRoute::Ignore(_)
        ));
        assert!(matches!(
            relays.broadcast_token_route(1, 6, BroadcastKind::ScreenShare),
            BroadcastTokenRoute::Ignore(_)
        ));
        assert!(matches!(
            relays.broadcast_token_route(2, 5, BroadcastKind::ScreenShare),
            BroadcastTokenRoute::Ignore(_)
        ));
    }

    #[test]
    fn an_announcement_without_uses_allows_only_the_old_uses() {
        let relays = relays_with_uses(None, None);
        let old_uses = [RelayUse::Images, RelayUse::Files, RelayUse::ScreenShare];
        for relay_use in old_uses {
            assert_eq!(relays.availability_for(1, relay_use), Ok(()));
            assert!(relays.allows(1, relay_use));
        }
        // A bot without `u` predates cameras and can't handle their broadcasts (FR-8.2).
        assert_eq!(
            relays.availability_for(1, RelayUse::Camera),
            Err(RelayUnavailable::UseNotAllowed(RelayUse::Camera))
        );
        // Plugin data is off unless announced (spec 09 FR-9.84).
        assert_eq!(
            relays.availability_for(1, RelayUse::PluginData),
            Err(RelayUnavailable::UseNotAllowed(RelayUse::PluginData))
        );
        assert_eq!(relays.plugin_data_kbps(1), None);
        let info = relays.info(1);
        assert_eq!(info.uses, old_uses);
        assert_eq!(info.broadcast_max, None);
    }

    #[test]
    fn cam_in_uses_makes_the_relay_usable_for_cameras() {
        let relays = relays_with_uses(Some(vec![RelayUse::Camera]), Some(10));
        assert_eq!(relays.availability_for(1, RelayUse::Camera), Ok(()));
        assert_eq!(relays.info(1).uses, [RelayUse::Camera]);
    }

    #[test]
    fn the_camera_availability_is_reported_only_when_it_changes() {
        let relays = Relays::default();
        let none = None;
        let old = Some(CameraUnavailable::RelayTooOld);
        assert!(relays.camera_changed(1, none), "first report counts");
        assert!(!relays.camera_changed(1, none));
        assert!(relays.camera_changed(1, old));
        assert!(!relays.camera_changed(1, old));
        assert!(relays.camera_changed(1, none));
        assert!(relays.camera_changed(2, none), "servers are independent");
    }

    #[test]
    fn a_disabled_use_makes_the_relay_unavailable_for_it_only() {
        let relays = relays_with_uses(
            Some(vec![RelayUse::ScreenShare, RelayUse::Images]),
            Some(25),
        );
        assert_eq!(relays.availability(1), Ok(()));
        assert_eq!(relays.availability_for(1, RelayUse::Images), Ok(()));
        assert_eq!(
            relays.availability_for(1, RelayUse::Files),
            Err(RelayUnavailable::UseNotAllowed(RelayUse::Files))
        );
        assert!(!relays.allows(1, RelayUse::Files));
        let info = relays.info(1);
        assert_eq!(info.status, RelayStatus::Available);
        assert_eq!(
            info.uses,
            [RelayUse::Images, RelayUse::ScreenShare],
            "panel order"
        );
        assert_eq!(info.broadcast_max, Some(25));
        assert_eq!(
            relays
                .begin_request(1, 2, RequestId(1), RelayUse::Files)
                .err(),
            Some(RelayUnavailable::UseNotAllowed(RelayUse::Files)),
            "no token is asked for a disabled use"
        );
    }

    #[test]
    fn other_uses_in_a_new_announcement_change_the_panel() {
        let relays = relays_with_uses(None, Some(25));
        let changed = relays.change(1, |relay| {
            relay.announce(announcer(
                5,
                BOT,
                RelayAnnouncement {
                    uses: Some(vec![RelayUse::Images]),
                    broadcast: None,
                    plugin_data_kbps: None,
                    ..ann(1, RelayState::Up, 2)
                },
            ));
        });
        let info = changed.expect("the panel changes");
        assert_eq!(info.uses, [RelayUse::Images]);
        assert_eq!(relays.plugin_data_kbps(1), None);
        assert_eq!(info.broadcast_max, None);
    }

    #[test]
    fn without_a_relay_nothing_is_allowed_or_refused() {
        let relays = Relays::default();
        assert!(relays.info(1).uses.is_empty());
        assert_eq!(
            relays.availability_for(1, RelayUse::Images),
            Err(RelayUnavailable::NoRelay)
        );
        assert!(
            relays.allows(1, RelayUse::Images),
            "no relayed link to refuse"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn use_not_allowed_names_the_requested_use() {
        let relays = relays();
        let (_, _, rx) = relays
            .begin_request(1, 2, RequestId(1), RelayUse::ScreenShare)
            .expect("usable");
        relays.on_token_error(1, 5, RequestId(1), TokenErrorReason::UseNotAllowed, true);
        assert_eq!(
            relays.finish_request(1, RequestId(1), rx).await,
            Err(RelayUnavailable::UseNotAllowed(RelayUse::ScreenShare))
        );
    }
}
