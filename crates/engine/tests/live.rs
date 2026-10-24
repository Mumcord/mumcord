//! Engine against the local 1.5 test server, with the WAV file audio backend and a bot.
//! `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live -- --ignored --test-threads=1`
//!
//! Uses netsim on 127.0.0.1:64800 (proxy) / 64801 (control) in front of v15.
#![cfg(feature = "test-backend")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mumcord_bot::voice::{BotVoice, Clip, PlayOptions};
use mumcord_bot::{Bot, BotConfig, UserCheck, env};
use mumcord_engine::{
    ConnState, Engine, EngineEvent, ErrorKind, PasswordChange, SecretStore, ServerFields, Snapshot,
};
use tokio::sync::mpsc::UnboundedReceiver;

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
        if let EngineEvent::StateChanged(state) = &event {
            eprintln!("state: {state:?}");
        }
        if let Some(found) = pick(&event) {
            return found;
        }
    }
}

async fn wait_synced(rx: &mut UnboundedReceiver<EngineEvent>) -> Snapshot {
    wait_for(rx, "ServerSynced", |e| match e {
        EngineEvent::ServerSynced(s) => Some((**s).clone()),
        EngineEvent::CertTrustRequired { .. } => panic!("unexpected certificate question"),
        _ => None,
    })
    .await
}

async fn wait_own_channel(rx: &mut UnboundedReceiver<EngineEvent>, session: u32, channel: u32) {
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
        format!("127.0.0.1:{}", env::PORT_V15)
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
        .identity_create(name.clone())
        .await
        .expect("identity");
    let server = engine
        .server_add(server_fields(&name))
        .await
        .expect("server");

    let bot = Bot::connect(BotConfig {
        data_dir: Some(dir.join("bot")),
        ..BotConfig::new(&bot_name, env::PORT_V15)
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

    engine.join_channel(raid).expect("join Raid");
    wait_own_channel(&mut rx, snapshot.self_session, raid).await;

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

    engine.disconnect();
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
        .identity_create(name.clone())
        .await
        .expect("identity");
    let mut fields = server_fields(&name);
    fields.port = env::PORT_V15;
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
