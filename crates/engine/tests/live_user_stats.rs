//! Connection figures, client and OS of people from UserStats (FR-2.25, FR-2.27, AC-2.19)
//! against the local test server. The user logs in as SuperUser because the server sends a
//! person's client and OS only to admins. `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_user_stats -- --ignored --test-threads=1`
//! (`MUMCORD_PORT_BASE` selects another testenv instance, as in `testenv/env.sh`).
#![cfg(feature = "test-backend")]

mod common;

use std::path::Path;
use std::time::Duration;

use mumcord_bot::{Bot, BotConfig, env};
use mumcord_engine::{
    Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields, UserStatsView,
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

async fn next_stats(rx: &mut EventReceiver) -> UserStatsView {
    wait_for(rx, "user stats", |e| match e {
        EngineEvent::UserStatsChanged(s) => Some(s.clone()),
        _ => None,
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn stats_client_and_os_of_a_person_arrive_paced() {
    if !std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1") {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start the test servers");
        return;
    }
    let port = common::v15();
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("live_user_stats");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create("Stats Tester".into())
        .await
        .expect("identity");
    let password =
        env::testenv_var("SUPERUSER_PASSWORD").expect("SUPERUSER_PASSWORD in testenv/.env");
    let server = engine
        .server_add(ServerFields {
            label: format!("test {port} stats"),
            address: "127.0.0.1".into(),
            port,
            username: "SuperUser".into(),
            password: PasswordChange::Set(password),
        })
        .await
        .expect("server");
    engine.connect(server.id, None).expect("connect");
    loop {
        let event = wait_for(&mut rx, "sync", |e| match e {
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
            _ => break,
        }
    }
    let own = engine.snapshot().expect("snapshot").self_session;

    let name = format!("b13-{:06x}", std::process::id() & 0xff_ffff);
    let bot = Bot::connect(BotConfig::new(&name, port))
        .await
        .expect("bot");
    let bot_session = bot.own_user().expect("bot user").session;
    // The server only hands out the ping a client reported in its own keep-alive pings (every
    // 5 s): the first one measures, the next one carries the figure (F26).
    tokio::time::sleep(Duration::from_secs(11)).await;

    engine
        .user_stats_watch(server.id, vec![bot_session, own])
        .expect("watch");
    // Both people at once: the second answer comes one request gap after the first.
    let first = next_stats(&mut rx).await;
    let at = tokio::time::Instant::now();
    let second = next_stats(&mut rx).await;
    assert!(
        at.elapsed() >= Duration::from_millis(1500),
        "second answer came after {:?}",
        at.elapsed()
    );
    let (bot_stats, own_stats) = if first.session == bot_session {
        (first, second)
    } else {
        (second, first)
    };
    assert_eq!((bot_stats.session, own_stats.session), (bot_session, own));
    // Both in the root channel: everything is there; the bot is the Mumcord protocol client.
    assert!(
        bot_stats
            .client_release
            .as_deref()
            .is_some_and(|r| r.starts_with("Mumcord ")),
        "{bot_stats:?}"
    );
    assert!(bot_stats.client_version.is_some(), "{bot_stats:?}");
    assert_eq!(bot_stats.os.as_deref(), Some(std::env::consts::OS));
    assert!(bot_stats.ping_ms.is_some_and(|p| p > 0.0), "{bot_stats:?}");
    assert!(own_stats.ping_ms.is_some_and(|p| p > 0.0), "{own_stats:?}");
    assert!(bot_stats.transport.is_some(), "{bot_stats:?}");
    // The bot has no voice link, so no packets were counted: loss is unknown, not 0 %.
    assert_eq!(bot_stats.loss_percent, None, "{bot_stats:?}");
    assert!(bot_stats.bandwidth_kbps.is_some(), "{bot_stats:?}");
    assert!(own_stats.client_release.is_some(), "{own_stats:?}");

    // Nothing more within the 30 s refresh window once both were fetched.
    let quiet = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(EngineEvent::UserStatsChanged(s)) = rx.recv().await {
                return s;
            }
        }
    })
    .await;
    assert!(quiet.is_err(), "unexpected refresh: {quiet:?}");

    engine
        .user_stats_watch(server.id, Vec::new())
        .expect("stop");
    drop(bot);
    engine.disconnect(server.id);
}
