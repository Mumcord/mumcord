//! History sync between two Mumcord engines against the local test server (AC-1.51,
//! AC-1.51c). `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_history_sync -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumcord_bot::{Bot, BotConfig, UserCheck, env};
use mumcord_engine::{
    ChatMessageView, Engine, EngineEvent, PasswordChange, SecretStore, ServerFields, ServerView,
    Snapshot,
};
use tokio::sync::mpsc::UnboundedReceiver;

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

async fn wait_message(rx: &mut UnboundedReceiver<EngineEvent>, contains: &str) -> ChatMessageView {
    wait_for(rx, contains, |e| match e {
        EngineEvent::MessageReceived(m) if m.body.contains(contains) => Some((**m).clone()),
        _ => None,
    })
    .await
}

/// Collects synced messages of `channel` until its sync ends; returns them and the count.
async fn wait_sync(
    rx: &mut UnboundedReceiver<EngineEvent>,
    channel: u32,
) -> (Vec<ChatMessageView>, u32) {
    let mut synced = Vec::new();
    let added = wait_for(rx, "history sync", |e| match e {
        EngineEvent::HistorySynced {
            channel_id,
            messages,
        } if *channel_id == channel => {
            synced.extend(messages.iter().cloned());
            None
        }
        EngineEvent::HistorySyncStatus {
            channel_id,
            running: false,
            added,
        } if *channel_id == channel => Some(*added),
        _ => None,
    })
    .await;
    (synced, added)
}

async fn start_engine(dir: &Path, name: &str) -> (Engine, UnboundedReceiver<EngineEvent>) {
    let (engine, rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create(name.to_owned())
        .await
        .expect("identity");
    (engine, rx)
}

async fn add_server(engine: &Engine, name: &str) -> ServerView {
    engine
        .server_add(ServerFields {
            label: "test".into(),
            address: "127.0.0.1".into(),
            port: env::PORT_V15,
            username: name.into(),
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server")
}

/// Connects, trusting the server certificate on first use.
async fn connect(
    engine: &Engine,
    rx: &mut UnboundedReceiver<EngineEvent>,
    server: &ServerView,
) -> Snapshot {
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
            EngineEvent::ServerSynced(s) => return *s,
            _ => unreachable!(),
        }
    }
}

fn channel(snapshot: &Snapshot, name: &str) -> u32 {
    snapshot
        .channels
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("channel {name}"))
        .id
}

fn in_channel(name: &str) -> UserCheck {
    UserCheck {
        in_channel: Some(name.into()),
        ..UserCheck::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn late_joiner_receives_missed_lobby_messages() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("engine-live-history-sync");
    let (a_name, b_name, bot_name) = (unique("mc-hsa"), unique("mc-hsb"), unique("mc-hsbot"));
    let (a, mut a_rx) = start_engine(&dir.join("a"), &a_name).await;
    let (b, mut b_rx) = start_engine(&dir.join("b"), &b_name).await;
    let bot = Bot::connect(BotConfig {
        data_dir: Some(dir.join("bot")),
        ..BotConfig::new(&bot_name, env::PORT_V15)
    })
    .await
    .expect("bot");
    bot.join("Lobby").await.expect("bot joins Lobby");

    // A stays in Lobby while messages are sent; one private message too.
    let a_server = add_server(&a, &a_name).await;
    let snapshot = connect(&a, &mut a_rx, &a_server).await;
    let lobby = channel(&snapshot, "Lobby");
    a.join_channel(lobby).expect("A joins Lobby");
    bot.wait_user(&a_name, &in_channel("Lobby"))
        .await
        .expect("A in Lobby");
    for body in ["first missed", "second missed", "third missed"] {
        bot.send_channel(Some("Lobby"), body).expect("send");
        wait_message(&mut a_rx, body).await;
    }
    bot.send_private(&a_name, "private psst").expect("pm");
    wait_message(&mut a_rx, "private psst").await;
    // B's join must be clearly later than the messages (timestamps have second precision).
    tokio::time::sleep(Duration::from_millis(1100)).await;

    // B joins later and gets the missed messages from A's history, in order.
    let b_server = add_server(&b, &b_name).await;
    connect(&b, &mut b_rx, &b_server).await;
    b.join_channel(lobby).expect("B joins Lobby");
    let (synced, added) = wait_sync(&mut b_rx, lobby).await;
    let bodies: Vec<&str> = synced.iter().map(|m| m.body.as_str()).collect();
    assert_eq!(bodies, ["first missed", "second missed", "third missed"]);
    assert_eq!(added, 3);
    assert!(
        synced
            .iter()
            .all(|m| m.synced_from.as_deref() == Some(a_name.as_str()))
    );
    assert!(synced.iter().all(|m| m.sender_name == bot_name));
    // The stored copies keep their origin, so the marker survives reopening the chat.
    let stored = b
        .history_load(
            mumcord_engine::ConversationRef::Channel { channel_id: lobby },
            None,
            50,
        )
        .expect("history");
    assert_eq!(
        stored
            .iter()
            .filter(|m| m.synced_from.as_deref() == Some(a_name.as_str()))
            .count(),
        3
    );

    // A message after B's join arrives live and is not synced again by "load earlier".
    bot.send_channel(Some("Lobby"), "live after join")
        .expect("send");
    wait_message(&mut b_rx, "live after join").await;
    b.history_sync_request(lobby, None).expect("load earlier");
    let (again, added) = wait_sync(&mut b_rx, lobby).await;
    assert!(again.is_empty(), "nothing new: {again:?}");
    assert_eq!(added, 0);

    // With sharing off on A, B gets nothing.
    let mut settings = a.settings().expect("settings");
    settings.history_sharing = false;
    a.update_settings(settings).await.expect("save");
    b.history_clear(mumcord_engine::HistoryScope::All)
        .expect("clear B");
    b.history_sync_request(lobby, None).expect("load earlier");
    let (none, added) = wait_sync(&mut b_rx, lobby).await;
    assert!(none.is_empty());
    assert_eq!(added, 0);

    a.disconnect();
    b.disconnect();
}
