//! Direct encrypted connections between two Mumcord clients (FR-2.35).
//!
//! Transport: QUIC (quinn) over rustls/ring with mutual TLS. Each side presents its Mumble client
//! certificate and accepts the other side only if that certificate's hash equals the hash the
//! Mumble server reports for the user (`UserState.hash`); anything else drops the connection.
//!
//! Paths: IPv6 and LAN addresses of the network interfaces and a router port mapping (PCP,
//! NAT-PMP, UPnP). When no direct path works the attempt ends with [`P2pError::NoDirectPath`];
//! the caller may then use an optional community relay announced on the Mumble server
//! ([`P2p::connect_via_relay`]). The relay only forwards the same end-to-end encrypted QUIC
//! packets and cannot read them; Mumcord works fully without one.
//!
//! Signalling is transport-agnostic: [`Signal`] messages are encoded here and carried by the
//! caller as Mumble plugin data with the id [`DATA_ID`].
//!
//! Flow:
//! 1. Offering side: [`P2p::offer`] (or [`P2p::offer_with`]) → send [`PendingOffer::signal`] to
//!    the peer.
//! 2. Answering side: decode the offer, then either [`P2p::answer`] (or [`P2p::answer_with`]) → send
//!    [`PendingAnswer::signal`] back and call [`PendingAnswer::connect`], or reply with
//!    [`Signal::Decline`].
//! 3. Offering side: [`PendingOffer::complete`] with the received answer.
//!
//! Relay: both sides get the same [`RelayToken`] from the relay bot and call
//! [`P2p::connect_via_relay`]; the offering side dials the inner connection, the answering side
//! accepts it. [`PeerConnection::path`] tells how a connection travels.
//!
//! Consent (FR-2.36) is the caller's job: an offer or answer reveals this machine's addresses
//! (it also opens a router port mapping), and [`PendingOffer::complete`] and
//! [`PendingAnswer::connect`] are the only calls that dial the peer. Call `offer`/`answer` only
//! after the user allowed direct connections. [`Reveal`] without `direct` gathers nothing and
//! opens no port, so the peer never learns this machine's addresses (spec 04, FR-4.13).

mod gather;
mod peer;
mod relay;
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

pub use peer::{
    MAX_PEER_BYTES, PEER_DATA_ID, PEER_VERSION, PeerError, PeerMessage, RelayAnnouncement,
    RelayState, RelayToken, RequestId, TokenErrorReason,
};
pub use quinn;
pub use relay::{RelayRefusal, RelayTransport};
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
const CLOSE_RELAY_LOST: u32 = 3;

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
    /// No address of the other side could be reached in time.
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
    /// Neither QUIC on the relay's UDP port nor TLS on its TCP port connected in time.
    #[error("the relay could not be reached over UDP or TCP")]
    RelayUnreachable,
    /// The relay presented another certificate than the announced fingerprint.
    #[error("the relay's certificate does not match the announced one")]
    RelayCertMismatch,
    #[error("the relay refused the connection: {0}")]
    RelayRefused(RelayRefusal),
    /// The relay broke the handshake, closed the link, or the other side did not get through
    /// it in time.
    #[error("the relay connection failed")]
    RelayFailed,
}

/// What an offer or answer tells the peer (spec 04, FR-4.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reveal {
    /// Send this machine's addresses for a direct connection. Without it nothing is gathered,
    /// no router port is opened and the candidate list stays empty (the IP stays hidden).
    pub direct: bool,
    /// Tell the peer this side can use the relay announced on the server (`rl`).
    pub relay: bool,
}

impl Reveal {
    /// A direct connection only, as before relays existed.
    pub const DIRECT: Self = Self {
        direct: true,
        relay: false,
    };
}

/// Which side of the signalling a relay connection is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Sent the offer; dials the inner connection.
    Offering,
    /// Sent the answer; accepts the inner connection.
    Answering,
}

/// How a [`PeerConnection`] reaches the other side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionPath {
    Direct,
    Relay(RelayTransport),
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

    /// Prepares a direct connection to the user whose certificate hash the server reports as
    /// `peer_hash`. Binds a socket and gathers addresses; does not contact the peer.
    pub async fn offer(&self, peer_hash: &str) -> Result<PendingOffer, P2pError> {
        self.offer_with(peer_hash, Reveal::DIRECT).await
    }

    /// Like [`Self::offer`], revealing only what `reveal` allows.
    pub async fn offer_with(
        &self,
        peer_hash: &str,
        reveal: Reveal,
    ) -> Result<PendingOffer, P2pError> {
        let attempt = self.prepare_if(reveal.direct, peer_hash).await?;
        let session = SessionId::random();
        Ok(PendingOffer {
            signal: Signal::Offer {
                session,
                candidates: candidates_of(attempt.as_ref()),
                relay: reveal.relay,
            },
            attempt,
            session,
        })
    }

    /// Prepares the reply to a received `offer` from the user with certificate hash
    /// `peer_hash`. Does not contact the peer until [`PendingAnswer::connect`].
    pub async fn answer(&self, offer: &Signal, peer_hash: &str) -> Result<PendingAnswer, P2pError> {
        self.answer_with(offer, peer_hash, Reveal::DIRECT).await
    }

    /// Like [`Self::answer`], revealing only what `reveal` allows.
    pub async fn answer_with(
        &self,
        offer: &Signal,
        peer_hash: &str,
        reveal: Reveal,
    ) -> Result<PendingAnswer, P2pError> {
        let Signal::Offer {
            session,
            candidates,
            ..
        } = offer
        else {
            return Err(SignalError::Unexpected.into());
        };
        let attempt = self.prepare_if(reveal.direct, peer_hash).await?;
        Ok(PendingAnswer {
            signal: Signal::Answer {
                session: *session,
                candidates: candidates_of(attempt.as_ref()),
                relay: reveal.relay,
            },
            targets: candidates.clone(),
            attempt,
        })
    }

    async fn prepare_if(&self, direct: bool, peer_hash: &str) -> Result<Option<Attempt>, P2pError> {
        if direct {
            self.prepare(peer_hash).await.map(Some)
        } else {
            // Still reject a malformed hash early, as a direct attempt would.
            normalize_hash(peer_hash)?;
            Ok(None)
        }
    }

    async fn prepare(&self, peer_hash: &str) -> Result<Attempt, P2pError> {
        let peer_hash = normalize_hash(peer_hash)?;
        let peer = PinnedPeer::new(peer_hash.clone());
        let (client, mut server) = tls::quic_configs(&self.chain, &self.key, &peer)?;
        let transport = Arc::new(transport_config());
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

    /// Connects to the user with certificate hash `peer_hash` through `relay`, using the pair
    /// `token` both sides received from the relay bot. The inner connection is the same mutual
    /// TLS QUIC connection as a direct one, so the relay cannot read or alter it. Call only
    /// after the user allowed P2P connections; this reveals this machine's address to the
    /// relay, not to the peer.
    pub async fn connect_via_relay(
        &self,
        relay: &RelayAnnouncement,
        token: &RelayToken,
        peer_hash: &str,
        side: Side,
    ) -> Result<PeerConnection, P2pError> {
        let peer_hash = normalize_hash(peer_hash)?;
        let peer = PinnedPeer::new(peer_hash.clone());
        let (mut client, mut server) = tls::quic_configs(&self.chain, &self.key, &peer)?;
        let mut transport = transport_config();
        // The relay link fixes the packet size (spec 04 section 6.5); probing for more would
        // only lose packets.
        transport
            .initial_mtu(relay::INNER_MTU)
            .min_mtu(relay::INNER_MTU)
            .mtu_discovery_config(None);
        let transport = Arc::new(transport);
        client.transport_config(transport.clone());
        server.transport_config(transport);

        let socket = relay::connect(
            relay,
            token,
            &self.chain,
            &self.key,
            self.config.connect_timeout,
        )
        .await?;
        let path = ConnectionPath::Relay(socket.transport());
        let (remote, lost) = (socket.remote(), socket.lost());
        let mut endpoint = Endpoint::new_with_abstract_socket(
            EndpointConfig::default(),
            (side == Side::Answering).then_some(server),
            socket,
            Arc::new(TokioRuntime),
        )?;
        endpoint.set_default_client_config(client);

        let handshake = async {
            let connecting = match side {
                Side::Offering => endpoint.connect(remote, SERVER_NAME).map_err(|e| {
                    tracing::debug!("inner connection over the relay not started: {e}");
                })?,
                Side::Answering => endpoint.accept().await.ok_or(())?.accept().map_err(|e| {
                    tracing::debug!("inner connection over the relay not accepted: {e}");
                })?,
            };
            connecting.await.map_err(|e| {
                note_refusal(&peer, &e);
                tracing::debug!("inner connection over the relay failed: {e}");
            })
        };
        let connection = tokio::select! {
            result = tokio::time::timeout(self.config.connect_timeout, handshake) => {
                result.ok().and_then(Result::ok)
            }
            () = peer.mismatched() => None,
        };
        let Some(connection) = connection else {
            return Err(if peer.saw_mismatch() {
                P2pError::CertMismatch
            } else {
                P2pError::RelayFailed
            });
        };

        // The inner keep-alive would only notice a dead relay after the idle timeout.
        let watched = connection.clone();
        tokio::spawn(async move {
            tokio::select! {
                () = lost.notified() => {
                    watched.close(VarInt::from_u32(CLOSE_RELAY_LOST), b"relay lost");
                }
                _ = watched.closed() => {}
            }
        });
        Ok(PeerConnection {
            connection,
            peer_hash,
            path,
            _endpoint: endpoint,
            _mapping: None,
        })
    }
}

fn candidates_of(attempt: Option<&Attempt>) -> Vec<SocketAddr> {
    attempt.map(|a| a.candidates.clone()).unwrap_or_default()
}

/// Transport settings shared by direct and relayed connections.
fn transport_config() -> TransportConfig {
    let mut transport = TransportConfig::default();
    transport
        .max_idle_timeout(Some(VarInt::from_u32(30_000).into()))
        .keep_alive_interval(Some(Duration::from_secs(10)));
    transport
}

/// An offer waiting for the peer's answer.
pub struct PendingOffer {
    signal: Signal,
    session: SessionId,
    /// `None` when the offer hides this machine's addresses.
    attempt: Option<Attempt>,
}

impl PendingOffer {
    /// The offer to send to the peer.
    pub fn signal(&self) -> &Signal {
        &self.signal
    }

    /// Connects with the peer's reply: dials its addresses, accepts its incoming dials and
    /// keeps the first connection that passes the certificate check. An offer that hid this
    /// machine's addresses has no direct path ([`P2pError::NoDirectPath`]).
    pub async fn complete(self, reply: &Signal) -> Result<PeerConnection, P2pError> {
        if reply.session() != self.session {
            return Err(SignalError::Unexpected.into());
        }
        match reply {
            Signal::Answer { candidates, .. } => match self.attempt {
                Some(attempt) => attempt.establish(candidates, Role::Controlling).await,
                None => Err(P2pError::NoDirectPath),
            },
            Signal::Decline { reason, .. } => Err(P2pError::Declined(*reason)),
            Signal::Offer { .. } => Err(SignalError::Unexpected.into()),
        }
    }
}

/// An answer ready to be sent; the connection starts with [`PendingAnswer::connect`].
pub struct PendingAnswer {
    signal: Signal,
    targets: Vec<SocketAddr>,
    /// `None` when the answer hides this machine's addresses.
    attempt: Option<Attempt>,
}

impl PendingAnswer {
    /// The answer to send back to the peer.
    pub fn signal(&self) -> &Signal {
        &self.signal
    }

    /// Dials the offer's addresses and accepts the peer's dials until the offering side picks
    /// one connection. Send [`Self::signal`] before or while awaiting this. An answer that hid
    /// this machine's addresses has no direct path ([`P2pError::NoDirectPath`]).
    pub async fn connect(self) -> Result<PeerConnection, P2pError> {
        match self.attempt {
            Some(attempt) => attempt.establish(&self.targets, Role::Controlled).await,
            None => Err(P2pError::NoDirectPath),
        }
    }
}

/// An authenticated connection, direct or through a relay. Streams and datagrams are used
/// through [`PeerConnection::connection`]; dropping this closes the connection and removes the
/// router port mapping.
pub struct PeerConnection {
    connection: Connection,
    peer_hash: String,
    path: ConnectionPath,
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

    /// Whether the connection is direct or goes through a relay, and over which transport.
    pub fn path(&self) -> ConnectionPath {
        self.path
    }

    /// The other side's address, or the relay's for a relayed connection.
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
                path: ConnectionPath::Direct,
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
