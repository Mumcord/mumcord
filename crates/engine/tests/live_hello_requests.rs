//! Hello requests (issue #647): someone who never says hello is asked for one three times, then
//! counts as a plain Mumble client. Against the local test server; `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_hello_requests -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumbot::{Bot, BotConfig, env};
use mumcord_engine::EventReceiver;
use mumcord_engine::{Engine, EngineEvent, PasswordChange, SecretStore, ServerFields, Snapshot};

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
    let name = unique("mc-hello");
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
#[ignore = "needs the local test server (testenv/up.sh v15) and MUMCORD_LIVE=1; takes ~2.5 minutes"]
async fn a_client_without_hello_is_asked_three_times_then_left_alone() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_hello_requests");
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    connect(&engine, &mut rx).await;

    // A plain bot never says hello, like an official Mumble client.
    let name = unique("mc-hello-plain");
    let mut config = BotConfig::new(&name, common::v15());
    config.data_dir = Some(dir.join("bot"));
    let bot = Bot::connect(config).await.expect("plain bot");
    let mut events = bot.subscribe();

    let (mut greetings, mut requests) = (0, 0);
    // The requests come 10, 40 and 130 s after the bot appeared; wait a little longer than that.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(150);
    while let Ok(event) = tokio::time::timeout_at(deadline, events.recv()).await {
        let Ok(mumbot::protocol::Event::PluginData(data)) = event else {
            continue;
        };
        if data.data_id != mumcord_engine::mumcord::DATA_ID {
            continue;
        }
        let text = String::from_utf8_lossy(&data.data);
        if text.contains("\"ask\":true") {
            requests += 1;
        } else {
            greetings += 1;
        }
    }
    assert_eq!(greetings, 1, "one greeting when the bot joined");
    assert_eq!(
        requests, 3,
        "three requests, then the bot counts as a plain Mumble client"
    );
    bot.handle().disconnect();
}
