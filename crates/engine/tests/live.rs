//! Engine against the local 1.5 test server, with the WAV file audio backend and a bot.
//! `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live -- --ignored --test-threads=1`
//!
//! Uses netsim on 127.0.0.1:64800 (proxy) / 64801 (control) in front of v15.
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mumbot::voice::{BotVoice, Clip, PlayOptions};
use mumbot::{Bot, BotConfig, UserCheck, env};
use mumcord_engine::EventReceiver;
use mumcord_engine::{
    ConnState, Engine, EngineEvent, ErrorKind, PasswordChange, SecretStore, ServerFields, Snapshot,
};

const PROXY: &str = "127.0.0.1:64800";
const CONTROL: &str = "127.0.0.1:64801";
const WAIT: Duration = Duration::from_secs(20);

fn live() -> bool {
    let on = std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start testenv/up.sh v15");
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

/// Waits for the first event matching `pick`, skipping others.
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
        if let EngineEvent::StateChanged(state) = &event {
            eprintln!("state: {state:?}");
        }
        if let Some(found) = pick(&event) {
            return found;
        }
    }
}

async fn wait_synced(rx: &mut EventReceiver) -> Snapshot {
    wait_for(rx, "ServerSynced", |e| match e {
        EngineEvent::ServerSynced(s) => Some((**s).clone()),
        EngineEvent::CertTrustRequired { .. } => panic!("unexpected certificate question"),
        _ => None,
    })
    .await
}

async fn wait_own_channel(rx: &mut EventReceiver, session: u32, channel: u32) {
    wait_for(rx, "own user in channel", |e| match e {
        EngineEvent::UserUpserted(u) if u.session == session && u.channel_id == channel => Some(()),
        _ => None,
    })
    .await;
}

fn server_fields(name: &str) -> ServerFields {
    ServerFields {
        label: "v15 via netsim".into(),
        address: "127.0.0.1".into(),
        port: 64800,
        username: name.into(),
        password: match env::testenv_var("SERVER_PASSWORD") {
            Some(p) => PasswordChange::Set(p),
            None => PasswordChange::Keep,
        },
    }
}

fn muted(m: bool) -> UserCheck {
    UserCheck {
        muted: Some(m),
        ..UserCheck::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn tofu_voice_mute_and_reconnect() {
    if !live() {
        return;
    }
    let dir = work_dir("engine-live");
    let name = unique("mc-engine");
    let bot_name = unique("mc-ear");

    // Microphone: one minute of speech; speakers are recorded to out.wav.
    let speech =
        Clip::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testenv/audio/speech.wav"))
            .expect("speech clip");
    let long = Clip::from_samples(speech.samples.repeat(10));
    let (input, output) = (dir.join("in.wav"), dir.join("out.wav"));
    long.save(&input).expect("save input");
    // SAFETY: set before any audio thread exists; no other thread reads the environment yet.
    unsafe {
        std::env::set_var(
            "MUMCORD_AUDIO_BACKEND",
            format!("file:{}:{}", input.display(), output.display()),
        );
    }

    let netsim = netsim::start(
        PROXY.parse().expect("addr"),
        format!("127.0.0.1:{}", common::v15())
            .parse()
            .expect("addr"),
        CONTROL.parse().expect("addr"),
    )
    .await
    .expect("netsim (ports 64800/64801 free?)");

    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create(name.clone(), None)
        .await
        .expect("identity");
    let server = engine
        .server_add(server_fields(&name))
        .await
        .expect("server");

    let bot = Bot::connect(BotConfig {
        data_dir: Some(dir.join("bot")),
        ..BotConfig::new(&bot_name, common::v15())
    })
    .await
    .expect("bot");
    bot.join("Raid").await.expect("bot joins Raid");
    let voice = BotVoice::start(&bot).await.expect("bot voice");

    // Trust on first use: the attempt stops and waits for the decision.
    engine.connect(server.id, None).expect("connect");
    let (fingerprint, previous) = wait_for(&mut rx, "CertTrustRequired", |e| match e {
        EngineEvent::CertTrustRequired {
            fingerprint,
            previous_fingerprint,
            ..
        } => Some((fingerprint.clone(), previous_fingerprint.clone())),
        _ => None,
    })
    .await;
    assert_eq!(previous, None);
    assert!(matches!(
        engine.connection_state(),
        ConnState::Connecting { .. }
    ));
    engine
        .cert_trust_decide(server.id, &fingerprint, true)
        .expect("trust");
    let snapshot = wait_synced(&mut rx).await;
    assert!(matches!(
        engine.connection_state(),
        ConnState::Connected { .. }
    ));
    let raid = snapshot
        .channels
        .iter()
        .find(|c| c.name == "Raid")
        .expect("Raid channel")
        .id;

    // A fresh connect lands read-only: self-muted and deafened, until an explicit join.
    let me = snapshot
        .users
        .iter()
        .find(|u| u.session == snapshot.self_session)
        .expect("own user");
    assert!(
        me.self_mute && me.self_deaf,
        "read-only after a fresh connect"
    );
    bot.wait_user(
        &name,
        &UserCheck {
            deaf: Some(true),
            muted: Some(true),
            ..UserCheck::default()
        },
    )
    .await
    .expect("deafened on the server after connecting");

    engine
        .join_channel(only_server(&engine), raid)
        .expect("join Raid");
    wait_own_channel(&mut rx, snapshot.self_session, raid).await;
    bot.wait_user(&name, &muted(false))
        .await
        .expect("voice on after the explicit join");

    // The bot hears our microphone (WAV through DSP, Opus and the voice transport).
    let hit = voice
        .wait_audio(Some(&name), -40.0, Instant::now(), WAIT)
        .await
        .expect("bot hears the engine");
    eprintln!("bot heard us at {:.1} dB", hit.level_db);

    // We hear the bot: its talking indicator comes from our playback mixer.
    let bot_session = voice.session_of(&bot_name).expect("bot session");
    let play = {
        let clip = speech.clone();
        let voice = &voice;
        async move {
            voice
                .play(
                    &clip,
                    PlayOptions {
                        max_duration: Some(Duration::from_secs(2)),
                        ..PlayOptions::default()
                    },
                )
                .await
        }
    };
    let talk = wait_for(&mut rx, "bot talking", |e| match e {
        EngineEvent::TalkStateChanged { session, talking } if *session == bot_session => {
            Some(*talking)
        }
        _ => None,
    });
    let (played, talking) = tokio::join!(play, talk);
    played.expect("bot plays");
    assert!(talking);

    // Deafen implies mute; undeafen restores "not muted".
    engine.set_self_deaf(true).expect("deafen");
    bot.wait_user(
        &name,
        &UserCheck {
            deaf: Some(true),
            muted: Some(true),
            ..UserCheck::default()
        },
    )
    .await
    .expect("deafened on the server");
    engine.set_self_deaf(false).expect("undeafen");
    bot.wait_user(&name, &muted(false))
        .await
        .expect("unmuted again");

    // Mute, then cut the TCP connection: reconnect, back in Raid, mute restored.
    engine.set_self_mute(true).expect("mute");
    bot.wait_user(&name, &muted(true)).await.expect("muted");
    netsim.controls.kill_tcp();
    wait_for(&mut rx, "Reconnecting", |e| match e {
        EngineEvent::StateChanged(ConnState::Reconnecting { attempt: 1, .. }) => Some(()),
        _ => None,
    })
    .await;
    let snapshot = wait_synced(&mut rx).await;
    wait_own_channel(&mut rx, snapshot.self_session, raid).await;
    bot.wait_user(
        &name,
        &UserCheck {
            in_channel: Some("Raid".into()),
            muted: Some(true),
            ..UserCheck::default()
        },
    )
    .await
    .expect("back in Raid and still muted");

    engine.set_self_mute(false).expect("unmute");
    voice
        .wait_audio(Some(&name), -40.0, Instant::now(), WAIT)
        .await
        .expect("bot hears the engine after reconnect");

    engine.disconnect(only_server(&engine));
    wait_for(&mut rx, "Disconnected", |e| match e {
        EngineEvent::StateChanged(ConnState::Disconnected { error: None }) => Some(()),
        _ => None,
    })
    .await;
    bot.wait_user_gone(&name).await.expect("left the server");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn declining_the_certificate_disconnects() {
    if !live() {
        return;
    }
    let dir = work_dir("engine-live-decline");
    let name = unique("mc-decline");
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create(name.clone(), None)
        .await
        .expect("identity");
    let mut fields = server_fields(&name);
    fields.port = common::v15();
    let server = engine.server_add(fields).await.expect("server");

    engine.connect(server.id, None).expect("connect");
    let fingerprint = wait_for(&mut rx, "CertTrustRequired", |e| match e {
        EngineEvent::CertTrustRequired { fingerprint, .. } => Some(fingerprint.clone()),
        _ => None,
    })
    .await;
    engine
        .cert_trust_decide(server.id, &fingerprint, false)
        .expect("decline");
    let error = wait_for(&mut rx, "Disconnected", |e| match e {
        EngineEvent::StateChanged(ConnState::Disconnected { error }) => Some(error.clone()),
        _ => None,
    })
    .await;
    assert_eq!(error.map(|e| e.kind), Some(ErrorKind::CertificateRejected));
    assert!(engine.snapshot().is_none());
}

/// Connects `server` (trusting its certificate if asked) and returns the synced snapshot.
async fn connect_synced(
    engine: &Engine,
    rx: &mut EventReceiver,
    server: mumcord_engine::ServerId,
) -> Snapshot {
    engine.connect(server, None).expect("connect");
    loop {
        match wait_for(rx, "ServerSynced or CertTrustRequired", |e| match e {
            EngineEvent::ServerSynced(s) => Some(Ok((**s).clone())),
            EngineEvent::CertTrustRequired { fingerprint, .. } => Some(Err(fingerprint.clone())),
            _ => None,
        })
        .await
        {
            Ok(snapshot) => return snapshot,
            Err(fingerprint) => engine
                .cert_trust_decide(server, &fingerprint, true)
                .expect("trust"),
        }
    }
}

async fn disconnected(engine: &Engine, rx: &mut EventReceiver) {
    engine.disconnect(only_server(engine));
    wait_for(rx, "Disconnected", |e| match e {
        EngineEvent::StateChanged(ConnState::Disconnected { error: None }) => Some(()),
        _ => None,
    })
    .await;
}

fn channel_id(snapshot: &Snapshot, name: &str) -> u32 {
    snapshot
        .channels
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("{name} channel"))
        .id
}

/// QA.7 / AC-1.35: a Join after the read-only connect restores the user's last own mute choice,
/// also after an app restart; the forced read-only state never overwrites it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs testenv v14 (MUMCORD_LIVE=1)"]
async fn join_restores_the_last_own_mute_after_a_restart() {
    if !live() {
        return;
    }
    let dir = work_dir("engine-live-own-voice");
    let name = unique("mc-ownvoice");
    let data = dir.join("data");
    // The identity lives in (in-memory) secure storage; `secrets.reopen()` hands the same
    // items to the restarted engine, like the OS store would.
    let secrets = SecretStore::in_memory();
    let bot = Bot::connect(BotConfig {
        data_dir: Some(dir.join("bot")),
        ..BotConfig::new(&unique("mc-ownear"), common::v14())
    })
    .await
    .expect("bot");
    let in_raid = |muted: bool, deaf: bool| UserCheck {
        in_channel: Some("Raid".into()),
        muted: Some(muted),
        deaf: Some(deaf),
        ..UserCheck::default()
    };

    let server = {
        let (engine, mut rx) =
            Engine::new(&data, secrets.reopen(), tokio::runtime::Handle::current())
                .expect("engine");
        engine
            .identity_create(name.clone(), None)
            .await
            .expect("identity");
        let mut fields = server_fields(&name);
        fields.port = common::v14();
        let server = engine.server_add(fields).await.expect("server");
        let snapshot = connect_synced(&engine, &mut rx, server.id).await;
        let raid = channel_id(&snapshot, "Raid");

        // Never muted before: Join turns voice fully on.
        engine
            .join_channel(only_server(&engine), raid)
            .expect("join Raid");
        bot.wait_user(&name, &in_raid(false, false))
            .await
            .expect("voice on after the first join");
        engine.set_self_mute(true).expect("mute");
        bot.wait_user(&name, &in_raid(true, false))
            .await
            .expect("muted");
        disconnected(&engine, &mut rx).await;
        server.id
    };

    // App restart: a new engine on the same data directory and the same secure storage.
    let (engine, mut rx) = Engine::new(&data, secrets.reopen(), tokio::runtime::Handle::current())
        .expect("engine restart");
    let snapshot = connect_synced(&engine, &mut rx, server).await;
    let me = snapshot
        .users
        .iter()
        .find(|u| u.session == snapshot.self_session)
        .expect("own user");
    assert!(me.self_mute && me.self_deaf, "read-only after the restart");
    // Toggling while read-only is no own choice: deafen stays forced, mute is unchanged.
    engine.set_self_mute(true).expect("mute while read-only");

    engine
        .join_channel(only_server(&engine), channel_id(&snapshot, "Raid"))
        .expect("join Raid after restart");
    bot.wait_user(&name, &in_raid(true, false))
        .await
        .expect("still muted, not deafened after the join");

    // Unmuting while joined is remembered for the next join.
    engine.set_self_mute(false).expect("unmute");
    bot.wait_user(&name, &in_raid(false, false))
        .await
        .expect("unmuted");
    disconnected(&engine, &mut rx).await;
    let snapshot = connect_synced(&engine, &mut rx, server).await;
    engine
        .join_channel(only_server(&engine), channel_id(&snapshot, "Raid"))
        .expect("join Raid again");
    bot.wait_user(&name, &in_raid(false, false))
        .await
        .expect("voice on: the unmute was remembered");
    disconnected(&engine, &mut rx).await;
}

async fn restarted_engine(data: &Path, secrets: &SecretStore) -> (Engine, EventReceiver) {
    let (engine, rx) = Engine::new(data, secrets.reopen(), tokio::runtime::Handle::current())
        .expect("engine restart");
    (engine, rx)
}

/// QA.9 / FR-1.14: quitting while joined for voice offers the channel once after the restart;
/// a deliberate disconnect before quitting offers nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs testenv v14 (MUMCORD_LIVE=1)"]
async fn quitting_while_joined_offers_a_rejoin_once_after_the_restart() {
    if !live() {
        return;
    }
    let dir = work_dir("engine-live-rejoin");
    let name = unique("mc-rejoin");
    let data = dir.join("data");
    let secrets = SecretStore::in_memory();
    let bot = Bot::connect(BotConfig {
        data_dir: Some(dir.join("bot")),
        ..BotConfig::new(&unique("mc-rejoinear"), common::v14())
    })
    .await
    .expect("bot");
    let in_raid = UserCheck {
        in_channel: Some("Raid".into()),
        ..UserCheck::default()
    };

    let (server, raid) = {
        let (engine, mut rx) =
            Engine::new(&data, secrets.reopen(), tokio::runtime::Handle::current())
                .expect("engine");
        engine
            .identity_create(name.clone(), None)
            .await
            .expect("identity");
        let mut fields = server_fields(&name);
        fields.port = common::v14();
        let server = engine.server_add(fields).await.expect("server");
        let snapshot = connect_synced(&engine, &mut rx, server.id).await;
        assert!(snapshot.read_only);
        assert_eq!(snapshot.rejoin_channel_id, None, "nothing to offer yet");
        let raid = channel_id(&snapshot, "Raid");
        engine
            .join_channel(only_server(&engine), raid)
            .expect("join Raid");
        bot.wait_user(&name, &in_raid).await.expect("in Raid");
        // Quit while joined.
        engine.shutdown().await;
        wait_for(&mut rx, "Disconnected", |e| match e {
            EngineEvent::StateChanged(ConnState::Disconnected { error: None }) => Some(()),
            _ => None,
        })
        .await;
        (server.id, raid)
    };

    let (engine, mut rx) = restarted_engine(&data, &secrets).await;
    let snapshot = connect_synced(&engine, &mut rx, server).await;
    assert!(snapshot.read_only, "still read-only until Rejoin");
    assert_eq!(snapshot.rejoin_channel_id, Some(raid), "Raid offered");
    let me = snapshot
        .users
        .iter()
        .find(|u| u.session == snapshot.self_session)
        .expect("own user");
    assert_ne!(me.channel_id, raid, "no automatic rejoin");
    disconnected(&engine, &mut rx).await;
    let snapshot = connect_synced(&engine, &mut rx, server).await;
    assert_eq!(
        snapshot.rejoin_channel_id, None,
        "offered once per app start"
    );
    engine
        .join_channel(only_server(&engine), raid)
        .expect("rejoin Raid");
    bot.wait_user(&name, &in_raid).await.expect("back in Raid");
    let snapshot = engine.snapshot().expect("snapshot");
    assert!(!snapshot.read_only, "voice on after the join");
    // Deliberate disconnect, then quit.
    disconnected(&engine, &mut rx).await;
    drop(engine);

    let (engine, mut rx) = restarted_engine(&data, &secrets).await;
    let snapshot = connect_synced(&engine, &mut rx, server).await;
    assert_eq!(
        snapshot.rejoin_channel_id, None,
        "not joined for voice when the app quit"
    );
    disconnected(&engine, &mut rx).await;
}

/// QA.7 / FR-2.2: connecting another server while joined for voice lands read-only there, and
/// a Join there moves voice and restores the own mute/deafen choice from the previous server,
/// not the forced state, also when voice was turned on by undeafening the read-only state
/// instead of a Join.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs testenv v14 and v15 (MUMCORD_LIVE=1)"]
async fn join_after_a_server_switch_restores_the_own_mute() {
    if !live() {
        return;
    }
    let dir = work_dir("engine-live-switch-voice");
    let name = unique("mc-switch");
    let bot_on = |port: u16, dir: PathBuf| {
        Bot::connect(BotConfig {
            data_dir: Some(dir),
            ..BotConfig::new(&unique("mc-switchear"), port)
        })
    };
    let bot14 = bot_on(common::v14(), dir.join("bot14"))
        .await
        .expect("bot v14");
    let bot15 = bot_on(common::v15(), dir.join("bot15"))
        .await
        .expect("bot v15");
    let in_raid = |muted: bool, deaf: bool| UserCheck {
        in_channel: Some("Raid".into()),
        muted: Some(muted),
        deaf: Some(deaf),
        ..UserCheck::default()
    };
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create(name.clone(), None)
        .await
        .expect("identity");
    let mut fields = server_fields(&name);
    fields.port = common::v14();
    let v14 = engine.server_add(fields).await.expect("server v14").id;
    let mut fields = server_fields(&name);
    fields.label = "v15".into();
    fields.port = common::v15();
    let v15 = engine.server_add(fields).await.expect("server v15").id;

    // An earlier session ended deafened while joined.
    let snapshot = connect_synced(&engine, &mut rx, v14).await;
    engine
        .join_channel(v14, channel_id(&snapshot, "Raid"))
        .expect("join Raid on v14");
    engine.set_self_deaf(true).expect("deafen");
    bot14
        .wait_user(&name, &in_raid(true, true))
        .await
        .expect("deafened at v14");
    disconnected(&engine, &mut rx).await;

    // Voice on at v14 by undeafening the read-only state: that is the new own choice.
    connect_synced(&engine, &mut rx, v14).await;
    engine.set_self_deaf(false).expect("undeafen");
    let voice_on = UserCheck {
        muted: Some(false),
        deaf: Some(false),
        ..UserCheck::default()
    };
    bot14
        .wait_user(&name, &voice_on)
        .await
        .expect("voice on at v14");

    // Connect v15 next to v14: it lands read-only, a Join there moves voice (FR-2.2).
    let snapshot = connect_synced(&engine, &mut rx, v15).await;
    assert!(snapshot.read_only, "read-only on the second server");
    engine
        .join_channel(v15, channel_id(&snapshot, "Raid"))
        .expect("join Raid on v15");
    bot15
        .wait_user(&name, &in_raid(false, false))
        .await
        .expect("voice fully on after joining v15");
    let snapshot = engine.snapshot().expect("snapshot");
    let me = snapshot
        .users
        .iter()
        .find(|u| u.session == snapshot.self_session)
        .expect("own user");
    assert!(!me.self_mute && !me.self_deaf, "the app shows voice on");

    // Muted on v15, then switch back: the join on v14 restores muted only.
    engine.set_self_mute(true).expect("mute");
    bot15
        .wait_user(&name, &in_raid(true, false))
        .await
        .expect("muted at v15");
    let snapshot = engine.snapshot_of(v14).expect("v14 still connected");
    assert!(snapshot.read_only, "read-only since voice moved to v15");
    engine
        .join_channel(v14, channel_id(&snapshot, "Raid"))
        .expect("join Raid on v14 again");
    bot14
        .wait_user(&name, &in_raid(true, false))
        .await
        .expect("muted, not deafened after joining v14");
    disconnected(&engine, &mut rx).await;
}

/// The test's only connected server (0, which no call accepts, when none is).
fn only_server(engine: &Engine) -> mumcord_engine::ServerId {
    engine
        .connections()
        .first()
        .and_then(mumcord_engine::ConnState::server_id)
        .unwrap_or(0)
}
