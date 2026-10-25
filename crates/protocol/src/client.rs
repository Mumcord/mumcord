//! The control-channel connection: login, background read/write tasks, the event stream and the
//! command API.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use prost::Message;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::{Notify, mpsc};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::error::{CommandError, ConnectError, PermissionDenied, ProtocolError, RejectReason};
use crate::framing::{self, Frame, MessageType};
use crate::proto;
use crate::ratelimit::{self, LeakyBucket};
use crate::state::{Channel, CryptSetup, ServerInfo, ServerState, User, UserChange};
use crate::tls::{self, CertVerifier, ClientIdentity};
use crate::version::MumbleVersion;

/// Interval of TCP keep-alive pings; Mumble's own client uses the same.
const PING_INTERVAL: Duration = Duration::from_secs(5);
/// Servers drop clients after 30 s of silence; if we hear nothing for that long the link is dead.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Everything needed to open a connection.
pub struct ConnectConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    /// Server or user password.
    pub password: Option<String>,
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
    /// Final event; the handle stops working afterwards.
    Disconnected(DisconnectReason),
}

/// A live connection: the command handle plus the event stream.
pub struct Connection {
    pub handle: Handle,
    /// Unbounded so a slow consumer can never stall the reader and miss pings.
    pub events: mpsc::UnboundedReceiver<Event>,
}

enum Outgoing {
    Frame(Frame),
    Disconnect,
}

struct Shared {
    state: Mutex<ServerState>,
    shutdown: Notify,
    reason: Mutex<Option<DisconnectReason>>,
    connected: AtomicBool,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, ServerState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Records the first reason for ending the connection and wakes the reader.
    fn shut_down(&self, reason: DisconnectReason) {
        let mut slot = self.reason.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_none() {
            *slot = Some(reason);
        }
        self.shutdown.notify_one();
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
    let limit = config.login_timeout;
    tokio::time::timeout(limit, connect_inner(config))
        .await
        .map_err(|_| ConnectError::Timeout)?
}

async fn connect_inner(config: ConnectConfig) -> Result<Connection, ConnectError> {
    let server_name = ServerName::try_from(config.host.clone())
        .map_err(|_| ConnectError::InvalidAddress(config.host.clone()))?;
    let (tls_config, seen_cert) = tls::client_config(config.identity, config.verify_cert)?;
    let tcp = TcpStream::connect((config.host.as_str(), config.port)).await?;
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
                ) => ConnectError::CertificateRejected(cert),
                (Some(e), _) => ConnectError::Tls(e),
                (None, _) => ConnectError::Io(err),
            });
        }
    };
    // Version must reach the server before Authenticate: the server judges the client by the
    // version it knows when handling Authenticate (e.g. the "client does not support
    // ChannelListener" warning for clients it takes as pre-1.4). Version is a rate-limited type,
    // so the paced write loop would let the unlimited Authenticate overtake it; both are
    // therefore written here, in order, before the write loop starts.
    let mut handshake = BytesMut::new();
    for frame in handshake_frames(config.username, config.password) {
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
        reason: Mutex::new(None),
        connected: AtomicBool::new(true),
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
                Control::Continue => {}
                Control::Synced => {
                    tokio::spawn(read_loop(reader, buf, dispatcher, handle.tx.downgrade()));
                    return Ok(Connection { handle, events });
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

/// The login messages in the order the server must receive them: Version, then Authenticate.
fn handshake_frames(username: String, password: Option<String>) -> [Frame; 2] {
    let version = proto::Version {
        version_v1: Some(MumbleVersion::CLIENT.to_v1()),
        version_v2: Some(MumbleVersion::CLIENT.to_v2()),
        release: Some(format!("Mumcord {}", env!("CARGO_PKG_VERSION"))),
        os: Some(std::env::consts::OS.to_owned()),
        os_version: None,
    };
    let auth = proto::Authenticate {
        username: Some(username),
        password,
        opus: Some(true),
        ..Default::default()
    };
    [
        Frame::from_message(MessageType::Version, &version),
        Frame::from_message(MessageType::Authenticate, &auth),
    ]
}

async fn write_loop(
    mut writer: WriteHalf<TlsStream<TcpStream>>,
    mut rx: mpsc::UnboundedReceiver<Outgoing>,
    shared: Arc<Shared>,
) {
    let started = Instant::now();
    let mut ping =
        tokio::time::interval_at(tokio::time::Instant::now() + PING_INTERVAL, PING_INTERVAL);
    let mut buf = BytesMut::new();
    let mut bucket = LeakyBucket::new(tokio::time::Instant::now());
    // Rate-limited frames wait here in order; everything else (voice, pings) goes out at once.
    let mut paced: VecDeque<Frame> = VecDeque::new();
    loop {
        let now = tokio::time::Instant::now();
        let paced_ready_at = (!paced.is_empty()).then(|| now + bucket.wait_time(now));
        let frame = tokio::select! {
            msg = rx.recv() => match msg {
                Some(Outgoing::Frame(frame)) if ratelimit::is_limited(frame.kind) => {
                    paced.push_back(frame);
                    continue;
                }
                Some(Outgoing::Frame(frame)) => frame,
                Some(Outgoing::Disconnect) | None => {
                    // Best effort close_notify; the connection is going away either way.
                    let _ = writer.shutdown().await;
                    shared.shut_down(DisconnectReason::Requested);
                    return;
                }
            },
            () = sleep_until_some(paced_ready_at) => {
                bucket.take(tokio::time::Instant::now());
                match paced.pop_front() {
                    Some(frame) => frame,
                    None => continue,
                }
            }
            _ = ping.tick() => {
                let ping = proto::Ping {
                    timestamp: Some(started.elapsed().as_millis() as u64),
                    ..Default::default()
                };
                Frame::from_message(MessageType::Ping, &ping)
            }
        };
        buf.clear();
        frame.encode(&mut buf);
        if let Err(err) = async {
            writer.write_all(&buf).await?;
            writer.flush().await
        }
        .await
        {
            tracing::warn!(error = %err, "control channel write failed");
            shared.shut_down(DisconnectReason::Error(err.to_string()));
            return;
        }
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
        let _ = tx.send(Outgoing::Disconnect);
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
            // Pings are only keep-alive here; the rest is not used by this client.
            _ => tracing::trace!(?kind, "ignoring control message"),
        }
        Ok(Control::Continue)
    }
}

impl Handle {
    fn send(&self, frame: Frame) -> Result<(), CommandError> {
        if !self.is_connected() {
            return Err(CommandError::NotConnected);
        }
        self.tx
            .send(Outgoing::Frame(frame))
            .map_err(|_| CommandError::NotConnected)
    }

    fn close(&self) {
        let _ = self.tx.send(Outgoing::Disconnect);
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
    /// [`PLUGIN_DATA_MAX`] bytes and rate-limits senders, so keep it small and infrequent.
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

    /// Closes the connection; an [`Event::Disconnected`] with `Requested` follows.
    pub fn disconnect(&self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::*;

    #[test]
    fn send_acl_queues_an_acl_frame() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let handle = Handle {
            tx,
            shared: Arc::new(Shared {
                state: Mutex::new(ServerState::default()),
                shutdown: Notify::new(),
                reason: Mutex::new(None),
                connected: AtomicBool::new(true),
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
                reason: Mutex::new(None),
                connected: AtomicBool::new(true),
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
mod handshake_tests {
    use super::*;

    #[test]
    fn version_precedes_authenticate_with_both_encodings() {
        let [version, auth] = handshake_frames("alice".to_owned(), None);
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
    }
}
