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
    let outer = tokio::time::timeout(SETUP_TIMEOUT, open(relay, crypto, &pin))
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
async fn open(
    relay: &RelayAnnouncement,
    crypto: Arc<rustls::ClientConfig>,
    pin: &RelayPin,
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
            let result = open_quic(udp, &host, crypto).await;
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

async fn open_quic(addr: SocketAddr, host: &str, crypto: Arc<rustls::ClientConfig>) -> Attempt {
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
        .keep_alive_interval(Some(Duration::from_secs(10)));
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

#[cfg(test)]
mod tests {
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
