//! Direct connections between Mumcord users (FR-2.35, FR-2.36).
//!
//! One encrypted connection per pair of users on a server ([`mumcord_p2p`]), introduced over
//! plugin data ([`DATA_ID`]) and shared by every feature that goes direct. A feature asks for
//! a stream with [`Engine::p2p_open_stream`] and a stream kind of its own; the other side's
//! accept loop routes each incoming stream by that first byte ([`STREAM_FULL_IMAGE`], …).
//!
//! Consent: an offer or answer reveals this machine's addresses and may open a router port,
//! so neither happens before the user allowed direct connections. While the setting is still
//! unset, wanting a connection asks the UI once ([`EngineEvent::DirectConsentRequired`]) and
//! waits for [`Engine::p2p_consent_decide`]; an offer that arrives meanwhile is declined, so
//! the other side falls back to plugin data at once.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use mumcord_p2p::quinn::{self, RecvStream, SendStream};
pub use mumcord_p2p::{DATA_ID, P2pConfig, P2pError, PeerConnection};
use mumcord_p2p::{DeclineReason, P2p, SessionId, Signal};
use mumcord_protocol::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use mumcord_protocol::{ClientIdentity, PluginData};
use mumcord_store::Consent;
use tokio::sync::{Notify, oneshot};

use crate::ServerId;
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

#[derive(Debug, thiserror::Error)]
pub enum DirectError {
    #[error("direct connections are not allowed")]
    NotAllowed,
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

#[derive(Default)]
struct State {
    open: HashMap<Key, Arc<PeerConnection>>,
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

    fn open(&self, key: Key) -> Option<Arc<PeerConnection>> {
        lock(&self.state)
            .open
            .get(&key)
            .filter(|c| c.connection().close_reason().is_none())
            .cloned()
    }

    /// Asks the UI for consent unless the question is already showing.
    fn ask(&self, ctx: &SessionCtx) {
        if !self.asking.swap(true, Ordering::Relaxed) {
            ctx.events.emit(EngineEvent::DirectConsentRequired);
        }
    }

    /// Keeps a new connection and starts routing the streams the other side opens.
    fn register(&self, engine: &Engine, key: Key, conn: PeerConnection) -> Arc<PeerConnection> {
        let conn = Arc::new(conn);
        let previous = lock(&self.state).open.insert(key, conn.clone());
        if let Some(previous) = previous {
            previous.close();
        }
        tracing::info!("direct connection open");
        engine.runtime().spawn(accept_streams(
            engine.downgrade(),
            key,
            conn.connection().clone(),
        ));
        self.opened.notify_waiters();
        conn
    }

    /// The connection ended on its own: forget it unless a newer one replaced it.
    fn closed(&self, key: Key, stable_id: usize) {
        let mut state = lock(&self.state);
        if state
            .open
            .get(&key)
            .is_some_and(|c| c.connection().stable_id() == stable_id)
        {
            state.open.remove(&key);
        }
    }

    /// A person on `server_id` left.
    pub(crate) fn forget(&self, server_id: ServerId, session: u32) {
        let mut state = lock(&self.state);
        let key = (server_id, session);
        if let Some(conn) = state.open.remove(&key) {
            conn.close();
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
        state.open.retain(|key, conn| {
            let keep = !matches(key);
            if !keep {
                conn.close();
            }
            keep
        });
        state.offers.retain(|key, _| !matches(key));
        state.attempts.retain(|key, _| !matches(key));
    }

    /// The consent setting changed (dialog or Settings → Privacy).
    pub(crate) fn consent_changed(&self, consent: Consent) {
        self.asking.store(false, Ordering::Relaxed);
        if consent != Consent::Yes {
            self.close_where(|_| true);
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

/// Answers an offer from `from`, or declines it without consent.
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
    match engine.direct_consent() {
        Consent::Yes => {}
        Consent::No => return decline(DeclineReason::Declined),
        Consent::Unset => {
            engine.direct().ask(ctx);
            return decline(DeclineReason::Unavailable);
        }
    }
    let (Some(peer_hash), Some((p2p, _))) = (peer_hash, engine.p2p().await) else {
        return decline(DeclineReason::Unavailable);
    };
    let pending = match p2p.answer(offer, &peer_hash).await {
        Ok(pending) => pending,
        Err(e) => {
            tracing::info!(error = %e, "direct connection not offered back");
            return decline(DeclineReason::Unavailable);
        }
    };
    send(ctx, from, pending.signal());
    match pending.connect().await {
        Ok(conn) => {
            engine
                .direct()
                .register(engine, (ctx.server_id, from), conn);
        }
        Err(e) => tracing::info!(error = %e, "direct connection not established"),
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
    if let Some(engine) = engine.upgrade() {
        engine.direct().closed(key, connection.stable_id());
    }
    tracing::info!("direct connection closed");
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

    /// Whether a direct connection to `session` on `server_id` is open right now.
    pub fn p2p_connected(&self, server_id: ServerId, session: u32) -> bool {
        self.direct().open((server_id, session)).is_some()
    }

    /// The direct connection to `session` on `server_id`: the open one, else a new one after
    /// consent. Errors mean the feature has to use plugin data (or is unavailable).
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
        if !self.direct_allowed(&ctx).await {
            return Err(DirectError::NotAllowed);
        }
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
        let offer = p2p.offer(&peer_hash).await?;
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
                let conn = offer.complete(&reply).await?;
                Ok(self.direct().register(self, key, conn))
            }
            Ok(Ok(Reply::Superseded)) => {
                let wait = lock(&self.direct().config).connect_timeout + ANSWER_WAIT;
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
