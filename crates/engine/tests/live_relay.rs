//! P2P connections through the community relay (spec 04: FR-4.7 – FR-4.19, AC-4.5 – AC-4.8,
//! AC-4.13) against the local test server and a local mumpeer. The test plays the relay bot
//! itself with a registered test user: it announces the relay to both engines over
//! `mumcord.peer`, answers `token-req` with mumpeer's `POST /v1/pairs` and sends the token to
//! both sides. mumpeer runs from `MUMPEER_BIN` on loopback ports with a random API secret.
//! `testenv/up.sh v15`, build mumpeer (`cargo build --release` in its repository), then:
//! `MUMCORD_LIVE=1 MUMPEER_BIN=/path/to/mumpeer cargo test -p mumcord-engine --features test-backend --test live_relay -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::io::Cursor;
use std::net::{TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use image::{ImageFormat, Rgb, RgbImage};
use mumbot::protocol::{Event, PluginData};
use mumbot::{Bot, BotConfig, env};
use mumcord_engine::p2p::{DirectError, P2pConfig};
use mumcord_engine::peer::{RelayStatus, RelayUnavailable};
use mumcord_engine::store::P2pMode;
use mumcord_engine::{
    ChatMessageView, Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields,
    ServerId, ServerView,
};
use mumcord_p2p::{
    ConnectionPath, PEER_DATA_ID, PeerMessage, RelayAnnouncement, RelayState, RelayToken,
    RelayTransport, RequestId,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

const WAIT: Duration = Duration::from_secs(30);
const THUMBNAIL: &str = "data:image/png;base64,iVBORw0KGgo=";
/// Seconds a pair token stays valid (mumpeer's default).
const TOKEN_TTL: u16 = 120;

fn live() -> bool {
    let on = std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start the test servers");
    }
    on
}

/// The mumpeer binary, `None` (test skipped) when `MUMPEER_BIN` is not set.
fn mumpeer_bin() -> Option<PathBuf> {
    let bin = std::env::var_os("MUMPEER_BIN").map(PathBuf::from);
    if bin.is_none() {
        eprintln!("skipped: set MUMPEER_BIN to a built mumpeer binary");
    }
    bin
}

fn unique(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{prefix}-{:06x}", (nanos ^ std::process::id()) & 0xff_ffff)
}

fn work_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    dir
}

fn unhex32(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    assert_eq!(s.len(), 64, "fingerprint {s}");
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).expect("hex fingerprint");
    }
    out
}

/// A loopback port that was free a moment ago.
fn free_udp_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0")
        .and_then(|s| s.local_addr())
        .expect("udp port")
        .port()
}

fn free_tcp_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|s| s.local_addr())
        .expect("tcp port")
        .port()
}

/// A running mumpeer on loopback; killed when dropped.
struct Mumpeer {
    _child: Child,
    api: u16,
    secret: String,
    udp: u16,
    tcp: u16,
    fingerprint: [u8; 32],
}

impl Mumpeer {
    async fn start(bin: &Path, dir: &Path) -> Self {
        let (udp, tcp, api_port) = (free_udp_port(), free_tcp_port(), free_tcp_port());
        let data = dir.join("mumpeer-data");
        let config = dir.join("mumpeer.toml");
        std::fs::write(
            &config,
            format!(
                "public_host = \"127.0.0.1\"\nudp_listen = \"127.0.0.1:{udp}\"\n\
                 tcp_listen = \"127.0.0.1:{tcp}\"\napi_listen = \"127.0.0.1:{api_port}\"\n\
                 data_dir = \"{}\"\n",
                data.display()
            ),
        )
        .expect("mumpeer config");
        let secret: String = (0..4)
            .map(|_| format!("{:016x}", RequestId::random().0))
            .collect();
        let child = Command::new(bin)
            .arg("--config")
            .arg(&config)
            .env("MUMPEER_API_SECRET", &secret)
            .env("RUST_LOG", "info")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("start mumpeer");
        let mut relay = Self {
            _child: child,
            api: api_port,
            secret,
            udp,
            tcp,
            fingerprint: [0; 32],
        };
        let deadline = tokio::time::Instant::now() + WAIT;
        let status = loop {
            if let Ok(status) = relay.status().await {
                break status;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "mumpeer did not answer"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        relay.fingerprint = unhex32(status["fp"].as_str().expect("fp"));
        relay
    }

    /// One request to the allowlist API (plain HTTP/1.1 on loopback); status and JSON body.
    async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> std::io::Result<(u16, serde_json::Value)> {
        let body = body.map(ToString::to_string).unwrap_or_default();
        let mut stream = TcpStream::connect(("127.0.0.1", self.api)).await?;
        let head = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.secret,
            body.len()
        );
        stream.write_all(head.as_bytes()).await?;
        stream.write_all(body.as_bytes()).await?;
        let mut response = String::new();
        stream.read_to_string(&mut response).await?;
        let (head, body) = response.split_once("\r\n\r\n").unwrap_or((&response, ""));
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let json = serde_json::from_str(body.trim()).unwrap_or(serde_json::Value::Null);
        Ok((status, json))
    }

    async fn status(&self) -> std::io::Result<serde_json::Value> {
        match self.request("GET", "/v1/status", None).await? {
            (200, body) => Ok(body),
            (status, _) => Err(std::io::Error::other(format!("status {status}"))),
        }
    }

    /// `POST /v1/pairs` for two Mumble certificate hashes, as the relay bot does.
    async fn pair(&self, a: &str, b: &str) -> RelayToken {
        let (status, body) = self
            .request(
                "POST",
                "/v1/pairs",
                Some(&serde_json::json!({ "a": a, "b": b })),
            )
            .await
            .expect("pair request");
        assert_eq!(status, 201, "pair created: {body}");
        RelayToken::parse(body["tok"].as_str().expect("tok")).expect("token")
    }

    fn announcement(&self, seq: u32) -> RelayAnnouncement {
        RelayAnnouncement {
            host: "127.0.0.1".into(),
            udp: self.udp,
            tcp: self.tcp,
            fingerprint: self.fingerprint,
            state: RelayState::Up,
            seq,
        }
    }
}

/// The test's relay bot: a registered user that hands out pair tokens until dropped.
struct RelayBot {
    bot: Arc<Bot>,
    tokens: JoinHandle<()>,
}

impl RelayBot {
    async fn start(dir: &Path, relay: Arc<Mumpeer>) -> Self {
        let mut config = BotConfig::new(&unique("relaybot"), common::v15());
        config.data_dir = Some(dir.join("bot-certs"));
        let bot = Bot::connect(config).await.expect("bot connected");
        bot.register().await.expect("bot registered");
        let bot = Arc::new(bot);
        let tokens = tokio::spawn(answer_token_requests(bot.clone(), relay));
        Self { bot, tokens }
    }

    fn session_of(&self, name: &str) -> u32 {
        self.bot
            .state()
            .users
            .values()
            .find(|u| u.name == name)
            .map(|u| u.session)
            .unwrap_or_else(|| panic!("{name} is not on the server"))
    }

    fn announce(&self, names: &[&str], announcement: RelayAnnouncement) {
        let sessions: Vec<u32> = names.iter().map(|n| self.session_of(n)).collect();
        let bytes = PeerMessage::Relay(announcement).encode().expect("encoded");
        self.bot
            .handle()
            .send_plugin_data(&sessions, PEER_DATA_ID, bytes)
            .expect("announced");
    }
}

impl Drop for RelayBot {
    fn drop(&mut self) {
        self.tokens.abort();
    }
}

/// Answers every `token-req` with a fresh pair from mumpeer, sent to both sides (FR-4.26).
async fn answer_token_requests(bot: Arc<Bot>, relay: Arc<Mumpeer>) {
    let mut events = bot.subscribe();
    loop {
        let Ok(event) = events.recv().await else {
            return;
        };
        let Event::PluginData(PluginData {
            sender: Some(requester),
            data_id,
            data,
        }) = event
        else {
            continue;
        };
        if data_id != PEER_DATA_ID {
            continue;
        }
        let Ok(PeerMessage::TokenRequest { rid, peer }) = PeerMessage::decode(&data) else {
            continue;
        };
        let hash_of = |session: u32| {
            bot.state()
                .users
                .get(&session)
                .and_then(|u| u.cert_hash.clone())
                .expect("certificate hash")
        };
        let token = relay.pair(&hash_of(requester), &hash_of(peer)).await;
        let message = PeerMessage::Token {
            rid,
            a: requester,
            b: peer,
            token,
            ttl: TOKEN_TTL,
        };
        bot.handle()
            .send_plugin_data(
                &[requester, peer],
                PEER_DATA_ID,
                message.encode().expect("encoded"),
            )
            .expect("token sent");
    }
}

async fn wait_for<T>(
    rx: &mut EventReceiver,
    what: &str,
    mut pick: impl FnMut(&EngineEvent) -> Option<T>,
) -> T {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
            .expect("event stream open");
        if let Some(found) = pick(&event) {
            return found;
        }
    }
}

async fn engine(dir: &Path, name: &str, mode: P2pMode) -> (Engine, EventReceiver, ServerView) {
    let (engine, rx) = Engine::new(
        &dir.join(name),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create(name.into(), None)
        .await
        .expect("identity");
    let mut settings = engine.settings().expect("settings");
    settings.p2p_mode = mode;
    engine.update_settings(settings).await.expect("mode");
    engine.p2p_set_config(P2pConfig {
        connect_timeout: Duration::from_secs(10),
        port_mapping: false,
        allow_loopback: true,
    });
    let server = engine
        .server_add(ServerFields {
            label: format!("test {} {name}", common::v15()),
            address: "127.0.0.1".into(),
            port: common::v15(),
            username: name.into(),
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server");
    (engine, rx, server)
}

async fn connect(engine: &Engine, rx: &mut EventReceiver, server: &ServerView) {
    engine.connect(server.id, None).expect("connect");
    loop {
        let event = wait_for(rx, "sync", |e| match e {
            EngineEvent::CertTrustRequired { .. } | EngineEvent::ServerSynced(_) => Some(e.clone()),
            _ => None,
        })
        .await;
        match event {
            EngineEvent::CertTrustRequired {
                server_id,
                fingerprint,
                ..
            } => engine
                .cert_trust_decide(server_id, &fingerprint, true)
                .expect("trust"),
            EngineEvent::ServerSynced(_) => return,
            _ => unreachable!(),
        }
    }
}

/// The test's only connected server (0, which no call accepts, when none is).
fn only_server(engine: &Engine) -> ServerId {
    engine
        .connections()
        .first()
        .and_then(mumcord_engine::ConnState::server_id)
        .unwrap_or(0)
}

struct Side {
    engine: Engine,
    rx: EventReceiver,
    name: String,
}

impl Side {
    fn server(&self) -> ServerId {
        only_server(&self.engine)
    }

    /// Waits until the engine shows the relay status `want`.
    async fn relay_status(&self, what: &str, want: impl Fn(&RelayStatus) -> bool) {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let info = self.engine.relay_info(self.server());
            if want(&info.status) {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for {what}: {info:?}"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// The session of `other` as this engine sees it, once it knows them as a P2P user.
    async fn session_of(&self, other: &str) -> u32 {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let found = self
                .engine
                .snapshot_of(self.server())
                .and_then(|s| s.users.into_iter().find(|u| u.name == other))
                .map(|u| u.session)
                .filter(|s| {
                    self.engine.mumcord_user_has_feature(
                        self.server(),
                        *s,
                        mumcord_engine::p2p::FEATURE,
                    )
                });
            if let Some(session) = found {
                return session;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "{other} never became a P2P user"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// Two engines with these modes on the test server, both synced.
async fn pair(dir: &Path, alice_mode: P2pMode, bob_mode: P2pMode) -> (Side, Side) {
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let alice_name = unique("r62-alice");
    let bob_name = unique("r62-bob");
    let (alice, mut alice_rx, alice_server) = engine(dir, &alice_name, alice_mode).await;
    let (bob, mut bob_rx, bob_server) = engine(dir, &bob_name, bob_mode).await;
    connect(&alice, &mut alice_rx, &alice_server).await;
    connect(&bob, &mut bob_rx, &bob_server).await;
    (
        Side {
            engine: alice,
            rx: alice_rx,
            name: alice_name,
        },
        Side {
            engine: bob,
            rx: bob_rx,
            name: bob_name,
        },
    )
}

fn disconnect(sides: &[&Side]) {
    for side in sides {
        side.engine.disconnect(side.server());
    }
}

/// A noisy PNG (does not compress).
fn picture(width: u32, height: u32, seed: u32) -> Vec<u8> {
    let mut state = seed | 1;
    let image = RgbImage::from_fn(width, height, |_, _| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let [r, g, b, _] = state.to_le_bytes();
        Rgb([r, g, b])
    });
    let mut out = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .expect("png");
    out
}

/// Alice posts a large picture and Bob loads the full image; returns how long that took and
/// Alice's session as Bob sees it.
async fn full_image(alice: &Side, bob: &mut Side) -> (Duration, u32) {
    // About 350 plugin-data chunks: minutes over plugin data, a moment over P2P.
    let original = picture(300, 200, 7);
    let reference = alice.engine.full_image_keep(original).await.expect("kept");
    let html = format!("<img src=\"{THUMBNAIL}\" data-mumcord-full=\"{reference}\" />");
    alice
        .engine
        .send_channel_message(alice.server(), 0, &html)
        .await
        .expect("sent");
    let alice_name = alice.name.clone();
    let message: ChatMessageView = wait_for(&mut bob.rx, "the image message", |e| match e {
        EngineEvent::MessageReceived(m) if m.sender_name == alice_name => Some((**m).clone()),
        _ => None,
    })
    .await;
    let started = tokio::time::Instant::now();
    let data_url = bob
        .engine
        .full_image_fetch(
            bob.server(),
            &reference,
            message.sender_session,
            message.sender_cert_hash.clone(),
        )
        .await
        .expect("full image");
    assert!(data_url.starts_with("data:image/png;base64,"));
    (
        started.elapsed(),
        message.sender_session.expect("sender session"),
    )
}

/// mumpeer, the relay bot and two engines in `mode`, the relay announced and available.
async fn relay_setup(
    test: &str,
    bin: &Path,
    alice_mode: P2pMode,
    bob_mode: P2pMode,
) -> (Arc<Mumpeer>, RelayBot, Side, Side) {
    let dir = work_dir(test);
    let relay = Arc::new(Mumpeer::start(bin, &dir).await);
    let bot = RelayBot::start(&dir, relay.clone()).await;
    let (alice, bob) = pair(&dir, alice_mode, bob_mode).await;
    (relay, bot, alice, bob)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 and MUMPEER_BIN (MUMCORD_LIVE=1)"]
async fn relay_first_loads_the_full_image_through_the_relay_over_udp() {
    let (true, Some(bin)) = (live(), mumpeer_bin()) else {
        return;
    };
    let (relay, bot, alice, mut bob) = relay_setup(
        "live_relay_udp",
        &bin,
        P2pMode::RelayFirst,
        P2pMode::RelayFirst,
    )
    .await;
    bot.announce(&[&alice.name, &bob.name], relay.announcement(1));
    for side in [&alice, &bob] {
        side.relay_status("relay available", |s| *s == RelayStatus::Available)
            .await;
    }
    bob.session_of(&alice.name).await;

    let (took, alice_session) = full_image(&alice, &mut bob).await;
    assert!(took < Duration::from_secs(15), "came via P2P: {took:?}");
    let link = bob
        .engine
        .p2p_link(bob.server(), alice_session)
        .expect("P2P link open");
    assert_eq!(link.path, ConnectionPath::Relay(RelayTransport::Udp));
    assert!(link.ip_hidden, "relay_first sends no addresses");
    let bob_session = alice.session_of(&bob.name).await;
    let link = alice
        .engine
        .p2p_link(alice.server(), bob_session)
        .expect("P2P link open on Alice's side");
    assert_eq!(link.path, ConnectionPath::Relay(RelayTransport::Udp));
    assert!(link.ip_hidden);
    disconnect(&[&alice, &bob]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 and MUMPEER_BIN (MUMCORD_LIVE=1)"]
async fn closed_udp_port_falls_back_to_the_relay_over_tcp() {
    let (true, Some(bin)) = (live(), mumpeer_bin()) else {
        return;
    };
    let (relay, bot, alice, bob) = relay_setup(
        "live_relay_tcp",
        &bin,
        P2pMode::RelayOnly,
        P2pMode::RelayOnly,
    )
    .await;
    // Nothing listens on the announced UDP port, as if UDP were blocked (AC-4.6).
    let announcement = RelayAnnouncement {
        udp: free_udp_port(),
        ..relay.announcement(1)
    };
    bot.announce(&[&alice.name, &bob.name], announcement);
    for side in [&alice, &bob] {
        side.relay_status("relay available", |s| *s == RelayStatus::Available)
            .await;
    }
    let alice_session = bob.session_of(&alice.name).await;
    let conn = bob
        .engine
        .p2p_connection(bob.server(), alice_session)
        .await
        .expect("connected through the relay");
    assert_eq!(conn.path(), ConnectionPath::Relay(RelayTransport::Tcp));
    let link = bob
        .engine
        .p2p_link(bob.server(), alice_session)
        .expect("link");
    assert!(link.ip_hidden);
    disconnect(&[&alice, &bob]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn relay_only_without_an_announcement_has_no_relay() {
    if !live() {
        return;
    }
    let dir = work_dir("live_relay_none");
    let (alice, bob) = pair(&dir, P2pMode::DirectRelay, P2pMode::RelayOnly).await;
    let alice_session = bob.session_of(&alice.name).await;
    let result = bob.engine.p2p_connection(bob.server(), alice_session).await;
    assert!(
        matches!(result, Err(DirectError::Relay(RelayUnavailable::NoRelay))),
        "{:?}",
        result.err()
    );
    assert!(bob.engine.relay_info(bob.server()).bots.is_empty());
    disconnect(&[&alice, &bob]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 and MUMPEER_BIN (MUMCORD_LIVE=1)"]
async fn changed_fingerprint_is_untrusted_and_not_used() {
    let (true, Some(bin)) = (live(), mumpeer_bin()) else {
        return;
    };
    let (relay, bot, alice, bob) = relay_setup(
        "live_relay_changed",
        &bin,
        P2pMode::RelayOnly,
        P2pMode::RelayOnly,
    )
    .await;
    bot.announce(&[&alice.name, &bob.name], relay.announcement(1));
    bob.relay_status("relay available", |s| *s == RelayStatus::Available)
        .await;
    // The relay comes back with another key (AC-4.13): the pin no longer matches.
    let mut changed = relay.announcement(2);
    changed.fingerprint[0] ^= 0xff;
    bot.announce(&[&alice.name, &bob.name], changed);
    bob.relay_status("relay untrusted", |s| {
        matches!(
            s,
            RelayStatus::Untrusted {
                kept_blocked: false
            }
        )
    })
    .await;
    let alice_session = bob.session_of(&alice.name).await;
    let result = bob.engine.p2p_connection(bob.server(), alice_session).await;
    assert!(
        matches!(result, Err(DirectError::Relay(RelayUnavailable::Untrusted))),
        "{:?}",
        result.err()
    );
    assert_eq!(relay.status().await.expect("status")["sessions"], 0);
    disconnect(&[&alice, &bob]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 and MUMPEER_BIN (MUMCORD_LIVE=1)"]
async fn relay_with_another_certificate_than_announced_is_untrusted() {
    let (true, Some(bin)) = (live(), mumpeer_bin()) else {
        return;
    };
    let (relay, bot, alice, bob) = relay_setup(
        "live_relay_mismatch",
        &bin,
        P2pMode::RelayOnly,
        P2pMode::RelayOnly,
    )
    .await;
    // The first announcement pins a fingerprint the relay's certificate does not have.
    let mut wrong = relay.announcement(1);
    wrong.fingerprint = [0x5a; 32];
    bot.announce(&[&alice.name, &bob.name], wrong);
    for side in [&alice, &bob] {
        side.relay_status("relay available", |s| *s == RelayStatus::Available)
            .await;
    }
    let alice_session = bob.session_of(&alice.name).await;
    let result = bob.engine.p2p_connection(bob.server(), alice_session).await;
    assert!(
        matches!(result, Err(DirectError::Relay(RelayUnavailable::Untrusted))),
        "{:?}",
        result.err()
    );
    disconnect(&[&alice, &bob]);
}
