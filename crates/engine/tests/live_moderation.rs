//! Server-side mute, deafen and suppression of other people reach the UI's user view, against
//! the local test server with a SuperUser bot as the admin. `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_moderation -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumcord_bot::{Bot, BotConfig, Moderation, env};
use mumcord_engine::EventReceiver;
use mumcord_engine::{
    Engine, EngineEvent, PasswordChange, SecretStore, ServerFields, Snapshot, UserView,
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

async fn connect(engine: &Engine, rx: &mut EventReceiver) -> Snapshot {
    let name = unique("mc-mod");
    engine
        .identity_create(name.clone())
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

async fn wait_user(
    rx: &mut EventReceiver,
    what: &str,
    name: &str,
    mut done: impl FnMut(&UserView) -> bool,
) -> UserView {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
            .expect("event stream open");
        if let EngineEvent::UserUpserted(view) = event
            && view.name == name
            && done(&view)
        {
            return view;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the local test server (testenv/up.sh v15) and MUMCORD_LIVE=1"]
async fn server_mute_deafen_and_suppress_reach_the_user_view() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_moderation");

    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    connect(&engine, &mut rx).await;

    let target_name = unique("mc-mod-target");
    let mut config = BotConfig::new(&target_name, common::v15());
    config.data_dir = Some(dir.join("bot"));
    let target = Bot::connect(config).await.expect("target bot");
    let view = wait_user(&mut rx, "target appears", &target_name, |_| true).await;
    assert!(!view.server_mute && !view.server_deaf && !view.suppressed);

    let admin = Bot::connect(BotConfig::superuser(common::v15()))
        .await
        .expect("SuperUser bot (SUPERUSER_PASSWORD in testenv/.env)");
    let moderate = |m: Moderation| admin.moderate(&target_name, m).expect("moderate");

    moderate(Moderation {
        mute: Some(true),
        ..Default::default()
    });
    let view = wait_user(&mut rx, "server mute", &target_name, |v| v.server_mute).await;
    assert!(!view.self_mute && !view.suppressed && !view.server_deaf);

    moderate(Moderation {
        deaf: Some(true),
        ..Default::default()
    });
    let view = wait_user(&mut rx, "server deafen", &target_name, |v| v.server_deaf).await;
    assert!(view.server_mute && !view.suppressed && !view.self_deaf);

    moderate(Moderation {
        mute: Some(false),
        deaf: Some(false),
    });
    wait_user(&mut rx, "lifted", &target_name, |v| {
        !v.server_mute && !v.server_deaf
    })
    .await;

    // Suppression comes from a missing Speak permission in the channel (servers refuse to
    // suppress on request). A temporary channel moves its creator in and disappears with it.
    let room = unique("mc-mod-quiet");
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
    admin.deny_speak(&room).await.expect("deny speak");
    admin.move_user(&target_name, &room).expect("move");
    let view = wait_user(&mut rx, "suppressed", &target_name, |v| v.suppressed).await;
    assert!(!view.server_mute, "suppression is not a server mute");

    admin.move_user(&target_name, "Root/").expect("move back");
    wait_user(&mut rx, "suppression lifted", &target_name, |v| {
        !v.suppressed
    })
    .await;

    engine.disconnect(only_server(&engine));
    admin.handle().disconnect();
    target.handle().disconnect();
}

/// The test's only connected server (0, which no call accepts, when none is).
fn only_server(engine: &Engine) -> mumcord_engine::ServerId {
    engine
        .connections()
        .first()
        .and_then(mumcord_engine::ConnState::server_id)
        .unwrap_or(0)
}
