//! Server-side mute, deafen and suppression of other people reach the UI's user view, against
//! the local test server with a SuperUser bot as the admin. `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_moderation -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumbot::{Bot, BotConfig, Moderation, env};
use mumcord_engine::EventReceiver;
use mumcord_engine::{
    Engine, EngineEvent, PasswordChange, SecretStore, ServerFields, Snapshot, UserView,
};
// mumbot pins its own mumcord-protocol release (the `bot_protocol` alias), so bot-side messages use its types.
use bot_protocol::proto;

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
    let name = unique("mc-mod");
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

async fn wait_user(
    rx: &mut EventReceiver,
    what: &str,
    name: &str,
    mut done: impl FnMut(&UserView) -> bool,
) -> UserView {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
            .expect("event stream open");
        if let EngineEvent::UserUpserted(view) = event
            && view.name == name
            && done(&view)
        {
            return view;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the local test server (testenv/up.sh v15) and MUMCORD_LIVE=1"]
async fn server_mute_deafen_and_suppress_reach_the_user_view() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_moderation");

    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    connect(&engine, &mut rx).await;

    let target_name = unique("mc-mod-target");
    let mut config = BotConfig::new(&target_name, common::v15());
    config.data_dir = Some(dir.join("bot"));
    let target = Bot::connect(config).await.expect("target bot");
    let view = wait_user(&mut rx, "target appears", &target_name, |_| true).await;
    assert!(!view.server_mute && !view.server_deaf && !view.suppressed);

    let admin = Bot::connect(BotConfig::superuser(common::v15()))
        .await
        .expect("SuperUser bot (SUPERUSER_PASSWORD in testenv/.env)");
    let moderate = |m: Moderation| admin.moderate(&target_name, m).expect("moderate");

    moderate(Moderation {
        mute: Some(true),
        ..Default::default()
    });
    let view = wait_user(&mut rx, "server mute", &target_name, |v| v.server_mute).await;
    assert!(!view.self_mute && !view.suppressed && !view.server_deaf);

    moderate(Moderation {
        deaf: Some(true),
        ..Default::default()
    });
    let view = wait_user(&mut rx, "server deafen", &target_name, |v| v.server_deaf).await;
    assert!(view.server_mute && !view.suppressed && !view.self_deaf);

    moderate(Moderation {
        mute: Some(false),
        deaf: Some(false),
    });
    wait_user(&mut rx, "lifted", &target_name, |v| {
        !v.server_mute && !v.server_deaf
    })
    .await;

    // Suppression comes from a missing Speak permission in the channel (servers refuse to
    // suppress on request). A temporary channel moves its creator in and disappears with it.
    let room = unique("mc-mod-quiet");
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
    admin.deny_speak(&room).await.expect("deny speak");
    admin.move_user(&target_name, &room).expect("move");
    let view = wait_user(&mut rx, "suppressed", &target_name, |v| v.suppressed).await;
    assert!(!view.server_mute, "suppression is not a server mute");

    admin.move_user(&target_name, "Root/").expect("move back");
    wait_user(&mut rx, "suppression lifted", &target_name, |v| {
        !v.suppressed
    })
    .await;

    engine.disconnect(only_server(&engine));
    admin.handle().disconnect();
    target.handle().disconnect();
}

/// The test's only connected server (0, which no call accepts, when none is).
fn only_server(engine: &Engine) -> mumcord_engine::ServerId {
    engine
        .connections()
        .first()
        .and_then(mumcord_engine::ConnState::server_id)
        .unwrap_or(0)
}

/// Waits until the bot, as the person acted on, sees its own state change.
async fn bot_sees(bot: &Bot, what: &str, check: impl Fn(&bot_protocol::User) -> bool) {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let state = bot.state();
        let me = state.session.and_then(|s| state.users.get(&s).cloned());
        if me.as_ref().is_some_and(&check) {
            return;
        }
        assert!(tokio::time::Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Mumcord as the moderator (issue #257): in a room where everyone may mute others, it server-mutes,
/// deafens, lifts both, makes priority speaker and un-suppresses a second client, which sees each
/// change itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the local test server (testenv/up.sh v15) and MUMCORD_LIVE=1"]
async fn mumcord_mutes_deafens_prioritises_and_unsuppresses() {
    use mumcord_engine::ModerateAction;
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_moderation_actions");
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    connect(&engine, &mut rx).await;
    let server_id = only_server(&engine);

    let admin = Bot::connect(BotConfig::superuser(common::v15()))
        .await
        .expect("SuperUser bot (SUPERUSER_PASSWORD in testenv/.env)");
    // The server refuses moderation in temporary channels, so this is one fixed room, created on
    // the first run and reused afterwards. Everyone may mute others there and nobody may speak, so
    // the target is suppressed.
    let room = "mc-mod-room".to_owned();
    if admin.channel_id(&room).is_err() {
        admin
            .handle()
            .send_channel_state(proto::ChannelState {
                parent: Some(0),
                name: Some(room.clone()),
                ..Default::default()
            })
            .expect("create room");
    }
    let deadline = tokio::time::Instant::now() + WAIT;
    let room_id = loop {
        if let Ok(id) = admin.channel_id(&room) {
            break id;
        }
        assert!(tokio::time::Instant::now() < deadline, "room not created");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let rule = |grant: u32, deny: u32| proto::acl::ChanAcl {
        apply_here: Some(true),
        apply_subs: Some(true),
        inherited: Some(false),
        user_id: None,
        group: Some("all".to_owned()),
        grant: Some(grant),
        deny: Some(deny),
    };
    admin
        .handle()
        .send_acl(proto::Acl {
            channel_id: room_id,
            inherit_acls: Some(true),
            groups: Vec::new(),
            acls: vec![rule(
                bot_protocol::permissions::MUTE_DEAFEN,
                bot_protocol::permissions::SPEAK,
            )],
            query: Some(false),
        })
        .expect("room rules");

    let target_name = unique("mc-mod-target");
    let mut config = BotConfig::new(&target_name, common::v15());
    config.data_dir = Some(dir.join("bot"));
    let target = Bot::connect(config).await.expect("target bot");
    let deadline = tokio::time::Instant::now() + WAIT;
    while !admin.state().users.values().any(|u| u.name == target_name) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "admin never saw the target"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    admin.move_user(&target_name, &room).expect("move target");
    let view = wait_user(
        &mut rx,
        "target suppressed in the room",
        &target_name,
        |v| v.suppressed,
    )
    .await;
    let session = view.session;
    let act = |action| {
        engine
            .person_moderate(server_id, session, action)
            .expect("moderate");
    };

    act(ModerateAction::Mute(true));
    bot_sees(&target, "server mute", |u| u.mute).await;
    act(ModerateAction::Deafen(true));
    bot_sees(&target, "server deafen mutes too", |u| u.deaf && u.mute).await;
    act(ModerateAction::Mute(false));
    bot_sees(&target, "unmute lifts both", |u| !u.deaf && !u.mute).await;
    act(ModerateAction::PrioritySpeaker(true));
    bot_sees(&target, "priority speaker", |u| u.priority_speaker).await;
    wait_user(&mut rx, "priority speaker in the view", &target_name, |v| {
        v.priority_speaker
    })
    .await;
    act(ModerateAction::Unsuppress);
    bot_sees(&target, "un-suppressed", |u| !u.suppress).await;

    engine.disconnect(server_id);
    target.handle().disconnect();
    admin.handle().disconnect();
}
