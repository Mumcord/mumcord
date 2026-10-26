//! A chat message the server refuses is reported, not stored, and can be sent again (FR-2.47,
//! AC-2.30). `testenv/up.sh v15` (SuperUser bot needs SUPERUSER_PASSWORD in testenv/.env), then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_chat_refused -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::Path;
use std::time::Duration;

use mumcord_bot::{Bot, BotConfig, env};
use mumcord_engine::{
    Engine, EngineEvent, ErrorKind, EventReceiver, PasswordChange, SecretStore, ServerFields,
    Snapshot,
};

const WAIT: Duration = Duration::from_secs(20);

async fn connect(engine: &Engine, rx: &mut EventReceiver, name: &str) -> (i64, Snapshot) {
    let server = engine
        .server_add(ServerFields {
            label: "refused".into(),
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
    engine.connect(server.id, None).expect("connect");
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .expect("sync in time")
            .expect("event stream open");
        match event {
            EngineEvent::CertTrustRequired {
                server_id,
                fingerprint,
                ..
            } => engine
                .cert_trust_decide(server_id, &fingerprint, true)
                .expect("trust"),
            EngineEvent::ServerSynced(s) => return (server.id, *s),
            _ => {}
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn refused_message_is_reported_and_can_be_retried() {
    if !std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1") {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start the test servers");
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("engine-live-chat-refused");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    let name = format!("mc-refused-{}", std::process::id());
    engine
        .identity_create(name.clone())
        .await
        .expect("identity");
    let (server, snapshot) = connect(&engine, &mut rx, &name).await;
    let lobby = snapshot
        .channels
        .iter()
        .find(|c| c.name == "Lobby")
        .expect("Lobby")
        .id;

    let admin = Bot::connect(BotConfig::superuser(common::v15()))
        .await
        .expect("SuperUser bot (SUPERUSER_PASSWORD in testenv/.env)");
    admin.deny_text("Lobby").await.expect("deny text");

    let refused = engine
        .send_channel_message(server, lobby, "blocked")
        .await
        .expect_err("refused");
    assert_eq!(refused.kind, ErrorKind::PermissionDenied);

    admin.allow_text("Lobby").await.expect("allow text");
    // The client learns about the changed permission a moment later.
    let mut sent = None;
    for _ in 0..20 {
        match engine.send_channel_message(server, lobby, "blocked").await {
            Ok(message) => {
                sent = Some(message);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
        }
    }
    assert!(sent.is_some_and(|m| m.own), "the retry is accepted");
}
