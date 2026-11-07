//! Create, edit, move, link and delete channels from Mumcord (Epic 4, issue #250), against the
//! local test server: a SuperUser bot grants the rights in a test room and checks as a second
//! client. `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_channel_admin -- --ignored --test-threads=1`

#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumbot::{Bot, BotConfig, env};
use mumcord_engine::{
    ChannelChange, ChannelCreate, ChannelOutcome, ChannelView, DenialKind, Engine, EngineEvent,
    EventReceiver, PasswordChange, Right, SecretStore, ServerFields,
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

async fn connect(engine: &Engine, rx: &mut EventReceiver) -> mumcord_engine::ServerId {
    let name = unique("mc-chadm");
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
            EngineEvent::ServerSynced(_) => return server.id,
            _ => {}
        }
    }
}

/// Waits until the engine reports `name` with permissions that satisfy `done`.
async fn wait_channel(
    rx: &mut EventReceiver,
    what: &str,
    name: &str,
    mut done: impl FnMut(&ChannelView) -> bool,
) -> ChannelView {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let event = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
            .expect("event stream open");
        let views = match event {
            EngineEvent::ChannelsUpserted(views) => views,
            EngineEvent::ChannelUpserted(view) => vec![view],
            _ => continue,
        };
        if let Some(view) = views.into_iter().find(|v| v.name == name && done(v)) {
            return view;
        }
    }
}

/// Waits until the bot, as a second client, sees what `check` looks for.
async fn bot_sees(bot: &Bot, what: &str, check: impl Fn(&bot_protocol::ServerState) -> bool) {
    let deadline = tokio::time::Instant::now() + WAIT;
    while !check(&bot.state()) {
        assert!(tokio::time::Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The channel a confirmed change names.
fn confirmed(outcome: ChannelOutcome) -> u32 {
    match outcome {
        ChannelOutcome::Confirmed { channel_id } => channel_id,
        other => panic!("expected the server to confirm, got {other:?}"),
    }
}

fn create(parent: u32, name: &str) -> ChannelCreate {
    ChannelCreate {
        parent,
        name: name.to_owned(),
        description: None,
        position: 0,
        max_users: 0,
        temporary: false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the local test server (testenv/up.sh v15) and MUMCORD_LIVE=1"]
async fn create_edit_move_link_and_delete_channels() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_channel_admin");

    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    let server_id = connect(&engine, &mut rx).await;

    // A SuperUser bot makes a room in which everyone may create, edit and link channels.
    let admin = Bot::connect(BotConfig::superuser(common::v15()))
        .await
        .expect("SuperUser bot (SUPERUSER_PASSWORD in testenv/.env)");
    let room = unique("mc-chadm-room");
    admin
        .handle()
        .send_channel_state(proto::ChannelState {
            parent: Some(0),
            name: Some(room.clone()),
            ..Default::default()
        })
        .expect("create room");
    let deadline = tokio::time::Instant::now() + WAIT;
    let room_id = loop {
        if let Ok(id) = admin.channel_id(&room) {
            break id;
        }
        assert!(tokio::time::Instant::now() < deadline, "room not created");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let rights = bot_protocol::permissions::WRITE
        | bot_protocol::permissions::MAKE_CHANNEL
        | bot_protocol::permissions::LINK_CHANNEL;
    admin
        .handle()
        .send_acl(proto::Acl {
            channel_id: room_id,
            inherit_acls: Some(true),
            groups: Vec::new(),
            acls: vec![proto::acl::ChanAcl {
                apply_here: Some(true),
                apply_subs: Some(true),
                inherited: Some(false),
                user_id: None,
                group: Some("all".to_owned()),
                grant: Some(rights),
                deny: Some(0),
            }],
            query: Some(false),
        })
        .expect("grant rights");
    wait_channel(&mut rx, "the rights in the room", &room, |v| {
        v.permissions.make_channel && v.permissions.write && v.permissions.link_channel
    })
    .await;

    // Create two channels, rename one and give it a limit and a description.
    let raid = confirmed(
        engine
            .channel_create(server_id, &create(room_id, "Raid"))
            .await
            .expect("create Raid"),
    );
    let officers = confirmed(
        engine
            .channel_create(server_id, &create(room_id, "Officers"))
            .await
            .expect("create Officers"),
    );
    let edit = ChannelChange {
        name: Some("Raid night".into()),
        description: Some("<b>Tuesdays</b>".into()),
        max_users: Some(3),
        position: Some(1),
        ..Default::default()
    };
    assert_eq!(
        confirmed(
            engine
                .channel_edit(server_id, raid, &edit)
                .await
                .expect("edit")
        ),
        raid
    );
    bot_sees(&admin, "a second client sees the edit", |s| {
        s.channels
            .get(&raid)
            .is_some_and(|c| c.name == "Raid night" && c.max_users == Some(3))
    })
    .await;

    // Link, unlink, and move Raid under Officers.
    let link = ChannelChange {
        links_add: vec![officers],
        ..Default::default()
    };
    confirmed(
        engine
            .channel_edit(server_id, raid, &link)
            .await
            .expect("link"),
    );
    let unlink = ChannelChange {
        links_remove: vec![officers],
        ..Default::default()
    };
    confirmed(
        engine
            .channel_edit(server_id, raid, &unlink)
            .await
            .expect("unlink"),
    );
    let reparent = ChannelChange {
        parent: Some(officers),
        ..Default::default()
    };
    confirmed(
        engine
            .channel_edit(server_id, raid, &reparent)
            .await
            .expect("move"),
    );
    bot_sees(&admin, "a second client sees the move", |s| {
        s.channels
            .get(&raid)
            .is_some_and(|c| c.parent == Some(officers))
    })
    .await;

    // Refusals come back with the server's reason: a name a sibling already has, and a missing
    // right.
    let outcome = engine
        .channel_create(server_id, &create(room_id, "Officers"))
        .await
        .expect("duplicate name");
    assert!(
        matches!(
            outcome,
            ChannelOutcome::Refused {
                kind: DenialKind::ChannelName,
                ..
            }
        ),
        "{outcome:?}"
    );
    let outcome = engine
        .channel_create(server_id, &create(0, &unique("mc-chadm-root")))
        .await
        .expect("create in root");
    assert!(
        matches!(
            outcome,
            ChannelOutcome::Refused {
                permission: Some(Right::MakeChannel),
                ..
            }
        ),
        "{outcome:?}"
    );

    // Deleting Officers takes Raid with it; then the room goes too.
    confirmed(
        engine
            .channel_remove(server_id, officers)
            .await
            .expect("delete"),
    );
    bot_sees(&admin, "a second client sees the delete", |s| {
        !s.channels.contains_key(&officers) && !s.channels.contains_key(&raid)
    })
    .await;
    confirmed(
        engine
            .channel_remove(server_id, room_id)
            .await
            .expect("delete room"),
    );
}
