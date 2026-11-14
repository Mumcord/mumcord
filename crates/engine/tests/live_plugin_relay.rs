//! Plugin data 1:1 through the community relay (spec 09 FR-9.57 … FR-9.63) against the local
//! test server and a local mumpeer built from main. As in `live_relay`, the test plays the relay
//! bot: it announces the relay with its uses (`plg` and `pk` when plugin data is on, as `mumbot
//! peer` does) and answers `token-req` with mumpeer's `POST /v1/pairs` (`use = plugin_data` for
//! `plg`). `testenv/up.sh v15`, build mumpeer, then:
//! `MUMCORD_LIVE=1 MUMPEER_BIN=/path/to/mumpeer cargo test -p mumcord-engine --features test-backend --test live_plugin_relay -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::net::{TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use mumbot::protocol::{Event, PluginData};
use mumbot::{Bot, BotConfig, env};
use mumcord_engine::p2p::P2pConfig;
use mumcord_engine::peer::{RelayStatus, RelayUse};
use mumcord_engine::plugin_relay::{MAX_DATA, PluginRelayAvailability};
use mumcord_engine::store::P2pMode;
use mumcord_engine::{
    Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields, ServerId,
    ServerView,
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
/// Seconds a pair token stays valid (mumpeer's default).
const TOKEN_TTL: u16 = 120;
/// Every use, as a relay with plugin data on announces them.
const ALL_USES: [RelayUse; 4] = [
    RelayUse::Images,
    RelayUse::Files,
    RelayUse::ScreenShare,
    RelayUse::PluginData,
];
/// A relay with plugin data off (mumpeer's default).
const NO_PLUGIN_DATA: [RelayUse; 3] = [RelayUse::Images, RelayUse::Files, RelayUse::ScreenShare];

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
    async fn start(bin: &Path, dir: &Path, plugin_data: bool) -> Self {
        let (udp, tcp, api_port) = (free_udp_port(), free_tcp_port(), free_tcp_port());
        let data = dir.join("mumpeer-data");
        let config = dir.join("mumpeer.toml");
        std::fs::write(
            &config,
            format!(
                "public_host = \"127.0.0.1\"\nudp_listen = \"127.0.0.1:{udp}\"\n\
                 tcp_listen = \"127.0.0.1:{tcp}\"\napi_listen = \"127.0.0.1:{api_port}\"\n\
                 data_dir = \"{}\"\n[uses]\nplugin_data = {plugin_data}\n",
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

    /// `POST /v1/pairs` for two Mumble certificate hashes with the requested use, as the relay
    /// bot does (`plg` is mumpeer's `plugin_data`). The HTTP status and the token if created.
    async fn pair(
        &self,
        a: &str,
        b: &str,
        relay_use: Option<RelayUse>,
    ) -> (u16, Option<RelayToken>) {
        let mut body = serde_json::json!({ "a": a, "b": b });
        if relay_use == Some(RelayUse::PluginData) {
            body["use"] = "plugin_data".into();
        }
        let (status, body) = self
            .request("POST", "/v1/pairs", Some(&body))
            .await
            .expect("pair request");
        let token = body["tok"].as_str().and_then(|t| RelayToken::parse(t).ok());
        (status, token)
    }

    /// The announcement a current bot sends: `u` lists the uses, `pk` comes with plugin data.
    fn announcement(&self, seq: u32, uses: &[RelayUse]) -> RelayAnnouncement {
        RelayAnnouncement {
            host: "127.0.0.1".into(),
            udp: self.udp,
            tcp: self.tcp,
            fingerprint: self.fingerprint,
            state: RelayState::Up,
            seq,
            uses: Some(uses.to_vec()),
            broadcast: None,
            plugin_data_kbps: uses.contains(&RelayUse::PluginData).then_some(512),
            registered_only: false,
            media: false,
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
        let Ok(PeerMessage::TokenRequest {
            rid,
            peer,
            relay_use,
        }) = PeerMessage::decode(&data)
        else {
            continue;
        };
        let hash_of = |session: u32| {
            bot.state()
                .users
                .get(&session)
                .and_then(|u| u.cert_hash.clone())
                .expect("certificate hash")
        };
        let (status, token) = relay
            .pair(&hash_of(requester), &hash_of(peer), relay_use)
            .await;
        let Some(token) = token else {
            panic!("the relay refused a {relay_use:?} pair: HTTP {status}");
        };
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
    let alice_name = unique("r3a-alice");
    let bob_name = unique("r3a-bob");
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

async fn relay_setup(
    test: &str,
    bin: &Path,
    plugin_data: bool,
) -> (Arc<Mumpeer>, RelayBot, Side, Side) {
    let dir = work_dir(test);
    let relay = Arc::new(Mumpeer::start(bin, &dir, plugin_data).await);
    let bot = RelayBot::start(&dir, relay.clone()).await;
    let (alice, bob) = pair(&dir, P2pMode::RelayFirst, P2pMode::RelayFirst).await;
    (relay, bot, alice, bob)
}

/// The next plugin relay message `side` receives: plugin id, API and payload.
async fn next_message(side: &mut Side) -> (u32, String, (u8, u8), Vec<u8>) {
    wait_for(&mut side.rx, "plugin relay data", |e| match e {
        EngineEvent::PluginRelayData {
            session,
            plugin,
            api,
            data,
        } => Some((*session, plugin.clone(), *api, data.0.clone())),
        _ => None,
    })
    .await
}

fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31) ^ seed)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 and MUMPEER_BIN (MUMCORD_LIVE=1)"]
async fn plugin_data_reaches_the_other_side_once_and_in_order_through_the_relay() {
    let (true, Some(bin)) = (live(), mumpeer_bin()) else {
        return;
    };
    let (relay, bot, alice, mut bob) = relay_setup("live_plugin_relay", &bin, true).await;
    bot.announce(&[&alice.name, &bob.name], relay.announcement(1, &ALL_USES));
    for side in [&alice, &bob] {
        side.relay_status("relay available", |s| *s == RelayStatus::Available)
            .await;
        assert_eq!(
            side.engine.plugin_relay_availability(side.server()).await,
            PluginRelayAvailability::Available { kbps: Some(512) }
        );
    }
    let bob_session = alice.session_of(&bob.name).await;
    let alice_session = bob.session_of(&alice.name).await;

    // A full 64 KiB message, a small one and an empty one, in this order.
    let sent = [pattern(MAX_DATA, 1), pattern(997, 2), Vec::new()];
    for data in &sent {
        alice.engine.plugin_relay_send(
            alice.server(),
            bob_session,
            "test-relay",
            (1, 1),
            data.clone(),
        );
    }
    for data in &sent {
        let (session, plugin, api, got) = next_message(&mut bob).await;
        assert_eq!(
            (session, plugin.as_str(), api),
            (alice_session, "test-relay", (1, 1))
        );
        assert_eq!(got.len(), data.len());
        assert!(
            got == *data,
            "payload of {} bytes arrived intact",
            data.len()
        );
    }
    let link = alice
        .engine
        .p2p_link(alice.server(), bob_session)
        .expect("P2P link open");
    assert_eq!(link.path, ConnectionPath::Relay(RelayTransport::Udp));

    // Bob answers on the same pair session; another plugin counts on its own.
    bob.engine
        .plugin_relay_send(bob.server(), alice_session, "other", (1, 1), vec![42]);
    let mut alice = alice;
    let (_, plugin, _, got) = next_message(&mut alice).await;
    assert_eq!(
        (plugin.as_str(), got.as_slice()),
        ("other", [42].as_slice())
    );

    // Switched off on Bob's side: nothing more is delivered (FR-9.63).
    bob.engine.plugin_relay_set_enabled(false);
    alice
        .engine
        .plugin_relay_send(alice.server(), bob_session, "test-relay", (1, 1), vec![1]);
    let quiet = tokio::time::timeout(Duration::from_secs(3), next_message(&mut bob)).await;
    assert!(quiet.is_err(), "nothing arrives while switched off");
    disconnect(&[&alice, &bob]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 and MUMPEER_BIN (MUMCORD_LIVE=1)"]
async fn a_relay_without_plugin_data_is_not_allowed() {
    let (true, Some(bin)) = (live(), mumpeer_bin()) else {
        return;
    };
    let (relay, bot, alice, bob) = relay_setup("live_plugin_relay_off", &bin, false).await;
    bot.announce(
        &[&alice.name, &bob.name],
        relay.announcement(1, &NO_PLUGIN_DATA),
    );
    alice
        .relay_status("relay available", |s| *s == RelayStatus::Available)
        .await;
    assert_eq!(
        alice.engine.plugin_relay_availability(alice.server()).await,
        PluginRelayAvailability::UseNotAllowed
    );
    // An announcement from an older bot (no `u`) does not allow plugin data either.
    let mut old = relay.announcement(2, &NO_PLUGIN_DATA);
    old.uses = None;
    bot.announce(&[&alice.name], old);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        alice.engine.plugin_relay_availability(alice.server()).await,
        PluginRelayAvailability::UseNotAllowed
    );
    // The relay itself refuses a plugin-data pair.
    let (a, b) = ("a".repeat(40), "b".repeat(40));
    let (status, token) = relay.pair(&a, &b, Some(RelayUse::PluginData)).await;
    assert_eq!((status, token.is_none()), (403, true));
    disconnect(&[&alice, &bob]);
}
