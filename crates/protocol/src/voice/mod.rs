//! Voice transport: encrypted UDP (OCB2-AES128) to the server with automatic fallback to the TCP
//! tunnel when UDP is blocked or stops working, and recovery back to UDP.
//!
//! # Usage
//! Start a [`VoiceTransport`] after [`connect`](crate::connect) with the server's resolved
//! address. The control connection has a single event stream, so the owner of
//! [`Connection::events`](crate::Connection) must forward [`Event::CryptSetup`](crate::Event) to
//! [`VoiceTransport::on_crypt_setup`] and [`Event::UdpTunnel`](crate::Event) to
//! [`VoiceTransport::on_tunnel`]. Received audio and transport changes arrive as
//! [`VoiceEvent`]s.
//!
//! # Transport
//! An encrypted UDP ping goes out every second. Voice starts on the TCP tunnel and moves to UDP
//! at the first ping reply; after 4 s without any valid UDP packet it falls back to TCP, and
//! after 3 answered pings in a row it returns to UDP. Pings are never tunnelled: the server
//! (`ServerUser::aiUdpFlag`) sends to us over UDP once it learned our UDP address from a ping,
//! but switches to TCP for good when we tunnel anything, until it receives UDP *audio* from us
//! again. So right after a recovery, voice may keep arriving over TCP until we talk; both paths
//! are always accepted.
//!
//! # Packet format
//! Servers ≥ 1.5 get the protobuf format (`MumbleUDP.proto`), older or unknown ones the legacy
//! format; see `packet`.
//!
//! # Listener detection (`via_listener`)
//! Determined from the Mumble server source:
//! - 1.4 (`v1.4.230`, `Server::processMsg`): copies for channel listeners are sent with header
//!   `type | SpeechFlags::Listen`, and `SpeechFlags.h` defines `Listen = 3`. Users in the
//!   speaker's channel are removed from the listener set first, so they get `Normal = 0`.
//! - 1.5 (`v1.5.735`, `Server::processMsg` + `AudioReceiverBuffer::forceAddReceiver`): listeners
//!   are added with `AudioContext::LISTEN = 3` in the `context` field; a receiver reached several
//!   ways keeps the smallest context, so `LISTEN` only appears when listening is the only reason.
//!
//! So `via_listener` is "legacy target bits == 3" / "protobuf context == 3".

mod crypt;
mod link;
mod packet;

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub use crypt::CryptStats;
pub use link::{LinkStats, Transport};

use crate::client::Handle;
use crate::error::CommandError;
use crate::state::CryptSetup;
use crypt::{CryptState, OVERHEAD};
use link::{Link, PING_INTERVAL};
use packet::{AudioIn, AudioOut, Format, Incoming, MAX_PACKET};

pub use packet::MAX_VOICE_PAYLOAD;

/// Oversized outgoing packets are logged at most this often, with the count since the last
/// log line, so a misbehaving encoder cannot flood the log.
const OVERSIZE_LOG_INTERVAL: Duration = Duration::from_secs(10);

/// Legacy target / protobuf context value of audio delivered because we listen to the channel.
const CONTEXT_LISTEN: u8 = 3;

/// One outgoing Opus packet.
#[derive(Debug, Clone, PartialEq)]
pub struct OutgoingAudio {
    /// Encoded Opus data.
    pub payload: Bytes,
    /// Number of 10 ms frames in this packet; the sequence number advances by this.
    pub frames: u8,
    /// Last packet of a transmission (terminator flag).
    pub is_last: bool,
    /// Voice target: 0 = normal talking to the own channel.
    pub target: u8,
    /// The sender's avatar position (x, y, z) for positional audio.
    pub positional: Option<[f32; 3]>,
}

impl OutgoingAudio {
    /// Normal talking (target 0) without positional data.
    pub fn new(payload: Bytes, frames: u8, is_last: bool) -> Self {
        Self {
            payload,
            frames,
            is_last,
            target: 0,
            positional: None,
        }
    }

    /// The same packet carrying the sender's position.
    pub fn with_position(mut self, positional: Option<[f32; 3]>) -> Self {
        self.positional = positional;
        self
    }
}

/// One received Opus packet.
#[derive(Debug, Clone, PartialEq)]
pub struct VoicePacket {
    /// Session of the speaking user.
    pub session: u32,
    /// Sequence number in 10 ms frames.
    pub seq: u64,
    /// Opus data (untrusted).
    pub payload: Bytes,
    /// Last packet of the speaker's transmission.
    pub is_last: bool,
    /// The audio reaches us only because we listen to (follow) the speaker's channel.
    pub via_listener: bool,
    pub positional: Option<[f32; 3]>,
    /// The path this packet arrived on.
    pub transport: Transport,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VoiceEvent {
    Packet(VoicePacket),
    /// The outgoing voice path changed (UDP blocked/recovered).
    TransportChanged(Transport),
}

enum Command {
    Audio(OutgoingAudio),
    CryptSetup(CryptSetup),
    Tunnel(Bytes),
}

/// Running voice transport of one connection. Dropping it stops the transport; it also stops
/// by itself when the control connection ends.
pub struct VoiceTransport {
    tx: mpsc::UnboundedSender<Command>,
    stats: Arc<Mutex<LinkStats>>,
    task: JoinHandle<()>,
}

impl VoiceTransport {
    /// Opens the UDP socket and starts the transport. Uses the crypto parameters the server
    /// sent during login (`ServerState::crypt`) and picks the packet format from the server
    /// version.
    pub async fn start(
        handle: Handle,
        server: SocketAddr,
    ) -> io::Result<(Self, mpsc::UnboundedReceiver<VoiceEvent>)> {
        let local: SocketAddr = if server.is_ipv4() {
            (Ipv4Addr::UNSPECIFIED, 0).into()
        } else {
            (Ipv6Addr::UNSPECIFIED, 0).into()
        };
        let socket = UdpSocket::bind(local).await?;
        socket.connect(server).await?;

        let now = Instant::now();
        let link = Link::new(now);
        let mut crypt = CryptState::new();
        let initial = handle.state().crypt;
        let mut applied = None;
        if let Some(setup) = initial {
            apply_full_key(&mut crypt, &mut applied, &setup);
        }
        let stats = Arc::new(Mutex::new(link.stats(crypt.stats)));
        let (tx, rx) = mpsc::unbounded_channel();
        let (events_tx, events) = mpsc::unbounded_channel();
        let worker = Worker {
            format: Format::for_server(handle.server_version()),
            handle,
            socket,
            crypt,
            applied,
            link,
            started: now,
            seq: 0,
            oversized: 0,
            oversized_logged: 0,
            oversize_logged_at: None,
            events: events_tx,
            stats: Arc::clone(&stats),
        };
        let task = tokio::spawn(worker.run(rx));
        Ok((Self { tx, stats, task }, events))
    }

    /// Queues one Opus packet without waiting.
    pub fn send_audio(&self, audio: OutgoingAudio) {
        // A closed channel means the transport has stopped; there is nowhere to send to.
        let _ = self.tx.send(Command::Audio(audio));
    }

    /// Forward every [`Event::CryptSetup`](crate::Event) here.
    pub fn on_crypt_setup(&self, setup: CryptSetup) {
        let _ = self.tx.send(Command::CryptSetup(setup));
    }

    /// Forward every [`Event::UdpTunnel`](crate::Event) here.
    pub fn on_tunnel(&self, packet: Bytes) {
        let _ = self.tx.send(Command::Tunnel(packet));
    }

    /// Current link statistics (loss, RTT, transport) for the bitrate controller.
    pub fn stats(&self) -> LinkStats {
        *lock(&self.stats)
    }

    pub fn transport(&self) -> Transport {
        lock(&self.stats).transport
    }

    /// Whether the transport is still running (stops when the control connection ends).
    pub fn is_running(&self) -> bool {
        !self.task.is_finished()
    }
}

impl Drop for VoiceTransport {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn lock(stats: &Mutex<LinkStats>) -> MutexGuard<'_, LinkStats> {
    stats.lock().unwrap_or_else(|p| p.into_inner())
}

type KeyMaterial = (Vec<u8>, Vec<u8>, Vec<u8>);

/// Installs a full key unless it is the one already applied: the login `CryptSetup` is both in
/// the state we start from and in the forwarded events, and re-applying it would rewind our
/// nonces (the server would then drop our packets as replays).
fn apply_full_key(
    crypt: &mut CryptState,
    applied: &mut Option<KeyMaterial>,
    setup: &CryptSetup,
) -> bool {
    let (Some(key), Some(cn), Some(sn)) = (&setup.key, &setup.client_nonce, &setup.server_nonce)
    else {
        return false;
    };
    let material = (key.clone(), cn.clone(), sn.clone());
    if applied.as_ref() == Some(&material) {
        return true;
    }
    if crypt.set_key(key, cn, sn) {
        *applied = Some(material);
    } else {
        tracing::warn!("ignoring CryptSetup with invalid key or nonce length");
    }
    true
}

struct Worker {
    handle: Handle,
    socket: UdpSocket,
    format: Format,
    crypt: CryptState,
    applied: Option<KeyMaterial>,
    link: Link,
    started: Instant,
    /// Sequence number of the next outgoing packet, in 10 ms frames.
    seq: u64,
    /// Outgoing packets dropped as larger than [`MAX_PACKET`], reported in [`LinkStats`].
    oversized: u64,
    /// Value of `oversized` at the last log line, and when it was written.
    oversized_logged: u64,
    oversize_logged_at: Option<Instant>,
    events: mpsc::UnboundedSender<VoiceEvent>,
    stats: Arc<Mutex<LinkStats>>,
}

impl Worker {
    async fn run(mut self, mut commands: mpsc::UnboundedReceiver<Command>) {
        let mut ticker = tokio::time::interval(PING_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut buf = vec![0u8; MAX_PACKET + OVERHEAD + 64];
        loop {
            let result = tokio::select! {
                received = self.socket.recv(&mut buf) => {
                    match received {
                        Ok(n) => self.on_udp(&buf[..n]),
                        // E.g. ICMP "port unreachable" on the connected socket; the link
                        // timeout handles a lasting outage.
                        Err(err) => tracing::debug!(error = %err, "udp receive failed"),
                    }
                    Ok(())
                }
                command = commands.recv() => match command {
                    Some(Command::Audio(audio)) => self.send_audio(&audio).await,
                    Some(Command::CryptSetup(setup)) => self.on_crypt_setup(&setup),
                    Some(Command::Tunnel(packet)) => {
                        self.on_tunnel(&packet);
                        Ok(())
                    }
                    None => break,
                },
                _ = ticker.tick() => self.on_tick().await,
            };
            if result.is_err() || !self.handle.is_connected() {
                break;
            }
            let mut stats = self.link.stats(self.crypt.stats);
            stats.oversized = self.oversized;
            *lock(&self.stats) = stats;
        }
    }

    fn emit(&self, event: VoiceEvent) {
        // The receiver may be gone while the owner shuts down; nothing to do then.
        let _ = self.events.send(event);
    }

    fn on_udp(&mut self, data: &[u8]) {
        let now = Instant::now();
        let Some(plain) = self.crypt.decrypt(data) else {
            if self.crypt.is_valid() && self.link.decrypt_failed(now) {
                tracing::info!("udp decrypt keeps failing, requesting crypt resync");
                if let Err(err) = self.handle.send_crypt_setup(CryptSetup::default()) {
                    tracing::debug!(error = %err, "crypt resync request not sent");
                }
            }
            return;
        };
        self.link.udp_received(now);
        match packet::decode(self.format, &plain) {
            Some(Incoming::Ping(timestamp)) => {
                let rtt = now
                    .duration_since(self.started)
                    .saturating_sub(std::time::Duration::from_micros(timestamp));
                if let Some(to) = self.link.ping_reply(timestamp, rtt) {
                    tracing::info!(transport = ?to, "voice transport changed");
                    self.emit(VoiceEvent::TransportChanged(to));
                }
            }
            Some(Incoming::Audio(audio)) => self.emit_audio(audio, Transport::Udp),
            None => tracing::debug!("dropping undecodable udp voice packet"),
        }
    }

    fn on_tunnel(&mut self, data: &[u8]) {
        match packet::decode(self.format, data) {
            Some(Incoming::Audio(audio)) => self.emit_audio(audio, Transport::Tcp),
            Some(Incoming::Ping(_)) => {}
            None => tracing::debug!("dropping undecodable tunnelled voice packet"),
        }
    }

    fn emit_audio(&self, audio: AudioIn, transport: Transport) {
        self.emit(VoiceEvent::Packet(VoicePacket {
            session: audio.session,
            seq: audio.seq,
            payload: audio.payload,
            is_last: audio.is_last,
            via_listener: audio.context == CONTEXT_LISTEN,
            positional: audio.positional,
            transport,
        }));
    }

    /// Mirrors Mumble's client (`MainWindow::msgCryptSetup`).
    fn on_crypt_setup(&mut self, setup: &CryptSetup) -> Result<(), CommandError> {
        if apply_full_key(&mut self.crypt, &mut self.applied, setup) {
            return Ok(());
        }
        if let Some(nonce) = &setup.server_nonce {
            // The server answered our resync request with its encrypt nonce.
            if self.crypt.set_decrypt_iv(nonce) {
                self.crypt.stats.resync = self.crypt.stats.resync.wrapping_add(1);
            }
            return Ok(());
        }
        if self.crypt.is_valid() {
            // The server asks for our encrypt nonce.
            return self.handle.send_crypt_setup(CryptSetup {
                client_nonce: Some(self.crypt.encrypt_iv()),
                ..CryptSetup::default()
            });
        }
        Ok(())
    }

    async fn send_audio(&mut self, audio: &OutgoingAudio) -> Result<(), CommandError> {
        let out = AudioOut {
            target: audio.target,
            seq: self.seq,
            payload: &audio.payload,
            is_last: audio.is_last,
            positional: audio.positional,
        };
        self.seq += u64::from(audio.frames);
        let Some(plain) = packet::encode_audio(self.format, &out).filter(|p| p.len() <= MAX_PACKET)
        else {
            // The server would refuse it. The owner lowers the bitrate from the count in
            // `LinkStats::oversized`; the log line is rate-limited.
            self.oversized += 1;
            self.log_oversized(audio.payload.len());
            return Ok(());
        };
        match self.link.transport() {
            Transport::Udp => {
                self.send_udp(&plain).await;
                Ok(())
            }
            Transport::Tcp => self.handle.send_tunnel(Bytes::from(plain)),
        }
    }

    fn log_oversized(&mut self, payload_len: usize) {
        let now = Instant::now();
        if self
            .oversize_logged_at
            .is_some_and(|at| now.duration_since(at) < OVERSIZE_LOG_INTERVAL)
        {
            return;
        }
        tracing::warn!(
            payload_len,
            dropped = self.oversized - self.oversized_logged,
            "dropping oversized voice packets; lowering the bitrate"
        );
        self.oversized_logged = self.oversized;
        self.oversize_logged_at = Some(now);
    }

    async fn send_udp(&mut self, plain: &[u8]) {
        let Some(packet) = self.crypt.encrypt(plain) else {
            return;
        };
        if let Err(err) = self.socket.send(&packet).await {
            tracing::debug!(error = %err, "udp send failed");
        }
    }

    async fn on_tick(&mut self) -> Result<(), CommandError> {
        let now = Instant::now();
        if let Some(to) = self.link.tick(now, self.crypt.stats) {
            tracing::info!(transport = ?to, "voice transport changed");
            self.emit(VoiceEvent::TransportChanged(to));
        }
        if self.crypt.is_valid() {
            let timestamp =
                u64::try_from(now.duration_since(self.started).as_micros()).unwrap_or(u64::MAX);
            let ping = packet::encode_ping(self.format, timestamp);
            self.link.ping_sent(timestamp);
            self.send_udp(&ping).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_login_key_does_not_rewind_nonces() {
        let setup = CryptSetup {
            key: Some(vec![1; 16]),
            client_nonce: Some(vec![2; 16]),
            server_nonce: Some(vec![3; 16]),
        };
        let mut crypt = CryptState::new();
        let mut applied = None;
        assert!(apply_full_key(&mut crypt, &mut applied, &setup));
        let first = crypt.encrypt(b"x").expect("key set");
        assert!(apply_full_key(&mut crypt, &mut applied, &setup));
        let second = crypt.encrypt(b"x").expect("key set");
        assert_ne!(first[0], second[0], "nonce must keep counting");

        let nonce_only = CryptSetup {
            server_nonce: Some(vec![4; 16]),
            ..CryptSetup::default()
        };
        assert!(!apply_full_key(&mut crypt, &mut applied, &nonce_only));
    }
}
