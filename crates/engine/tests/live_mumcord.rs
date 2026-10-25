//! Mumcord badge (FR-1.61, AC-1.52) against the local test server: two engines recognise each
//! other through the plugin-data hello, a bot (an official-protocol client) never gets the
//! badge. `testenv/up.sh v14`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_mumcord -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumcord_bot::{Bot, BotConfig, env};
use mumcord_engine::{Engine, EngineEvent, PasswordChange, SecretStore, ServerFields, ServerView};
use tokio::sync::mpsc::UnboundedReceiver;

const PORT: u16 = 64739;
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
    rx: &mut UnboundedReceiver<EngineEvent>,
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
            label: format!("test {PORT} {name}"),
            address: "127.0.0.1".into(),
            port: PORT,
            username: name.into(),
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server")
}

async fn connect(engine: &Engine, rx: &mut UnboundedReceiver<EngineEvent>, server: &ServerView) {
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

async fn start(dir: &Path, who: &str) -> (Engine, UnboundedReceiver<EngineEvent>, String) {
    let (engine, mut rx) = Engine::new(
        &dir.join(who),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create(format!("{who} Tester"))
        .await
        .expect("identity");
    let name = unique(who);
    let server = add_server(&engine, &name).await;
    connect(&engine, &mut rx, &server).await;
    (engine, rx, name)
}

fn session_of(engine: &Engine, name: &str) -> u32 {
    engine
        .snapshot()
        .expect("synced")
        .users
        .iter()
        .find(|u| u.name == name)
        .unwrap_or_else(|| panic!("{name} not on the server"))
        .session
}

/// Waits until `name` is known with (`true`) the Mumcord features.
async fn wait_badge(rx: &mut UnboundedReceiver<EngineEvent>, name: &str) -> Vec<String> {
    wait_for(rx, "a Mumcord user", |e| match e {
        EngineEvent::UserUpserted(u) if u.name == name => u.mumcord_features.clone(),
        _ => None,
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v14 (MUMCORD_LIVE=1)"]
async fn mumcord_clients_see_each_other_but_not_official_ones() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_mumcord");
    let bot = Bot::connect(BotConfig {
        data_dir: Some(dir.join("bot")),
        ..BotConfig::new(&unique("b51-bot"), PORT)
    })
    .await
    .expect("bot");
    let bot_name = bot.own_user().expect("bot user").name;

    let (a, mut a_rx, a_name) = start(&dir, "b51a").await;
    let (b, mut b_rx, b_name) = start(&dir, "b51b").await;

    let features = wait_badge(&mut a_rx, &b_name).await;
    assert!(features.iter().any(|f| f == "now-playing"));
    wait_badge(&mut b_rx, &a_name).await;

    let a_session = session_of(&a, &a_name);
    assert!(b.is_mumcord_user(a_session));
    assert!(!b.is_mumcord_user(session_of(&b, &bot_name)));
    assert!(!a.is_mumcord_user(session_of(&a, &bot_name)));

    // A reconnect of B (new session) is recognised again.
    b.disconnect();
    wait_for(&mut a_rx, "B leaving", |e| match e {
        EngineEvent::UserRemoved(_) => Some(()),
        _ => None,
    })
    .await;
    let (_b2, _b2_rx, b2_name) = start(&dir, "b51c").await;
    wait_badge(&mut a_rx, &b2_name).await;
}

/// Regression (B85): a Mumcord hello republishes the person while another thread reads the
/// snapshot (as the in-game overlay does on every engine event). Both paths take the `core` and
/// `store` locks; in opposite order they deadlocked and the session stopped emitting events.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v14 (MUMCORD_LIVE=1)"]
async fn hellos_while_reading_snapshots_do_not_deadlock() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_mumcord_locks");
    // A deadlock blocks runtime threads for good, so neither a timeout nor the runtime shutdown
    // would ever end the test: fail the whole process instead of hanging.
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let done = done.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(90));
            if !done.load(std::sync::atomic::Ordering::Relaxed) {
                eprintln!("deadlock: no progress within 90 s");
                std::process::exit(1);
            }
        });
    }
    let (a, mut a_rx, _) = start(&dir, "b85a").await;

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader = {
        let (a, stop) = (a.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = a.snapshot();
            }
        })
    };
    for round in 0..5 {
        let (b, _b_rx, b_name) = start(&dir, &format!("b85b{round}")).await;
        wait_badge(&mut a_rx, &b_name).await;
        b.disconnect();
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    reader.join().expect("snapshot reader");
    done.store(true, std::sync::atomic::Ordering::Relaxed);
}
