//! "Now playing" (FR-1.58, AC-1.49) against the local test server: the engine shares a track
//! as plugin data and as a comment line, a bot (standing in for another client) receives both,
//! and turning the setting off removes them again. The bot's own payload reaches the engine as
//! a `NowPlayingChanged` event. `testenv/up.sh v14`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_now_playing -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumbot::{Bot, BotConfig, UserCheck, env};
use mumcord_engine::EventReceiver;
use mumcord_engine::now_playing::{DATA_ID, SharedTrack, payload};
use mumcord_engine::{
    Engine, EngineEvent, LocalTrack, PasswordChange, SecretStore, ServerFields, ServerView,
};
use mumcord_protocol::Event;
use tokio::sync::broadcast;

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

/// The next now-playing payload the bot receives.
async fn next_payload(events: &mut broadcast::Receiver<Event>) -> Option<SharedTrack> {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("timed out waiting for plugin data")
            .expect("bot events open");
        if let Event::PluginData(data) = event
            && data.data_id == DATA_ID
        {
            return payload::decode(&data.data).expect("valid payload");
        }
    }
}

/// Waits until the user's comment, as the bot sees it, satisfies `check`.
async fn wait_comment(bot: &Bot, name: &str, check: impl Fn(&str) -> bool) -> String {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let comment = bot
            .state()
            .users
            .values()
            .find(|u| u.name == name)
            .and_then(|u| u.comment.clone())
            .unwrap_or_default();
        if check(&comment) {
            return comment;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "comment never matched; last: {comment:?}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn track() -> LocalTrack {
    LocalTrack {
        title: "Test Song".into(),
        artist: "Test Band".into(),
        album: Some("Test Record".into()),
        art_url: Some("https://example.org/cover.jpg".into()),
        position_ms: 30_000,
        duration_ms: Some(180_000),
        playing: true,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v14 (MUMCORD_LIVE=1)"]
async fn now_playing_reaches_others_and_goes_away_when_turned_off() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_now_playing");
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create("Now Playing Tester".into(), None)
        .await
        .expect("identity");
    let mut settings = engine.settings().expect("settings");
    assert!(!settings.now_playing, "off by default");
    settings.now_playing = true;
    engine.update_settings(settings).await.expect("turn on");

    let bot = Bot::connect(BotConfig {
        data_dir: Some(dir.join("bot")),
        ..BotConfig::new(&unique("b45-bot"), common::v14())
    })
    .await
    .expect("bot");
    let mut bot_events = bot.subscribe();

    let name = unique("b45-np");
    let server = add_server(&engine, &name).await;
    connect(&engine, &mut rx, &server).await;
    bot.wait_user(&name, &UserCheck::default())
        .await
        .expect("bot sees the user");

    // Stand-in for the app shell's poller.
    let poller = {
        let engine = engine.clone();
        tokio::spawn(async move {
            let started = tokio::time::Instant::now();
            loop {
                let mut current = track();
                current.position_ms += u64::try_from(started.elapsed().as_millis()).unwrap_or(0);
                engine.now_playing_update(Some(current));
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        })
    };

    let shared = next_payload(&mut bot_events).await.expect("a track");
    assert_eq!(shared.title, "Test Song");
    assert_eq!(shared.artist, "Test Band");
    assert_eq!(shared.art.as_deref(), Some("https://example.org/cover.jpg"));
    assert_eq!(shared.duration_ms, Some(180_000));
    assert!(shared.playing && shared.position_ms >= 30_000);

    // The UI gets the own track as an event with the own session (it never comes back from
    // the server), for the own rows, preview, card and user panel.
    let own_session = engine_session(&engine);
    let own_track = wait_for(&mut rx, "the own track", |e| match e {
        EngineEvent::NowPlayingChanged { session, track } if *session == own_session => {
            Some(track.clone())
        }
        _ => None,
    })
    .await
    .expect("own track shown");
    assert_eq!(own_track.title, "Test Song");
    assert!(own_track.has_cover);

    // Official clients: the comment line, next to what the user wrote.
    engine
        .set_own_comment(only_server(&engine), "Hello there".into())
        .await
        .expect("comment");
    let comment = wait_comment(&bot, &name, |c| c.contains("🎵 Test Band – Test Song")).await;
    assert!(comment.starts_with("Hello there"), "{comment}");
    let own = engine
        .person_profile(only_server(&engine), engine_session(&engine))
        .await
        .expect("own profile");
    assert_eq!(
        own.comment.as_deref(),
        Some("Hello there"),
        "edited without the line"
    );

    // The bot shares a track too: the engine reports it.
    let bot_session = bot.own_user().expect("bot user").session;
    let data = payload::encode(Some(&SharedTrack {
        title: "Bot Song".into(),
        artist: "Bot".into(),
        album: None,
        art: None,
        position_ms: 1000,
        duration_ms: Some(60_000),
        playing: true,
        ts_ms: 0,
    }));
    bot.handle()
        .send_plugin_data(&[own_session], DATA_ID, data)
        .expect("bot sends");
    let seen = wait_for(&mut rx, "the bot's track", |e| match e {
        EngineEvent::NowPlayingChanged { session, track } if *session == bot_session => {
            Some(track.clone())
        }
        _ => None,
    })
    .await
    .expect("a track");
    assert_eq!(
        (seen.title.as_str(), seen.artist.as_str()),
        ("Bot Song", "Bot")
    );

    // Off: stop payload and the line disappears, the user's text stays.
    let mut settings = engine.settings().expect("settings");
    settings.now_playing = false;
    engine.update_settings(settings).await.expect("turn off");
    let mut stopped = false;
    for _ in 0..5 {
        if next_payload(&mut bot_events).await.is_none() {
            stopped = true;
            break;
        }
    }
    assert!(stopped, "stop payload sent");
    wait_comment(&bot, &name, |c| c == "Hello there").await;

    poller.abort();
    engine.disconnect(only_server(&engine));
}

fn engine_session(engine: &Engine) -> u32 {
    engine
        .session_ctx_for(only_server(engine))
        .expect("connected")
        .handle
        .with_state(|s| s.session)
        .expect("own session")
}

/// The test's only connected server (0, which no call accepts, when none is).
fn only_server(engine: &Engine) -> mumcord_engine::ServerId {
    engine
        .connections()
        .first()
        .and_then(mumcord_engine::ConnState::server_id)
        .unwrap_or(0)
}
