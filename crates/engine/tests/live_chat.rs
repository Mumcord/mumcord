//! Chat, follow, history and profile (B14) against the local test servers, with bots.
//! `testenv/up.sh v15` (plus `limit` and `v13` for the follow error tests), then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_chat -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumcord_bot::samples::{PNG, Sample};
use mumcord_bot::{Bot, BotConfig, MessageCheck, UserCheck, env};
use mumcord_engine::{
    ChatMessageView, ConversationRef, Engine, EngineEvent, ErrorKind, HistoryScope, PasswordChange,
    SecretStore, ServerFields, ServerView, Snapshot,
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

fn null_audio() {
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
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

async fn wait_unread(
    rx: &mut UnboundedReceiver<EngineEvent>,
    conversation: &ConversationRef,
) -> u32 {
    wait_for(rx, "UnreadChanged", |e| match e {
        EngineEvent::UnreadChanged(u) if &u.conversation == conversation => Some(u.count),
        _ => None,
    })
    .await
}

async fn start_engine(dir: &Path) -> (Engine, UnboundedReceiver<EngineEvent>) {
    Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine")
}

async fn add_server(engine: &Engine, name: &str, port: u16) -> ServerView {
    engine
        .server_add(ServerFields {
            label: format!("test {port}"),
            address: "127.0.0.1".into(),
            port,
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

fn bot_config(dir: &Path, name: &str, port: u16) -> BotConfig {
    BotConfig {
        data_dir: Some(dir.join("bot")),
        ..BotConfig::new(name, port)
    }
}

fn listening(channel: &str) -> UserCheck {
    UserCheck {
        listening: Some(channel.into()),
        ..UserCheck::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn chat_follow_history_and_profile() {
    if !live() {
        return;
    }
    null_audio();
    let dir = work_dir("engine-live-chat");
    let name = unique("mc-chat");
    let bot_name = unique("mc-pal");
    let (engine, mut rx) = start_engine(&dir).await;
    engine
        .identity_create(name.clone())
        .await
        .expect("identity");
    let server = add_server(&engine, &name, env::PORT_V15).await;
    let bot = Bot::connect(bot_config(&dir, &bot_name, env::PORT_V15))
        .await
        .expect("bot");
    bot.join("Lobby").await.expect("bot joins Lobby");
    let bot_session = bot.own_user().expect("bot user").session;
    let bot_hash = bot
        .own_user()
        .expect("bot user")
        .cert_hash
        .expect("bot cert");

    let snapshot = connect(&engine, &mut rx, &server).await;
    let (lobby, raid) = (channel(&snapshot, "Lobby"), channel(&snapshot, "Raid"));
    let lobby_conv = ConversationRef::Channel { channel_id: lobby };
    let raid_conv = ConversationRef::Channel { channel_id: raid };
    engine.join_channel(lobby).expect("join Lobby");
    bot.wait_user(
        &name,
        &UserCheck {
            in_channel: Some("Lobby".into()),
            ..UserCheck::default()
        },
    )
    .await
    .expect("we are in Lobby");

    // Incoming channel messages are sanitized (AC-1.13).
    bot.send_channel(Some("Lobby"), &Sample::Rich.html())
        .expect("rich");
    let rich = wait_message(&mut rx, "bold text").await;
    assert_eq!(rich.conversation, lobby_conv);
    assert_eq!(rich.sender_name, bot_name);
    assert!(!rich.own);
    assert!(rich.body.contains("<b>bold text</b>") && rich.body.contains("data:image/png;base64,"));
    bot.send_channel(Some("Lobby"), &Sample::Script.html())
        .expect("script");
    let script = wait_message(&mut rx, "before").await;
    assert!(!script.body.contains("script") && !script.body.contains("alert"));
    bot.send_channel(Some("Lobby"), &Sample::ExternalImage.html())
        .expect("external");
    let external = wait_message(&mut rx, "remote image").await;
    assert!(!external.body.contains("tracker.example"));

    // Own channel message reaches the bot and is returned stored.
    let sent = engine
        .send_channel_message(lobby, "<b>hello</b> from mumcord")
        .expect("send");
    assert!(sent.own && sent.id.parse::<i64>().is_ok());
    bot.wait_message(&MessageCheck {
        from: Some(name.clone()),
        contains: Some("from mumcord".into()),
        private: Some(false),
        channel: Some("Lobby".into()),
    })
    .await
    .expect("bot receives channel message");

    // Private messages both ways, with an unread badge (AC-1.18).
    let private_conv = ConversationRef::Private {
        cert_hash: bot_hash.clone(),
    };
    bot.send_private(&name, "psst private").expect("pm");
    let pm = wait_message(&mut rx, "psst private").await;
    assert_eq!(pm.conversation, private_conv);
    assert_eq!(wait_unread(&mut rx, &private_conv).await, 1);
    engine
        .send_private_message(bot_session, "private reply")
        .expect("reply");
    bot.wait_message(&MessageCheck {
        from: Some(name.clone()),
        contains: Some("private reply".into()),
        private: Some(true),
        channel: None,
    })
    .await
    .expect("bot receives reply");
    engine
        .conversation_mark_read(private_conv.clone())
        .expect("mark read");
    assert_eq!(wait_unread(&mut rx, &private_conv).await, 0);

    // Too large messages are refused locally (FR-1.30).
    let limit = snapshot.info.max_message_length as usize;
    if limit > 0 {
        let err = engine
            .send_channel_message(lobby, &"x".repeat(limit + 1))
            .expect_err("too large");
        assert_eq!(err.kind, ErrorKind::MessageTooLarge);
    }

    // Follow Raid while staying in Lobby (AC-1.15).
    bot.join("Raid").await.expect("bot joins Raid");
    engine.channel_follow(raid).await.expect("follow Raid");
    let followed = wait_for(&mut rx, "FollowedChannelsChanged", |e| match e {
        EngineEvent::FollowedChannelsChanged(ids) => Some(ids.clone()),
        _ => None,
    })
    .await;
    assert_eq!(followed, vec![raid]);
    bot.wait_user(&name, &listening("Raid"))
        .await
        .expect("bot sees the ear icon");
    bot.send_channel(Some("Raid"), "raid news")
        .expect("raid msg");
    let news = wait_message(&mut rx, "raid news").await;
    assert_eq!(news.conversation, raid_conv);
    assert_eq!(wait_unread(&mut rx, &raid_conv).await, 1);

    // Comment, avatar and registration are visible to the bot (AC-1.19, AC-1.20).
    engine
        .set_own_comment("<b>my</b> mumcord comment".into())
        .await
        .expect("comment");
    bot.wait_user(
        &name,
        &UserCheck {
            comment_contains: Some("mumcord comment".into()),
            ..UserCheck::default()
        },
    )
    .await
    .expect("bot sees comment");
    let avatar = dir.join("avatar.png");
    std::fs::write(&avatar, PNG).expect("avatar file");
    engine.set_own_avatar(Some(avatar)).await.expect("avatar");
    bot.wait_user(
        &name,
        &UserCheck {
            has_avatar: Some(true),
            ..UserCheck::default()
        },
    )
    .await
    .expect("bot sees avatar");
    engine.register_self().await.expect("register");
    bot.wait_user(
        &name,
        &UserCheck {
            registered: Some(true),
            ..UserCheck::default()
        },
    )
    .await
    .expect("bot sees registration");

    // History, paged backwards.
    let lobby_history = engine
        .history_load(lobby_conv.clone(), None, 50)
        .expect("history");
    assert_eq!(lobby_history.len(), 4);
    assert!(lobby_history.last().expect("last").own);
    let older = engine
        .history_load(lobby_conv.clone(), Some(&lobby_history[2].id), 50)
        .expect("page");
    assert_eq!(older, lobby_history[..2].to_vec());

    // Restart: history, follows and unread badges come back (AC-1.17).
    let pkcs12 = dir.join("identity.p12");
    engine
        .identity_export(pkcs12.clone(), "pw".into())
        .await
        .expect("export");
    engine.disconnect();
    drop(engine);
    drop(rx);
    let (engine, mut rx) = start_engine(&dir).await;
    engine
        .identity_import(pkcs12, Some("pw".into()))
        .await
        .expect("import");
    let snapshot = connect(&engine, &mut rx, &server).await;
    assert_eq!(snapshot.followed_channel_ids, vec![raid]);
    assert!(
        snapshot
            .unread
            .iter()
            .any(|u| u.conversation == raid_conv && u.count == 1),
        "unread restored: {:?}",
        snapshot.unread
    );
    bot.wait_user(&name, &listening("Raid"))
        .await
        .expect("follow restored");
    let private_history = engine
        .history_load(private_conv.clone(), None, 50)
        .expect("private history");
    let bodies: Vec<_> = private_history
        .iter()
        .map(|m| (m.own, m.body.as_str()))
        .collect();
    assert_eq!(bodies, [(false, "psst private"), (true, "private reply")]);
    assert_eq!(
        engine
            .history_load(lobby_conv.clone(), None, 50)
            .expect("lobby")
            .len(),
        4
    );

    // Clear one conversation; with saving off nothing new is kept (FR-1.33).
    engine
        .history_clear(HistoryScope::Conversation {
            server_id: server.id,
            conversation: lobby_conv.clone(),
        })
        .expect("clear");
    assert!(
        engine
            .history_load(lobby_conv.clone(), None, 50)
            .expect("empty")
            .is_empty()
    );
    engine
        .set_save_history(server.id, false)
        .expect("saving off");
    bot.send_channel(Some("Raid"), "not kept")
        .expect("raid msg");
    let transient = wait_message(&mut rx, "not kept").await;
    assert!(transient.id.starts_with("tmp-"));
    let raid_history = engine
        .history_load(raid_conv.clone(), None, 50)
        .expect("raid");
    assert!(raid_history.iter().all(|m| !m.body.contains("not kept")));
    engine.set_save_history(server.id, true).expect("saving on");
    engine.history_clear(HistoryScope::All).expect("clear all");
    assert!(
        engine
            .history_load(private_conv, None, 50)
            .expect("empty")
            .is_empty()
    );

    // Unfollow stops listening.
    engine.channel_unfollow(raid).expect("unfollow");
    bot.wait_user(
        &name,
        &UserCheck {
            not_listening: Some("Raid".into()),
            ..UserCheck::default()
        },
    )
    .await
    .expect("ear icon gone");
    engine.disconnect();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv limit (MUMCORD_LIVE=1)"]
async fn follow_refused_when_listener_limit_reached() {
    if !live() {
        return;
    }
    null_audio();
    let dir = work_dir("engine-live-limit");
    let name = unique("mc-limit");
    let (engine, mut rx) = start_engine(&dir).await;
    engine
        .identity_create(name.clone())
        .await
        .expect("identity");
    let server = add_server(&engine, &name, env::PORT_LIMIT).await;
    let snapshot = connect(&engine, &mut rx, &server).await;
    let err = engine
        .channel_follow(channel(&snapshot, "Raid"))
        .await
        .expect_err("refused");
    assert_eq!(err.kind, ErrorKind::ListenerLimitReached, "{err}");
    engine.disconnect();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v13 (MUMCORD_LIVE=1)"]
async fn follow_needs_server_1_4() {
    if !live() {
        return;
    }
    null_audio();
    let dir = work_dir("engine-live-v13");
    let name = unique("mc-old");
    let (engine, mut rx) = start_engine(&dir).await;
    engine
        .identity_create(name.clone())
        .await
        .expect("identity");
    let server = add_server(&engine, &name, env::PORT_V13).await;
    let snapshot = connect(&engine, &mut rx, &server).await;
    assert!(!snapshot.info.follow_supported);
    let err = engine
        .channel_follow(channel(&snapshot, "Raid"))
        .await
        .expect_err("too old");
    assert_eq!(err.kind, ErrorKind::ServerTooOld);
    // Chat still works on old servers.
    engine
        .send_channel_message(channel(&snapshot, "Lobby"), "hello old server")
        .expect("send");
    engine.disconnect();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn follow_all_and_unfollow_all() {
    if !live() {
        return;
    }
    null_audio();
    let dir = work_dir("engine-live-follow-all");
    let name = unique("mc-all");
    let (engine, mut rx) = start_engine(&dir).await;
    engine
        .identity_create(name.clone())
        .await
        .expect("identity");
    let server = add_server(&engine, &name, env::PORT_V15).await;
    let snapshot = connect(&engine, &mut rx, &server).await;
    let own = snapshot
        .users
        .iter()
        .find(|u| u.session == snapshot.self_session)
        .map(|u| u.channel_id)
        .expect("own user");

    let summary = engine.channel_follow_all().await.expect("follow all");
    let followed = wait_for(&mut rx, "FollowedChannelsChanged", |e| match e {
        EngineEvent::FollowedChannelsChanged(ids) => Some(ids.clone()),
        _ => None,
    })
    .await;
    assert!(summary.followed > 0, "{summary:?}");
    assert_eq!(summary.limit_reached + summary.no_answer, 0, "{summary:?}");
    assert_eq!(followed.len(), summary.followed as usize);
    assert!(!followed.contains(&own), "own voice channel is skipped");
    assert_eq!(
        (summary.followed + summary.not_allowed) as usize,
        snapshot.channels.len() - 1
    );

    // A second run finds nothing new.
    let again = engine.channel_follow_all().await.expect("follow all again");
    assert_eq!(again.followed, 0, "{again:?}");

    engine.channel_unfollow_all().expect("unfollow all");
    let followed = wait_for(&mut rx, "FollowedChannelsChanged", |e| match e {
        EngineEvent::FollowedChannelsChanged(ids) => Some(ids.clone()),
        _ => None,
    })
    .await;
    assert!(followed.is_empty());
    engine.disconnect();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv limit (MUMCORD_LIVE=1)"]
async fn follow_all_stops_at_the_listener_limit() {
    if !live() {
        return;
    }
    null_audio();
    let dir = work_dir("engine-live-limit-all");
    let name = unique("mc-limall");
    let (engine, mut rx) = start_engine(&dir).await;
    engine
        .identity_create(name.clone())
        .await
        .expect("identity");
    let server = add_server(&engine, &name, env::PORT_LIMIT).await;
    let snapshot = connect(&engine, &mut rx, &server).await;
    let summary = engine.channel_follow_all().await.expect("follow all");
    assert_eq!(summary.followed, 0, "{summary:?}");
    assert!(summary.limit_reached > 0, "{summary:?}");
    assert_eq!(summary.no_answer, 0, "{summary:?}");
    assert_eq!(
        (summary.limit_reached + summary.not_allowed) as usize,
        snapshot.channels.len() - 1
    );
    engine.disconnect();
}
