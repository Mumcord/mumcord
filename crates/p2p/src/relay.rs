//! Link to a community relay (spec 04, section 6.5) and the virtual socket that carries the inner
//! QUIC connection over it.
//!
//! The outer link is QUIC with datagrams on the announced UDP port, or TLS over TCP when UDP is
//! blocked. Both present the user's Mumble certificate (the relay admits only the two hashes of
//! the token's pair) and accept only the relay certificate with the announced fingerprint. After
//! `bind` and `ready` the relay copies packets between the two sides without reading them; the
//! inner connection keeps its own end-to-end mutual TLS, so the relay never sees content.

use std::fmt;
use std::io::{self, IoSliceMut};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use quinn::udp::{RecvMeta, Transmit};
use quinn::{AsyncUdpSocket, Connection, Endpoint, RecvStream, SendStream, UdpPoller};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use serde::{Deserialize, Serialize};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt,
};
use tokio::io::{BufReader, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::{Notify, mpsc};
use tokio::task::{AbortHandle, JoinSet};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::broadcast::{
    Bind as BroadcastBind, BindRole, Content, ControlKind, HEADER_LEN, Header, HeaderError,
};
use crate::tls::{self, RelayPin};
use crate::{P2pError, RelayAnnouncement, RelayToken};

/// Head start of QUIC before TLS over TCP is tried in parallel (FR-4.15).
const TCP_DELAY: Duration = Duration::from_secs(2);
/// Longest time to get any outer link to the relay, including name resolution (FR-4.15).
const SETUP_TIMEOUT: Duration = Duration::from_secs(10);
/// Outer QUIC packet size: one inner packet of [`INNER_MTU`] plus the outer header and datagram
/// frame must fit into a single outer packet.
const OUTER_MTU: u16 = 1300;
/// Fixed size of inner QUIC packets over the relay; the relay path is never probed for more.
pub(crate) const INNER_MTU: u16 = 1200;
/// Longest control message, without the newline.
const MAX_CONTROL: usize = 512;
/// Longest TCP frame payload.
const MAX_FRAME: usize = 1500;
/// TCP frame types.
const FRAME_CONTROL: u8 = 0x00;
const FRAME_INNER: u8 = 0x01;
/// Packets queued between the link and the inner endpoint. When full, packets are dropped like
/// on a congested UDP path and the inner QUIC recovers them.
const QUEUE: usize = 256;

/// How a connection to the other person travels through the relay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayTransport {
    /// QUIC datagrams on the relay's UDP port.
    Udp,
    /// TLS over TCP, for networks that block UDP.
    Tcp,
}

/// Why the relay refused a `bind` (the error codes of spec 04 section 6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RelayRefusal {
    #[error("the relay does not know this connection")]
    UnknownToken,
    #[error("the connection request expired")]
    Expired,
    #[error("this connection is not allowed")]
    NotAllowed,
    #[error("the relay bot is gone")]
    BotGone,
    #[error("the relay is full")]
    RelayFull,
    #[error("too many relay connections for this user")]
    PeerLimit,
    #[error("too many requests, try again later")]
    RateLimited,
    #[error("this Mumcord version is not supported by the relay")]
    Version,
    #[error("the relay did not understand the request")]
    BadRequest,
    /// The relay owner turned off this use (spec 07, FR-7.1).
    #[error("the relay does not allow this use")]
    UseNotAllowed,
    /// The broadcast has as many viewers as the relay allows (spec 07, FR-7.46).
    #[error("this share is full")]
    BroadcastFull,
}

impl RelayRefusal {
    fn from_code(code: &str) -> Option<Self> {
        Some(match code {
            "unknown_token" => Self::UnknownToken,
            "expired" => Self::Expired,
            "not_allowed" => Self::NotAllowed,
            "bot_gone" => Self::BotGone,
            "relay_full" => Self::RelayFull,
            "peer_limit" => Self::PeerLimit,
            "rate_limited" => Self::RateLimited,
            "version" => Self::Version,
            "bad_request" => Self::BadRequest,
            "use_not_allowed" => Self::UseNotAllowed,
            "broadcast_full" => Self::BroadcastFull,
            _ => return None,
        })
    }
}

#[derive(Serialize)]
struct Bind<'a> {
    v: u32,
    t: &'static str,
    tok: &'a str,
}

#[derive(Deserialize)]
struct Control {
    t: String,
    #[serde(default)]
    code: Option<String>,
}

/// Opens the outer link, binds `token` and waits up to `ready_timeout` for the other side.
/// Returns the socket the inner endpoint runs on.
pub(crate) async fn connect(
    relay: &RelayAnnouncement,
    token: &RelayToken,
    chain: &[CertificateDer<'static>],
    key: &PrivateKeyDer<'static>,
    ready_timeout: Duration,
) -> Result<Arc<RelaySocket>, P2pError> {
    let pin = RelayPin::new(relay.fingerprint);
    let crypto = tls::relay_client_config(chain, key, &pin)?;
    let outer = tokio::time::timeout(SETUP_TIMEOUT, open(relay, crypto, &pin, Liveness::PAIR))
        .await
        .map_err(|_| P2pError::RelayUnreachable)??;
    tokio::time::timeout(ready_timeout, outer.bind(token))
        .await
        .map_err(|_| {
            tracing::debug!("the other side did not reach the relay in time");
            P2pError::RelayFailed
        })?
}

/// A finished outer handshake, before `bind`.
enum Outer {
    Quic {
        endpoint: Endpoint,
        connection: Connection,
    },
    Tcp(Box<TlsStream<TcpStream>>),
}

type Attempt = Result<Outer, Box<dyn std::error::Error + Send + Sync>>;

/// Races QUIC against a delayed TLS-over-TCP attempt and keeps the first outer link that works.
/// How the outer QUIC link notices a relay that went away.
#[derive(Clone, Copy)]
struct Liveness {
    keep_alive: Duration,
    /// `None` keeps quinn's default.
    idle: Option<Duration>,
}

impl Liveness {
    /// A pair link: the inner connection has its own timeouts; keep-alives only stop the link
    /// from idling out while the first side waits for its partner.
    const PAIR: Self = Self {
        keep_alive: Duration::from_secs(10),
        idle: None,
    };
    /// A broadcast link: a relay lost mid-share must be noticed quickly, so viewers move to
    /// direct copies within seconds (FR-7.49).
    const BROADCAST: Self = Self {
        keep_alive: Duration::from_secs(1),
        idle: Some(Duration::from_secs(4)),
    };
}

async fn open(
    relay: &RelayAnnouncement,
    crypto: Arc<rustls::ClientConfig>,
    pin: &RelayPin,
    liveness: Liveness,
) -> Result<Outer, P2pError> {
    let udp = match tokio::net::lookup_host((relay.host.as_str(), relay.udp)).await {
        Ok(mut addresses) => addresses.next(),
        Err(e) => {
            tracing::debug!("relay name not resolved: {e}");
            None
        }
    }
    .ok_or(P2pError::RelayUnreachable)?;
    let tcp = SocketAddr::new(udp.ip(), relay.tcp);

    let (tx, mut results) = mpsc::channel::<(RelayTransport, Attempt)>(2);
    let quic_failed = Arc::new(Notify::new());
    // Dropping the set when one attempt wins aborts the other.
    let mut attempts = JoinSet::new();
    {
        let (tx, crypto, host, quic_failed) = (
            tx.clone(),
            crypto.clone(),
            relay.host.clone(),
            quic_failed.clone(),
        );
        attempts.spawn(async move {
            let result = open_quic(udp, &host, crypto, liveness).await;
            if result.is_err() {
                quic_failed.notify_one();
            }
            drop(tx.send((RelayTransport::Udp, result)).await);
        });
    }
    {
        let host = relay.host.clone();
        attempts.spawn(async move {
            // No need to wait out the head start once QUIC has failed.
            tokio::select! {
                () = tokio::time::sleep(TCP_DELAY) => {}
                () = quic_failed.notified() => {}
            }
            drop(
                tx.send((RelayTransport::Tcp, open_tcp(tcp, &host, crypto).await))
                    .await,
            );
        });
    }

    while let Some((transport, result)) = results.recv().await {
        match result {
            Ok(outer) => return Ok(outer),
            Err(e) => {
                tracing::debug!("relay link over {transport:?} failed: {e}");
                if pin.saw_mismatch() {
                    return Err(P2pError::RelayCertMismatch);
                }
            }
        }
    }
    Err(P2pError::RelayUnreachable)
}

async fn open_quic(
    addr: SocketAddr,
    host: &str,
    crypto: Arc<rustls::ClientConfig>,
    liveness: Liveness,
) -> Attempt {
    let bind: SocketAddr = if addr.is_ipv6() {
        (Ipv6Addr::UNSPECIFIED, 0).into()
    } else {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    };
    let endpoint = Endpoint::client(bind)?;
    let mut transport = quinn::TransportConfig::default();
    // A path that cannot carry this size must fail here so TLS over TCP takes over (6.5).
    transport
        .initial_mtu(OUTER_MTU)
        .min_mtu(OUTER_MTU)
        .mtu_discovery_config(None)
        // The side that bound first may wait for its partner up to the token lifetime; keep the
        // link from idling out meanwhile.
        .keep_alive_interval(Some(liveness.keep_alive));
    if let Some(idle) = liveness.idle {
        transport.max_idle_timeout(Some(idle.try_into()?));
    }
    let mut config = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto)?,
    ));
    config.transport_config(Arc::new(transport));
    let connection = endpoint.connect_with(config, addr, host)?.await?;
    match connection.max_datagram_size() {
        Some(size) if size >= usize::from(INNER_MTU) => Ok(Outer::Quic {
            endpoint,
            connection,
        }),
        _ => Err("the relay does not accept datagrams of the inner packet size".into()),
    }
}

async fn open_tcp(addr: SocketAddr, host: &str, crypto: Arc<rustls::ClientConfig>) -> Attempt {
    let name = ServerName::try_from(host.to_owned())?;
    let stream = TcpStream::connect(addr).await?;
    // Inner QUIC packets are latency-sensitive and already sized; never hold them back.
    stream.set_nodelay(true)?;
    let stream = TlsConnector::from(crypto).connect(name, stream).await?;
    Ok(Outer::Tcp(Box::new(stream)))
}

impl Outer {
    async fn bind(self, token: &RelayToken) -> Result<Arc<RelaySocket>, P2pError> {
        let bind = serde_json::to_vec(&Bind {
            v: 1,
            t: "bind",
            tok: token.expose(),
        })
        .map_err(io::Error::other)?;
        let (inbound_tx, inbound) = mpsc::channel(QUEUE);
        let lost = Arc::new(Notify::new());
        match self {
            Outer::Quic {
                endpoint,
                connection,
            } => {
                let handshake = async {
                    let (mut send, recv) = connection.open_bi().await.map_err(failed)?;
                    let mut line = bind;
                    line.push(b'\n');
                    send.write_all(&line).await.map_err(failed)?;
                    let mut recv = BufReader::new(recv);
                    while !handle_control(&read_line(&mut recv).await?)? {}
                    Ok::<_, P2pError>((send, recv))
                };
                let (send, recv) = match handshake.await {
                    Ok(control) => control,
                    // The relay may refuse by closing the connection with the error code as the
                    // close reason instead of an error line (e.g. `not_allowed` right after the
                    // TLS handshake).
                    Err(error) => return Err(close_refusal(&connection).unwrap_or(error)),
                };

                let reader =
                    tokio::spawn(read_datagrams(connection.clone(), inbound_tx, lost.clone()))
                        .abort_handle();
                Ok(Arc::new(RelaySocket {
                    transport: RelayTransport::Udp,
                    local: endpoint.local_addr()?,
                    remote: connection.remote_address(),
                    outbound: Outbound::Quic {
                        connection,
                        _endpoint: endpoint,
                        _control: (send, recv),
                    },
                    inbound: Mutex::new(inbound),
                    lost,
                    reader,
                }))
            }
            Outer::Tcp(mut stream) => {
                write_frame(&mut stream, FRAME_CONTROL, &bind)
                    .await
                    .map_err(failed)?;
                loop {
                    match read_frame(&mut stream).await.map_err(failed)? {
                        (FRAME_CONTROL, message) => {
                            if handle_control(&message)? {
                                break;
                            }
                        }
                        // The relay forwards only after `ready`; keep an early packet anyway
                        // rather than costing the inner handshake a retransmission.
                        (FRAME_INNER, packet) => drop(inbound_tx.try_send(packet.into())),
                        (kind, _) => tracing::debug!("relay frame of unknown type {kind} ignored"),
                    }
                }

                let (local, remote) = {
                    let tcp = stream.get_ref().0;
                    (tcp.local_addr()?, tcp.peer_addr()?)
                };
                let (read, write) = tokio::io::split(*stream);
                let (outbound, frames) = mpsc::channel(QUEUE);
                tokio::spawn(write_frames(write, frames));
                let reader =
                    tokio::spawn(read_frames(read, inbound_tx, lost.clone())).abort_handle();
                Ok(Arc::new(RelaySocket {
                    transport: RelayTransport::Tcp,
                    local,
                    remote,
                    outbound: Outbound::Tcp(outbound),
                    inbound: Mutex::new(inbound),
                    lost,
                    reader,
                }))
            }
        }
    }
}

/// Any failure of the link before `ready` that is not a refusal: the relay broke the protocol or
/// closed the connection.
fn failed(error: impl fmt::Display) -> P2pError {
    tracing::debug!("relay handshake failed: {error}");
    P2pError::RelayFailed
}

fn close_refusal(connection: &quinn::Connection) -> Option<P2pError> {
    match connection.close_reason()? {
        quinn::ConnectionError::ApplicationClosed(close) => {
            RelayRefusal::from_code(std::str::from_utf8(&close.reason).ok()?)
                .map(P2pError::RelayRefused)
        }
        _ => None,
    }
}

/// Handles one control message from the relay. Returns whether the session is ready.
fn handle_control(message: &[u8]) -> Result<bool, P2pError> {
    let control: Control = serde_json::from_slice(message).map_err(failed)?;
    match control.t.as_str() {
        "ready" => Ok(true),
        "waiting" => Ok(false),
        "error" => {
            let code = control.code.unwrap_or_default();
            Err(RelayRefusal::from_code(&code)
                .map(P2pError::RelayRefused)
                .unwrap_or_else(|| failed(format_args!("unknown relay error code {code:?}"))))
        }
        // Newer relays may send messages this version does not know.
        other => {
            tracing::debug!("relay control message {other:?} ignored");
            Ok(false)
        }
    }
}

/// Reads one newline-terminated control message of at most [`MAX_CONTROL`] bytes.
async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Vec<u8>, P2pError> {
    let mut line = Vec::new();
    (&mut *reader)
        .take(MAX_CONTROL as u64 + 1)
        .read_until(b'\n', &mut line)
        .await
        .map_err(failed)?;
    if line.pop() == Some(b'\n') {
        Ok(line)
    } else {
        Err(failed("control message too long or link closed"))
    }
}

async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<(u8, Vec<u8>)> {
    let mut head = [0u8; 3];
    reader.read_exact(&mut head).await?;
    let len = usize::from(u16::from_be_bytes([head[1], head[2]]));
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "relay frame too long",
        ));
    }
    let mut payload = vec![0; len];
    reader.read_exact(&mut payload).await?;
    Ok((head[0], payload))
}

async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    kind: u8,
    payload: &[u8],
) -> io::Result<()> {
    let len = u16::try_from(payload.len())
        .ok()
        .filter(|&len| usize::from(len) <= MAX_FRAME)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "relay frame too long"))?;
    let [high, low] = len.to_be_bytes();
    writer.write_all(&[kind, high, low]).await?;
    writer.write_all(payload).await?;
    writer.flush().await
}

async fn read_datagrams(connection: Connection, inbound: mpsc::Sender<Bytes>, lost: Arc<Notify>) {
    loop {
        match connection.read_datagram().await {
            Ok(packet) => {
                if inbound.send(packet).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                tracing::debug!("relay link closed: {e}");
                break;
            }
        }
    }
    lost.notify_one();
}

async fn read_frames(
    mut reader: ReadHalf<TlsStream<TcpStream>>,
    inbound: mpsc::Sender<Bytes>,
    lost: Arc<Notify>,
) {
    loop {
        match read_frame(&mut reader).await {
            Ok((FRAME_INNER, packet)) => {
                if inbound.send(packet.into()).await.is_err() {
                    break;
                }
            }
            Ok((kind, _)) => tracing::debug!("relay frame of type {kind} after ready ignored"),
            Err(e) => {
                tracing::debug!("relay link closed: {e}");
                break;
            }
        }
    }
    lost.notify_one();
}

/// Ends once the socket is gone, after sending what was queued, so the inner connection's close
/// still reaches the other side.
async fn write_frames(
    mut writer: WriteHalf<TlsStream<TcpStream>>,
    mut packets: mpsc::Receiver<Bytes>,
) {
    while let Some(packet) = packets.recv().await {
        if let Err(e) = write_frame(&mut writer, FRAME_INNER, &packet).await {
            tracing::debug!("relay link write failed: {e}");
            return;
        }
    }
    if let Err(e) = writer.shutdown().await {
        tracing::debug!("relay link not closed cleanly: {e}");
    }
}

enum Outbound {
    Quic {
        connection: Connection,
        // Kept for the link's lifetime: the endpoint drives the connection, and dropping the
        // control stream would tell the relay this side is gone.
        _endpoint: Endpoint,
        _control: (SendStream, BufReader<RecvStream>),
    },
    Tcp(mpsc::Sender<Bytes>),
}

/// The relay link seen as a UDP socket by the inner quinn endpoint: every inner packet is one
/// outer datagram or `0x01` frame, and the only "address" is the relay's.
pub(crate) struct RelaySocket {
    transport: RelayTransport,
    local: SocketAddr,
    remote: SocketAddr,
    outbound: Outbound,
    inbound: Mutex<mpsc::Receiver<Bytes>>,
    lost: Arc<Notify>,
    reader: AbortHandle,
}

impl RelaySocket {
    pub(crate) fn transport(&self) -> RelayTransport {
        self.transport
    }

    /// The address the inner endpoint dials; packets go to the relay whatever the address.
    pub(crate) fn remote(&self) -> SocketAddr {
        self.remote
    }

    /// Wakes once the relay link is gone (also when that happened before the call).
    pub(crate) fn lost(&self) -> Arc<Notify> {
        self.lost.clone()
    }
}

impl Drop for RelaySocket {
    fn drop(&mut self) {
        self.reader.abort();
        if let Outbound::Quic { connection, .. } = &self.outbound {
            connection.close(0u32.into(), b"done");
        }
    }
}

impl fmt::Debug for RelaySocket {
    // Leaves out the addresses: relay IPs are not logged.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelaySocket")
            .field("transport", &self.transport)
            .finish_non_exhaustive()
    }
}

impl AsyncUdpSocket for RelaySocket {
    fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>> {
        Box::pin(AlwaysWritable)
    }

    fn try_send(&self, transmit: &Transmit) -> io::Result<()> {
        let packet = Bytes::copy_from_slice(transmit.contents);
        match &self.outbound {
            Outbound::Quic { connection, .. } => match connection.send_datagram(packet) {
                // A lost link is reported through `lost`; the packet is gone like on UDP.
                Ok(()) | Err(quinn::SendDatagramError::ConnectionLost(_)) => {}
                Err(e) => tracing::debug!("inner packet not sent over the relay: {e}"),
            },
            // A full queue drops the packet like a congested UDP path and a closed one means
            // the link is gone (reported through `lost`); the inner QUIC handles both.
            Outbound::Tcp(frames) => drop(frames.try_send(packet)),
        }
        Ok(())
    }

    fn poll_recv(
        &self,
        cx: &mut Context,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [RecvMeta],
    ) -> Poll<io::Result<usize>> {
        let mut inbound = self
            .inbound
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match inbound.poll_recv(cx) {
            Poll::Ready(Some(packet)) => {
                let (Some(buf), Some(meta)) = (bufs.first_mut(), meta.first_mut()) else {
                    return Poll::Ready(Ok(0));
                };
                let len = packet.len().min(buf.len());
                buf[..len].copy_from_slice(&packet[..len]);
                *meta = RecvMeta {
                    addr: self.remote,
                    len,
                    stride: len,
                    ecn: None,
                    dst_ip: None,
                };
                Poll::Ready(Ok(1))
            }
            // The link is gone and nothing more arrives; the inner connection is closed through
            // `lost` instead of failing the whole endpoint.
            Poll::Ready(None) | Poll::Pending => Poll::Pending,
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.local)
    }

    fn may_fragment(&self) -> bool {
        false
    }
}

/// Sending never blocks (see [`RelaySocket::try_send`]), so the socket is always writable.
#[derive(Debug)]
struct AlwaysWritable;

impl UdpPoller for AlwaysWritable {
    fn poll_writable(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

// --- Broadcast links (spec 07, FR-7.41; mumpeer `docs/protocol.md` "Screen-share broadcasts") ---

/// Units waiting to be written to a broadcast link. The publisher paces itself before this
/// queue (one hub queue per link), so it only has to absorb short bursts.
const UNIT_QUEUE: usize = 64;
/// Units read from a broadcast link and not yet taken by the engine.
const UNIT_INBOX: usize = 64;
/// Unidirectional streams the relay may have open towards this side; the relay needs at least
/// 64 (protocol).
const UNI_STREAMS: u32 = 64;

/// Why a broadcast link ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BroadcastEnd {
    /// `closed`: the publisher closed the broadcast (or this side closed its own link).
    Closed,
    /// `idle`: no unit from the publisher for the relay's idle timeout.
    Idle,
    /// `deleted`: the bot deleted the broadcast or removed this viewer.
    Deleted,
    /// `revoked`: the certificate hash was dropped, or this viewer got a new token.
    Revoked,
    /// The link broke without an `end` (relay restart, network loss).
    Lost,
}

impl BroadcastEnd {
    fn from_reason(reason: &str) -> Self {
        match reason {
            "closed" => Self::Closed,
            "idle" => Self::Idle,
            "deleted" => Self::Deleted,
            "revoked" => Self::Revoked,
            // A newer relay may name another reason; the broadcast is over all the same.
            _ => Self::Lost,
        }
    }
}

/// One unit received on a broadcast link: the checked clear header and the whole unit (header
/// and payload) as it came, ready for decryption in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub header: Header,
    pub bytes: Vec<u8>,
}

impl Unit {
    /// The payload after the clear header.
    pub fn payload(&self) -> &[u8] {
        &self.bytes[HEADER_LEN..]
    }

    /// A control unit with `payload`; fails for a payload the relay would refuse.
    pub fn control(kind: ControlKind, payload: &[u8]) -> Result<Vec<u8>, HeaderError> {
        let header = Header {
            content: Content::Control(kind),
            key_id: 0,
            sequence: 0,
            payload_len: u32::try_from(payload.len())
                .map_err(|_| HeaderError::BadSize(u32::MAX))?,
        };
        let mut unit = Vec::with_capacity(HEADER_LEN + payload.len());
        unit.extend_from_slice(&header.encode()?);
        unit.extend_from_slice(payload);
        Ok(unit)
    }
}

/// A bound broadcast link to the relay, as publisher or subscriber. Dropping it closes the link
/// (the relay ends the broadcast when it was the publisher's).
pub struct BroadcastLink {
    transport: RelayTransport,
    outbound: mpsc::Sender<Vec<u8>>,
    inbound: mpsc::Receiver<Unit>,
    end: Arc<Mutex<Option<BroadcastEnd>>>,
    tasks: Vec<AbortHandle>,
    quic: Option<(Connection, Endpoint)>,
}

impl fmt::Debug for BroadcastLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BroadcastLink")
            .field("transport", &self.transport)
            .finish_non_exhaustive()
    }
}

impl BroadcastLink {
    pub fn transport(&self) -> RelayTransport {
        self.transport
    }

    /// A sender for whole units (clear header + payload), usable from other tasks. Sending
    /// fails once the link is gone.
    pub fn sender(&self) -> mpsc::Sender<Vec<u8>> {
        self.outbound.clone()
    }

    /// The next unit, or why the link ended.
    pub async fn recv(&mut self) -> Result<Unit, BroadcastEnd> {
        match self.inbound.recv().await {
            Some(unit) => Ok(unit),
            None => Err(lock_end(&self.end).unwrap_or(BroadcastEnd::Lost)),
        }
    }
}

impl Drop for BroadcastLink {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        if let Some((connection, _)) = &self.quic {
            connection.close(0u32.into(), b"closed");
        }
    }
}

fn lock_end(end: &Mutex<Option<BroadcastEnd>>) -> Option<BroadcastEnd> {
    *end.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn set_end(end: &Mutex<Option<BroadcastEnd>>, reason: BroadcastEnd) {
    end.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_or_insert(reason);
}

/// The `end` reason in a control message from the relay, if it is one.
fn end_of(message: &[u8]) -> Option<BroadcastEnd> {
    #[derive(Deserialize)]
    struct End {
        t: String,
        #[serde(default)]
        reason: String,
    }
    let end: End = serde_json::from_slice(message).ok()?;
    (end.t == "end").then(|| BroadcastEnd::from_reason(&end.reason))
}

impl crate::P2p {
    /// Opens an outer link to `relay` (QUIC, or TLS over TCP when UDP is blocked) and binds the
    /// broadcast `token` as `role`. The relay answers `ready` right away; there is no partner to
    /// wait for. Media units must already be end-to-end encrypted.
    pub async fn broadcast(
        &self,
        relay: &RelayAnnouncement,
        token: &RelayToken,
        role: BindRole,
    ) -> Result<BroadcastLink, P2pError> {
        let pin = RelayPin::new(relay.fingerprint);
        let crypto = tls::relay_client_config(&self.chain, &self.key, &pin)?;
        let outer = tokio::time::timeout(
            SETUP_TIMEOUT,
            open(relay, crypto, &pin, Liveness::BROADCAST),
        )
        .await
        .map_err(|_| P2pError::RelayUnreachable)??;
        let bind = BroadcastBind {
            token: token.clone(),
            role: Some(role),
        }
        .encode();
        tokio::time::timeout(SETUP_TIMEOUT, outer.bind_broadcast(bind))
            .await
            .map_err(|_| P2pError::RelayFailed)?
    }
}

impl Outer {
    async fn bind_broadcast(self, bind: Vec<u8>) -> Result<BroadcastLink, P2pError> {
        let (out_tx, out_rx) = mpsc::channel(UNIT_QUEUE);
        let (in_tx, inbound) = mpsc::channel(UNIT_INBOX);
        let end = Arc::new(Mutex::new(None));
        match self {
            Outer::Quic {
                endpoint,
                connection,
            } => {
                let handshake = async {
                    let (mut send, recv) = connection.open_bi().await.map_err(failed)?;
                    let mut line = bind;
                    line.push(b'\n');
                    send.write_all(&line).await.map_err(failed)?;
                    let mut recv = BufReader::new(recv);
                    while !handle_control(&read_line(&mut recv).await?)? {}
                    Ok::<_, P2pError>((send, recv))
                };
                let (send, recv) = match handshake.await {
                    Ok(control) => control,
                    Err(error) => return Err(close_refusal(&connection).unwrap_or(error)),
                };
                // The relay sends every unit on its own stream; it may keep this many open.
                connection.set_max_concurrent_uni_streams(UNI_STREAMS.into());
                let tasks = vec![
                    tokio::spawn(read_control(send, recv, end.clone())).abort_handle(),
                    tokio::spawn(read_unit_streams(connection.clone(), in_tx, end.clone()))
                        .abort_handle(),
                    tokio::spawn(write_unit_streams(connection.clone(), out_rx)).abort_handle(),
                ];
                Ok(BroadcastLink {
                    transport: RelayTransport::Udp,
                    outbound: out_tx,
                    inbound,
                    end,
                    tasks,
                    quic: Some((connection, endpoint)),
                })
            }
            Outer::Tcp(mut stream) => {
                write_frame(&mut stream, FRAME_CONTROL, &bind)
                    .await
                    .map_err(failed)?;
                let mut units = UnitStream::default();
                loop {
                    match read_frame(&mut stream).await.map_err(failed)? {
                        (FRAME_CONTROL, message) => {
                            if handle_control(&message)? {
                                break;
                            }
                        }
                        // Units only follow `ready`; keep an early one all the same.
                        (FRAME_INNER, bytes) => {
                            for unit in units.push(&bytes).map_err(failed)? {
                                drop(in_tx.try_send(unit));
                            }
                        }
                        (kind, _) => tracing::debug!("relay frame of unknown type {kind} ignored"),
                    }
                }
                let (read, write) = tokio::io::split(*stream);
                let tasks = vec![
                    tokio::spawn(read_unit_frames(read, units, in_tx, end.clone())).abort_handle(),
                    tokio::spawn(write_unit_frames(write, out_rx)).abort_handle(),
                ];
                Ok(BroadcastLink {
                    transport: RelayTransport::Tcp,
                    outbound: out_tx,
                    inbound,
                    end,
                    tasks,
                    quic: None,
                })
            }
        }
    }
}

/// Keeps the QUIC control stream open (closing it would end the link) and reads the `end`.
async fn read_control(
    _send: SendStream,
    mut recv: BufReader<RecvStream>,
    end: Arc<Mutex<Option<BroadcastEnd>>>,
) {
    while let Ok(line) = read_line(&mut recv).await {
        if let Some(reason) = end_of(&line) {
            set_end(&end, reason);
        }
    }
}

/// Reads one unit per incoming unidirectional stream, in the order the streams were opened.
async fn read_unit_streams(
    connection: Connection,
    inbound: mpsc::Sender<Unit>,
    end: Arc<Mutex<Option<BroadcastEnd>>>,
) {
    loop {
        let stream = match connection.accept_uni().await {
            Ok(stream) => stream,
            Err(e) => {
                if let quinn::ConnectionError::ApplicationClosed(close) = &e
                    && let Ok(reason) = std::str::from_utf8(&close.reason)
                {
                    set_end(&end, BroadcastEnd::from_reason(reason));
                }
                tracing::debug!("broadcast link closed: {e}");
                break;
            }
        };
        match read_unit_stream(stream).await {
            Ok(unit) => {
                if inbound.send(unit).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                tracing::debug!("broadcast unit refused: {e}");
                connection.close(0u32.into(), b"bad_request");
                break;
            }
        }
    }
}

async fn read_unit_stream(
    mut stream: RecvStream,
) -> Result<Unit, Box<dyn std::error::Error + Send + Sync>> {
    let mut head = [0u8; HEADER_LEN];
    stream.read_exact(&mut head).await?;
    let header = Header::decode(&head)?;
    let len = usize::try_from(header.payload_len)?;
    let mut bytes = Vec::with_capacity(HEADER_LEN + len);
    bytes.extend_from_slice(&head);
    bytes.resize(HEADER_LEN + len, 0);
    stream.read_exact(&mut bytes[HEADER_LEN..]).await?;
    // A stream carrying more than its header announces is broken.
    let mut extra = [0u8; 1];
    if stream.read(&mut extra).await?.is_some() {
        return Err("unit longer than its header says".into());
    }
    Ok(Unit { header, bytes })
}

/// Sends every unit on its own unidirectional stream, one after the other, so they stay in
/// order at the relay.
async fn write_unit_streams(connection: Connection, mut units: mpsc::Receiver<Vec<u8>>) {
    while let Some(unit) = units.recv().await {
        let sent = async {
            let mut stream = connection.open_uni().await?;
            stream.write_all(&unit).await?;
            stream.finish()?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        };
        if let Err(e) = sent.await {
            tracing::debug!("broadcast link write failed: {e}");
            return;
        }
    }
}

/// Puts units back together from the byte stream of `0x01` frames on TCP.
#[derive(Default)]
struct UnitStream {
    buffer: Vec<u8>,
}

impl UnitStream {
    /// Adds the bytes of one frame; returns the units it completed.
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<Unit>, HeaderError> {
        self.buffer.extend_from_slice(bytes);
        let mut units = Vec::new();
        loop {
            if self.buffer.len() < HEADER_LEN {
                break;
            }
            // Checked before any payload is waited for, so a bad size never grows the buffer.
            let header = Header::decode(&self.buffer)?;
            let total = HEADER_LEN + usize::try_from(header.payload_len).unwrap_or(usize::MAX);
            if self.buffer.len() < total {
                break;
            }
            let rest = self.buffer.split_off(total);
            let bytes = std::mem::replace(&mut self.buffer, rest);
            units.push(Unit { header, bytes });
        }
        Ok(units)
    }
}

async fn read_unit_frames(
    mut reader: ReadHalf<TlsStream<TcpStream>>,
    mut units: UnitStream,
    inbound: mpsc::Sender<Unit>,
    end: Arc<Mutex<Option<BroadcastEnd>>>,
) {
    loop {
        match read_frame(&mut reader).await {
            Ok((FRAME_INNER, bytes)) => match units.push(&bytes) {
                Ok(done) => {
                    for unit in done {
                        if inbound.send(unit).await.is_err() {
                            return;
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!("broadcast unit refused: {e}");
                    return;
                }
            },
            Ok((FRAME_CONTROL, message)) => {
                if let Some(reason) = end_of(&message) {
                    set_end(&end, reason);
                }
            }
            Ok((kind, _)) => tracing::debug!("relay frame of type {kind} after ready ignored"),
            Err(e) => {
                tracing::debug!("broadcast link closed: {e}");
                return;
            }
        }
    }
}

/// Cuts units into `0x01` frames of at most [`MAX_FRAME`] bytes.
async fn write_unit_frames(
    mut writer: WriteHalf<TlsStream<TcpStream>>,
    mut units: mpsc::Receiver<Vec<u8>>,
) {
    while let Some(unit) = units.recv().await {
        for chunk in unit.chunks(MAX_FRAME) {
            if let Err(e) = write_frame(&mut writer, FRAME_INNER, chunk).await {
                tracing::debug!("broadcast link write failed: {e}");
                return;
            }
        }
    }
    if let Err(e) = writer.shutdown().await {
        tracing::debug!("broadcast link not closed cleanly: {e}");
    }
}

#[cfg(test)]
mod tests {

    fn video_unit(len: usize, sequence: u32) -> Vec<u8> {
        let header = Header {
            content: Content::Video {
                codec: crate::broadcast::VideoCodec::Vp9,
                keyframe: sequence == 0,
                temporal_layer: 0,
            },
            key_id: 1,
            sequence,
            payload_len: u32::try_from(len).expect("test"),
        };
        let mut unit = header.encode().expect("test").to_vec();
        unit.extend((0..len).map(|i| i as u8));
        unit
    }

    #[test]
    fn tcp_units_are_cut_into_frames_and_put_back_together() {
        let units = [
            video_unit(4000, 0),
            Unit::control(ControlKind::KeyframeRequest, &[]).expect("test"),
            video_unit(10, 1),
        ];
        let stream: Vec<u8> = units.concat();
        let mut reader = UnitStream::default();
        let mut got = Vec::new();
        // Frames cut anywhere inside units, as a relay may.
        for chunk in stream.chunks(MAX_FRAME).flat_map(|c| c.chunks(700)) {
            got.extend(reader.push(chunk).expect("test"));
        }
        let bytes: Vec<Vec<u8>> = got.iter().map(|u| u.bytes.clone()).collect();
        assert_eq!(bytes, units);
        assert_eq!(
            got[1].header.content,
            Content::Control(ControlKind::KeyframeRequest)
        );
        assert_eq!(got[2].payload().len(), 10);
    }

    #[test]
    fn a_bad_unit_header_is_refused_before_its_payload() {
        let mut unit = video_unit(10, 0);
        unit[0] = 9;
        assert!(UnitStream::default().push(&unit).is_err());
    }

    #[test]
    fn control_units_check_their_payload() {
        assert!(Unit::control(ControlKind::Layers, &[2]).is_ok());
        assert!(Unit::control(ControlKind::Layers, &[]).is_err());
        assert!(Unit::control(ControlKind::Key, &[0; 2000]).is_err());
    }

    #[test]
    fn reads_end_reasons() {
        assert_eq!(
            end_of(br#"{"v":1,"t":"end","reason":"idle"}"#),
            Some(BroadcastEnd::Idle)
        );
        assert_eq!(
            end_of(br#"{"v":1,"t":"end","reason":"deleted"}"#),
            Some(BroadcastEnd::Deleted)
        );
        assert_eq!(
            end_of(br#"{"v":1,"t":"end","reason":"new"}"#),
            Some(BroadcastEnd::Lost)
        );
        assert_eq!(end_of(br#"{"v":1,"t":"ready"}"#), None);
    }
    use super::*;

    #[test]
    fn maps_every_error_code() {
        for (code, refusal) in [
            ("unknown_token", RelayRefusal::UnknownToken),
            ("expired", RelayRefusal::Expired),
            ("not_allowed", RelayRefusal::NotAllowed),
            ("bot_gone", RelayRefusal::BotGone),
            ("relay_full", RelayRefusal::RelayFull),
            ("peer_limit", RelayRefusal::PeerLimit),
            ("rate_limited", RelayRefusal::RateLimited),
            ("version", RelayRefusal::Version),
            ("bad_request", RelayRefusal::BadRequest),
            ("use_not_allowed", RelayRefusal::UseNotAllowed),
            ("broadcast_full", RelayRefusal::BroadcastFull),
        ] {
            let message = format!(r#"{{"v":1,"t":"error","code":"{code}"}}"#);
            assert!(matches!(
                handle_control(message.as_bytes()),
                Err(P2pError::RelayRefused(r)) if r == refusal
            ));
        }
        assert!(matches!(
            handle_control(br#"{"v":1,"t":"error","code":"new_code"}"#),
            Err(P2pError::RelayFailed)
        ));
    }

    #[test]
    fn handles_waiting_ready_and_unknown_messages() {
        assert!(!handle_control(br#"{"v":1,"t":"waiting"}"#).expect("test"));
        assert!(handle_control(br#"{"v":1,"t":"ready"}"#).expect("test"));
        assert!(!handle_control(br#"{"v":2,"t":"news","x":1}"#).expect("test"));
        assert!(handle_control(b"not json").is_err());
    }

    #[test]
    fn bind_has_the_wire_format() {
        let token = RelayToken::parse(&"a".repeat(43)).expect("test");
        let bind = serde_json::to_string(&Bind {
            v: 1,
            t: "bind",
            tok: token.expose(),
        })
        .expect("test");
        assert_eq!(
            bind,
            format!(r#"{{"v":1,"t":"bind","tok":"{}"}}"#, "a".repeat(43))
        );
    }

    #[tokio::test]
    async fn control_lines_are_limited() {
        let mut ok = &b"{\"t\":\"ready\"}\nrest"[..];
        assert_eq!(
            read_line(&mut ok).await.expect("test"),
            b"{\"t\":\"ready\"}"
        );
        let long = vec![b'x'; MAX_CONTROL + 10];
        assert!(read_line(&mut &long[..]).await.is_err());
        assert!(read_line(&mut &b"no newline"[..]).await.is_err());
    }

    #[tokio::test]
    async fn frames_round_trip_and_are_limited() {
        let mut wire = Vec::new();
        write_frame(&mut wire, FRAME_INNER, b"packet")
            .await
            .expect("test");
        assert_eq!(&wire[..3], &[FRAME_INNER, 0, 6]);
        assert_eq!(
            read_frame(&mut &wire[..]).await.expect("test"),
            (FRAME_INNER, b"packet".to_vec())
        );
        assert!(
            write_frame(&mut Vec::new(), FRAME_INNER, &[0; MAX_FRAME + 1])
                .await
                .is_err()
        );
        let too_long = [FRAME_INNER, 0x05, 0xdd];
        assert!(read_frame(&mut &too_long[..]).await.is_err());
    }
}
