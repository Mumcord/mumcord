//! Channel passwords as Mumble access tokens (FR-2.78, AC-2.59) against the local test server:
//! a SuperUser bot creates a channel only the token group `#secret` may enter.
//! `testenv/up.sh v15` (SuperUser bot needs SUPERUSER_PASSWORD in testenv/.env), then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_channel_password -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::time::Duration;

use mumbot::{Bot, BotConfig};
use mumcord_engine::{
    ChannelPasswordState, Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore,
    ServerFields, ServerId,
};
use mumcord_protocol::proto;

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

/// No password prompt arrives within `quiet`.
async fn no_prompt_for(rx: &mut EventReceiver, quiet: Duration) {
    let deadline = tokio::time::Instant::now() + quiet;
    while let Ok(event) = tokio::time::timeout_at(deadline, rx.recv()).await {
        if let Some(EngineEvent::ChannelPassword { channel_id, state }) = event {
            panic!("unexpected password prompt {state:?} for channel {channel_id}");
        }
    }
}

/// Connects (trusting the test server's certificate when asked) and waits for the sync.
async fn connect(engine: &Engine, rx: &mut EventReceiver, server: ServerId) {
    engine.connect(server, None).expect("connect");
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
            _ => return,
        }
    }
}

async fn prompt(rx: &mut EventReceiver, what: &str, channel: u32) -> ChannelPasswordState {
    wait_for(rx, what, |e| match e {
        EngineEvent::ChannelPassword { channel_id, state } if *channel_id == channel => {
            Some(*state)
        }
        _ => None,
    })
    .await
}

async fn arrive(rx: &mut EventReceiver, what: &str, own: u32, channel: u32) {
    wait_for(rx, what, |e| match e {
        EngineEvent::UserUpserted(u) if u.session == own && u.channel_id == channel => Some(()),
        _ => None,
    })
    .await;
}

fn own_channel(engine: &Engine, server: ServerId) -> (u32, u32) {
    let snapshot = engine.snapshot_of(server).expect("snapshot");
    let own = snapshot.self_session;
    let channel = snapshot
        .users
        .iter()
        .find(|u| u.session == own)
        .expect("own user")
        .channel_id;
    (own, channel)
}

/// The token removal after a refusal runs in the background; give it a moment.
async fn count_becomes(engine: &Engine, server: ServerId, expected: usize) {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let count = engine.channel_passwords(server).await.expect("list").len();
        if count == expected {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "stored channel passwords stayed at {count}, expected {expected}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 with SUPERUSER_PASSWORD (MUMCORD_LIVE=1)"]
async fn channel_password_is_asked_stored_and_sent_on_reconnect() {
    if !std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1") {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start the test servers");
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("live_channel_password");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    let tag = format!("{:06x}", std::process::id() & 0xff_ffff);
    engine
        .identity_create(format!("Token Tester {tag}"), None)
        .await
        .expect("identity");
    let server = engine
        .server_add(ServerFields {
            label: "test tokens".into(),
            address: "127.0.0.1".into(),
            port: common::v15(),
            username: format!("b63-{tag}"),
            password: PasswordChange::Keep,
        })
        .await
        .expect("server")
        .id;
    connect(&engine, &mut rx, server).await;
    let (own, start) = own_channel(&engine, server);

    // A temporary channel moves its creator in and disappears once it is empty again.
    let admin = Bot::connect(BotConfig::superuser(common::v15()))
        .await
        .expect("SuperUser bot (SUPERUSER_PASSWORD in testenv/.env)");
    let room = format!("mc-b63-{tag}");
    admin
        .handle()
        .send_channel_state(proto::ChannelState {
            parent: Some(0),
            name: Some(room.clone()),
            temporary: Some(true),
            ..Default::default()
        })
        .expect("create channel");
    let deadline = tokio::time::Instant::now() + WAIT;
    while admin.channel_id(&room).is_err() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "channel not created"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let room_id = admin.channel_id(&room).expect("room");
    admin.require_token(&room, "secret").await.expect("ACL");

    // The lock: the server tells the client it may not enter.
    wait_for(&mut rx, "locked channel", |e| match e {
        EngineEvent::ChannelUpserted(c) if c.id == room_id && !c.permissions.enter => Some(()),
        _ => None,
    })
    .await;

    // The admin sits in the locked channel. Murmur refuses the statistics of someone there with
    // an Enter denial for that channel; nobody tried to join, so that never asks for a password.
    let admin_session = engine
        .snapshot_of(server)
        .expect("snapshot")
        .users
        .iter()
        .find(|u| u.channel_id == room_id && u.session != own)
        .expect("admin in the locked channel")
        .session;
    engine
        .user_stats_watch(server, vec![admin_session])
        .expect("watch stats");
    wait_for(&mut rx, "statistics refused", |e| match e {
        EngineEvent::ServerDenied {
            channel_id: Some(id),
            ..
        } if *id == room_id => Some(()),
        EngineEvent::ChannelPassword { .. } => panic!("asked for a password without a join"),
        _ => None,
    })
    .await;
    engine
        .user_stats_watch(server, Vec::new())
        .expect("unwatch");
    no_prompt_for(&mut rx, Duration::from_secs(1)).await;

    engine.join_channel(server, room_id).expect("join");
    assert_eq!(
        prompt(&mut rx, "password prompt", room_id).await,
        ChannelPasswordState::Required
    );

    engine
        .channel_join_with_password(server, room_id, "wrong".into())
        .await
        .expect("send wrong");
    assert_eq!(
        prompt(&mut rx, "wrong password", room_id).await,
        ChannelPasswordState::Wrong
    );
    assert_eq!(own_channel(&engine, server).1, start, "still outside");
    count_becomes(&engine, server, 0).await;

    engine
        .channel_join_with_password(server, room_id, "secret".into())
        .await
        .expect("send secret");
    assert_eq!(
        prompt(&mut rx, "accepted", room_id).await,
        ChannelPasswordState::Ended
    );
    assert_eq!(own_channel(&engine, server).1, room_id, "moved in");
    count_becomes(&engine, server, 1).await;

    // The password remembers the channel it was entered for (issue #146); saving it runs in
    // the background.
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let saved = engine.channel_passwords(server).await.expect("list");
        if let Some(channel) = saved[0].channels.first() {
            assert_eq!(channel.channel_id, room_id);
            assert_eq!(channel.path, std::slice::from_ref(&room));
            assert_eq!(channel.found, Some(true));
            assert!(saved[0].last_used_at.is_some(), "last use");
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the channel was not saved with the password"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Renamed on the server: the password keeps the old name and shows it as not found.
    admin
        .handle()
        .send_channel_state(proto::ChannelState {
            channel_id: Some(room_id),
            name: Some(format!("{room}-renamed")),
            ..Default::default()
        })
        .expect("rename channel");
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let saved = engine.channel_passwords(server).await.expect("list");
        if saved[0].channels[0].found == Some(false) {
            assert_eq!(saved[0].channels[0].path, std::slice::from_ref(&room));
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the renamed channel still counts as found"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Reconnect: the stored token goes out with the login, so joining works without asking.
    engine.join_channel(server, start).expect("leave");
    arrive(&mut rx, "back outside", own, start).await;
    engine.disconnect(server);
    connect(&engine, &mut rx, server).await;
    let snapshot = engine.snapshot_of(server).expect("snapshot");
    let (own, start) = own_channel(&engine, server);
    assert!(
        snapshot
            .channels
            .iter()
            .any(|c| c.id == room_id && c.permissions.enter),
        "no lock after reconnecting with the token"
    );
    engine.join_channel(server, room_id).expect("join again");
    arrive(&mut rx, "joined with the stored token", own, room_id).await;

    // Removing the token asks again on the next join.
    engine
        .channel_passwords_remove(server, vec![0])
        .await
        .expect("remove");
    count_becomes(&engine, server, 0).await;
    engine.join_channel(server, start).expect("leave again");
    arrive(&mut rx, "outside again", own, start).await;
    engine
        .join_channel(server, room_id)
        .expect("join without token");
    assert_eq!(
        prompt(&mut rx, "asked again", room_id).await,
        ChannelPasswordState::Required
    );

    engine.disconnect(server);
    admin.handle().disconnect();
}
