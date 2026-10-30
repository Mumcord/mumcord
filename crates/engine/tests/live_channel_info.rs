//! Pinned channel info (FR-1.48b) against the local test server, with a SuperUser bot that
//! creates a temporary channel with a long description. `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_channel_info -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumbot::{Bot, BotConfig, env};
use mumcord_engine::EventReceiver;
use mumcord_engine::{
    ChannelView, Engine, EngineEvent, PasswordChange, SecretStore, ServerFields, Snapshot,
};
use mumcord_protocol::proto;

const WAIT: Duration = Duration::from_secs(20);

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

async fn wait_channel(
    rx: &mut EventReceiver,
    what: &str,
    id: u32,
    mut done: impl FnMut(&ChannelView) -> bool,
) -> ChannelView {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
            .expect("event stream open");
        if let EngineEvent::ChannelUpserted(view) = event
            && view.id == id
            && done(&view)
        {
            return view;
        }
    }
}

async fn connect(engine: &Engine, rx: &mut EventReceiver) -> Snapshot {
    let name = unique("mc-info");
    engine
        .identity_create(name.clone(), None)
        .await
        .expect("identity");
    let server = engine
        .server_add(ServerFields {
            label: "test v15".into(),
            address: "127.0.0.1".into(),
            port: common::v15(),
            username: name,
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server");
    engine.connect(server.id, None).expect("connect");
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .expect("timed out waiting for sync")
            .expect("event stream open");
        match event {
            EngineEvent::CertTrustRequired {
                server_id,
                fingerprint,
                ..
            } => engine
                .cert_trust_decide(server_id, &fingerprint, true)
                .expect("trust"),
            EngineEvent::ServerSynced(s) => return *s,
            _ => {}
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the local test server (testenv/up.sh v15) and MUMCORD_LIVE=1"]
async fn description_is_fetched_on_demand_and_updates_live() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_channel_info");

    // Long enough (≥ 128 bytes) that the server announces it only by hash.
    let name = unique("mc-info-room");
    let long = format!(
        "<p><b>Raid rules</b>: be on time.</p><script>alert(1)</script><p>{}</p>",
        "Bring potions and food. ".repeat(10)
    );
    let admin = Bot::connect(BotConfig::superuser(common::v15()))
        .await
        .expect("SuperUser bot (SUPERUSER_PASSWORD in testenv/.env)");
    // A temporary channel moves its creator in and disappears when the bot leaves.
    admin
        .handle()
        .send_channel_state(proto::ChannelState {
            parent: Some(0),
            name: Some(name.clone()),
            description: Some(long),
            temporary: Some(true),
            ..Default::default()
        })
        .expect("create channel");
    let deadline = tokio::time::Instant::now() + WAIT;
    let room = loop {
        if let Some(id) = admin
            .state()
            .channels
            .values()
            .find(|c| c.name == name)
            .map(|c| c.id)
        {
            break id;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "channel not created"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    let snapshot = connect(&engine, &mut rx).await;

    assert!(
        snapshot
            .info
            .welcome_text
            .as_deref()
            .is_some_and(|t| t.contains("Welcome")),
        "welcome text in the snapshot"
    );
    assert!(snapshot.info.default_channel_id.is_some());

    let view = snapshot
        .channels
        .iter()
        .find(|c| c.id == room)
        .expect("room in snapshot");
    assert!(view.has_description);
    assert_eq!(view.description, None, "only the hash is sent on connect");

    engine
        .channel_description_request(only_server(&engine), room)
        .expect("request");
    let view = wait_channel(&mut rx, "fetched description", room, |v| {
        v.description.is_some()
    })
    .await;
    let text = view.description.expect("description");
    assert!(text.contains("<b>Raid rules</b>"));
    assert!(!text.contains("script"), "sanitized");
    // Asking again for the same hash sends nothing and succeeds.
    engine
        .channel_description_request(only_server(&engine), room)
        .expect("repeat");

    admin
        .handle()
        .send_channel_state(proto::ChannelState {
            channel_id: Some(room),
            description: Some("Updated rules".into()),
            ..Default::default()
        })
        .expect("edit description");
    let view = wait_channel(&mut rx, "changed description", room, |v| {
        v.description.as_deref() == Some("Updated rules")
    })
    .await;
    assert!(view.has_description);

    engine.disconnect(only_server(&engine));
    admin.handle().disconnect();
}

/// The test's only connected server (0, which no call accepts, when none is).
fn only_server(engine: &Engine) -> mumcord_engine::ServerId {
    engine
        .connections()
        .first()
        .and_then(mumcord_engine::ConnState::server_id)
        .unwrap_or(0)
}
