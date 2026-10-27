//! Default avatar sent once per saved server (FR-1.47, AC-1.36) and the avatar remembered
//! per server sent again when the server keeps none (FR-1.20, QA.10), against the local test
//! server, checked by a bot. `testenv/up.sh v14`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_default_avatar -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumbot::samples::PNG;
use mumbot::{Bot, BotConfig, UserCheck, env};
use mumcord_engine::EventReceiver;
use mumcord_engine::{
    ConnState, Engine, EngineEvent, PasswordChange, SecretStore, ServerFields, ServerView,
};

const WAIT: Duration = Duration::from_secs(20);
/// Time the engine gets to (wrongly) send an avatar before the bot checks there is none.
const SETTLE: Duration = Duration::from_secs(3);

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

async fn add_server(engine: &Engine, name: &str) -> ServerView {
    engine
        .server_add(ServerFields {
            label: format!("test {} {name}", common::v14()),
            address: "127.0.0.1".into(),
            port: common::v14(),
            username: name.into(),
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server")
}

/// Fresh connect, trusting the server certificate on first use.
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

async fn disconnect(engine: &Engine, rx: &mut EventReceiver) {
    engine.disconnect(only_server(engine));
    wait_for(rx, "disconnected", |e| {
        matches!(e, EngineEvent::StateChanged(ConnState::Disconnected { .. })).then_some(())
    })
    .await;
}

/// The server's hash of the user's avatar, once the bot sees one.
async fn avatar_hash(bot: &Bot, name: &str) -> Vec<u8> {
    bot.wait_user(name, &avatar(true))
        .await
        .expect("bot sees the avatar")
        .texture_hash
        .expect("avatar hash")
}

/// A plain PNG that differs from the sample image.
fn other_png() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(16, 16, image::Rgba([200, 30, 30, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("png");
    out.into_inner()
}

fn avatar(has: bool) -> UserCheck {
    UserCheck {
        has_avatar: Some(has),
        ..UserCheck::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v14 (MUMCORD_LIVE=1)"]
async fn default_avatar_is_sent_once_per_server() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_default_avatar");
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create("Avatar Tester".into())
        .await
        .expect("identity");
    assert!(engine.settings().expect("settings").send_default_avatar);
    let source = dir.join("me.png");
    std::fs::write(&source, PNG).expect("image");
    let preview = engine
        .default_avatar_set(Some(source))
        .await
        .expect("set default avatar");
    assert!(preview.is_some_and(|p| p.starts_with("data:image/png;base64,")));

    let bot = Bot::connect(BotConfig {
        data_dir: Some(dir.join("bot")),
        ..BotConfig::new(&unique("f32-bot"), common::v14())
    })
    .await
    .expect("bot");

    // Fresh server without an avatar there: the default avatar is sent.
    let first_name = unique("f32-first");
    let first = add_server(&engine, &first_name).await;
    connect(&engine, &mut rx, &first).await;
    bot.wait_user(&first_name, &avatar(true))
        .await
        .expect("bot sees the default avatar");

    // The engine remembers the server once the server confirmed the avatar; a human never
    // removes it faster than that.
    tokio::time::sleep(SETTLE).await;

    // Removed manually, then disconnect and connect again: not sent a second time.
    engine
        .set_own_avatar(only_server(&engine), None)
        .await
        .expect("remove avatar");
    bot.wait_user(&first_name, &avatar(false))
        .await
        .expect("bot sees the removal");
    disconnect(&engine, &mut rx).await;
    bot.wait_user_gone(&first_name).await.expect("gone");
    connect(&engine, &mut rx, &first).await;
    tokio::time::sleep(SETTLE).await;
    bot.wait_user(&first_name, &avatar(false))
        .await
        .expect("not re-sent after manual removal");
    disconnect(&engine, &mut rx).await;

    // Setting off, another fresh server: nothing is sent.
    let mut settings = engine.settings().expect("settings");
    settings.send_default_avatar = false;
    engine.update_settings(settings).await.expect("toggle off");
    let second_name = unique("f32-second");
    let second = add_server(&engine, &second_name).await;
    connect(&engine, &mut rx, &second).await;
    tokio::time::sleep(SETTLE).await;
    bot.wait_user(&second_name, &avatar(false))
        .await
        .expect("no avatar with the setting off");
    disconnect(&engine, &mut rx).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v14 (MUMCORD_LIVE=1)"]
async fn remembered_avatar_is_resent_when_the_server_keeps_none() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_remembered_avatar");
    let data = dir.join("data");
    let (engine, mut rx) = Engine::new(
        &data,
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create("Avatar Tester".into())
        .await
        .expect("identity");
    let source = dir.join("me.png");
    std::fs::write(&source, PNG).expect("image");

    let bot = Bot::connect(BotConfig {
        data_dir: Some(dir.join("bot")),
        ..BotConfig::new(&unique("f55-bot"), common::v14())
    })
    .await
    .expect("bot");

    // Unregistered: the avatar set by hand is gone after a disconnect, so Mumcord sends it
    // again on the next connect.
    let guest_name = unique("f55-guest");
    let guest = add_server(&engine, &guest_name).await;
    connect(&engine, &mut rx, &guest).await;
    engine
        .set_own_avatar(only_server(&engine), Some(source.clone()))
        .await
        .expect("set avatar");
    let sent = avatar_hash(&bot, &guest_name).await;
    disconnect(&engine, &mut rx).await;
    bot.wait_user_gone(&guest_name).await.expect("gone");
    connect(&engine, &mut rx, &guest).await;
    assert_eq!(
        avatar_hash(&bot, &guest_name).await,
        sent,
        "avatar sent again after reconnect"
    );
    disconnect(&engine, &mut rx).await;
    bot.wait_user_gone(&guest_name).await.expect("gone");

    // Registered: the server keeps its copy, so nothing is sent. The remembered avatar is
    // swapped behind Mumcord's back; a resend would replace the server's copy with it.
    let member_name = unique("f55-member");
    let member = add_server(&engine, &member_name).await;
    connect(&engine, &mut rx, &member).await;
    engine
        .register_self(only_server(&engine))
        .await
        .expect("register");
    engine
        .set_own_avatar(only_server(&engine), Some(source))
        .await
        .expect("set avatar");
    let kept = avatar_hash(&bot, &member_name).await;
    disconnect(&engine, &mut rx).await;
    bot.wait_user_gone(&member_name).await.expect("gone");
    mumcord_store::Store::open(&data)
        .expect("store")
        .set_server_avatar(member.id, Some(&other_png()))
        .expect("swap remembered avatar");
    connect(&engine, &mut rx, &member).await;
    tokio::time::sleep(SETTLE).await;
    assert_eq!(
        avatar_hash(&bot, &member_name).await,
        kept,
        "server copy kept, nothing re-sent"
    );
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
