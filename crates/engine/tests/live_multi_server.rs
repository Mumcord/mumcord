//! Several servers at once (FR-2.1–2.3, FR-2.6, FR-2.9; AC-2.1, AC-2.2, AC-2.5) against the
//! local test servers v15 and v15b, plus v14 as a third server. `testenv/up.sh v15 v15b v14`,
//! then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_multi_server -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::Path;
use std::time::Duration;

use mumbot::{Bot, BotConfig, UserCheck, env};
use mumcord_engine::{
    ConnState, Engine, EngineEvent, ErrorKind, EventReceiver, PasswordChange, SecretStore,
    ServerFields, ServerId, Snapshot,
};

const WAIT: Duration = Duration::from_secs(20);

/// Waits for the first event `pick` accepts; also hands it the event's server.
async fn wait_for<T>(
    rx: &mut EventReceiver,
    what: &str,
    mut pick: impl FnMut(Option<ServerId>, &EngineEvent) -> Option<T>,
) -> T {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let (server, event) = tokio::time::timeout_at(deadline, rx.recv_tagged())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
            .expect("event stream open");
        if let Some(found) = pick(server, &event) {
            return found;
        }
    }
}

/// Connects and answers trust questions; the sync event must carry the server's id.
async fn connect(engine: &Engine, rx: &mut EventReceiver, server_id: ServerId) -> Snapshot {
    engine.connect(server_id, None).expect("connect");
    loop {
        let (tag, event) = wait_for(rx, "sync", |tag, e| match e {
            EngineEvent::CertTrustRequired { server_id: id, .. } if *id == server_id => {
                Some((tag, e.clone()))
            }
            EngineEvent::ServerSynced(s) if s.server_id == server_id => Some((tag, e.clone())),
            _ => None,
        })
        .await;
        match event {
            EngineEvent::CertTrustRequired { fingerprint, .. } => engine
                .cert_trust_decide(server_id, &fingerprint, true)
                .expect("trust"),
            EngineEvent::ServerSynced(snapshot) => {
                assert_eq!(tag, Some(server_id), "sync tagged with its server");
                return *snapshot;
            }
            _ => unreachable!(),
        }
    }
}

async fn wait_voice_server(rx: &mut EventReceiver, expected: Option<ServerId>) {
    wait_for(rx, "voice server change", |_, e| {
        matches!(e, EngineEvent::VoiceServerChanged(v) if *v == expected).then_some(())
    })
    .await;
}

fn channel_id(snapshot: &Snapshot, name: &str) -> u32 {
    snapshot
        .channels
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("channel {name}"))
        .id
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15, v15b and v14 (MUMCORD_LIVE=1)"]
async fn voice_moves_between_connected_servers_within_the_limit() {
    if !std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1") {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start the test servers");
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("live_multi_server");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create("Multi Tester".into())
        .await
        .expect("identity");
    let name = format!("ms-{:06x}", std::process::id() & 0xff_ffff);
    let add = async |port: u16| {
        engine
            .server_add(ServerFields {
                label: format!("test {port}"),
                address: "127.0.0.1".into(),
                port,
                username: name.clone(),
                password: match env::testenv_var("SERVER_PASSWORD") {
                    Some(p) => PasswordChange::Set(p),
                    None => PasswordChange::Keep,
                },
            })
            .await
            .expect("server")
            .id
    };
    let a = add(common::v15()).await;
    let b = add(common::v15b()).await;
    let c = add(common::v14()).await;
    let mut settings = engine.settings().expect("settings");
    settings.max_connected_servers = 2;
    engine.update_settings(settings).await.expect("limit 2");

    let bot_a = Bot::connect(BotConfig {
        data_dir: Some(dir.join("bot-a")),
        ..BotConfig::new(&format!("{name}-ea"), common::v15())
    })
    .await
    .expect("bot on A");
    let bot_b = Bot::connect(BotConfig {
        data_dir: Some(dir.join("bot-b")),
        ..BotConfig::new(&format!("{name}-eb"), common::v15b())
    })
    .await
    .expect("bot on B");
    let check = |deaf: bool| UserCheck {
        in_channel: Some("Raid".into()),
        deaf: Some(deaf),
        ..UserCheck::default()
    };

    // Both connect read-only; nothing has voice (FR-2.3).
    let snap_a = connect(&engine, &mut rx, a).await;
    let snap_b = connect(&engine, &mut rx, b).await;
    assert!(snap_a.read_only && snap_b.read_only);
    assert_eq!(engine.connections().len(), 2);
    assert_eq!(engine.voice_server(), None);

    // A third server is beyond the limit (AC-2.5).
    let refused = engine.connect(c, None).expect_err("limit");
    assert_eq!(refused.kind, ErrorKind::ServerLimitReached, "{refused}");

    // Join on A: voice there, B stays read-only and deafened (AC-2.1).
    engine
        .join_channel(a, channel_id(&snap_a, "Raid"))
        .expect("join Raid on A");
    wait_voice_server(&mut rx, Some(a)).await;
    bot_a
        .wait_user(&name, &check(false))
        .await
        .expect("A hears the user");
    assert!(engine.snapshot_of(b).expect("B").read_only);

    // Join on B: voice moves, A becomes deafened in its channel (AC-2.2).
    engine
        .join_channel(b, channel_id(&snap_b, "Raid"))
        .expect("join Raid on B");
    wait_voice_server(&mut rx, Some(b)).await;
    bot_b
        .wait_user(&name, &check(false))
        .await
        .expect("B hears the user");
    bot_a
        .wait_user(&name, &check(true))
        .await
        .expect("A shows the user deafened");
    assert!(engine.snapshot_of(a).expect("A").read_only);
    assert!(!engine.snapshot_of(b).expect("B").read_only);
    assert_eq!(engine.snapshot().expect("voice snapshot").server_id, b);

    // Disconnecting A leaves B and its voice alone; C fits again (FR-2.9).
    engine.disconnect(a);
    wait_for(&mut rx, "A disconnected", |tag, e| {
        (tag == Some(a) && matches!(e, EngineEvent::StateChanged(ConnState::Disconnected { .. })))
            .then_some(())
    })
    .await;
    assert_eq!(engine.voice_server(), Some(b));
    assert!(matches!(
        engine.connection_state_of(b),
        ConnState::Connected { .. }
    ));
    let snap_c = connect(&engine, &mut rx, c).await;
    assert!(snap_c.read_only, "voice stays on B");
    assert_eq!(engine.voice_server(), Some(b));

    // Quitting records the connected servers and the voice server (FR-2.7, FR-2.3).
    engine.shutdown().await;
    assert!(engine.connections().is_empty());
    let servers = engine.servers().await.expect("servers");
    let voice: Vec<ServerId> = servers
        .iter()
        .filter(|s| s.was_voice_server_at_quit)
        .map(|s| s.id)
        .collect();
    assert_eq!(voice, [b]);
}
