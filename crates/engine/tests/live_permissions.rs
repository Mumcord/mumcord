//! Rights changed by an admin while connected reach the UI without reconnecting (issue #248):
//! after an ACL change the server flushes every cached permission, and the session asks again.
//! Against the local test server with a SuperUser bot as the admin. `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_permissions -- --ignored --test-threads=1`

#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumbot::{Bot, BotConfig, env};
use mumcord_engine::{
    ChannelView, Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields,
};

// mumbot pins its own mumcord-protocol release (the `bot_protocol` alias), so bot-side messages use its types.
use bot_protocol::proto;

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

async fn connect(engine: &Engine, rx: &mut EventReceiver) {
    let name = unique("mc-perm");
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
            EngineEvent::ServerSynced(_) => return,
            _ => {}
        }
    }
}

/// Waits until the engine reports `name` with permissions that satisfy `done`.
async fn wait_channel(
    rx: &mut EventReceiver,
    what: &str,
    name: &str,
    mut done: impl FnMut(&ChannelView) -> bool,
) -> ChannelView {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
            .expect("event stream open");
        let views = match event {
            EngineEvent::ChannelsUpserted(views) => views,
            EngineEvent::ChannelUpserted(view) => vec![view],
            _ => continue,
        };
        if let Some(view) = views.into_iter().find(|v| v.name == name && done(v)) {
            return view;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the local test server (testenv/up.sh v15) and MUMCORD_LIVE=1"]
async fn an_acl_change_while_connected_updates_the_rights() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_permissions");

    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    connect(&engine, &mut rx).await;

    let admin = Bot::connect(BotConfig::superuser(common::v15()))
        .await
        .expect("SuperUser bot (SUPERUSER_PASSWORD in testenv/.env)");
    // A temporary channel keeps its creator inside and disappears with it.
    let room = unique("mc-perm-room");
    admin
        .handle()
        .send_channel_state(proto::ChannelState {
            parent: Some(0),
            name: Some(room.clone()),
            temporary: Some(true),
            ..Default::default()
        })
        .expect("create channel");
    let deadline = tokio::time::Instant::now() + WAIT;
    while admin.channel_id(&room).is_err() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "channel not created"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let view = wait_channel(&mut rx, "the new channel", &room, |_| true).await;
    assert!(view.permissions.speak, "speaking is allowed by default");

    // The ACL change flushes every cached right; the client asks again without reconnecting.
    admin.deny_speak(&room).await.expect("deny speak");
    wait_channel(&mut rx, "speak denied", &room, |v| !v.permissions.speak).await;

    admin.allow_speak(&room).await.expect("allow speak");
    wait_channel(&mut rx, "speak allowed again", &room, |v| {
        v.permissions.speak
    })
    .await;
}
