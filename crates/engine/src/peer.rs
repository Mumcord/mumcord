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

use mumcord_p2p::{MAX_PEER_BYTES, PEER_DATA_ID, PeerMessage, RequestId, TokenErrorReason};
pub use mumcord_p2p::{RelayAnnouncement, RelayRefusal, RelayState, RelayToken};
use mumcord_protocol::PluginData;
use mumcord_store::{IdentityId, RelayPin, ServerId, Store, UnixMillis};
use tokio::sync::oneshot;
use tokio::time::Instant;

use crate::inspector::Inspector;
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
    /// The other person left the server before the bot answered (`peer-gone`).
    PeerGone,
    /// Too many token requests (`rate-limited`).
    RateLimited,
    /// The relay has no room for another pair (`relay-full`).
    RelayFull,
}

impl From<TokenErrorReason> for RelayUnavailable {
    fn from(reason: TokenErrorReason) -> Self {
        match reason {
            TokenErrorReason::Denied => Self::Denied,
            TokenErrorReason::PeerGone => Self::PeerGone,
            TokenErrorReason::PeerNotMumcord => Self::PeerCannotRelay,
            TokenErrorReason::RateLimited => Self::RateLimited,
            TokenErrorReason::RelayDown => Self::RelayDown,
            TokenErrorReason::RelayFull => Self::RelayFull,
        }
    }
}

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
                host: None,
                status: RelayStatus::Conflict,
                first_seen,
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
        let status = if !self.trusted(&announcer.pair) {
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

struct Pending {
    bot: u32,
    peer: u32,
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
    /// Generation of the bot-gone timers; unique across servers and reconnects.
    generation: u64,
}

/// Relay state of the engine.
#[derive(Default)]
pub(crate) struct Relays {
    engine: OnceLock<WeakEngine>,
    state: Mutex<State>,
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

    fn info(&self, server_id: ServerId) -> RelayInfo {
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
                self.on_token_error(ctx.server_id, sender, rid, reason);
            }
            // Only relay bots answer token requests.
            PeerMessage::TokenRequest { .. } => {}
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

    fn on_token_error(
        &self,
        server_id: ServerId,
        sender: u32,
        rid: RequestId,
        reason: TokenErrorReason,
    ) {
        let mut state = lock(&self.state);
        let key = (server_id, rid);
        if state.requests.get(&key).is_some_and(|p| p.bot == sender)
            && let Some(pending) = state.requests.remove(&key)
        {
            let _ = pending.reply.send(Err(reason.into()));
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
    ) -> Result<(u32, RelayAnnouncement, oneshot::Receiver<TokenReply>), RelayUnavailable> {
        let mut state = lock(&self.state);
        let (bot, ann) = state
            .servers
            .get(&server_id)
            .ok_or(RelayUnavailable::NoRelay)?
            .usable()
            .map(|a| (a.session, a.ann.clone()))?;
        let (reply, rx) = oneshot::channel();
        state
            .requests
            .insert((server_id, rid), Pending { bot, peer, reply });
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
fn pin_identity(store: &Store, server_id: ServerId) -> mumcord_store::Result<Option<IdentityId>> {
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
        }
        Ok(())
    }

    /// Asks the pinned relay bot of `server_id` for a token for this client and `peer` (the
    /// offering side, FR-4.14). Returns the announcement to connect to and the token.
    pub async fn relay_token_request(
        &self,
        server_id: ServerId,
        peer: u32,
    ) -> Result<(RelayAnnouncement, RelayToken), RelayUnavailable> {
        let ctx = self
            .session_ctx_for(server_id)
            .map_err(|_| RelayUnavailable::NotConnected)?;
        let rid = RequestId::random();
        let (bot, ann, rx) = self.relays().begin_request(server_id, peer, rid)?;
        let message = PeerMessage::TokenRequest { rid, peer };
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
        ]
        .into_iter()
        .map(Into::into)
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
            ]
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
        let (bot, _, rx) = relays.begin_request(1, 2, rid).expect("usable");
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
        let (_, _, rx) = relays.begin_request(1, 2, RequestId(1)).expect("usable");
        // Wrong sender, wrong peer, and someone else's pair are ignored.
        relays.on_token(1, Some(1), 6, RequestId(1), 1, 2, token('x'), 30);
        relays.on_token(1, Some(1), 5, RequestId(1), 1, 3, token('x'), 30);
        relays.on_token(1, Some(1), 5, RequestId(1), 4, 2, token('x'), 30);
        relays.on_token(1, Some(1), 5, RequestId(1), 1, 2, token('a'), 30);
        assert_eq!(
            relays.finish_request(1, RequestId(1), rx).await,
            Ok(token('a'))
        );

        let (_, _, rx) = relays.begin_request(1, 2, RequestId(2)).expect("usable");
        relays.on_token_error(1, 6, RequestId(2), TokenErrorReason::RelayFull);
        relays.on_token_error(1, 5, RequestId(2), TokenErrorReason::PeerNotMumcord);
        assert_eq!(
            relays.finish_request(1, RequestId(2), rx).await,
            Err(RelayUnavailable::PeerCannotRelay)
        );
    }

    #[test]
    fn a_token_request_needs_a_usable_relay() {
        let relays = Relays::default();
        assert_eq!(
            relays.begin_request(1, 2, RequestId(1)).err(),
            Some(RelayUnavailable::NoRelay)
        );
        let relays = relays_with_down();
        assert_eq!(
            relays.begin_request(1, 2, RequestId(1)).err(),
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
        let (_, _, mut rx) = relays.begin_request(1, 2, RequestId(1)).expect("usable");
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
}
