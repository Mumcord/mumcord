//! Plugin data to the channel through one relay broadcast per sender (spec 09 FR-9.59 …
//! FR-9.62) against the local test server, a local mumpeer and the relay bot `mumbot peer`, both
//! built from main (the bot hands out `plg` broadcasts since mumbot#45). Alice sends to Bob and
//! Carol: the first send starts the broadcast and admits both, the messages (up to 64 KiB) arrive
//! decrypted, once and in order; when Carol leaves, Alice's broadcast rekeys and Bob keeps
//! receiving. `testenv/up.sh v15`, build mumpeer and mumbot, then:
//! `MUMCORD_LIVE=1 MUMPEER_BIN=/path/to/mumpeer MUMBOT_BIN=/path/to/mumbot cargo test -p mumcord-engine --features test-backend --test live_plugin_broadcast -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::net::{TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use mumbot::env;
use mumcord_engine::p2p::P2pConfig;
use mumcord_engine::plugin_relay::{MAX_DATA, PluginRelayAvailability};
use mumcord_engine::store::P2pMode;
use mumcord_engine::{
    Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields, ServerId,
};
use mumcord_p2p::RequestId;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};

const WAIT: Duration = Duration::from_secs(30);
const PLUGIN: &str = "test-relay";

fn live() -> bool {
    let on = std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start the test servers");
    }
    on
}

/// `MUMPEER_BIN` and `MUMBOT_BIN`; `None` (test skipped) when one is not set.
fn bins() -> Option<(PathBuf, PathBuf)> {
    let get = |name| std::env::var_os(name).map(PathBuf::from);
    let bins = get("MUMPEER_BIN").zip(get("MUMBOT_BIN"));
    if bins.is_none() {
        eprintln!("skipped: set MUMPEER_BIN and MUMBOT_BIN to binaries built from main");
    }
    bins
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

fn free_udp_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0")
        .and_then(|s| s.local_addr())
        .expect("free udp port")
        .port()
}

fn free_tcp_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|s| s.local_addr())
        .expect("free tcp port")
        .port()
}

/// mumpeer with plugin data on, and the relay bot announcing it on the test server.
struct Relay {
    _mumpeer: Child,
    _bot: Child,
}

impl Relay {
    async fn start(mumpeer: &Path, mumbot: &Path, dir: &Path) -> Self {
        let (udp, tcp, api) = (free_udp_port(), free_tcp_port(), free_tcp_port());
        let config = dir.join("mumpeer.toml");
        std::fs::write(
            &config,
            format!(
                "public_host = \"127.0.0.1\"\nudp_listen = \"127.0.0.1:{udp}\"\n\
                 tcp_listen = \"127.0.0.1:{tcp}\"\napi_listen = \"127.0.0.1:{api}\"\n\
                 data_dir = \"{}\"\n[uses]\nplugin_data = true\n",
                dir.join("mumpeer-data").display()
            ),
        )
        .expect("mumpeer config");
        let secret: String = (0..4)
            .map(|_| format!("{:016x}", RequestId::random().0))
            .collect();
        let mumpeer_child = Command::new(mumpeer)
            .arg("--config")
            .arg(&config)
            .env("MUMPEER_API_SECRET", &secret)
            .env("RUST_LOG", "info")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("start mumpeer");
        let deadline = tokio::time::Instant::now() + WAIT;
        while !status_ok(api, &secret).await {
            assert!(
                tokio::time::Instant::now() < deadline,
                "mumpeer did not answer"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // The peer bot insists on the server certificate's SHA-256, as testenv's `up.sh` reads it.
        let fingerprint = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!(
                "openssl s_client -connect 127.0.0.1:{} </dev/null 2>/dev/null \
                 | openssl x509 -outform der | sha256sum | cut -d' ' -f1",
                common::v15()
            ))
            .output()
            .expect("openssl");
        let fingerprint = String::from_utf8_lossy(&fingerprint.stdout)
            .trim()
            .to_owned();
        assert_eq!(fingerprint.len(), 64, "server certificate fingerprint");
        let name = unique("relaybot");
        let data = dir.join("bot-data");
        std::fs::create_dir_all(&data).expect("bot data");
        let bot_config = dir.join("bot.toml");
        std::fs::write(
            &bot_config,
            format!(
                "[server]\nhost = \"127.0.0.1\"\nport = {}\nname = \"{name}\"\n\
                 server_cert_sha256 = \"{fingerprint}\"\n\n\
                 [peer]\napi_url = \"http://127.0.0.1:{api}\"\n",
                common::v15()
            ),
        )
        .expect("bot config");
        let bot = |args: &[&str]| {
            let mut command = Command::new(mumbot);
            command
                .args(args)
                .env("MUMBOT_DATA", &data)
                .env("MUMBOT_PEER_API_SECRET", &secret)
                .env("RUST_LOG", "info")
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .kill_on_drop(true);
            command
        };
        // The relay bot must be registered so clients accept its announcement (FR-4.8).
        let port = common::v15().to_string();
        let registered = bot(&["--port", &port, "--name", &name, "register"])
            .status()
            .await
            .expect("register the bot");
        assert!(registered.success(), "bot registration failed");
        let config = bot_config.to_string_lossy().into_owned();
        let bot_child = bot(&["peer", "--config", &config])
            .spawn()
            .expect("start the relay bot");
        Self {
            _mumpeer: mumpeer_child,
            _bot: bot_child,
        }
    }
}

async fn status_ok(api: u16, secret: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", api)).await else {
        return false;
    };
    let head = format!(
        "GET /v1/status HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {secret}\r\n\
         Connection: close\r\n\r\n"
    );
    if stream.write_all(head.as_bytes()).await.is_err() {
        return false;
    }
    let mut response = String::new();
    stream.read_to_string(&mut response).await.is_ok() && response.starts_with("HTTP/1.1 200")
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

struct Client {
    engine: Engine,
    rx: EventReceiver,
    session: u32,
}

impl Client {
    async fn start(dir: &Path, who: &str) -> Self {
        let (engine, mut rx) = Engine::new(
            &dir.join(who),
            SecretStore::in_memory(),
            tokio::runtime::Handle::current(),
        )
        .expect("engine");
        engine
            .identity_create(format!("{who} Tester"), None)
            .await
            .expect("identity");
        let mut settings = engine.settings().expect("settings");
        settings.p2p_mode = P2pMode::RelayFirst;
        engine.update_settings(settings).await.expect("mode");
        engine.p2p_set_config(P2pConfig {
            connect_timeout: Duration::from_secs(10),
            port_mapping: false,
            allow_loopback: true,
        });
        let server = engine
            .server_add(ServerFields {
                label: format!("broadcast {}", unique(who)),
                address: "127.0.0.1".into(),
                port: common::v15(),
                username: unique(who),
                password: match env::testenv_var("SERVER_PASSWORD") {
                    Some(p) => PasswordChange::Set(p),
                    None => PasswordChange::Keep,
                },
            })
            .await
            .expect("server");
        engine.connect(server.id, None).expect("connect");
        let session = loop {
            let event = wait_for(&mut rx, "sync", |e| match e {
                EngineEvent::CertTrustRequired { .. } | EngineEvent::ServerSynced(_) => {
                    Some(e.clone())
                }
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
                EngineEvent::ServerSynced(s) => break s.self_session,
                _ => unreachable!(),
            }
        };
        // The relay bot serves registered users only (its default).
        engine.register_self(server.id).await.expect("register");
        Self {
            engine,
            rx,
            session,
        }
    }

    fn server(&self) -> ServerId {
        self.engine
            .connections()
            .first()
            .and_then(mumcord_engine::ConnState::server_id)
            .unwrap_or(0)
    }

    async fn wait_available(&self) {
        let deadline = tokio::time::Instant::now() + WAIT;
        while !matches!(
            self.engine.plugin_relay_availability(self.server()).await,
            PluginRelayAvailability::Available { .. }
        ) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "relay plugin data never became available"
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    fn send(&self, data: Vec<u8>, receivers: &[u32]) {
        self.engine
            .plugin_relay_broadcast(self.server(), PLUGIN, (1, 1), data, receivers);
    }

    /// The next channel message from `sender`, if one comes within `wait`.
    async fn next_from(&mut self, sender: u32, wait: Duration) -> Option<Vec<u8>> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let event = tokio::time::timeout_at(deadline, self.rx.recv())
                .await
                .ok()??;
            if let EngineEvent::PluginRelayData {
                session,
                plugin,
                api,
                data,
            } = event
                && session == sender
            {
                assert_eq!((plugin.as_str(), api), (PLUGIN, (1, 1)));
                return Some(data.0);
            }
        }
    }

    async fn joined(&mut self) -> Vec<u32> {
        wait_for(&mut self.rx, "joined receivers", |e| match e {
            EngineEvent::PluginRelayJoined { joined } => Some(joined.clone()),
            _ => None,
        })
        .await
    }
}

fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

/// Sends small probes until every receiver has the keys and got one, then drains the rest. The
/// probes before that are lost for a receiver that is still joining, by design.
async fn until_joined(alice: &Client, receivers: &mut [&mut Client]) {
    let mut got = vec![false; receivers.len()];
    for round in 0..40u8 {
        alice.send(vec![0xEE, round], &[]);
        for (receiver, got) in receivers.iter_mut().zip(got.iter_mut()) {
            *got |= receiver
                .next_from(alice.session, Duration::from_millis(300))
                .await
                .is_some();
        }
        if got.iter().all(|g| *g) {
            for receiver in receivers.iter_mut() {
                while receiver
                    .next_from(alice.session, Duration::from_secs(1))
                    .await
                    .is_some()
                {}
            }
            return;
        }
    }
    panic!("a receiver never got the plugin broadcast");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15, MUMPEER_BIN and MUMBOT_BIN (MUMCORD_LIVE=1)"]
async fn channel_plugin_data_goes_once_through_the_relay_and_rekeys_on_leave() {
    if !live() {
        return;
    }
    let Some((mumpeer, mumbot)) = bins() else {
        return;
    };
    let dir = work_dir("live_plugin_broadcast");
    let _relay = Relay::start(&mumpeer, &mumbot, &dir).await;
    let mut alice = Client::start(&dir, "alice").await;
    let mut bob = Client::start(&dir, "bob").await;
    let mut carol = Client::start(&dir, "carol").await;
    for client in [&alice, &bob, &carol] {
        client.wait_available().await;
    }

    // The first send starts the broadcast; Bob and Carol are admitted when it runs.
    let receivers = [bob.session, carol.session];
    alice.send(vec![1], &receivers);
    let mut joined = alice.joined().await;
    joined.sort_unstable();
    let mut expected = receivers.to_vec();
    expected.sort_unstable();
    assert_eq!(joined, expected);
    until_joined(&alice, &mut [&mut bob, &mut carol]).await;

    // Once and in order for both, up to the full 64 KiB (two relay units).
    let messages: Vec<Vec<u8>> = [10, 0, MAX_DATA, 997, 20_000]
        .iter()
        .enumerate()
        .map(|(i, len)| pattern(*len, i as u8))
        .collect();
    for message in &messages {
        alice.send(message.clone(), &receivers);
    }
    for receiver in [&mut bob, &mut carol] {
        for message in &messages {
            let got = receiver
                .next_from(alice.session, WAIT)
                .await
                .expect("message through the broadcast");
            assert_eq!(got.len(), message.len());
            assert!(got == *message, "payload changed on the way");
        }
        assert_eq!(
            receiver
                .next_from(alice.session, Duration::from_secs(2))
                .await,
            None,
            "nothing twice"
        );
    }

    // Carol leaves: Alice takes her out and switches keys; Bob keeps receiving.
    carol.engine.disconnect(carol.server());
    assert_eq!(alice.joined().await, vec![bob.session]);
    for round in 0..3u8 {
        let message = pattern(500, 100 + round);
        alice.send(message.clone(), &[bob.session]);
        assert_eq!(
            bob.next_from(alice.session, WAIT).await,
            Some(message),
            "Bob gets the data after the rekey"
        );
    }

    // Switched off: the broadcast ends at once and nothing more arrives.
    alice.engine.plugin_relay_set_enabled(false);
    assert_eq!(alice.joined().await, Vec::<u32>::new());
    alice.send(vec![9], &[bob.session]);
    assert_eq!(
        bob.next_from(alice.session, Duration::from_secs(3)).await,
        None
    );
    for client in [&alice, &bob] {
        client.engine.disconnect(client.server());
    }
}
