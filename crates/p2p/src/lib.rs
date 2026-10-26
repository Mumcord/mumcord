//! Direct encrypted connections between two Mumcord clients (FR-2.35).
//!
//! Transport: QUIC (quinn) over rustls/ring with mutual TLS. Each side presents its Mumble client
//! certificate and accepts the other side only if that certificate's hash equals the hash the
//! Mumble server reports for the user (`UserState.hash`); anything else drops the connection.
//!
//! Paths: IPv6 and LAN addresses of the network interfaces and a router port mapping (PCP,
//! NAT-PMP, UPnP). There is no STUN, TURN or relay server, by design (client-only rule, QA.90):
//! when no path works the attempt ends with [`P2pError::NoDirectPath`].
//!
//! Signalling is transport-agnostic: [`Signal`] messages are encoded here and carried by the
//! caller as Mumble plugin data with the id [`DATA_ID`].
//!
//! Flow:
//! 1. Offering side: [`P2p::offer`] → send [`PendingOffer::signal`] to the peer.
//! 2. Answering side: decode the offer, then either [`P2p::answer`] → send
//!    [`PendingAnswer::signal`] back and call [`PendingAnswer::connect`], or reply with
//!    [`Signal::Decline`].
//! 3. Offering side: [`PendingOffer::complete`] with the received answer.
//!
//! Consent (FR-2.36) is the caller's job: an offer or answer reveals this machine's addresses
//! (it also opens a router port mapping), and [`PendingOffer::complete`] and
//! [`PendingAnswer::connect`] are the only calls that dial the peer. Call `offer`/`answer` only
//! after the user allowed direct connections.

mod gather;
mod signal;
mod tls;

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use mumcord_protocol::ClientIdentity;
use mumcord_protocol::pki_types::{CertificateDer, PrivateKeyDer};
use quinn::{Connection, Endpoint, EndpointConfig, TokioRuntime, TransportConfig, VarInt};
use tokio::sync::mpsc;
use tokio::task::JoinSet;

pub use quinn;
pub use signal::{
    DATA_ID, DeclineReason, MAX_CANDIDATES, MAX_SIGNAL_BYTES, SessionId, Signal, SignalError,
    VERSION,
};

use gather::MappingGuard;
use tls::PinnedPeer;

/// QUIC server name; the certificate check ignores names, so any fixed value works.
const SERVER_NAME: &str = "mumcord-peer";
/// Byte the offering side sends on a fresh unidirectional stream to pick the one connection both
/// sides keep when several paths worked.
const NOMINATE: u8 = 1;
/// Application close codes.
const CLOSE_UNUSED: u32 = 1;
const CLOSE_DONE: u32 = 2;

#[derive(Debug, Clone)]
pub struct P2pConfig {
    /// Longest wait for a direct connection after the answer, from either side.
    pub connect_timeout: Duration,
    /// Ask the home router for a port mapping (PCP, NAT-PMP, UPnP).
    pub port_mapping: bool,
    /// Offer and dial loopback addresses. Only for tests with two clients on one machine.
    pub allow_loopback: bool,
}

impl Default for P2pConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            port_mapping: true,
            allow_loopback: false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum P2pError {
    /// No address of the other side could be reached in time; there is no relay to fall back to.
    #[error("no direct connection possible")]
    NoDirectPath,
    #[error("the other side's certificate does not match the one the server reported")]
    CertMismatch,
    #[error("the other side did not accept a direct connection")]
    Declined(DeclineReason),
    #[error("invalid signalling message: {0}")]
    BadSignal(#[from] SignalError),
    /// The certificate hash given for the peer is not a 40-digit hex SHA-1.
    #[error("invalid certificate hash for the other side")]
    BadPeerHash,
    #[error("own certificate not usable for direct connections: {0}")]
    Tls(#[from] rustls::Error),
    #[error("network error: {0}")]
    Io(#[from] std::io::Error),
}

/// Sets up direct connections with the user's Mumble identity.
pub struct P2p {
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    config: P2pConfig,
}

impl P2p {
    pub fn new(identity: &ClientIdentity, config: P2pConfig) -> Self {
        Self {
            chain: identity.chain.clone(),
            key: identity.key.clone_key(),
            config,
        }
    }

    /// Prepares a connection to the user whose certificate hash the server reports as
    /// `peer_hash`. Binds a socket and gathers addresses; does not contact the peer.
    pub async fn offer(&self, peer_hash: &str) -> Result<PendingOffer, P2pError> {
        let attempt = self.prepare(peer_hash).await?;
        let session = SessionId::random();
        Ok(PendingOffer {
            signal: Signal::Offer {
                session,
                candidates: attempt.candidates.clone(),
            },
            attempt,
            session,
        })
    }

    /// Prepares the reply to a received `offer` from the user with certificate hash
    /// `peer_hash`. Does not contact the peer until [`PendingAnswer::connect`].
    pub async fn answer(&self, offer: &Signal, peer_hash: &str) -> Result<PendingAnswer, P2pError> {
        let Signal::Offer {
            session,
            candidates,
        } = offer
        else {
            return Err(SignalError::Unexpected.into());
        };
        let attempt = self.prepare(peer_hash).await?;
        Ok(PendingAnswer {
            signal: Signal::Answer {
                session: *session,
                candidates: attempt.candidates.clone(),
            },
            targets: candidates.clone(),
            attempt,
        })
    }

    async fn prepare(&self, peer_hash: &str) -> Result<Attempt, P2pError> {
        let peer_hash = normalize_hash(peer_hash)?;
        let peer = PinnedPeer::new(peer_hash.clone());
        let (client, mut server) = tls::quic_configs(&self.chain, &self.key, &peer)?;
        let mut transport = TransportConfig::default();
        transport
            .max_idle_timeout(Some(VarInt::from_u32(30_000).into()))
            .keep_alive_interval(Some(Duration::from_secs(10)));
        let transport = Arc::new(transport);
        server.transport_config(transport.clone());
        let mut client = client;
        client.transport_config(transport);

        let (socket, dual_stack) = bind()?;
        let port = socket.local_addr()?.port();
        let mut endpoint = Endpoint::new(
            EndpointConfig::default(),
            Some(server),
            socket,
            Arc::new(TokioRuntime),
        )?;
        endpoint.set_default_client_config(client);

        let (mut candidates, mapping) =
            gather::candidates(port, self.config.port_mapping, self.config.allow_loopback).await;
        if !dual_stack {
            candidates.retain(SocketAddr::is_ipv4);
        }
        Ok(Attempt {
            endpoint,
            peer,
            peer_hash,
            candidates,
            mapping,
            dual_stack,
            config: self.config.clone(),
        })
    }
}

/// An offer waiting for the peer's answer.
pub struct PendingOffer {
    signal: Signal,
    session: SessionId,
    attempt: Attempt,
}

impl PendingOffer {
    /// The offer to send to the peer.
    pub fn signal(&self) -> &Signal {
        &self.signal
    }

    /// Connects with the peer's reply: dials its addresses, accepts its incoming dials and
    /// keeps the first connection that passes the certificate check.
    pub async fn complete(self, reply: &Signal) -> Result<PeerConnection, P2pError> {
        if reply.session() != self.session {
            return Err(SignalError::Unexpected.into());
        }
        match reply {
            Signal::Answer { candidates, .. } => {
                self.attempt.establish(candidates, Role::Controlling).await
            }
            Signal::Decline { reason, .. } => Err(P2pError::Declined(*reason)),
            Signal::Offer { .. } => Err(SignalError::Unexpected.into()),
        }
    }
}

/// An answer ready to be sent; the connection starts with [`PendingAnswer::connect`].
pub struct PendingAnswer {
    signal: Signal,
    targets: Vec<SocketAddr>,
    attempt: Attempt,
}

impl PendingAnswer {
    /// The answer to send back to the peer.
    pub fn signal(&self) -> &Signal {
        &self.signal
    }

    /// Dials the offer's addresses and accepts the peer's dials until the offering side picks
    /// one connection. Send [`Self::signal`] before or while awaiting this.
    pub async fn connect(self) -> Result<PeerConnection, P2pError> {
        let targets = self.targets.clone();
        self.attempt.establish(&targets, Role::Controlled).await
    }
}

/// An authenticated direct connection. Streams and datagrams are used through
/// [`PeerConnection::connection`]; dropping this closes the connection and removes the router
/// port mapping.
pub struct PeerConnection {
    connection: Connection,
    peer_hash: String,
    // Kept alive for the connection's lifetime.
    _endpoint: Endpoint,
    _mapping: Option<MappingGuard>,
}

impl PeerConnection {
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Certificate hash of the peer (lower-case hex SHA-1), as checked during the handshake.
    pub fn peer_hash(&self) -> &str {
        &self.peer_hash
    }

    pub fn remote_address(&self) -> SocketAddr {
        self.connection.remote_address()
    }

    pub fn close(&self) {
        self.connection.close(VarInt::from_u32(CLOSE_DONE), b"done");
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    /// Offering side: picks the connection.
    Controlling,
    /// Answering side: keeps the connection the other side picked.
    Controlled,
}

struct Attempt {
    endpoint: Endpoint,
    peer: Arc<PinnedPeer>,
    peer_hash: String,
    candidates: Vec<SocketAddr>,
    mapping: Option<MappingGuard>,
    dual_stack: bool,
    config: P2pConfig,
}

impl Attempt {
    async fn establish(
        self,
        targets: &[SocketAddr],
        role: Role,
    ) -> Result<PeerConnection, P2pError> {
        let targets: Vec<SocketAddr> = targets
            .iter()
            .copied()
            .filter(|a| self.config.allow_loopback || !a.ip().is_loopback())
            .filter(|a| self.dual_stack || a.is_ipv4())
            .collect();
        // A wrong certificate means the other side is not who the server says: end the attempt
        // right away rather than waiting for another path to the same impostor.
        let found = tokio::select! {
            found = tokio::time::timeout(
                self.config.connect_timeout,
                race(&self.endpoint, &self.peer, &targets, role),
            ) => found.ok().flatten(),
            () = self.peer.mismatched() => None,
        };
        match found {
            Some(connection) => Ok(PeerConnection {
                connection,
                peer_hash: self.peer_hash,
                _endpoint: self.endpoint,
                _mapping: self.mapping,
            }),
            _ if self.peer.saw_mismatch() => Err(P2pError::CertMismatch),
            _ => Err(P2pError::NoDirectPath),
        }
    }
}

/// Dials every target and accepts every incoming dial at once; returns the connection both
/// sides agree on, or `None` when every path failed.
async fn race(
    endpoint: &Endpoint,
    peer: &Arc<PinnedPeer>,
    targets: &[SocketAddr],
    role: Role,
) -> Option<Connection> {
    let (tx, mut established) = mpsc::channel::<Connection>(MAX_CANDIDATES * 2);
    let mut dials = JoinSet::new();
    for &addr in targets {
        match endpoint.connect(addr, SERVER_NAME) {
            Ok(connecting) => {
                let tx = tx.clone();
                let peer = peer.clone();
                dials.spawn(async move {
                    match connecting.await {
                        Ok(connection) => drop(tx.send(connection).await),
                        Err(e) => {
                            note_refusal(&peer, &e);
                            tracing::debug!("direct dial failed: {e}");
                        }
                    }
                });
            }
            Err(e) => tracing::debug!("direct dial not started: {e}"),
        }
    }
    let accepting = endpoint.clone();
    let peer = peer.clone();
    dials.spawn(async move {
        let mut handshakes = JoinSet::new();
        loop {
            tokio::select! {
                incoming = accepting.accept() => {
                    let Some(incoming) = incoming else { break };
                    let tx = tx.clone();
                    let peer = peer.clone();
                    handshakes.spawn(async move {
                        match incoming.await {
                            Ok(connection) => drop(tx.send(connection).await),
                            Err(e) => {
                                note_refusal(&peer, &e);
                                tracing::debug!("incoming direct connection failed: {e}");
                            }
                        }
                    });
                }
                // Keep the set from growing with finished handshakes.
                Some(_) = handshakes.join_next(), if !handshakes.is_empty() => {}
            }
        }
    });

    let chosen = match role {
        Role::Controlling => nominate(&mut established).await,
        Role::Controlled => await_nomination(&mut established).await,
    };
    // Dropping the set stops the remaining dials and the accept loop.
    drop(dials);
    chosen
}

/// rustls answers a certificate the pin rejects with the TLS alert `access_denied`; seeing it
/// from the other side means it expects a different certificate than ours.
fn note_refusal(peer: &PinnedPeer, error: &quinn::ConnectionError) {
    const ACCESS_DENIED: u8 = 49;
    if let quinn::ConnectionError::ConnectionClosed(close) = error
        && close.error_code == quinn::TransportErrorCode::crypto(ACCESS_DENIED)
    {
        peer.refused_by_peer();
    }
}

async fn nominate(established: &mut mpsc::Receiver<Connection>) -> Option<Connection> {
    while let Some(connection) = established.recv().await {
        match send_nomination(&connection).await {
            Ok(()) => return Some(connection),
            Err(e) => {
                tracing::debug!("nominating a direct connection failed: {e}");
                connection.close(VarInt::from_u32(CLOSE_UNUSED), b"unused");
            }
        }
    }
    None
}

async fn send_nomination(connection: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = connection.open_uni().await?;
    stream.write_all(&[NOMINATE]).await?;
    stream.finish()?;
    Ok(())
}

async fn await_nomination(established: &mut mpsc::Receiver<Connection>) -> Option<Connection> {
    let mut waiting = JoinSet::new();
    loop {
        tokio::select! {
            Some(connection) = established.recv() => {
                waiting.spawn(async move {
                    let mut stream = connection.accept_uni().await.ok()?;
                    let mut byte = [0u8; 1];
                    stream.read_exact(&mut byte).await.ok()?;
                    (byte[0] == NOMINATE).then_some(connection)
                });
            }
            Some(joined) = waiting.join_next(), if !waiting.is_empty() => {
                if let Ok(Some(connection)) = joined {
                    return Some(connection);
                }
            }
            else => return None,
        }
    }
}

/// A dual-stack UDP socket on a random port, or IPv4 only where IPv6 is switched off. Returns
/// whether IPv6 works.
fn bind() -> std::io::Result<(UdpSocket, bool)> {
    let socket =
        socket2::Socket::new(socket2::Domain::IPV6, socket2::Type::DGRAM, None).and_then(|s| {
            // Not the default on Windows: one socket for both families keeps one mapped port.
            s.set_only_v6(false)?;
            s.bind(&SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)).into())?;
            Ok(s)
        });
    match socket {
        Ok(socket) => Ok((socket.into(), true)),
        Err(e) => {
            tracing::info!("no IPv6 socket ({e}); direct connections use IPv4 only");
            Ok((UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?, false))
        }
    }
}

fn normalize_hash(hash: &str) -> Result<String, P2pError> {
    let hash = hash.trim().to_ascii_lowercase();
    if hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(hash)
    } else {
        Err(P2pError::BadPeerHash)
    }
}
