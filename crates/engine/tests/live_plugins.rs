//! Engine bridge for plugins (spec 06, B13) against the local test server: two engines see each
//! other's `plugins` feature, exchange a plugin message, a block stops it (AC-6.13), a server
//! plugin list of an unregistered user is ignored with "sender not registered" while a
//! registered sender is pinned (AC-6.26), a whole game burst of plugin messages never holds up a
//! watch party and shows in the inspector as hex (AC-6.12), and a 1.3 server has no shared plugin
//! features (AC-6.17). `testenv/up.sh`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_plugins -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumbot::{Bot, BotConfig, env};
use mumcord_engine::inspector::TapDirection;
use mumcord_engine::plugins::{PluginEngineEvent, PluginSendError};
use mumcord_engine::watch_party::WatchPartyView;
use mumcord_engine::{
    ConnState, Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields,
    ServerId, WatchCommand,
};
use tokio::sync::mpsc::Receiver;

const WAIT: Duration = Duration::from_secs(20);
/// How long a message that must not arrive is waited for.
const QUIET: Duration = Duration::from_secs(3);
/// A YouTube video id for the watch party; nothing is played.
const VIDEO: &str = "dQw4w9WgXcQ";
/// A game burst's worth of plugin messages: 4 per second for 10 s (FR-6.36).
const BURST: usize = 40;
const SERVER_PLUGINS: &[u8] =
    br#"{"v":1,"seq":1,"plugins":[{"id":"dice","min":"1.0.0","src":"official","for":"Dice"}]}"#;

fn live() -> bool {
    let on = std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start the test servers");
    }
    on
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

async fn wait_engine(rx: &mut EventReceiver, mut pick: impl FnMut(&EngineEvent) -> bool) {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .expect("timed out waiting for an engine event")
            .expect("event stream open");
        if pick(&event) {
            return;
        }
    }
}

async fn wait_plugin<T>(
    rx: &mut Receiver<PluginEngineEvent>,
    what: &str,
    mut pick: impl FnMut(&PluginEngineEvent) -> Option<T>,
) -> T {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
            .expect("plugin stream open");
        if let Some(found) = pick(&event) {
            return found;
        }
    }
}

async fn wait_party(
    rx: &mut EventReceiver,
    what: &str,
    mut pick: impl FnMut(&WatchPartyView) -> bool,
) {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
            .expect("event stream open");
        if let EngineEvent::WatchPartyChanged(Some(party)) = &event
            && pick(party)
        {
            return;
        }
    }
}

/// Whether a plugin message with `data_id` arrives within [`QUIET`].
async fn receives(rx: &mut Receiver<PluginEngineEvent>, data_id: &str) -> bool {
    let deadline = tokio::time::Instant::now() + QUIET;
    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        if matches!(&event, PluginEngineEvent::PeerData { data_id: id, .. } if id == data_id) {
            return true;
        }
    }
    false
}

struct Client {
    engine: Engine,
    events: EventReceiver,
    plugins: Receiver<PluginEngineEvent>,
    name: String,
}

impl Client {
    async fn start(dir: &Path, who: &str, port: u16) -> Self {
        let (engine, mut rx) = Engine::new(
            &dir.join(who),
            SecretStore::in_memory(),
            tokio::runtime::Handle::current(),
        )
        .expect("engine");
        let plugins = engine.plugin_events().expect("plugin events");
        assert!(engine.plugin_events().is_none(), "one consumer only");
        engine
            .identity_create(format!("{who} Tester"), None)
            .await
            .expect("identity");
        let name = unique(who);
        let server = engine
            .server_add(ServerFields {
                label: format!("test {port} {name}"),
                address: "127.0.0.1".into(),
                port,
                username: name.clone(),
                password: match env::testenv_var("SERVER_PASSWORD") {
                    Some(p) => PasswordChange::Set(p),
                    None => PasswordChange::Keep,
                },
            })
            .await
            .expect("server");
        engine.connect(server.id, None).expect("connect");
        loop {
            let mut trust = None;
            let mut synced = false;
            wait_engine(&mut rx, |e| match e {
                EngineEvent::CertTrustRequired {
                    server_id,
                    fingerprint,
                    ..
                } => {
                    trust = Some((*server_id, fingerprint.clone()));
                    true
                }
                EngineEvent::ServerSynced(_) => {
                    synced = true;
                    true
                }
                _ => false,
            })
            .await;
            if let Some((server_id, fingerprint)) = trust {
                engine
                    .cert_trust_decide(server_id, &fingerprint, true)
                    .expect("trust");
            }
            if synced {
                break;
            }
        }
        Self {
            engine,
            events: rx,
            plugins,
            name,
        }
    }

    fn server(&self) -> ServerId {
        self.engine
            .connections()
            .first()
            .and_then(ConnState::server_id)
            .expect("connected")
    }

    fn session_of(&self, name: &str) -> u32 {
        self.engine
            .snapshot()
            .expect("synced")
            .users
            .iter()
            .find(|u| u.name == name)
            .unwrap_or_else(|| panic!("{name} not on the server"))
            .session
    }

    async fn shared_state(&mut self) -> bool {
        wait_plugin(&mut self.plugins, "the shared state", |e| match e {
            PluginEngineEvent::SharedState { supported, .. } => Some(*supported),
            _ => None,
        })
        .await
    }

    fn ignored(&self, kind: &str) -> u64 {
        self.engine
            .poll_packet_capture(None)
            .counts
            .iter()
            .filter(|c| c.kind == kind)
            .map(|c| c.count)
            .sum()
    }
}

async fn bot(dir: &Path, who: &str, port: u16) -> Bot {
    Bot::connect(BotConfig {
        data_dir: Some(dir.join(who)),
        ..BotConfig::new(&unique(who), port)
    })
    .await
    .expect("bot")
}

fn bot_session_of(bot: &Bot, name: &str) -> u32 {
    bot.state()
        .users
        .values()
        .find(|u| u.name == name)
        .map(|u| u.session)
        .unwrap_or_else(|| panic!("{name} is not on the server"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v14 (MUMCORD_LIVE=1)"]
async fn plugin_messages_flow_between_engines_until_blocked() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_plugins");
    let mut a = Client::start(&dir, "b13a", common::v14()).await;
    assert!(a.shared_state().await);
    let mut b = Client::start(&dir, "b13b", common::v14()).await;
    assert!(b.shared_state().await);
    let (a_server, b_server) = (a.server(), b.server());
    let b_session = a.session_of(&b.name);
    let a_session = b.session_of(&a.name);

    wait_plugin(&mut a.plugins, "B with plugins", |e| match e {
        PluginEngineEvent::PeerCapable {
            session, plugins, ..
        } => (*session == b_session && *plugins).then_some(()),
        _ => None,
    })
    .await;
    assert!(
        a.engine
            .plugin_presence(a_server)
            .iter()
            .any(|p| p.session == b_session),
        "B is in A's channel"
    );

    // The user's own channel message counts as chat activity for plugins; B sees it too.
    let a_channel = a
        .engine
        .snapshot()
        .expect("synced")
        .users
        .iter()
        .find(|u| u.session == a_session)
        .expect("A on the server")
        .channel_id;
    a.engine
        .send_channel_message(a_server, a_channel, "hello")
        .await
        .expect("message sent");
    for (rx, what) in [(&mut a.plugins, "A's own"), (&mut b.plugins, "B sees A's")] {
        wait_plugin(rx, &format!("{what} chat activity"), |e| match e {
            PluginEngineEvent::ChatActivity {
                sender, channel, ..
            } => (sender.session == a_session && channel.id == a_channel).then_some(()),
            _ => None,
        })
        .await;
    }

    // Invalid ids, oversized payloads and unknown people never reach the server.
    assert_eq!(
        a.engine
            .plugin_send(a_server, &[b_session], "mumcord.hello", vec![1])
            .err(),
        Some(PluginSendError::InvalidId)
    );
    assert_eq!(
        a.engine
            .plugin_send(a_server, &[b_session], "mumcord.p.dice", vec![0; 1001])
            .err(),
        Some(PluginSendError::TooLarge)
    );
    assert_eq!(
        a.engine
            .plugin_send(a_server, &[999_999], "mumcord.p.dice", vec![1])
            .err(),
        Some(PluginSendError::NoRecipients)
    );

    a.engine
        .plugin_send(a_server, &[b_session], "mumcord.p.dice", vec![1, 1, 2, 6])
        .expect("sent");
    let (sender, data) = wait_plugin(&mut b.plugins, "A's dice message", |e| match e {
        PluginEngineEvent::PeerData {
            sender,
            data_id,
            data,
            ..
        } if data_id == "mumcord.p.dice" => Some((sender.clone(), data.clone())),
        _ => None,
    })
    .await;
    assert_eq!(sender.session, a_session);
    assert!(sender.cert_hash.is_some());
    assert_eq!(data, [1, 1, 2, 6]);

    // B blocks A: A vanishes for B's plugins and A's messages are dropped and counted.
    b.engine
        .block_session(b_server, a_session)
        .expect("blocked");
    wait_plugin(&mut b.plugins, "A leaving for B's plugins", |e| match e {
        PluginEngineEvent::Left { peer, .. } => (peer.session == a_session).then_some(()),
        _ => None,
    })
    .await;
    // A still sees B as a plugin peer; the drop happens on B's side.
    a.engine
        .plugin_send(a_server, &[b_session], "mumcord.p.dice", vec![1, 1, 2, 3])
        .expect("sent");
    assert!(!receives(&mut b.plugins, "mumcord.p.dice").await);
    assert!(b.engine.plugin_stats(b_server).blocked_dropped >= 1);
    assert!(b.engine.plugin_presence(b_server).is_empty());
    assert_eq!(
        b.engine
            .plugin_send(b_server, &[a_session], "mumcord.p.dice", vec![1])
            .err(),
        Some(PluginSendError::NoRecipients)
    );
    drop(b);

    // Server plugin lists: an unregistered sender is ignored and shown in the inspector.
    a.engine.set_packet_capture(true);
    let stranger = bot(&dir, "b13-stranger", common::v14()).await;
    stranger
        .handle()
        .send_plugin_data(
            &[bot_session_of(&stranger, &a.name)],
            "mumcord.srvplugins",
            SERVER_PLUGINS.to_vec(),
        )
        .expect("sent");
    let not_registered = "mumcord.srvplugins (ignored: sender not registered)";
    let deadline = tokio::time::Instant::now() + WAIT;
    while a.ignored(not_registered) == 0 {
        assert!(tokio::time::Instant::now() < deadline, "not counted");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(a.engine.plugin_stats(a_server).unregistered_dropped, 1);

    // A registered sender is pinned on first use and its list reaches the plugin service.
    let mumbot = bot(&dir, "b13-mumbot", common::v14()).await;
    mumbot.register().await.expect("registered");
    mumbot
        .handle()
        .send_plugin_data(
            &[bot_session_of(&mumbot, &a.name)],
            "mumcord.srvplugins",
            SERVER_PLUGINS.to_vec(),
        )
        .expect("sent");
    let data = wait_plugin(&mut a.plugins, "the server plugin list", |e| match e {
        PluginEngineEvent::ServerPlugins { data, .. } => Some(data.clone()),
        _ => None,
    })
    .await;
    assert_eq!(data, SERVER_PLUGINS);
    drop(stranger);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v13 (MUMCORD_LIVE=1)"]
async fn servers_before_1_4_have_no_shared_plugin_features() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_plugins_v13");
    let mut a = Client::start(&dir, "b13old", common::v13()).await;
    assert!(!a.shared_state().await);
    let server = a.server();
    assert!(!a.engine.plugin_shared_supported(server));
    assert_eq!(
        a.engine
            .plugin_send(server, &[1], "mumcord.p.dice", vec![1])
            .err(),
        Some(PluginSendError::Unsupported)
    );
}

/// AC-6.12 (transport): even the most a plugin may send at once (a whole game burst) waits in the
/// low-priority lane behind Mumcord's own traffic, so a watch party started at the same time stays
/// responsive; the plugin messages still arrive or are counted as expired. The inspector shows
/// them as hex only (FR-6.39).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v14 (MUMCORD_LIVE=1)"]
async fn a_plugin_burst_never_holds_up_the_watch_party() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_plugins_burst");
    let mut a = Client::start(&dir, "c6a", common::v14()).await;
    assert!(a.shared_state().await);
    let mut b = Client::start(&dir, "c6b", common::v14()).await;
    assert!(b.shared_state().await);
    let (a_server, b_server) = (a.server(), b.server());
    let b_session = a.session_of(&b.name);
    let a_session = b.session_of(&a.name);
    wait_plugin(&mut a.plugins, "B with plugins", |e| match e {
        PluginEngineEvent::PeerCapable {
            session, plugins, ..
        } => (*session == b_session && *plugins).then_some(()),
        _ => None,
    })
    .await;
    a.engine.set_packet_capture(true);
    b.engine.set_packet_capture(true);

    for i in 0..BURST {
        a.engine
            .plugin_send(
                a_server,
                &[b_session],
                "mumcord.p.dice",
                vec![1, 1, 1, u8::try_from(i).expect("small")],
            )
            .expect("queued");
    }
    let started = std::time::Instant::now();
    a.engine
        .watch_party_start(a_server, VIDEO)
        .expect("party starts");
    wait_party(&mut b.events, "the invitation", |_| true).await;
    let invited = started.elapsed();
    b.engine.watch_party_join(b_server).expect("join");
    wait_party(&mut a.events, "B in the party", |p| {
        p.participants.contains(&b_session)
    })
    .await;
    let sent = std::time::Instant::now();
    a.engine
        .watch_party_command(a_server, WatchCommand::Pause { position_ms: 5000 })
        .expect("pause");
    wait_party(&mut b.events, "the pause", |p| {
        !p.playing && p.last_action.is_some()
    })
    .await;
    let paused = sent.elapsed();
    let mut received = 0;
    while let Ok(event) = b.plugins.try_recv() {
        if matches!(&event, PluginEngineEvent::PeerData { data_id, .. } if data_id == "mumcord.p.dice")
        {
            received += 1;
        }
    }
    assert!(
        received < BURST,
        "the burst was still queued while the party went ahead"
    );
    assert!(
        invited < Duration::from_secs(1),
        "invitation took {invited:?}"
    );
    assert!(paused < Duration::from_secs(1), "pause took {paused:?}");

    // The rest of the burst arrives later or is dropped and counted after 10 s.
    let deadline = tokio::time::Instant::now() + WAIT;
    while received + usize::try_from(a.engine.plugin_stats(a_server).low_expired).expect("small")
        < BURST
    {
        if let Ok(Some(PluginEngineEvent::PeerData {
            sender, data_id, ..
        })) = tokio::time::timeout_at(deadline, b.plugins.recv()).await
            && data_id == "mumcord.p.dice"
        {
            assert_eq!(sender.session, a_session);
            received += 1;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{received} of {BURST} arrived, the rest was not counted"
        );
    }
    assert!(received > 0, "plugin messages still get through");

    // Inspector: plugin messages as hex only, with their pacing on the sending side.
    let on_b = b.engine.poll_packet_capture(None).packets;
    let first = on_b
        .iter()
        .find(|p| p.kind == "mumcord.p.dice" && p.direction == TapDirection::Received)
        .expect("B's inspector shows the plugin message");
    assert_eq!(first.sender, Some(a_session));
    assert_eq!(first.decoded, None);
    assert_eq!(first.raw_hex, "01010100");
    let on_a = a.engine.poll_packet_capture(None).packets;
    let sent_rows: Vec<_> = on_a
        .iter()
        .filter(|p| p.kind == "mumcord.p.dice" && p.direction == TapDirection::Sent)
        .collect();
    assert_eq!(sent_rows.len(), received);
    assert!(
        sent_rows
            .iter()
            .all(|p| p.decoded.is_none() && p.delay_ms.is_some())
    );
    assert!(
        on_a.iter()
            .any(|p| p.kind == "mumcord.watch" && p.direction == TapDirection::Sent),
        "the party went out on its own lane"
    );
}
