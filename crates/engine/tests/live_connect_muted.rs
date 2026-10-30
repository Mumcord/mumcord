//! "Connect muted until I join a channel" (FR-1.14, QA.22) against the local test server.
//! `testenv/up.sh v14`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_connect_muted -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::Path;
use std::time::Duration;

use mumbot::env;
use mumcord_engine::EventReceiver;
use mumcord_engine::{
    ConnState, Engine, EngineEvent, PasswordChange, SecretStore, ServerFields, ServerView,
};

const WAIT: Duration = Duration::from_secs(20);

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

async fn connect(engine: &Engine, rx: &mut EventReceiver, server: &ServerView) {
    engine.connect(server.id, None).expect("connect");
    loop {
        let event = wait_for(rx, "sync", |e| match e {
            EngineEvent::CertTrustRequired { .. } | EngineEvent::ServerSynced(_) => Some(e.clone()),
            _ => None,
        })
        .await;
        if let EngineEvent::CertTrustRequired {
            server_id,
            fingerprint,
            ..
        } = event
        {
            engine
                .cert_trust_decide(server_id, &fingerprint, true)
                .expect("trust");
        } else {
            return;
        }
    }
}

async fn disconnect(engine: &Engine, rx: &mut EventReceiver) {
    engine.disconnect(only_server(engine));
    wait_for(rx, "disconnected", |e| {
        matches!(e, EngineEvent::StateChanged(ConnState::Disconnected { .. })).then_some(())
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v14 (MUMCORD_LIVE=1)"]
async fn setting_off_connects_with_the_last_own_voice_choice() {
    if !std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1") {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start the test servers");
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("live_connect_muted");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create("Connect Tester".into(), None)
        .await
        .expect("identity");
    let name = format!("f74-{:06x}", std::process::id() & 0xff_ffff);
    let server = engine
        .server_add(ServerFields {
            label: format!("test {} {name}", common::v14()),
            address: "127.0.0.1".into(),
            port: common::v14(),
            username: name,
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server");

    // Default: read-only. Leave it by undeafening, then mute: the own choice is "muted".
    assert!(engine.settings().expect("settings").connect_muted);
    connect(&engine, &mut rx, &server).await;
    assert!(engine.snapshot().expect("snapshot").read_only);
    engine.set_self_deaf(false).expect("undeafen");
    engine.set_self_mute(true).expect("mute");
    disconnect(&engine, &mut rx).await;

    // Still on: a fresh connect is read-only again.
    connect(&engine, &mut rx, &server).await;
    assert!(engine.snapshot().expect("snapshot").read_only);
    disconnect(&engine, &mut rx).await;

    // Off: not read-only, the last choice (muted, not deafened) applies right away.
    let mut settings = engine.settings().expect("settings");
    settings.connect_muted = false;
    engine.update_settings(settings).await.expect("toggle off");
    connect(&engine, &mut rx, &server).await;
    let snapshot = engine.snapshot().expect("snapshot");
    assert!(!snapshot.read_only);
    let me = snapshot
        .users
        .iter()
        .find(|u| u.session == snapshot.self_session)
        .expect("own user");
    assert!(me.self_mute && !me.self_deaf);
    disconnect(&engine, &mut rx).await;
}

/// The test's only connected server (0, which no call accepts, when none is).
fn only_server(engine: &Engine) -> mumcord_engine::ServerId {
    engine
        .connections()
        .first()
        .and_then(mumcord_engine::ConnState::server_id)
        .unwrap_or(0)
}
