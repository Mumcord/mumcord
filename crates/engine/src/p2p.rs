//! Peer-to-peer connections between Mumcord users (FR-2.35, FR-2.36, spec 04).
//!
//! One encrypted connection per pair of users on a server ([`mumcord_p2p`]), introduced over
//! plugin data ([`DATA_ID`]) and shared by every P2P feature. A feature asks for a stream with
//! [`Engine::p2p_open_stream`] and a stream kind of its own; the other side's accept loop routes
//! each incoming stream by that first byte ([`STREAM_FULL_IMAGE`], …).
//!
//! The connection goes direct or through the relay the server's relay bot announces
//! ([`crate::peer`]). The effective `p2p_mode` of the server, the relay's availability and the
//! consent decide what an offer or answer reveals ([`plan`]); after offer and answer both sides
//! pick the same way ([`route`], FR-4.14): direct first when both sent addresses, else, or when
//! direct finds no path, the relay when both can use it. The offering side asks the bot for the
//! token, the answering side waits for it.
//!
//! Consent: a direct offer or answer reveals this machine's addresses and may open a router
//! port, so neither happens before the user allowed direct connections. While the setting is
//! still unset, the first direct attempt asks the UI once
//! ([`EngineEvent::DirectConsentRequired`]) and waits for [`Engine::p2p_consent_decide`]; an
//! offer that arrives meanwhile is answered through the relay only, or declined without one.
//! "No" never stops the relay (FR-4.5).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use mumcord_p2p::quinn::{self, RecvStream, SendStream};
pub use mumcord_p2p::{
    ConnectionPath, DATA_ID, P2pConfig, P2pError, PeerConnection, RelayTransport,
};
use mumcord_p2p::{DeclineReason, P2p, Reveal, SessionId, Side, Signal};
use mumcord_protocol::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use mumcord_protocol::{ClientIdentity, PluginData};
use mumcord_store::{Consent, P2pMode, Store};
use tokio::sync::{Notify, oneshot};

use crate::ServerId;
use crate::peer::RelayUnavailable;
use crate::session::{Engine, EngineEvent, EngineResult, SessionCtx, WeakEngine, lock};

/// Feature announced in the Mumcord hello; direct connections are only offered to peers that
/// announced it.
pub const FEATURE: &str = "p2p";

/// First byte of a stream on a direct connection: what it carries.
pub const STREAM_FULL_IMAGE: u8 = 1;
pub const STREAM_SCREEN: u8 = 2;

/// How long an offer waits for the other side's answer or decline.
const ANSWER_WAIT: Duration = Duration::from_secs(15);
/// How long a wanted connection waits for the user's answer to the consent question; after
/// that the feature falls back to plugin data and the question stays open.
const CONSENT_WAIT: Duration = Duration::from_secs(60);
/// How long the answering side waits for the relay token. The offering side asks the bot only
/// after its own direct attempt failed, which may end a little later than this side's, and the
/// bot then has up to 5 s (FR-4.14).
const TOKEN_AWAIT: Duration = Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
pub enum DirectError {
    /// "Peer-to-peer is off" (FR-4.3).
    #[error("peer-to-peer is off")]
    Off,
    /// Direct connections are not allowed and the relay cannot be used either.
    #[error("direct connections are not allowed")]
    NotAllowed,
    /// The relay was needed but cannot be used (FR-4.4, FR-4.11, FR-4.14).
    #[error("the relay cannot be used: {0:?}")]
    Relay(RelayUnavailable),
    #[error("the other person cannot take direct connections")]
    Unsupported,
    #[error("not connected to that server")]
    NotConnected,
    #[error("the other person did not answer")]
    NoAnswer,
    #[error(transparent)]
    P2p(#[from] P2pError),
    #[error("stream on the direct connection failed: {0}")]
    Stream(String),
}

/// A person on a server; sessions are only unique per server.
type Key = (ServerId, u32);

/// How an open P2P connection travels, for the connection info (FR-4.19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct P2pLink {
    pub path: ConnectionPath,
    /// "IP hidden": this client sent no addresses and the connection goes through the relay,
    /// so the other person does not see this client's IP address.
    pub ip_hidden: bool,
}

/// Whether this side may connect directly (FR-4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectUse {
    Yes,
    /// The consent is still unset: ask before the first direct attempt.
    Ask,
    No,
}

/// What this side may use for one connection, before talking to the peer (FR-4.3 – FR-4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) direct: DirectUse,
    /// The relay may be used: the offer or answer carries `rl`.
    pub(crate) relay: bool,
}

impl Plan {
    /// What the offer or answer reveals once an open consent question is settled:
    /// `ask_answer` is the answer to it (ignored when nothing was asked).
    pub(crate) fn reveal(self, ask_answer: bool) -> Result<Reveal, DirectError> {
        let direct = match self.direct {
            DirectUse::Yes => true,
            DirectUse::Ask => ask_answer,
            DirectUse::No => false,
        };
        if !direct && !self.relay {
            return Err(DirectError::NotAllowed);
        }
        Ok(Reveal {
            direct,
            relay: self.relay,
        })
    }
}

/// What this side may use given the effective mode of the server, the relay's availability and
/// the consent (FR-4.3 – FR-4.5). Errors: P2P features are unavailable on this server.
pub(crate) fn plan(
    mode: P2pMode,
    relay: Result<(), RelayUnavailable>,
    consent: Consent,
) -> Result<Plan, DirectError> {
    let by_consent = match consent {
        Consent::Yes => DirectUse::Yes,
        Consent::Unset => DirectUse::Ask,
        Consent::No => DirectUse::No,
    };
    let available = relay.is_ok();
    let plan = match mode {
        P2pMode::Off => return Err(DirectError::Off),
        P2pMode::DirectRelay => Plan {
            direct: by_consent,
            relay: available,
        },
        P2pMode::DirectOnly => Plan {
            direct: by_consent,
            relay: false,
        },
        P2pMode::RelayFirst if available => Plan {
            direct: DirectUse::No,
            relay: true,
        },
        // Without a relay "always relay" goes direct (FR-4.4); choosing it never asks
        // (FR-4.5), but a stored "no" still forbids direct.
        P2pMode::RelayFirst => Plan {
            direct: if consent == Consent::No {
                DirectUse::No
            } else {
                DirectUse::Yes
            },
            relay: false,
        },
        P2pMode::RelayOnly => {
            relay.map_err(DirectError::Relay)?;
            Plan {
                direct: DirectUse::No,
                relay: true,
            }
        }
    };
    if plan.direct == DirectUse::No && !plan.relay {
        return Err(DirectError::NotAllowed);
    }
    Ok(plan)
}

/// What follows when the direct attempt is skipped or finds no path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Then {
    Relay,
    /// "The other person can't use the relay (older Mumcord or relay turned off)."
    PeerCannotRelay,
    NoPath,
}

/// The way after offer and answer (FR-4.14, spec 04 section 7.3); both sides get the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Route {
    /// Try direct first.
    pub(crate) direct: bool,
    pub(crate) then: Then,
}

/// Decides the way from what this side revealed and sent and what the peer sent. Between two
/// relay-capable sides an empty address list means "hidden": direct only when both sent
/// addresses. Against a peer without `rl` (older Mumcord, no relay) direct runs as before,
/// even with an empty list.
pub(crate) fn route(own: Reveal, own_sent: bool, peer_sent: bool, peer_relay: bool) -> Route {
    let direct = own.direct && (!peer_relay || (own_sent && peer_sent));
    let then = match (own.relay, peer_relay) {
        (true, true) => Then::Relay,
        (true, false) => Then::PeerCannotRelay,
        (false, _) => Then::NoPath,
    };
    Route { direct, then }
}

/// The effective mode of `server_id` and the consent; "off" when they cannot be read, as
/// nothing may be revealed without knowing.
fn mode_and_consent(store: &Store, server_id: ServerId) -> (P2pMode, Consent) {
    let read = store.settings().and_then(|settings| {
        let mode = store
            .server(server_id)?
            .map_or(settings.p2p_mode, |s| s.effective_p2p_mode(&settings));
        Ok((mode, settings.allow_direct_connections))
    });
    read.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "reading the peer-to-peer settings failed");
        (P2pMode::Off, Consent::No)
    })
}

/// The [`plan`] for `server_id` from the stored settings.
pub(crate) fn plan_for(
    store: &Store,
    server_id: ServerId,
    relay: Result<(), RelayUnavailable>,
) -> Result<Plan, DirectError> {
    let (mode, consent) = mode_and_consent(store, server_id);
    plan(mode, relay, consent)
}

/// Maps a failed relay connection to the reason the UI shows (FR-4.11).
fn relay_error(e: P2pError) -> DirectError {
    match e {
        P2pError::RelayUnreachable => DirectError::Relay(RelayUnavailable::Unreachable),
        P2pError::RelayRefused(refusal) => DirectError::Relay(RelayUnavailable::Refused(refusal)),
        // A relay that shows another certificate than the pinned one is not used (FR-4.9).
        P2pError::RelayCertMismatch => DirectError::Relay(RelayUnavailable::Untrusted),
        // The outer link or the pairing broke: from the user's view the relay is not usable.
        P2pError::RelayFailed => DirectError::Relay(RelayUnavailable::Unreachable),
        other => other.into(),
    }
}

fn sent_candidates(signal: &Signal) -> bool {
    matches!(signal, Signal::Offer { candidates, .. } | Signal::Answer { candidates, .. }
        if !candidates.is_empty())
}

/// What an offer gets back.
enum Reply {
    Signal(Signal),
    /// Both sides offered at once and this side answers the other offer instead.
    Superseded,
}

struct Offer {
    session: SessionId,
    own_hash: String,
    reply: oneshot::Sender<Reply>,
}

/// An open connection and whether this side sent its addresses for it.
struct Link {
    conn: Arc<PeerConnection>,
    sent_candidates: bool,
}

impl Link {
    fn info(&self) -> P2pLink {
        let path = self.conn.path();
        P2pLink {
            path,
            ip_hidden: ip_hidden(self.sent_candidates, path),
        }
    }
}

/// "IP hidden" (FR-4.19): no addresses sent and the connection goes through the relay.
fn ip_hidden(sent_candidates: bool, path: ConnectionPath) -> bool {
    !sent_candidates && path != ConnectionPath::Direct
}

#[derive(Default)]
struct State {
    open: HashMap<Key, Link>,
    /// Own offers waiting for a reply, by the person they went to.
    offers: HashMap<Key, Offer>,
    /// One attempt per person at a time; later callers wait and reuse its connection.
    attempts: HashMap<Key, Arc<tokio::sync::Mutex<()>>>,
}

/// Direct connection state of the engine.
#[derive(Default)]
pub(crate) struct Direct {
    engine: OnceLock<WeakEngine>,
    state: Mutex<State>,
    /// Woken when a connection opens (callers whose offer was superseded wait for it).
    opened: Notify,
    /// Woken when the consent setting changes.
    decided: Notify,
    /// Whether the consent question is showing.
    asking: AtomicBool,
    config: Mutex<P2pConfig>,
}

impl Direct {
    pub(crate) fn bind(&self, engine: WeakEngine) {
        if self.engine.set(engine).is_err() {
            tracing::warn!("direct connections bound twice");
        }
    }

    fn engine(&self) -> Option<Engine> {
        self.engine.get().and_then(WeakEngine::upgrade)
    }

    fn open_link<T>(&self, key: Key, f: impl FnOnce(&Link) -> T) -> Option<T> {
        lock(&self.state)
            .open
            .get(&key)
            .filter(|l| l.conn.connection().close_reason().is_none())
            .map(f)
    }

    fn open(&self, key: Key) -> Option<Arc<PeerConnection>> {
        self.open_link(key, |l| l.conn.clone())
    }

    /// Asks the UI for consent unless the question is already showing.
    fn ask(&self, ctx: &SessionCtx) {
        if !self.asking.swap(true, Ordering::Relaxed) {
            ctx.events.emit(EngineEvent::DirectConsentRequired);
        }
    }

    /// Keeps a new connection and starts routing the streams the other side opens.
    fn register(
        &self,
        engine: &Engine,
        key: Key,
        conn: PeerConnection,
        sent_candidates: bool,
    ) -> Arc<PeerConnection> {
        let conn = Arc::new(conn);
        let link = Link {
            conn: conn.clone(),
            sent_candidates,
        };
        let info = link.info();
        let previous = lock(&self.state).open.insert(key, link);
        if let Some(previous) = previous {
            previous.conn.close();
        }
        tracing::info!(path = ?info.path, "peer-to-peer connection open");
        emit_link(engine, key, Some(info));
        engine.runtime().spawn(accept_streams(
            engine.downgrade(),
            key,
            conn.connection().clone(),
        ));
        self.opened.notify_waiters();
        conn
    }

    /// The connection ended on its own: forget it unless a newer one replaced it. Returns
    /// whether it was forgotten.
    fn closed(&self, key: Key, stable_id: usize) -> bool {
        let mut state = lock(&self.state);
        let current = state
            .open
            .get(&key)
            .is_some_and(|l| l.conn.connection().stable_id() == stable_id);
        if current {
            state.open.remove(&key);
        }
        current
    }

    /// A person on `server_id` left.
    pub(crate) fn forget(&self, server_id: ServerId, session: u32) {
        let mut state = lock(&self.state);
        let key = (server_id, session);
        if let Some(link) = state.open.remove(&key) {
            link.conn.close();
        }
        state.offers.remove(&key);
        state.attempts.remove(&key);
    }

    /// The connection to `server_id` (re)synced or ended: sessions are no longer valid.
    pub(crate) fn reset(&self, server_id: ServerId) {
        self.close_where(|(id, _)| *id == server_id);
    }

    fn close_where(&self, mut matches: impl FnMut(&Key) -> bool) {
        let mut state = lock(&self.state);
        state.open.retain(|key, link| {
            let keep = !matches(key);
            if !keep {
                link.conn.close();
            }
            keep
        });
        state.offers.retain(|key, _| !matches(key));
        state.attempts.retain(|key, _| !matches(key));
    }

    /// The consent setting changed (dialog or Settings → Privacy). Without consent the direct
    /// connections end; relayed ones reveal nothing to the peer and stay (FR-4.5).
    pub(crate) fn consent_changed(&self, consent: Consent) {
        self.asking.store(false, Ordering::Relaxed);
        if consent != Consent::Yes {
            lock(&self.state).open.retain(|_, link| {
                let keep = link.conn.path() != ConnectionPath::Direct;
                if !keep {
                    link.conn.close();
                }
                keep
            });
        }
        self.decided.notify_waiters();
    }

    /// Signalling from another client (session task).
    pub(crate) fn on_plugin_data(&self, ctx: &SessionCtx, data: &PluginData) {
        if data.data_id != DATA_ID {
            return;
        }
        let Some(sender) = data.sender else {
            return;
        };
        let (own, peer_hash) = ctx
            .handle
            .with_state(|s| (s.session, s.users.get(&sender).map(|u| u.cert_hash.clone())));
        let Some(peer_hash) = peer_hash else {
            return;
        };
        if own == Some(sender) {
            return;
        }
        let signal = match Signal::decode(&data.data) {
            Ok(signal) => signal,
            Err(e) => {
                tracing::debug!(error = %e, "ignoring a direct connection message");
                return;
            }
        };
        let key = (ctx.server_id, sender);
        let offer = match signal {
            Signal::Offer { .. } => signal,
            Signal::Answer { session, .. } | Signal::Decline { session, .. } => {
                let mut state = lock(&self.state);
                if state.offers.get(&key).is_some_and(|o| o.session == session)
                    && let Some(offer) = state.offers.remove(&key)
                {
                    // The offering task may have given up already.
                    let _ = offer.reply.send(Reply::Signal(signal));
                }
                return;
            }
        };
        {
            let mut state = lock(&self.state);
            if let Some(own_offer) = state.offers.get(&key) {
                // Both offered at once: the lower certificate hash keeps its offer, so exactly
                // one side answers.
                if peer_hash
                    .as_deref()
                    .is_some_and(|peer| own_offer.own_hash.as_str() < peer)
                {
                    return;
                }
                if let Some(own_offer) = state.offers.remove(&key) {
                    let _ = own_offer.reply.send(Reply::Superseded);
                }
            }
        }
        let Some(engine) = self.engine() else {
            return;
        };
        let ctx = ctx.clone();
        engine.runtime().clone().spawn(async move {
            answer(&engine, &ctx, sender, peer_hash, &offer).await;
        });
    }
}

/// Answers an offer from `from` as the mode, the relay and the consent allow (FR-4.14), or
/// declines it.
async fn answer(
    engine: &Engine,
    ctx: &SessionCtx,
    from: u32,
    peer_hash: Option<String>,
    offer: &Signal,
) {
    let decline = |reason| {
        send(
            ctx,
            from,
            &Signal::Decline {
                session: offer.session(),
                reason,
            },
        );
    };
    let Signal::Offer {
        candidates,
        relay: peer_relay,
        ..
    } = offer
    else {
        return;
    };
    let relay = engine.relays().availability(ctx.server_id);
    let plan = plan_for(&ctx.store(), ctx.server_id, relay);
    let reveal = match plan {
        Ok(plan) => {
            if plan.direct == DirectUse::Ask {
                // Not waiting for the user here: the offer would time out meanwhile.
                engine.direct().ask(ctx);
            }
            plan.reveal(false)
        }
        Err(e) => Err(e),
    };
    let reveal = match reveal {
        Ok(reveal) => reveal,
        Err(e) => {
            tracing::info!(error = %e, "peer-to-peer offer declined");
            let declined = matches!(e, DirectError::Off)
                || (matches!(e, DirectError::NotAllowed) && engine.direct_consent() == Consent::No);
            return decline(if declined {
                DeclineReason::Declined
            } else {
                DeclineReason::Unavailable
            });
        }
    };
    let (Some(peer_hash), Some((p2p, _))) = (peer_hash, engine.p2p().await) else {
        return decline(DeclineReason::Unavailable);
    };
    let pending = match p2p.answer_with(offer, &peer_hash, reveal).await {
        Ok(pending) => pending,
        Err(e) => {
            tracing::info!(error = %e, "peer-to-peer connection not offered back");
            return decline(DeclineReason::Unavailable);
        }
    };
    let own_sent = sent_candidates(pending.signal());
    let route = route(reveal, own_sent, !candidates.is_empty(), *peer_relay);
    send(ctx, from, pending.signal());
    let key = (ctx.server_id, from);
    if route.direct {
        match pending.connect().await {
            Ok(conn) => {
                engine.direct().register(engine, key, conn, own_sent);
                return;
            }
            Err(P2pError::NoDirectPath) => {}
            Err(e) => return tracing::info!(error = %e, "direct connection not established"),
        }
    } else {
        drop(pending);
    }
    if route.then != Then::Relay {
        return tracing::info!(then = ?route.then, "no peer-to-peer path");
    }
    let relayed = async {
        let (ann, token) = engine
            .relay_token_await(ctx.server_id, from, TOKEN_AWAIT)
            .await
            .map_err(DirectError::Relay)?;
        p2p.connect_via_relay(&ann, &token, &peer_hash, Side::Answering)
            .await
            .map_err(relay_error)
    };
    match relayed.await {
        Ok(conn) => {
            engine.direct().register(engine, key, conn, own_sent);
        }
        Err(e) => tracing::info!(error = %e, "relayed connection not established"),
    }
}

/// Tells the UI how the connection to a person travels now (FR-4.19).
fn emit_link(engine: &Engine, key: Key, link: Option<P2pLink>) {
    if let Ok(ctx) = engine.session_ctx_for(key.0) {
        ctx.events.emit(EngineEvent::P2pLinkChanged {
            session: key.1,
            link,
        });
    }
}

fn send(ctx: &SessionCtx, to: u32, signal: &Signal) {
    let bytes = match signal.encode() {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!(error = %e, "direct connection message not encodable");
            return;
        }
    };
    if let Err(e) = ctx.handle.send_plugin_data(&[to], DATA_ID, bytes) {
        tracing::debug!(error = %e, "direct connection message not sent");
    }
}

/// Routes the streams the other side opens on `connection` until it closes.
async fn accept_streams(engine: WeakEngine, key: Key, connection: quinn::Connection) {
    while let Ok((send, mut recv)) = connection.accept_bi().await {
        let Some(engine) = engine.upgrade() else {
            return;
        };
        engine.runtime().clone().spawn(async move {
            let mut kind = [0u8; 1];
            if let Err(e) = recv.read_exact(&mut kind).await {
                tracing::debug!(error = %e, "direct stream ended before its kind");
                return;
            }
            match kind[0] {
                STREAM_FULL_IMAGE => crate::full_image::serve_direct(&engine, send, recv).await,
                STREAM_SCREEN => {
                    crate::screen_share::serve_direct(&engine, key.0, key.1, send, recv).await;
                }
                other => tracing::debug!(kind = other, "ignoring an unknown direct stream"),
            }
        });
    }
    if let Some(engine) = engine.upgrade()
        && engine.direct().closed(key, connection.stable_id())
    {
        emit_link(&engine, key, None);
    }
    tracing::info!("peer-to-peer connection closed");
}

fn client_identity(identity: &mumcord_store::Identity) -> ClientIdentity {
    ClientIdentity {
        chain: vec![CertificateDer::from(identity.cert_der().to_vec())],
        key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key_pkcs8_der().to_vec())),
    }
}

impl Engine {
    /// The stored consent; "no" when the settings cannot be read, as nothing may be revealed
    /// without consent.
    fn direct_consent(&self) -> Consent {
        self.settings().map_or_else(
            |e| {
                tracing::warn!(error = %e, "reading the direct connection consent failed");
                Consent::No
            },
            |s| s.allow_direct_connections,
        )
    }

    /// Whether the user allows direct connections; asks once while unset (see module docs).
    async fn direct_allowed(&self, ctx: &SessionCtx) -> bool {
        let decided = self.direct().decided.notified();
        tokio::pin!(decided);
        decided.as_mut().enable();
        match self.direct_consent() {
            Consent::Yes => return true,
            Consent::No => return false,
            Consent::Unset => {}
        }
        self.direct().ask(ctx);
        if tokio::time::timeout(CONSENT_WAIT, decided).await.is_err() {
            tracing::info!("no answer to the direct connection question yet");
            // Ask again the next time; the UI shows one question however often it is asked.
            self.direct().asking.store(false, Ordering::Relaxed);
            return false;
        }
        self.direct_consent() == Consent::Yes
    }

    /// A direct connection helper with the user's identity; `None` before the wizard ran.
    async fn p2p(&self) -> Option<(P2p, String)> {
        let identity = match self.stored_identity().await {
            Ok(identity) => identity?,
            Err(e) => {
                tracing::warn!(error = %e, "identity for a direct connection not readable");
                return None;
            }
        };
        let config = lock(&self.direct().config).clone();
        Some((
            P2p::new(&client_identity(&identity), config),
            identity.cert_hash(),
        ))
    }

    /// Stores the answer to [`EngineEvent::DirectConsentRequired`] as the
    /// `allow_direct_connections` setting; waiting features continue or fall back.
    pub async fn p2p_consent_decide(&self, allow: bool) -> EngineResult<()> {
        let mut settings = self.settings()?;
        settings.allow_direct_connections = if allow { Consent::Yes } else { Consent::No };
        self.update_settings(settings).await?;
        Ok(())
    }

    /// Whether a P2P connection to `session` on `server_id` is open right now.
    pub fn p2p_connected(&self, server_id: ServerId, session: u32) -> bool {
        self.direct().open((server_id, session)).is_some()
    }

    /// How the open P2P connection to `session` on `server_id` travels (FR-4.19); `None`
    /// without one. Changes arrive as [`EngineEvent::P2pLinkChanged`].
    pub fn p2p_link(&self, server_id: ServerId, session: u32) -> Option<P2pLink> {
        self.direct().open_link((server_id, session), Link::info)
    }

    /// Why P2P features cannot be used on `server_id` at all right now (mode, relay, consent;
    /// FR-4.3, FR-4.4); `None` when an attempt may work.
    pub(crate) fn p2p_unavailable(&self, server_id: ServerId) -> Option<DirectError> {
        let relay = self.relays().availability(server_id);
        let store = self.store_handle();
        let store = lock(&store);
        plan_for(&store, server_id, relay).err()
    }

    /// The P2P connection to `session` on `server_id`: the open one, else a new one, direct or
    /// through the relay as the mode allows (FR-4.14). Errors mean the feature has to use
    /// plugin data (or is unavailable).
    pub async fn p2p_connection(
        &self,
        server_id: ServerId,
        session: u32,
    ) -> Result<Arc<PeerConnection>, DirectError> {
        let key = (server_id, session);
        if let Some(conn) = self.direct().open(key) {
            return Ok(conn);
        }
        let ctx = self
            .session_ctx_for(server_id)
            .map_err(|_| DirectError::NotConnected)?;
        let peer_hash = ctx
            .handle
            .with_state(|s| s.users.get(&session).and_then(|u| u.cert_hash.clone()));
        let Some(peer_hash) = peer_hash else {
            return Err(DirectError::Unsupported);
        };
        if !self.mumcord_user_has_feature(server_id, session, FEATURE) {
            return Err(DirectError::Unsupported);
        }
        let relay = self.relays().availability(server_id);
        let plan = plan_for(&ctx.store(), server_id, relay)?;
        let asked = plan.direct == DirectUse::Ask && self.direct_allowed(&ctx).await;
        let reveal = plan.reveal(asked)?;
        let attempt = lock(&self.direct().state)
            .attempts
            .entry(key)
            .or_default()
            .clone();
        let _attempt = attempt.lock().await;
        if let Some(conn) = self.direct().open(key) {
            return Ok(conn);
        }
        let Some((p2p, own_hash)) = self.p2p().await else {
            return Err(DirectError::Unsupported);
        };
        let offer = p2p.offer_with(&peer_hash, reveal).await?;
        let own_sent = sent_candidates(offer.signal());
        let (reply_tx, reply_rx) = oneshot::channel();
        lock(&self.direct().state).offers.insert(
            key,
            Offer {
                session: offer.signal().session(),
                own_hash,
                reply: reply_tx,
            },
        );
        let opened = self.direct().opened.notified();
        tokio::pin!(opened);
        opened.as_mut().enable();
        send(&ctx, session, offer.signal());
        let reply = tokio::time::timeout(ANSWER_WAIT, reply_rx).await;
        match reply {
            Ok(Ok(Reply::Signal(reply))) => {
                let Signal::Answer {
                    candidates,
                    relay: peer_relay,
                    ..
                } = &reply
                else {
                    // A decline: `complete` turns it into the error.
                    let conn = offer.complete(&reply).await?;
                    return Ok(self.direct().register(self, key, conn, own_sent));
                };
                let route = route(reveal, own_sent, !candidates.is_empty(), *peer_relay);
                if route.direct {
                    match offer.complete(&reply).await {
                        Ok(conn) => return Ok(self.direct().register(self, key, conn, own_sent)),
                        Err(P2pError::NoDirectPath) => {}
                        Err(e) => return Err(e.into()),
                    }
                } else {
                    drop(offer);
                }
                match route.then {
                    Then::Relay => {}
                    Then::PeerCannotRelay => {
                        return Err(DirectError::Relay(RelayUnavailable::PeerCannotRelay));
                    }
                    Then::NoPath => return Err(P2pError::NoDirectPath.into()),
                }
                let (ann, token) = self
                    .relay_token_request(server_id, session)
                    .await
                    .map_err(DirectError::Relay)?;
                let conn = p2p
                    .connect_via_relay(&ann, &token, &peer_hash, Side::Offering)
                    .await
                    .map_err(relay_error)?;
                Ok(self.direct().register(self, key, conn, own_sent))
            }
            Ok(Ok(Reply::Superseded)) => {
                // The other side's attempt may try direct, then wait for the token and set up
                // the relay.
                let wait =
                    2 * lock(&self.direct().config).connect_timeout + ANSWER_WAIT + TOKEN_AWAIT;
                let deadline = tokio::time::Instant::now() + wait;
                loop {
                    if let Some(conn) = self.direct().open(key) {
                        return Ok(conn);
                    }
                    if tokio::time::timeout_at(deadline, opened.as_mut())
                        .await
                        .is_err()
                    {
                        return Err(P2pError::NoDirectPath.into());
                    }
                    opened.set(self.direct().opened.notified());
                    opened.as_mut().enable();
                }
            }
            Ok(Err(_)) => Err(DirectError::NotConnected),
            Err(_) => {
                lock(&self.direct().state).offers.remove(&key);
                Err(DirectError::NoAnswer)
            }
        }
    }

    /// Opens a stream of `kind` (a `STREAM_*` constant) on the direct connection to `session`,
    /// connecting first when needed. The kind byte is already sent; the other side routes the
    /// stream to its handler for that kind.
    pub async fn p2p_open_stream(
        &self,
        server_id: ServerId,
        session: u32,
        kind: u8,
    ) -> Result<(SendStream, RecvStream), DirectError> {
        let conn = self.p2p_connection(server_id, session).await?;
        let (mut send, recv) = conn
            .connection()
            .open_bi()
            .await
            .map_err(|e| DirectError::Stream(e.to_string()))?;
        send.write_all(&[kind])
            .await
            .map_err(|e| DirectError::Stream(e.to_string()))?;
        Ok((send, recv))
    }

    /// Replaces how direct connections are made (loopback, timeouts) for tests with several
    /// engines on one machine.
    #[cfg(feature = "test-backend")]
    pub fn p2p_set_config(&self, config: P2pConfig) {
        *lock(&self.direct().config) = config;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODES: [P2pMode; 5] = [
        P2pMode::DirectRelay,
        P2pMode::RelayFirst,
        P2pMode::DirectOnly,
        P2pMode::RelayOnly,
        P2pMode::Off,
    ];
    const CONSENTS: [Consent; 3] = [Consent::Yes, Consent::Unset, Consent::No];

    fn relay(available: bool) -> Result<(), RelayUnavailable> {
        if available {
            Ok(())
        } else {
            Err(RelayUnavailable::NoRelay)
        }
    }

    /// The plan as a short string: direct use and `rl`, or the error.
    fn plan_of(mode: P2pMode, available: bool, consent: Consent) -> String {
        match plan(mode, relay(available), consent) {
            Ok(p) => format!("{:?}{}", p.direct, if p.relay { "+rl" } else { "" }),
            Err(DirectError::Relay(r)) => format!("relay:{r:?}"),
            Err(e) => format!("{e:?}"),
        }
    }

    #[test]
    fn relay_errors_map_to_relay_reasons() {
        let reason = |e| match relay_error(e) {
            DirectError::Relay(r) => Some(r),
            _ => None,
        };
        assert_eq!(
            reason(P2pError::RelayUnreachable),
            Some(RelayUnavailable::Unreachable)
        );
        assert_eq!(
            reason(P2pError::RelayCertMismatch),
            Some(RelayUnavailable::Untrusted)
        );
        assert_eq!(
            reason(P2pError::RelayFailed),
            Some(RelayUnavailable::Unreachable)
        );
        assert_eq!(reason(P2pError::BadPeerHash), None);
    }

    #[test]
    fn plan_follows_mode_relay_and_consent() {
        use Consent::{No, Unset, Yes};
        use P2pMode::*;
        let table = [
            (DirectRelay, true, ["Yes+rl", "Ask+rl", "No+rl"]),
            (DirectRelay, false, ["Yes", "Ask", "NotAllowed"]),
            (RelayFirst, true, ["No+rl", "No+rl", "No+rl"]),
            (RelayFirst, false, ["Yes", "Yes", "NotAllowed"]),
            (DirectOnly, true, ["Yes", "Ask", "NotAllowed"]),
            (DirectOnly, false, ["Yes", "Ask", "NotAllowed"]),
            (RelayOnly, true, ["No+rl", "No+rl", "No+rl"]),
            (
                RelayOnly,
                false,
                ["relay:NoRelay", "relay:NoRelay", "relay:NoRelay"],
            ),
            (Off, true, ["Off", "Off", "Off"]),
            (Off, false, ["Off", "Off", "Off"]),
        ];
        for (mode, available, expected) in table {
            for (consent, expected) in [Yes, Unset, No].into_iter().zip(expected) {
                assert_eq!(
                    plan_of(mode, available, consent),
                    expected,
                    "{mode:?} relay {available} consent {consent:?}"
                );
            }
        }
    }

    #[test]
    fn relay_only_reports_why_the_relay_is_unavailable() {
        let result = plan(
            P2pMode::RelayOnly,
            Err(RelayUnavailable::BotLeft),
            Consent::Yes,
        );
        assert!(matches!(
            result,
            Err(DirectError::Relay(RelayUnavailable::BotLeft))
        ));
    }

    #[test]
    fn a_settled_question_decides_direct() {
        let ask = Plan {
            direct: DirectUse::Ask,
            relay: true,
        };
        assert_eq!(
            ask.reveal(true).ok(),
            Some(Reveal {
                direct: true,
                relay: true
            })
        );
        assert_eq!(
            ask.reveal(false).ok(),
            Some(Reveal {
                direct: false,
                relay: true
            })
        );
        let ask_without_relay = Plan {
            direct: DirectUse::Ask,
            relay: false,
        };
        assert!(matches!(
            ask_without_relay.reveal(false),
            Err(DirectError::NotAllowed)
        ));
    }

    /// One side of a connection attempt.
    #[derive(Clone, Copy, Debug)]
    enum Client {
        New(P2pMode, Consent),
        /// A Mumcord release before the relay: always addresses, never `rl`.
        Old,
    }

    /// What the side reveals; the offering side's open question is answered with "no", like
    /// the answering side treats it. `None`: it sends nothing (offer not sent or declined).
    fn reveal_of(client: Client, available: bool) -> Option<Reveal> {
        match client {
            Client::Old => Some(Reveal::DIRECT),
            Client::New(mode, consent) => plan(mode, relay(available), consent)
                .and_then(|p| p.reveal(false))
                .ok(),
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Way {
        /// Not tried: one side's mode or consent forbids it.
        Blocked,
        /// Direct first, then this on `NoDirectPath`.
        Direct(Then),
        /// No direct attempt.
        Skip(Then),
    }

    /// The way the offering side `a` goes with the answering side `b`; checks that both
    /// sides decide alike.
    fn way(a: Client, b: Client, available: bool) -> Way {
        let (Some(ra), Some(rb)) = (reveal_of(a, available), reveal_of(b, available)) else {
            return Way::Blocked;
        };
        let a_route = route(ra, ra.direct, rb.direct, rb.relay);
        if let Client::New(..) = b {
            let b_route = route(rb, rb.direct, ra.direct, ra.relay);
            assert_eq!(a_route.direct, b_route.direct, "{a:?} {b:?} {available}");
            assert_eq!(
                a_route.then == Then::Relay,
                b_route.then == Then::Relay,
                "{a:?} {b:?} {available}"
            );
        }
        if a_route.direct {
            Way::Direct(a_route.then)
        } else {
            Way::Skip(a_route.then)
        }
    }

    #[test]
    fn both_sides_always_pick_the_same_way() {
        let mut clients = vec![Client::Old];
        for mode in MODES {
            for consent in CONSENTS {
                clients.push(Client::New(mode, consent));
            }
        }
        for a in clients
            .iter()
            .copied()
            .filter(|c| matches!(c, Client::New(..)))
        {
            for b in clients.iter().copied() {
                for available in [true, false] {
                    way(a, b, available);
                }
            }
        }
    }

    #[test]
    fn decision_follows_fr_4_14() {
        use Consent::{No, Unset, Yes};
        use P2pMode::*;
        let new = Client::New;
        let cases = [
            // AC-4.4 / AC-4.5: direct first, relay when no direct path.
            (
                new(DirectRelay, Yes),
                new(DirectRelay, Yes),
                true,
                Way::Direct(Then::Relay),
            ),
            // AC-4.7: "always relay" hides the address; the relay is used at once.
            (
                new(RelayFirst, Yes),
                new(DirectRelay, Yes),
                true,
                Way::Skip(Then::Relay),
            ),
            (
                new(DirectRelay, Yes),
                new(RelayOnly, Yes),
                true,
                Way::Skip(Then::Relay),
            ),
            // Consent "no" or still unset on one side: never direct, the relay still works.
            (
                new(DirectRelay, No),
                new(DirectRelay, Yes),
                true,
                Way::Skip(Then::Relay),
            ),
            (
                new(DirectRelay, Yes),
                new(DirectRelay, Unset),
                true,
                Way::Skip(Then::Relay),
            ),
            // AC-4.8, AC-4.9.
            (
                new(RelayOnly, Yes),
                new(DirectRelay, Yes),
                false,
                Way::Blocked,
            ),
            (new(Off, Yes), new(DirectRelay, Yes), true, Way::Blocked),
            (new(DirectRelay, Yes), new(Off, Yes), true, Way::Blocked),
            // "Direct only" never sends or uses the relay.
            (
                new(DirectOnly, Yes),
                new(DirectRelay, Yes),
                true,
                Way::Direct(Then::NoPath),
            ),
            (
                new(DirectRelay, Yes),
                new(DirectOnly, Yes),
                true,
                Way::Direct(Then::PeerCannotRelay),
            ),
            (
                new(DirectOnly, Yes),
                new(RelayFirst, Yes),
                true,
                Way::Skip(Then::NoPath),
            ),
            // Older peers: direct as before; the relay is impossible.
            (
                new(DirectRelay, Yes),
                Client::Old,
                true,
                Way::Direct(Then::PeerCannotRelay),
            ),
            (
                new(DirectRelay, Yes),
                Client::Old,
                false,
                Way::Direct(Then::NoPath),
            ),
            (
                new(RelayFirst, Yes),
                Client::Old,
                true,
                Way::Skip(Then::PeerCannotRelay),
            ),
            // No relay on the server: as before the relay existed (FR-4.4, FR-4.12).
            (
                new(DirectRelay, Yes),
                new(DirectRelay, Yes),
                false,
                Way::Direct(Then::NoPath),
            ),
            (
                new(RelayFirst, Yes),
                new(RelayFirst, Unset),
                false,
                Way::Direct(Then::NoPath),
            ),
        ];
        for (a, b, available, expected) in cases {
            assert_eq!(
                way(a, b, available),
                expected,
                "{a:?} {b:?} relay {available}"
            );
        }
    }

    #[test]
    fn ip_is_hidden_only_without_addresses_over_the_relay() {
        let relayed = ConnectionPath::Relay(RelayTransport::Tcp);
        assert!(ip_hidden(false, relayed));
        assert!(!ip_hidden(true, relayed));
        assert!(!ip_hidden(true, ConnectionPath::Direct));
    }
}
