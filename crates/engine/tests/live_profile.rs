//! Rich comment and refused profile changes (FR-2.50, FR-2.51) against the local v15 server.
//! `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_profile -- --ignored`
#![cfg(feature = "test-backend")]

mod common;

use std::time::Duration;

use mumcord_bot::{Bot, BotConfig, UserCheck, env};
use mumcord_engine::{
    Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields,
};

const WAIT: Duration = Duration::from_secs(20);

async fn wait_synced(engine: &Engine, rx: &mut EventReceiver, id: mumcord_engine::ServerId) {
    engine.connect(id, None).expect("connect");
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn formatted_comment_reaches_other_clients_and_oversized_one_is_refused() {
    if !std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1") {
        return;
    }
    // SAFETY: set before any audio thread exists.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("engine-live-profile");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    let suffix = std::process::id();
    let name = format!("mc-prof-{suffix:x}");
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create(name.clone())
        .await
        .expect("identity");
    let server = engine
        .server_add(ServerFields {
            label: "profile".into(),
            address: "127.0.0.1".into(),
            port: common::v15(),
            username: name.clone(),
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server");
    let bot = Bot::connect(BotConfig {
        data_dir: Some(dir.join("bot")),
        ..BotConfig::new(&format!("mc-pal-{suffix:x}"), common::v15())
    })
    .await
    .expect("bot");
    wait_synced(&engine, &mut rx, server.id).await;

    let html = "<p><b>bold</b> <i>italic</i> <a href=\"https://example.org\">link</a></p><ul><li>one</li></ul>";
    engine
        .set_own_comment(server.id, html.into())
        .await
        .expect("formatted comment is accepted");
    bot.wait_user(
        &name,
        &UserCheck {
            comment_contains: Some("<b>bold</b>".into()),
            ..UserCheck::default()
        },
    )
    .await
    .expect("the other client sees the HTML");

    let too_long = format!("<p>{}</p>", "x".repeat(200_000));
    let refused = engine.set_own_comment(server.id, too_long).await;
    assert!(refused.is_err(), "an oversized comment must be refused");
}
