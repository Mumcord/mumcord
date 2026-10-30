//! Mumcord features on every connected server, not only the first one (FR-2.4, FR-2.11,
//! FR-2.12; AC-2.11): peers, now playing, friends, private messages and history sync work on
//! a server that was connected second. Uses the test servers v15 (A) and v15b (B) of the
//! testenv instance selected by `MUMCORD_PORT_BASE` (default 64738, B at +4).
//! `testenv/up.sh v15 && testenv/up.sh v15b`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_per_server_features -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::Path;
use std::time::Duration;

use mumbot::{Bot, BotConfig, UserCheck, env};
use mumcord_engine::{
    ConversationRef, Engine, EngineEvent, EventReceiver, LocalTrack, PasswordChange, SecretStore,
    ServerFields, ServerId, Snapshot,
};

const WAIT: Duration = Duration::from_secs(20);

/// Waits for the first event `pick` accepts; also hands it the event's server.
async fn wait_for<T>(
    rx: &mut EventReceiver,
    what: &str,
    mut pick: impl FnMut(Option<ServerId>, &EngineEvent) -> Option<T>,
) -> T {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let (server, event) = tokio::time::timeout_at(deadline, rx.recv_tagged())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
            .expect("event stream open");
        if let Some(found) = pick(server, &event) {
            return found;
        }
    }
}

async fn start_engine(dir: &Path, name: &str) -> (Engine, EventReceiver) {
    let (engine, rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create(name.to_owned(), None)
        .await
        .expect("identity");
    (engine, rx)
}

async fn add_server(engine: &Engine, name: &str, port: u16) -> ServerId {
    engine
        .server_add(ServerFields {
            label: format!("test {port}"),
            address: "127.0.0.1".into(),
            port,
            username: name.to_owned(),
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server")
        .id
}

/// Connects and answers trust questions.
async fn connect(engine: &Engine, rx: &mut EventReceiver, server_id: ServerId) -> Snapshot {
    engine.connect(server_id, None).expect("connect");
    loop {
        let event = wait_for(rx, "sync", |_, e| match e {
            EngineEvent::CertTrustRequired { server_id: id, .. } if *id == server_id => {
                Some(e.clone())
            }
            EngineEvent::ServerSynced(s) if s.server_id == server_id => Some(e.clone()),
            _ => None,
        })
        .await;
        match event {
            EngineEvent::CertTrustRequired { fingerprint, .. } => engine
                .cert_trust_decide(server_id, &fingerprint, true)
                .expect("trust"),
            EngineEvent::ServerSynced(snapshot) => return *snapshot,
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

fn session_of(engine: &Engine, server_id: ServerId, name: &str) -> u32 {
    engine
        .snapshot_of(server_id)
        .expect("synced")
        .users
        .iter()
        .find(|u| u.name == name)
        .unwrap_or_else(|| panic!("{name} not on the server"))
        .session
}

async fn wait_message(rx: &mut EventReceiver, server_id: ServerId, body: &str) -> ConversationRef {
    wait_for(rx, body, |server, e| match e {
        EngineEvent::MessageReceived(m) if m.body.contains(body) => {
            assert_eq!(server, Some(server_id), "message tagged with its server");
            Some(m.conversation.clone())
        }
        _ => None,
    })
    .await
}

async fn wait_friend_online(rx: &mut EventReceiver, cert: &str) -> ServerId {
    wait_for(rx, "friend online", |_, e| match e {
        EngineEvent::FriendsChanged(friends) => friends
            .iter()
            .find(|f| f.cert_hash == cert)
            .and_then(|f| f.online.as_ref())
            .map(|p| p.server_id),
        _ => None,
    })
    .await
}

fn own_cert(engine: &Engine, server_id: ServerId) -> String {
    let snapshot = engine.snapshot_of(server_id).expect("synced");
    snapshot
        .users
        .iter()
        .find(|u| u.session == snapshot.self_session)
        .and_then(|u| u.cert_hash.clone())
        .expect("own certificate")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 and v15b (MUMCORD_LIVE=1)"]
async fn features_work_on_the_second_connected_server() {
    if !std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1") {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start the test servers");
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("live_per_server_features");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    let (port_a, port_b) = (common::v15(), common::v15b());
    let tag = format!("{:06x}", std::process::id() & 0xff_ffff);
    let (x_name, y_name) = (format!("psx-{tag}"), format!("psy-{tag}"));
    let (x, mut x_rx) = start_engine(&dir.join("x"), &x_name).await;
    let (y, mut y_rx) = start_engine(&dir.join("y"), &y_name).await;
    let bot = Bot::connect(BotConfig {
        data_dir: Some(dir.join("bot")),
        ..BotConfig::new(&format!("psbot-{tag}"), port_b)
    })
    .await
    .expect("bot on B");
    bot.join("Lobby").await.expect("bot joins Lobby");

    // X: A first, then B. B is never the earliest server, so everything below runs there.
    let x_a = add_server(&x, &x_name, port_a).await;
    let x_b = add_server(&x, &x_name, port_b).await;
    connect(&x, &mut x_rx, x_a).await;
    let snapshot = connect(&x, &mut x_rx, x_b).await;
    let lobby = channel(&snapshot, "Lobby");
    x.join_channel(x_b, lobby).expect("X joins Lobby on B");
    bot.wait_user(
        &x_name,
        &UserCheck {
            in_channel: Some("Lobby".into()),
            ..UserCheck::default()
        },
    )
    .await
    .expect("X in Lobby");
    for body in ["missed one", "missed two"] {
        bot.send_channel(Some("Lobby"), body).expect("send");
        wait_message(&mut x_rx, x_b, body).await;
    }
    // Y's join must be clearly later than the messages (timestamps have second precision).
    tokio::time::sleep(Duration::from_millis(1100)).await;

    // Y: only B. Joining Lobby syncs the missed messages from X's history on B.
    let y_b = add_server(&y, &y_name, port_b).await;
    connect(&y, &mut y_rx, y_b).await;
    y.join_channel(y_b, lobby).expect("Y joins Lobby");
    let mut synced = Vec::new();
    wait_for(&mut y_rx, "history sync", |server, e| match e {
        EngineEvent::HistorySynced {
            channel_id,
            messages,
        } if *channel_id == lobby => {
            assert_eq!(server, Some(y_b));
            synced.extend(messages.iter().map(|m| m.body.clone()));
            None
        }
        EngineEvent::HistorySyncStatus {
            channel_id,
            running: false,
            ..
        } if *channel_id == lobby => Some(()),
        _ => None,
    })
    .await;
    assert_eq!(synced, ["missed one", "missed two"]);

    // Both know each other as Mumcord users on B.
    let x_on_b = session_of(&y, y_b, &x_name);
    let y_on_b = session_of(&x, x_b, &y_name);
    let deadline = tokio::time::Instant::now() + WAIT;
    while !(y.is_mumcord_user(y_b, x_on_b) && x.is_mumcord_user(x_b, y_on_b)) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "no Mumcord badge on B"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Now playing on X reaches Y on B (AC-2.11).
    let mut settings = x.settings().expect("settings");
    settings.now_playing = true;
    x.update_settings(settings).await.expect("now playing on");
    x.now_playing_update(Some(LocalTrack {
        title: "Second Server Song".into(),
        artist: "Test Band".into(),
        album: None,
        art_url: None,
        position_ms: 1_000,
        duration_ms: Some(180_000),
        playing: true,
    }));
    let title = wait_for(&mut y_rx, "X's track", |server, e| match e {
        EngineEvent::NowPlayingChanged {
            session,
            track: Some(track),
        } if *session == x_on_b => {
            assert_eq!(server, Some(y_b));
            Some(track.title.clone())
        }
        _ => None,
    })
    .await;
    assert_eq!(title, "Second Server Song");

    // Friends: online on B, also for X, who is on A too.
    y.friend_add(y_b, x_on_b).expect("Y adds X");
    assert_eq!(wait_friend_online(&mut y_rx, &own_cert(&x, x_b)).await, y_b);
    x.friend_add(x_b, y_on_b).expect("X adds Y");
    assert_eq!(wait_friend_online(&mut x_rx, &own_cert(&y, y_b)).await, x_b);

    // A private message on B arrives tagged with B, linked by certificate (FR-2.11).
    y.send_private_message(y_b, x_on_b, "pm on the second server")
        .await
        .expect("pm");
    let conversation = wait_message(&mut x_rx, x_b, "pm on the second server").await;
    assert_eq!(
        conversation,
        ConversationRef::Private {
            cert_hash: own_cert(&y, y_b)
        }
    );

    x.disconnect(x_a);
    x.disconnect(x_b);
    y.disconnect(y_b);
}
