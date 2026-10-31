//! The control-channel connection: login, background read/write tasks, the event stream and the
//! command API.

use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use prost::Message;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf};
use tokio::net::TcpStream;
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::error::{CommandError, ConnectError, PermissionDenied, ProtocolError, RejectReason};
use crate::framing::{self, Frame, MessageType};
use crate::proto;
use crate::ratelimit::{self, LeakyBucket, Limit};
use crate::state::{Channel, CryptSetup, ServerInfo, ServerState, User, UserChange};
use crate::tap::{PacketTap, TapDirection, TapPacing, TappedFrame};
use crate::tls::{self, CertVerifier, ClientIdentity, ServerCert};
use crate::version::MumbleVersion;
use crate::voice::CryptStats;

/// Interval of TCP keep-alive pings; Mumble's own client uses the same.
const PING_INTERVAL: Duration = Duration::from_secs(5);
/// Servers drop clients after 30 s of silence; if we hear nothing for that long the link is dead.
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a requested disconnect may spend sending what is still queued. After the burst of 4,
/// paced messages go out one per 1.2 s, so this covers a burst plus one more; a longer wait would
/// hold up quitting the app for messages that are rarely that many.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// Everything needed to open a connection.
pub struct ConnectConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    /// Server or user password.
    pub password: Option<String>,
    /// Access tokens (channel passwords) the server matches against `#token` ACL groups.
    pub tokens: Vec<String>,
    /// Client certificate; without one the user cannot register.
    pub identity: Option<ClientIdentity>,
    /// Trust decision for the server certificate (see [`crate::tls::ServerCert`]).
    pub verify_cert: CertVerifier,
    /// Upper bound for TCP connect + TLS + login.
    pub login_timeout: Duration,
}

/// Where a text message goes (or came to). Any combination is allowed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextTarget {
    /// Private recipients by session.
    pub sessions: Vec<u32>,
    pub channels: Vec<u32>,
    /// Channels including all sub-channels.
    pub trees: Vec<u32>,
}

impl TextTarget {
    pub fn channel(id: u32) -> Self {
        Self {
            channels: vec![id],
            ..Default::default()
        }
    }

    pub fn user(session: u32) -> Self {
        Self {
            sessions: vec![session],
            ..Default::default()
        }
    }
}

/// A received text message. `message` is server-provided HTML and must be sanitized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextMessage {
    /// Sender session; `None` for messages from the server itself.
    pub actor: Option<u32>,
    pub target: TextTarget,
    pub message: String,
}

/// Largest plugin data payload a standard server forwards (murmur's `MAX_DATA_LENGTH`).
pub const PLUGIN_DATA_MAX: usize = 1000;

/// Data another client sent through the server's plugin data channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginData {
    /// Sender session as stamped by the server.
    pub sender: Option<u32>,
    /// What the data is; receivers ignore ids they do not know.
    pub data_id: String,
    pub data: Bytes,
}

/// Why the connection ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisconnectReason {
    /// `Handle::disconnect` was called or all handles were dropped.
    Requested,
    /// The server removed us (kick or ban).
    Kicked {
        actor: Option<u32>,
        reason: Option<String>,
        ban: bool,
    },
    /// The server closed the connection.
    Closed,
    /// Nothing received for too long.
    Timeout,
    /// Network or protocol failure.
    Error(String),
}

/// Changes after login. State is already updated when an event arrives.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    ChannelUpdated(Channel),
    ChannelRemoved(Channel),
    UserUpdated(UserChange),
    UserRemoved {
        user: User,
        actor: Option<u32>,
        reason: Option<String>,
        ban: bool,
    },
    TextMessage(TextMessage),
    /// A request was refused; the typed reason is in the payload.
    PermissionDenied(PermissionDenied),
    /// Effective permissions changed (see `ServerState::permissions`).
    PermissionsUpdated {
        channel_id: Option<u32>,
    },
    ServerConfigUpdated(ServerInfo),
    /// UDP crypto parameters for the voice transport, passed through as received (also during
    /// login; the latest value is in `ServerState::crypt`).
    CryptSetup(CryptSetup),
    /// A voice packet tunnelled through TCP (raw, unencrypted Mumble UDP packet).
    UdpTunnel(Bytes),
    /// Plugin data from another client (server 1.4+). `data` is untrusted and unvalidated.
    PluginData(PluginData),
    /// Connection figures of a person, the answer to [`Handle::request_user_stats`]. Which
    /// fields are set depends on the own permissions; text fields are untrusted.
    UserStats(Box<proto::UserStats>),
    /// Final event; the handle stops working afterwards.
    Disconnected(DisconnectReason),
}

/// A live connection: the command handle plus the event stream.
pub struct Connection {
    pub handle: Handle,
    /// Unbounded so a slow consumer can never stall the reader and miss pings.
    pub events: mpsc::UnboundedReceiver<Event>,
    /// The server certificate seen during the TLS handshake, for the server info panel.
    pub server_cert: Option<ServerCert>,
}

enum Outgoing {
    Frame(Frame),
    /// Answered once everything queued before it is written and the server has answered a ping
    /// sent after it.
    Flush(oneshot::Sender<()>),
    /// Sends what is still queued (within [`DRAIN_TIMEOUT`]), then closes.
    Disconnect,
    /// Closes at once and drops anything queued: the connection is lost or the login failed.
    Abort,
}

struct Shared {
    state: Mutex<ServerState>,
    shutdown: Notify,
    /// Set by [`Handle::disconnect`]: no new commands are taken while the queue drains.
    closing: AtomicBool,
    /// Timestamp of the latest ping the server answered. The server handles a client's messages
    /// in order, so an answered ping means everything sent before it has been processed.
    pong: watch::Sender<u64>,
    reason: Mutex<Option<DisconnectReason>>,
    connected: AtomicBool,
    ping: Mutex<PingReport>,
    tap: Option<Arc<dyn PacketTap>>,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, ServerState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn ping(&self) -> MutexGuard<'_, PingReport> {
        self.ping.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Records the first reason for ending the connection and wakes the reader.
    fn shut_down(&self, reason: DisconnectReason) {
        let mut slot = self.reason.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_none() {
            *slot = Some(reason);
        }
        self.shutdown.notify_one();
    }

    fn tap(&self, direction: TapDirection, frame: &Frame, pacing: Option<TapPacing>) {
        if let Some(tap) = self.tap.as_ref().filter(|tap| tap.enabled()) {
            tap.on_frame(TappedFrame::new(
                direction,
                frame.kind,
                &frame.payload,
                pacing,
            ));
        }
    }
}

/// Own ping figures, sent along with every keep-alive ping. The server keeps only what each
/// client reports about itself and hands that out in UserStats, so without them everyone else
/// sees no ping for this user (FR-2.25).
struct PingReport {
    started: Instant,
    tcp_packets: u32,
    tcp_avg_ms: f64,
    /// Sum of squared deviations from the mean (Welford), for the variance.
    tcp_m2: f64,
    udp: Option<UdpPing>,
    /// Voice packets this client received, which the server shows others as "from client".
    crypt: CryptStats,
}

/// Smoothed UDP ping of the voice link, which runs apart from the control connection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UdpPing {
    pub avg_ms: f32,
    /// UDP packets received so far.
    pub packets: u32,
}

impl Default for PingReport {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            tcp_packets: 0,
            tcp_avg_ms: 0.0,
            tcp_m2: 0.0,
            udp: None,
            crypt: CryptStats::default(),
        }
    }
}

impl PingReport {
    /// Microseconds like Mumble: a local server answers well below one millisecond.
    fn now_us(&self) -> u64 {
        self.started.elapsed().as_micros() as u64
    }

    fn on_reply(&mut self, sent_us: u64) {
        let Some(rtt_us) = self.now_us().checked_sub(sent_us) else {
            return;
        };
        let rtt_ms = rtt_us as f64 / 1000.0;
        self.tcp_packets = self.tcp_packets.saturating_add(1);
        let delta = rtt_ms - self.tcp_avg_ms;
        self.tcp_avg_ms += delta / f64::from(self.tcp_packets);
        self.tcp_m2 += delta * (rtt_ms - self.tcp_avg_ms);
    }

    fn message(&self) -> proto::Ping {
        let tcp = self.tcp_packets > 0;
        proto::Ping {
            timestamp: Some(self.now_us()),
            tcp_packets: tcp.then_some(self.tcp_packets),
            tcp_ping_avg: tcp.then_some(self.tcp_avg_ms as f32),
            tcp_ping_var: tcp.then(|| (self.tcp_m2 / f64::from(self.tcp_packets)) as f32),
            udp_packets: self.udp.map(|u| u.packets),
            udp_ping_avg: self.udp.map(|u| u.avg_ms),
            good: Some(self.crypt.good),
            late: Some(self.crypt.late),
            lost: Some(self.crypt.lost),
            resync: Some(self.crypt.resync),
            ..Default::default()
        }
    }
}

/// Cloneable command API. All commands are queued without waiting; denials arrive later as
/// [`Event::PermissionDenied`], successes as state updates. Messages the server rate-limits
/// (own `UserState`, `TextMessage`) are paced to a burst of 4 and then one per 1.2 s, so batch
/// related changes (e.g. `listen` takes several channels) instead of sending many commands.
#[derive(Clone)]
pub struct Handle {
    tx: mpsc::UnboundedSender<Outgoing>,
    shared: Arc<Shared>,
}

/// Connects, logs in and returns once the server has sent `ServerSync`.
pub async fn connect(config: ConnectConfig) -> Result<Connection, ConnectError> {
    connect_with_progress(config, None).await
}

/// Called with the number of channels received so far while the initial tree streams in.
pub type SyncProgress = Arc<dyn Fn(usize) + Send + Sync>;

/// Like [`connect`], and reports the channel count while waiting for `ServerSync`.
pub async fn connect_with_progress(
    config: ConnectConfig,
    progress: Option<SyncProgress>,
) -> Result<Connection, ConnectError> {
    connect_with_tap(config, progress, None).await
}

/// Like [`connect_with_progress`], and hands every frame from the start of the login on to
/// `tap`, for debugging tools.
pub async fn connect_with_tap(
    config: ConnectConfig,
    progress: Option<SyncProgress>,
    tap: Option<Arc<dyn PacketTap>>,
) -> Result<Connection, ConnectError> {
    let limit = config.login_timeout;
    tokio::time::timeout(limit, connect_inner(config, progress, tap))
        .await
        .map_err(|_| ConnectError::Timeout)?
}

async fn connect_inner(
    config: ConnectConfig,
    progress: Option<SyncProgress>,
    tap: Option<Arc<dyn PacketTap>>,
) -> Result<Connection, ConnectError> {
    let server_name = ServerName::try_from(config.host.clone())
        .map_err(|_| ConnectError::InvalidAddress(config.host.clone()))?;
    let (tls_config, seen_cert) = tls::client_config(config.identity, config.verify_cert)?;
    // Resolved separately so a lookup failure can be told apart from an unreachable host.
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((config.host.as_str(), config.port))
        .await
        .map_err(ConnectError::Resolve)?
        .collect();
    if addrs.is_empty() {
        return Err(ConnectError::Resolve(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no addresses found",
        )));
    }
    let tcp = TcpStream::connect(addrs.as_slice()).await?;
    // Small control messages (and tunnelled voice) must not wait for Nagle.
    tcp.set_nodelay(true)?;
    let mut stream = match TlsConnector::from(tls_config)
        .connect(server_name, tcp)
        .await
    {
        Ok(stream) => stream,
        Err(err) => {
            let tls_err = err
                .get_ref()
                .and_then(|e| e.downcast_ref::<rustls::Error>())
                .cloned();
            return Err(match (tls_err, seen_cert.take()) {
                (
                    Some(rustls::Error::InvalidCertificate(
                        rustls::CertificateError::ApplicationVerificationFailure,
                    )),
                    Some(cert),
                ) => ConnectError::CertificateRejected(Box::new(cert)),
                (Some(e), _) => ConnectError::Tls(e),
                (None, _) => ConnectError::Io(err),
            });
        }
    };
    // The verifier keeps the certificate it saw; take it for the server info panel.
    let server_cert = seen_cert.take();
    // Version must reach the server before Authenticate: the server judges the client by the
    // version it knows when handling Authenticate (e.g. the "client does not support
    // ChannelListener" warning for clients it takes as pre-1.4). Version is a rate-limited type,
    // so the paced write loop would let the unlimited Authenticate overtake it; both are
    // therefore written here, in order, before the write loop starts.
    let mut handshake = BytesMut::new();
    for frame in handshake_frames(config.username, config.password, config.tokens) {
        frame.encode(&mut handshake);
    }
    async {
        stream.write_all(&handshake).await?;
        stream.flush().await
    }
    .await?;
    let (mut reader, writer) = tokio::io::split(stream);

    let (tx, rx) = mpsc::unbounded_channel();
    let (events_tx, events) = mpsc::unbounded_channel();
    let shared = Arc::new(Shared {
        state: Mutex::new(ServerState::default()),
        shutdown: Notify::new(),
        closing: AtomicBool::new(false),
        pong: watch::Sender::new(0),
        reason: Mutex::new(None),
        connected: AtomicBool::new(true),
        ping: Mutex::default(),
        tap,
    });
    tokio::spawn(write_loop(writer, rx, shared.clone()));
    let handle = Handle {
        tx,
        shared: shared.clone(),
    };

    let mut dispatcher = Dispatcher {
        shared: shared.clone(),
        events: events_tx,
        synced: false,
    };
    let mut buf = BytesMut::with_capacity(16 * 1024);
    let mut reported = 0;
    loop {
        if reader
            .read_buf(&mut buf)
            .await
            .inspect_err(|_| handle.close())?
            == 0
        {
            handle.close();
            return Err(ConnectError::Closed);
        }
        while let Some(frame) = framing::decode(&mut buf).inspect_err(|_| handle.close())? {
            match dispatcher.handle(frame) {
                Control::Continue => {
                    if let Some(progress) = &progress {
                        let count = handle.with_state(|s| s.channels.len());
                        if count != reported {
                            reported = count;
                            progress(count);
                        }
                    }
                }
                Control::Synced => {
                    tokio::spawn(read_loop(reader, buf, dispatcher, handle.tx.downgrade()));
                    return Ok(Connection {
                        handle,
                        events,
                        server_cert,
                    });
                }
                Control::Rejected(reason, message) => {
                    handle.close();
                    return Err(ConnectError::Rejected { reason, message });
                }
                Control::Kicked(_) => {
                    handle.close();
                    return Err(ConnectError::Closed);
                }
            }
        }
    }
}

/// The release name sent in the Version message; see [`set_client_release`].
static CLIENT_RELEASE: OnceLock<String> = OnceLock::new();

/// Sets the release name servers show for this client (default `Mumcord <crate version>`), so an
/// app can report its own build version. A process-wide setting, because the name belongs to the
/// running program rather than to one connection; only the first call takes effect.
pub fn set_client_release(release: impl Into<String>) {
    // A second call keeps the first name; nothing to report.
    let _ = CLIENT_RELEASE.set(release.into());
}

fn client_release() -> String {
    CLIENT_RELEASE
        .get()
        .cloned()
        .unwrap_or_else(|| format!("Mumcord {}", env!("CARGO_PKG_VERSION")))
}

/// The login messages in the order the server must receive them: Version, then Authenticate.
fn handshake_frames(username: String, password: Option<String>, tokens: Vec<String>) -> [Frame; 2] {
    let version = proto::Version {
        version_v1: Some(MumbleVersion::CLIENT.to_v1()),
        version_v2: Some(MumbleVersion::CLIENT.to_v2()),
        release: Some(client_release()),
        os: Some(std::env::consts::OS.to_owned()),
        os_version: None,
    };
    let auth = proto::Authenticate {
        username: Some(username),
        password,
        tokens,
        opus: Some(true),
        ..Default::default()
    };
    [
        Frame::from_message(MessageType::Version, &version),
        Frame::from_message(MessageType::Authenticate, &auth),
    ]
}

fn access_tokens_message(tokens: Vec<String>) -> proto::Authenticate {
    proto::Authenticate {
        tokens,
        ..Default::default()
    }
}

async fn write_loop<W: AsyncWrite + Unpin>(
    writer: W,
    mut rx: mpsc::UnboundedReceiver<Outgoing>,
    shared: Arc<Shared>,
) {
    let mut ping =
        tokio::time::interval_at(tokio::time::Instant::now() + PING_INTERVAL, PING_INTERVAL);
    let mut pong = shared.pong.subscribe();
    let mut out = Writer::new(writer, shared.clone(), tokio::time::Instant::now());
    // Set while a requested disconnect drains the queue: the deadline and the flush that ends it.
    let mut closing: Option<tokio::time::Instant> = None;
    let mut drained: Option<oneshot::Receiver<()>> = None;
    let mut handles_gone = false;
    let result = loop {
        let now = tokio::time::Instant::now();
        let messages_ready_at = out.messages.ready_at(now);
        let plugin_data_ready_at = out.plugin_data.ready_at(now);
        let step = tokio::select! {
            msg = rx.recv(), if !handles_gone => match msg {
                Some(Outgoing::Frame(frame)) => match ratelimit::limit_of(frame.kind) {
                    Some(Limit::Messages) => {
                        out.messages.push(frame, now);
                        Ok(())
                    }
                    Some(Limit::PluginData) => {
                        out.plugin_data.push(frame, now);
                        Ok(())
                    }
                    None => out.write(&frame, None).await,
                },
                Some(Outgoing::Flush(done)) => {
                    out.flushes.push(out.flush_marker(done));
                    out.confirm_written().await
                }
                Some(Outgoing::Disconnect) | None if closing.is_none() => {
                    handles_gone = msg.is_none();
                    closing = Some(tokio::time::Instant::now() + DRAIN_TIMEOUT);
                    let (done, wait) = oneshot::channel();
                    drained = Some(wait);
                    out.flushes.push(out.flush_marker(done));
                    out.confirm_written().await
                }
                Some(Outgoing::Disconnect) => Ok(()),
                None => {
                    handles_gone = true;
                    Ok(())
                }
                Some(Outgoing::Abort) => break Ok(()),
            },
            () = sleep_until_some(messages_ready_at) => match out.messages.pop() {
                Some((frame, _)) => out.write_paced(&frame, None).await,
                None => Ok(()),
            },
            () = sleep_until_some(plugin_data_ready_at) => match out.plugin_data.pop() {
                Some((frame, pacing)) => out.write_paced(&frame, Some(pacing)).await,
                None => Ok(()),
            },
            Ok(()) = pong.changed() => {
                out.confirm_answered(*pong.borrow_and_update());
                Ok(())
            }
            () = done_or_pending(&mut drained) => break Ok(()),
            () = sleep_until_some(closing) => {
                tracing::warn!("disconnect timed out before the queue was sent");
                break Ok(());
            }
            _ = ping.tick() => {
                let frame = Frame::from_message(MessageType::Ping, &shared.ping().message());
                out.write(&frame, None).await
            }
        };
        if let Err(err) = step {
            break Err(err);
        }
    };
    out.log_dropped();
    match result {
        Ok(()) => {
            // Best effort close_notify; the connection is going away either way.
            let _ = out.writer.shutdown().await;
            shared.shut_down(DisconnectReason::Requested);
        }
        Err(err) => {
            tracing::warn!(error = %err, "control channel write failed");
            shared.shut_down(DisconnectReason::Error(err.to_string()));
        }
    }
}

/// The write side of the control channel: rate-limited frames wait here in order, one queue per
/// server bucket; everything else (voice, pings) goes out at once.
struct Writer<W> {
    writer: W,
    buf: BytesMut,
    shared: Arc<Shared>,
    messages: Paced,
    plugin_data: Paced,
    /// Flushes whose frames are not all written yet.
    flushes: Vec<FlushMarker>,
    /// Flushes whose frames are written, waiting for the answer to the ping with this timestamp.
    unanswered: Vec<(u64, oneshot::Sender<()>)>,
}

/// A flush waits until both queues have sent as many frames as they had taken when it arrived.
struct FlushMarker {
    messages: u64,
    plugin_data: u64,
    done: oneshot::Sender<()>,
}

impl<W: AsyncWrite + Unpin> Writer<W> {
    fn new(writer: W, shared: Arc<Shared>, now: tokio::time::Instant) -> Self {
        Self {
            writer,
            buf: BytesMut::new(),
            shared,
            messages: Paced::new(Limit::Messages, now),
            plugin_data: Paced::new(Limit::PluginData, now),
            flushes: Vec::new(),
            unanswered: Vec::new(),
        }
    }

    async fn write(&mut self, frame: &Frame, pacing: Option<TapPacing>) -> std::io::Result<()> {
        self.buf.clear();
        frame.encode(&mut self.buf);
        self.writer.write_all(&self.buf).await?;
        self.writer.flush().await?;
        self.shared.tap(TapDirection::Sent, frame, pacing);
        Ok(())
    }

    async fn write_paced(
        &mut self,
        frame: &Frame,
        pacing: Option<TapPacing>,
    ) -> std::io::Result<()> {
        self.write(frame, pacing).await?;
        self.confirm_written().await
    }

    fn flush_marker(&self, done: oneshot::Sender<()>) -> FlushMarker {
        FlushMarker {
            messages: self.messages.pushed,
            plugin_data: self.plugin_data.pushed,
            done,
        }
    }

    /// Sends one ping for all flushes whose frames are now written; its answer completes them.
    async fn confirm_written(&mut self) -> std::io::Result<()> {
        let (sent, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.flushes)
            .into_iter()
            .partition(|f| {
                self.messages.popped >= f.messages && self.plugin_data.popped >= f.plugin_data
            });
        self.flushes = waiting;
        if sent.is_empty() {
            return Ok(());
        }
        let ping = self.shared.ping().message();
        let stamp = ping.timestamp.unwrap_or_default();
        self.unanswered
            .extend(sent.into_iter().map(|f| (stamp, f.done)));
        self.write(&Frame::from_message(MessageType::Ping, &ping), None)
            .await
    }

    /// Completes the flushes whose ping the server has answered (`answered` is its timestamp).
    fn confirm_answered(&mut self, answered: u64) {
        let (done, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.unanswered)
            .into_iter()
            .partition(|(stamp, _)| *stamp <= answered);
        self.unanswered = waiting;
        for (_, flush) in done {
            // The caller may have stopped waiting.
            let _ = flush.send(());
        }
    }

    /// Reports what is still queued and will not be sent: message kinds and counts, no content.
    fn log_dropped(&self) {
        let mut dropped: BTreeMap<u16, usize> = BTreeMap::new();
        for (frame, _) in self.messages.queue.iter().chain(&self.plugin_data.queue) {
            *dropped.entry(frame.kind).or_default() += 1;
        }
        for (kind, count) in dropped {
            let kind = MessageType::from_u16(kind);
            tracing::warn!(?kind, count, "queued messages dropped on disconnect");
        }
    }
}

/// Frames that count against one of the server's rate limits, sent no faster than it allows.
struct Paced {
    bucket: LeakyBucket,
    /// Frames with the time they were queued.
    queue: VecDeque<(Frame, tokio::time::Instant)>,
    /// Frames taken and sent so far, for flushes to tell when their frames are out.
    pushed: u64,
    popped: u64,
}

impl Paced {
    fn new(limit: Limit, now: tokio::time::Instant) -> Self {
        Self {
            bucket: LeakyBucket::new(limit, now),
            queue: VecDeque::new(),
            pushed: 0,
            popped: 0,
        }
    }

    fn push(&mut self, frame: Frame, now: tokio::time::Instant) {
        self.queue.push_back((frame, now));
        self.pushed += 1;
    }

    /// When the next queued frame may go, or `None` with nothing queued.
    fn ready_at(&self, now: tokio::time::Instant) -> Option<tokio::time::Instant> {
        (!self.queue.is_empty()).then(|| now + self.bucket.wait_time(now))
    }

    /// The next frame to send; counted as sent, since a failed write ends the connection anyway.
    fn pop(&mut self) -> Option<(Frame, TapPacing)> {
        let (frame, queued) = self.queue.pop_front()?;
        let now = tokio::time::Instant::now();
        self.bucket.take(now);
        self.popped += 1;
        let pacing = TapPacing {
            delay: now.saturating_duration_since(queued),
            tokens_left: self.bucket.tokens_left(now),
        };
        Some((frame, pacing))
    }
}

/// Waits for `rx` to be answered, or forever when there is nothing to wait for.
async fn done_or_pending(rx: &mut Option<oneshot::Receiver<()>>) {
    match rx {
        Some(rx) => {
            let _ = rx.await;
        }
        None => std::future::pending().await,
    }
}

/// Sleeps until `at`, or forever when there is nothing to wait for.
async fn sleep_until_some(at: Option<tokio::time::Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

async fn read_loop(
    mut reader: ReadHalf<TlsStream<TcpStream>>,
    mut buf: BytesMut,
    mut dispatcher: Dispatcher,
    writer: mpsc::WeakUnboundedSender<Outgoing>,
) {
    let shared = dispatcher.shared.clone();
    let reason = 'outer: loop {
        // Frames already buffered during login are handled before reading more.
        loop {
            match framing::decode(&mut buf) {
                Ok(Some(frame)) => {
                    if let Control::Kicked(reason) = dispatcher.handle(frame) {
                        break 'outer reason;
                    }
                }
                Ok(None) => break,
                Err(err) => break 'outer DisconnectReason::Error(err.to_string()),
            }
        }
        tokio::select! {
            read = tokio::time::timeout(READ_TIMEOUT, reader.read_buf(&mut buf)) => match read {
                Err(_) => break DisconnectReason::Timeout,
                Ok(Ok(0)) => break DisconnectReason::Closed,
                Ok(Ok(_)) => {}
                Ok(Err(err)) => break DisconnectReason::Error(err.to_string()),
            },
            () = shared.shutdown.notified() => {
                let stored = shared.reason.lock().unwrap_or_else(|p| p.into_inner()).take();
                break stored.unwrap_or(DisconnectReason::Requested);
            }
        }
    };
    shared.connected.store(false, Ordering::Release);
    // Stop the writer too; it is already gone if it caused the shutdown.
    if let Some(tx) = writer.upgrade() {
        let _ = tx.send(Outgoing::Abort);
    }
    tracing::info!(?reason, "control channel closed");
    let _ = dispatcher.events.send(Event::Disconnected(reason));
}

enum Control {
    Continue,
    Synced,
    Rejected(RejectReason, String),
    Kicked(DisconnectReason),
}

struct Dispatcher {
    shared: Arc<Shared>,
    events: mpsc::UnboundedSender<Event>,
    /// Before `ServerSync` the initial tree is only stored, not announced, so consumers don't
    /// treat everyone already on the server as "just joined".
    synced: bool,
}

fn decode<M: Message + Default>(kind: &'static str, payload: Bytes) -> Result<M, ProtocolError> {
    M::decode(payload).map_err(|source| ProtocolError::Decode { kind, source })
}

impl Dispatcher {
    fn emit(&self, event: Event) {
        // A dropped receiver only means nobody listens any more.
        let _ = self.events.send(event);
    }

    fn emit_synced(&self, event: Event) {
        if self.synced {
            self.emit(event);
        }
    }

    fn handle(&mut self, frame: Frame) -> Control {
        self.shared.tap(TapDirection::Received, &frame, None);
        let Some(kind) = frame.message_type() else {
            tracing::debug!(kind = frame.kind, "ignoring unknown control message");
            return Control::Continue;
        };
        match self.handle_known(kind, frame.payload) {
            Ok(control) => control,
            Err(err) => {
                // One malformed message is not worth dropping the connection over.
                tracing::warn!(error = %err, "skipping malformed control message");
                Control::Continue
            }
        }
    }

    fn handle_known(
        &mut self,
        kind: MessageType,
        payload: Bytes,
    ) -> Result<Control, ProtocolError> {
        let shared = self.shared.clone();
        match kind {
            MessageType::UdpTunnel => self.emit(Event::UdpTunnel(payload)),
            MessageType::Version => {
                let m: proto::Version = decode("Version", payload)?;
                let mut state = shared.state();
                state.server.version = MumbleVersion::from_wire(m.version_v1, m.version_v2);
                state.server.release = m.release;
                state.server.os = m.os;
            }
            MessageType::Reject => {
                let m: proto::Reject = decode("Reject", payload)?;
                let reason = RejectReason::from_wire(m.r#type);
                return Ok(Control::Rejected(reason, m.reason.unwrap_or_default()));
            }
            MessageType::ServerSync => {
                let m: proto::ServerSync = decode("ServerSync", payload)?;
                shared.state().apply_server_sync(&m);
                self.synced = true;
                return Ok(Control::Synced);
            }
            MessageType::ChannelState => {
                let m: proto::ChannelState = decode("ChannelState", payload)?;
                let channel = shared.state().apply_channel_state(m).cloned();
                if let Some(channel) = channel {
                    self.emit_synced(Event::ChannelUpdated(channel));
                }
            }
            MessageType::ChannelRemove => {
                let m: proto::ChannelRemove = decode("ChannelRemove", payload)?;
                let removed = shared.state().apply_channel_remove(m.channel_id);
                if let Some(channel) = removed {
                    self.emit_synced(Event::ChannelRemoved(channel));
                }
            }
            MessageType::UserState => {
                let m: proto::UserState = decode("UserState", payload)?;
                let change = shared.state().apply_user_state(m);
                if let Some(change) = change {
                    self.emit_synced(Event::UserUpdated(change));
                }
            }
            MessageType::UserRemove => {
                let m: proto::UserRemove = decode("UserRemove", payload)?;
                let ban = m.ban.unwrap_or(false);
                let (removed, own) = {
                    let mut state = shared.state();
                    (
                        state.apply_user_remove(m.session),
                        state.session == Some(m.session),
                    )
                };
                if own {
                    return Ok(Control::Kicked(DisconnectReason::Kicked {
                        actor: m.actor,
                        reason: m.reason,
                        ban,
                    }));
                }
                if let Some(user) = removed {
                    self.emit_synced(Event::UserRemoved {
                        user,
                        actor: m.actor,
                        reason: m.reason,
                        ban,
                    });
                }
            }
            MessageType::TextMessage => {
                let m: proto::TextMessage = decode("TextMessage", payload)?;
                self.emit(Event::TextMessage(TextMessage {
                    actor: m.actor,
                    target: TextTarget {
                        sessions: m.session,
                        channels: m.channel_id,
                        trees: m.tree_id,
                    },
                    message: m.message,
                }));
            }
            MessageType::PluginDataTransmission => {
                let m: proto::PluginDataTransmission = decode("PluginDataTransmission", payload)?;
                self.emit(Event::PluginData(PluginData {
                    sender: m.sender_session,
                    data_id: m.data_id.unwrap_or_default(),
                    data: m.data.map(Bytes::from).unwrap_or_default(),
                }));
            }
            MessageType::UserStats => {
                let m: proto::UserStats = decode("UserStats", payload)?;
                self.emit(Event::UserStats(Box::new(m)));
            }
            MessageType::PermissionDenied => {
                let m: proto::PermissionDenied = decode("PermissionDenied", payload)?;
                self.emit(Event::PermissionDenied(m.into()));
            }
            MessageType::CryptSetup => {
                let m: proto::CryptSetup = decode("CryptSetup", payload)?;
                let crypt = CryptSetup::from(m);
                {
                    let mut state = shared.state();
                    // A full setup carries the key; later ones only refresh a nonce.
                    match (&mut state.crypt, crypt.key.is_some()) {
                        (Some(current), false) => {
                            if crypt.server_nonce.is_some() {
                                current.server_nonce.clone_from(&crypt.server_nonce);
                            }
                        }
                        (slot, _) => *slot = Some(crypt.clone()),
                    }
                }
                self.emit(Event::CryptSetup(crypt));
            }
            MessageType::PermissionQuery => {
                let m: proto::PermissionQuery = decode("PermissionQuery", payload)?;
                shared.state().apply_permission_query(&m);
                self.emit_synced(Event::PermissionsUpdated {
                    channel_id: m.channel_id,
                });
            }
            MessageType::ServerConfig => {
                let m: proto::ServerConfig = decode("ServerConfig", payload)?;
                let info = {
                    let mut state = shared.state();
                    state.apply_server_config(m);
                    state.server.clone()
                };
                self.emit_synced(Event::ServerConfigUpdated(info));
            }
            MessageType::Ping => {
                let m: proto::Ping = decode("Ping", payload)?;
                if let Some(sent) = m.timestamp {
                    self.shared.ping().on_reply(sent);
                    self.shared.pong.send_if_modified(|latest| {
                        let newer = sent > *latest;
                        if newer {
                            *latest = sent;
                        }
                        newer
                    });
                }
            }
            _ => tracing::trace!(?kind, "ignoring control message"),
        }
        Ok(Control::Continue)
    }
}

impl Handle {
    fn send(&self, frame: Frame) -> Result<(), CommandError> {
        self.queue(Outgoing::Frame(frame))
    }

    fn queue(&self, out: Outgoing) -> Result<(), CommandError> {
        if !self.is_connected() || self.shared.closing.load(Ordering::Acquire) {
            return Err(CommandError::NotConnected);
        }
        self.tx.send(out).map_err(|_| CommandError::NotConnected)
    }

    /// Closes at once without sending what is queued (login failed).
    fn close(&self) {
        let _ = self.tx.send(Outgoing::Abort);
    }

    fn send_user_state(&self, mut m: proto::UserState) -> Result<(), CommandError> {
        m.session = self.shared.state().session;
        self.send(Frame::from_message(MessageType::UserState, &m))
    }

    fn require(&self, feature: &'static str, required: MumbleVersion) -> Result<(), CommandError> {
        let server = self.server_version().unwrap_or(MumbleVersion::new(0, 0, 0));
        if server >= required {
            Ok(())
        } else {
            Err(CommandError::Unsupported {
                feature,
                required,
                server,
            })
        }
    }

    pub fn is_connected(&self) -> bool {
        self.shared.connected.load(Ordering::Acquire)
    }

    /// Snapshot of the current server state.
    pub fn state(&self) -> ServerState {
        self.shared.state().clone()
    }

    /// Reads the state without cloning it.
    pub fn with_state<R>(&self, f: impl FnOnce(&ServerState) -> R) -> R {
        f(&self.shared.state())
    }

    pub fn server_version(&self) -> Option<MumbleVersion> {
        self.shared.state().server.version
    }

    pub fn join_channel(&self, channel_id: u32) -> Result<(), CommandError> {
        self.send_user_state(proto::UserState {
            channel_id: Some(channel_id),
            ..Default::default()
        })
    }

    /// Moves another person to a channel. The server checks the move permission and answers
    /// a refusal with `PermissionDenied`.
    pub fn move_user(&self, session: u32, channel_id: u32) -> Result<(), CommandError> {
        let m = proto::UserState {
            session: Some(session),
            channel_id: Some(channel_id),
            ..Default::default()
        };
        self.send(Frame::from_message(MessageType::UserState, &m))
    }

    pub fn set_self_mute(&self, mute: bool) -> Result<(), CommandError> {
        self.send_user_state(proto::UserState {
            self_mute: Some(mute),
            ..Default::default()
        })
    }

    /// Deafening also mutes (the server enforces this too).
    pub fn set_self_deaf(&self, deaf: bool) -> Result<(), CommandError> {
        let mut m = proto::UserState {
            self_deaf: Some(deaf),
            ..Default::default()
        };
        if deaf {
            m.self_mute = Some(true);
        }
        self.send_user_state(m)
    }

    /// Starts listening to (following) channels. Needs server 1.4+.
    pub fn listen(&self, channels: &[u32]) -> Result<(), CommandError> {
        self.require("listening", MumbleVersion::LISTENING)?;
        self.send_user_state(proto::UserState {
            listening_channel_add: channels.to_vec(),
            ..Default::default()
        })
    }

    pub fn unlisten(&self, channels: &[u32]) -> Result<(), CommandError> {
        self.require("listening", MumbleVersion::LISTENING)?;
        self.send_user_state(proto::UserState {
            listening_channel_remove: channels.to_vec(),
            ..Default::default()
        })
    }

    /// Stores a listener volume factor on the server (1.5+; 0.0 = silent). Older servers keep no
    /// such setting, so callers must also apply the volume locally.
    pub fn set_listener_volume(&self, channel_id: u32, volume: f32) -> Result<(), CommandError> {
        self.require("listener volume", MumbleVersion::LISTENER_VOLUME)?;
        self.send_user_state(proto::UserState {
            listening_volume_adjustment: vec![proto::user_state::VolumeAdjustment {
                listening_channel: Some(channel_id),
                volume_adjustment: Some(volume),
            }],
            ..Default::default()
        })
    }

    /// Sets the own avatar; an empty vector removes it. Scaling to the server limit is the
    /// caller's job.
    pub fn set_texture(&self, image: Vec<u8>) -> Result<(), CommandError> {
        self.send_user_state(proto::UserState {
            texture: Some(image),
            ..Default::default()
        })
    }

    /// Announces the game context and player identity for positional audio, like the official
    /// client: the server exchanges positions only between users with the same context. Empty
    /// values mean "no game".
    pub fn set_plugin_context(
        &self,
        context: Vec<u8>,
        identity: String,
    ) -> Result<(), CommandError> {
        self.send_user_state(proto::UserState {
            plugin_context: Some(context),
            plugin_identity: Some(identity),
            ..Default::default()
        })
    }

    /// Sets the own comment (HTML); an empty string removes it.
    pub fn set_comment(&self, comment: String) -> Result<(), CommandError> {
        self.send_user_state(proto::UserState {
            comment: Some(comment),
            ..Default::default()
        })
    }

    /// Registers the own certificate under the current name. Needs a client certificate and the
    /// SelfRegister permission; success shows up as a `user_id` on the own user.
    pub fn register_self(&self) -> Result<(), CommandError> {
        self.send_user_state(proto::UserState {
            user_id: Some(0),
            ..Default::default()
        })
    }

    /// Sends a text message (Mumble-compatible HTML).
    pub fn send_text(&self, target: &TextTarget, message: &str) -> Result<(), CommandError> {
        let m = proto::TextMessage {
            actor: None,
            session: target.sessions.clone(),
            channel_id: target.channels.clone(),
            tree_id: target.trees.clone(),
            message: message.to_owned(),
        };
        self.send(Frame::from_message(MessageType::TextMessage, &m))
    }

    /// Sends plugin data to the given sessions (server 1.4+). The server drops data over
    /// [`PLUGIN_DATA_MAX`] bytes and rate-limits senders; sends are paced below that limit, so a
    /// burst is delayed rather than lost, but keep it small and infrequent.
    pub fn send_plugin_data(
        &self,
        receivers: &[u32],
        data_id: &str,
        data: Vec<u8>,
    ) -> Result<(), CommandError> {
        self.require("plugin data", MumbleVersion::PLUGIN_DATA)?;
        let m = proto::PluginDataTransmission {
            sender_session: self.shared.state().session,
            receiver_sessions: receivers.to_vec(),
            data: Some(data),
            data_id: Some(data_id.to_owned()),
        };
        self.send(Frame::from_message(MessageType::PluginDataTransmission, &m))
    }

    /// Asks for full avatars/comments/descriptions that were announced only by hash.
    pub fn request_blobs(
        &self,
        textures: &[u32],
        comments: &[u32],
        channel_descriptions: &[u32],
    ) -> Result<(), CommandError> {
        let m = proto::RequestBlob {
            session_texture: textures.to_vec(),
            session_comment: comments.to_vec(),
            channel_description: channel_descriptions.to_vec(),
        };
        self.send(Frame::from_message(MessageType::RequestBlob, &m))
    }

    /// Asks for a person's connection figures; `stats_only` leaves out certificates and client
    /// version (the server sends those only to admins and for the own session anyway).
    pub fn request_user_stats(&self, session: u32, stats_only: bool) -> Result<(), CommandError> {
        let m = proto::UserStats {
            session: Some(session),
            stats_only: Some(stats_only),
            ..Default::default()
        };
        self.send(Frame::from_message(MessageType::UserStats, &m))
    }

    /// Sets the voice link figures the next keep-alive pings report: the UDP ping (`None` while
    /// voice runs over TCP) and the counts of received voice packets.
    pub fn report_voice_link(&self, ping: Option<UdpPing>, crypt: CryptStats) {
        let mut report = self.shared.ping();
        report.udp = ping;
        report.crypt = crypt;
    }

    /// Replaces the access tokens (channel passwords) of this session. A connected server takes
    /// only the tokens from a repeated Authenticate, like official clients send it; it then
    /// re-evaluates the ACLs and resends which channels the user may enter.
    pub fn set_access_tokens(&self, tokens: Vec<String>) -> Result<(), CommandError> {
        self.send(Frame::from_message(
            MessageType::Authenticate,
            &access_tokens_message(tokens),
        ))
    }

    /// Asks the server for the own permissions in a channel.
    pub fn query_permissions(&self, channel_id: u32) -> Result<(), CommandError> {
        let m = proto::PermissionQuery {
            channel_id: Some(channel_id),
            ..Default::default()
        };
        self.send(Frame::from_message(MessageType::PermissionQuery, &m))
    }

    /// Replaces a channel's ACL and groups (admin tooling such as test bots; needs the Write
    /// permission, e.g. as SuperUser). Unset groups/ACL entries are removed by the server.
    pub fn send_acl(&self, acl: proto::Acl) -> Result<(), CommandError> {
        self.send(Frame::from_message(MessageType::Acl, &acl))
    }

    /// Changes another person's state, e.g. server mute, deafen or suppress (admin tooling such
    /// as test bots; needs the MuteDeafen permission, e.g. as SuperUser). `m.session` names the
    /// person and is sent as given.
    pub fn send_admin_user_state(&self, m: proto::UserState) -> Result<(), CommandError> {
        self.send(Frame::from_message(MessageType::UserState, &m))
    }

    /// Creates or edits a channel (admin tooling such as test bots; needs the matching
    /// permissions, e.g. as SuperUser). Without `channel_id` the server creates a new channel.
    pub fn send_channel_state(&self, state: proto::ChannelState) -> Result<(), CommandError> {
        self.send(Frame::from_message(MessageType::ChannelState, &state))
    }

    /// Voice hook: sends a raw Mumble UDP packet through the TCP tunnel.
    pub fn send_tunnel(&self, packet: Bytes) -> Result<(), CommandError> {
        self.send(Frame {
            kind: MessageType::UdpTunnel as u16,
            payload: packet,
        })
    }

    /// Voice hook: sends a `CryptSetup` (empty = request a resync; with `client_nonce` = answer a
    /// server resync request).
    pub fn send_crypt_setup(&self, crypt: CryptSetup) -> Result<(), CommandError> {
        let m = proto::CryptSetup::from(crypt);
        self.send(Frame::from_message(MessageType::CryptSetup, &m))
    }

    /// Waits until everything queued so far has been sent and the server has processed it,
    /// which a ping answered afterwards confirms. Rate-limited messages beyond the burst take
    /// 1.2 s each, so this can take a while; wrap it in a timeout if needed. Fails with
    /// [`CommandError::NotConnected`] if the connection ends first.
    pub async fn flush(&self) -> Result<(), CommandError> {
        let (done, wait) = oneshot::channel();
        self.queue(Outgoing::Flush(done))?;
        wait.await.map_err(|_| CommandError::NotConnected)
    }

    /// Closes the connection gracefully: what is still queued is sent first, at the pace the
    /// server allows, and confirmed like [`Handle::flush`]; after 2 s the rest is dropped (and
    /// logged by kind and count). New commands fail from now on. An [`Event::Disconnected`] with
    /// `Requested` follows; [`Handle::closed`] waits for the end.
    pub fn disconnect(&self) {
        self.shared.closing.store(true, Ordering::Release);
        let _ = self.tx.send(Outgoing::Disconnect);
    }

    /// Sends a Mumble ping and waits for the server's answer: the round trip, for a one-shot
    /// reachability check (the server handles messages in order, so the answer also proves the
    /// connection is alive). `None` when the connection ends first.
    pub async fn ping(&self) -> Option<Duration> {
        let mut pong = self.shared.pong.subscribe();
        let sent_us = self.shared.ping().now_us();
        let frame = Frame::from_message(MessageType::Ping, &self.shared.ping().message());
        self.queue(Outgoing::Frame(frame)).ok()?;
        let started = Instant::now();
        loop {
            if *pong.borrow_and_update() > sent_us {
                return Some(started.elapsed());
            }
            pong.changed().await.ok()?;
        }
    }

    /// Resolves once the connection has stopped sending: after [`Handle::disconnect`] that is
    /// when the queue is drained or its time is up, otherwise when the connection is lost.
    pub fn closed(&self) -> impl Future<Output = ()> + Send + use<> {
        let tx = self.tx.clone();
        async move { tx.closed().await }
    }
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::*;

    #[test]
    fn pings_report_the_own_tcp_and_udp_figures() {
        let mut report = PingReport {
            started: Instant::now() - Duration::from_secs(1),
            ..Default::default()
        };
        assert_eq!(report.message().tcp_ping_avg, None);
        let now = report.now_us();
        report.on_reply(now - 30_000);
        report.on_reply(now - 10_000);
        report.udp = Some(UdpPing {
            avg_ms: 12.5,
            packets: 40,
        });
        let m = report.message();
        assert_eq!(m.tcp_packets, Some(2));
        let avg = m.tcp_ping_avg.expect("tcp ping");
        assert!((20.0..21.0).contains(&avg), "{avg}");
        assert!(m.tcp_ping_var.expect("var") > 90.0);
        assert_eq!((m.udp_packets, m.udp_ping_avg), (Some(40), Some(12.5)));
        assert_eq!(m.good, Some(0));
    }

    #[test]
    fn send_acl_queues_an_acl_frame() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let handle = Handle {
            tx,
            shared: Arc::new(Shared {
                state: Mutex::new(ServerState::default()),
                shutdown: Notify::new(),
                closing: AtomicBool::new(false),
                pong: watch::Sender::new(0),
                reason: Mutex::new(None),
                connected: AtomicBool::new(true),
                ping: Mutex::default(),
                tap: None,
            }),
        };
        let acl = proto::Acl {
            channel_id: 3,
            inherit_acls: Some(true),
            ..Default::default()
        };
        handle.send_acl(acl.clone()).expect("queued");
        let Ok(Outgoing::Frame(frame)) = rx.try_recv() else {
            panic!("expected a frame");
        };
        assert_eq!(frame.kind, MessageType::Acl as u16);
        assert_eq!(proto::Acl::decode(frame.payload).expect("decode"), acl);
    }

    #[test]
    fn plugin_data_needs_a_1_4_server_and_names_the_receivers() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let state = ServerState {
            session: Some(7),
            ..Default::default()
        };
        let handle = Handle {
            tx,
            shared: Arc::new(Shared {
                state: Mutex::new(state),
                shutdown: Notify::new(),
                closing: AtomicBool::new(false),
                pong: watch::Sender::new(0),
                reason: Mutex::new(None),
                connected: AtomicBool::new(true),
                ping: Mutex::default(),
                tap: None,
            }),
        };
        assert!(matches!(
            handle.send_plugin_data(&[3], "id", vec![1]),
            Err(CommandError::Unsupported { .. })
        ));
        handle.shared.state().server.version = Some(MumbleVersion::new(1, 4, 0));
        handle
            .send_plugin_data(&[3, 4], "mumcord.test", b"{}".to_vec())
            .expect("queued");
        let Ok(Outgoing::Frame(frame)) = rx.try_recv() else {
            panic!("expected a frame");
        };
        assert_eq!(frame.kind, MessageType::PluginDataTransmission as u16);
        let m = proto::PluginDataTransmission::decode(frame.payload).expect("decode");
        assert_eq!(m.sender_session, Some(7));
        assert_eq!(m.receiver_sessions, vec![3, 4]);
        assert_eq!(m.data_id.as_deref(), Some("mumcord.test"));
        assert_eq!(m.data.as_deref(), Some(&b"{}"[..]));
    }
}

#[cfg(test)]
mod drain_tests {
    use tokio::io::{AsyncReadExt, DuplexStream};
    use tokio::task::JoinHandle;

    use super::*;

    fn shared() -> Arc<Shared> {
        Arc::new(Shared {
            state: Mutex::new(ServerState::default()),
            shutdown: Notify::new(),
            closing: AtomicBool::new(false),
            pong: watch::Sender::new(0),
            reason: Mutex::new(None),
            connected: AtomicBool::new(true),
            ping: Mutex::default(),
            tap: None,
        })
    }

    fn text(i: u32) -> Frame {
        let m = proto::TextMessage {
            message: format!("message {i}"),
            ..Default::default()
        };
        Frame::from_message(MessageType::TextMessage, &m)
    }

    /// Plays the server: collects the text messages until the client closes, answering pings
    /// only when `answer` is set.
    fn server(
        mut stream: DuplexStream,
        shared: Arc<Shared>,
        answer: bool,
    ) -> JoinHandle<Vec<String>> {
        tokio::spawn(async move {
            let mut buf = BytesMut::new();
            let mut texts = Vec::new();
            while stream.read_buf(&mut buf).await.expect("read") > 0 {
                while let Some(frame) = framing::decode(&mut buf).expect("frame") {
                    if frame.kind == MessageType::TextMessage as u16 {
                        let m = proto::TextMessage::decode(frame.payload).expect("text");
                        texts.push(m.message);
                    } else if frame.kind == MessageType::Ping as u16 && answer {
                        let m = proto::Ping::decode(frame.payload).expect("ping");
                        shared.pong.send_replace(m.timestamp.expect("timestamp"));
                    }
                }
            }
            texts
        })
    }

    fn start(answer: bool) -> (Handle, JoinHandle<Vec<String>>) {
        start_with(answer, shared())
    }

    fn start_with(answer: bool, shared: Arc<Shared>) -> (Handle, JoinHandle<Vec<String>>) {
        let (client, server_side) = tokio::io::duplex(64 * 1024);
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(write_loop(client, rx, shared.clone()));
        let server = server(server_side, shared.clone(), answer);
        (Handle { tx, shared }, server)
    }

    fn expected(count: u32) -> Vec<String> {
        (0..count).map(|i| format!("message {i}")).collect()
    }

    #[tokio::test(start_paused = true)]
    async fn disconnect_sends_the_queue_beyond_the_burst_first() {
        let (handle, server) = start(true);
        let started = tokio::time::Instant::now();
        for i in 0..5 {
            handle.send(text(i)).expect("queued");
        }
        handle.disconnect();
        assert_eq!(handle.send(text(9)), Err(CommandError::NotConnected));
        handle.closed().await;
        assert_eq!(server.await.expect("server"), expected(5));
        // The fifth waited for the pace: the drain respects the rate limit.
        assert!(started.elapsed() >= Duration::from_millis(1200));
        assert_eq!(
            *handle.shared.reason.lock().expect("reason"),
            Some(DisconnectReason::Requested)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn disconnect_drops_what_does_not_fit_in_the_timeout() {
        let (handle, server) = start(true);
        let started = tokio::time::Instant::now();
        for i in 0..8 {
            handle.send(text(i)).expect("queued");
        }
        handle.disconnect();
        handle.closed().await;
        // Burst of 4, one more after 1.2 s; the next would be due at 2.4 s.
        assert_eq!(server.await.expect("server"), expected(5));
        assert_eq!(started.elapsed(), DRAIN_TIMEOUT);
    }

    #[tokio::test(start_paused = true)]
    async fn disconnect_waits_for_the_server_to_confirm() {
        let (handle, server) = start(false);
        let started = tokio::time::Instant::now();
        handle.send(text(0)).expect("queued");
        handle.disconnect();
        handle.closed().await;
        assert_eq!(server.await.expect("server"), expected(1));
        assert_eq!(started.elapsed(), DRAIN_TIMEOUT);
    }

    #[tokio::test(start_paused = true)]
    async fn ping_returns_the_round_trip_of_the_answer() {
        let (handle, server) = start(true);
        assert!(handle.ping().await.is_some());
        // A ping is queued like any other frame; the fake server sees it.
        handle.disconnect();
        assert_eq!(server.await.expect("server"), Vec::<String>::new());
    }

    #[tokio::test(start_paused = true)]
    async fn ping_gives_up_when_the_server_never_answers() {
        let (handle, _server) = start(false);
        let waited = tokio::time::timeout(Duration::from_secs(1), handle.ping()).await;
        assert!(waited.is_err(), "no answer means no result");
    }

    #[tokio::test(start_paused = true)]
    async fn ping_gives_up_when_the_connection_ends() {
        let (handle, server) = start(true);
        drop(server);
        let _ = handle.ping().await;
    }

    #[tokio::test(start_paused = true)]
    async fn flush_completes_once_the_queue_is_sent_and_confirmed() {
        let (handle, server) = start(true);
        let started = tokio::time::Instant::now();
        for i in 0..6 {
            handle.send(text(i)).expect("queued");
        }
        handle.flush().await.expect("flushed");
        // Burst of 4, then 1.2 s each.
        assert_eq!(started.elapsed(), Duration::from_millis(2400));
        handle.disconnect();
        assert_eq!(server.await.expect("server"), expected(6));
    }

    #[derive(Default)]
    struct Recorder(Mutex<Vec<u16>>);

    impl PacketTap for Recorder {
        fn enabled(&self) -> bool {
            true
        }

        fn on_frame(&self, frame: TappedFrame) {
            assert_eq!(frame.direction, TapDirection::Sent);
            self.0.lock().expect("recorder").push(frame.kind);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn frames_sent_while_draining_reach_the_tap() {
        let recorder = Arc::new(Recorder::default());
        let mut shared = shared();
        Arc::get_mut(&mut shared).expect("unshared").tap = Some(recorder.clone());
        let (handle, server) = start_with(true, shared);
        for i in 0..6 {
            handle.send(text(i)).expect("queued");
        }
        handle.disconnect();
        handle.closed().await;
        assert_eq!(server.await.expect("server"), expected(5));
        let texts = recorder
            .0
            .lock()
            .expect("recorder")
            .iter()
            .filter(|kind| **kind == MessageType::TextMessage as u16)
            .count();
        assert_eq!(texts, 5);
    }

    #[tokio::test(start_paused = true)]
    async fn abort_drops_the_queue() {
        let (handle, server) = start(true);
        for i in 0..6 {
            handle.send(text(i)).expect("queued");
        }
        handle.close();
        handle.closed().await;
        // Some of the burst may have gone out already; the paced rest never does.
        let sent = server.await.expect("server");
        assert!(sent.len() <= 4, "{sent:?}");
    }
}

#[cfg(test)]
mod handshake_tests {
    use super::*;

    #[test]
    fn version_precedes_authenticate_with_both_encodings() {
        let [version, auth] = handshake_frames("alice".to_owned(), None, vec!["secret".into()]);
        assert_eq!(version.message_type(), Some(MessageType::Version));
        assert_eq!(auth.message_type(), Some(MessageType::Authenticate));

        let v = proto::Version::decode(version.payload).expect("version");
        // Server 1.5 reads version_v2 (major << 48 | minor << 32 | patch << 16), older ones v1.
        assert_eq!(v.version_v2, Some(0x0001_0005_0000_0000));
        assert_eq!(v.version_v1, Some(0x0001_0500));
        assert!(
            MumbleVersion::from_wire(v.version_v1, v.version_v2)
                .is_some_and(|v| v.supports_listening())
        );
        assert!(v.release.is_some_and(|r| r.starts_with("Mumcord ")));

        let a = proto::Authenticate::decode(auth.payload).expect("authenticate");
        assert_eq!(a.username.as_deref(), Some("alice"));
        assert_eq!(a.opus, Some(true));
        assert_eq!(a.tokens, ["secret"]);
    }

    #[test]
    fn the_app_sets_the_release_name_servers_show() {
        // Process-wide: keeps the "Mumcord " prefix the other handshake test checks for.
        set_client_release("Mumcord dev~a1b2c3d");
        set_client_release("Mumcord ignored");
        let [version, _] = handshake_frames("alice".to_owned(), None, Vec::new());
        let v = proto::Version::decode(version.payload).expect("version");
        assert_eq!(v.release.as_deref(), Some("Mumcord dev~a1b2c3d"));
    }

    #[test]
    fn access_tokens_resend_carries_only_the_tokens() {
        let frame = Frame::from_message(
            MessageType::Authenticate,
            &access_tokens_message(vec!["one".into(), "two".into()]),
        );
        let a = proto::Authenticate::decode(frame.payload).expect("authenticate");
        assert_eq!(a.tokens, ["one", "two"]);
        assert_eq!(a.username, None);
        assert_eq!(a.password, None);
    }
}
