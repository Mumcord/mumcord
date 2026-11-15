//! A whisper is bound to the people it was started for, not to their names, against the local
//! test server: only the chosen person hears it, it ends when that person leaves, and somebody
//! who arrives later under the same name hears nothing of it. `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_whisper -- --ignored --test-threads=1`
//!
//! A stock Mumble server never has two connected people with one name: a second login under a
//! name is refused from another address and replaces the older login from the same address.
//! The test uses that replacement; two people with one name at the same moment (possible on
//! other servers) are covered by the unit tests in `whisper.rs`.
// FreeBSD: see the note on the mumbot dev-dependency in Cargo.toml.
#![cfg(all(feature = "test-backend", not(target_os = "freebsd")))]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumbot::voice::{BotVoice, Clip};
use mumbot::{Bot, BotConfig, env};
use mumcord_engine::store::WhisperReceivers;
use mumcord_engine::whisper::{WhisperDenied, WhisperTo};
use mumcord_engine::{
    Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields, Snapshot,
};

const WAIT: Duration = Duration::from_secs(20);
const SAMPLE_RATE: usize = 48_000;
/// Long enough for the voice activation to open and packets to arrive.
const TALK: Duration = Duration::from_secs(4);

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

/// A bot called `name` with its own certificate (kept in `dir`), sitting in `channel`.
async fn bot_in(dir: &Path, name: &str, channel: &str) -> (Bot, BotVoice) {
    let bot = Bot::connect(BotConfig {
        data_dir: Some(dir.to_path_buf()),
        ..BotConfig::new(name, common::v15())
    })
    .await
    .expect("bot connects");
    bot.join(channel).await.expect("bot joins");
    let voice = BotVoice::start(&bot).await.expect("bot voice");
    (bot, voice)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn a_whisper_stays_with_the_person_it_was_started_for() {
    if !live() {
        return;
    }
    let dir = work_dir("engine-live-whisper");
    let speech =
        Clip::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testenv/audio/speech.wav"))
            .expect("speech clip");
    // The microphone hears speech all the time.
    let mic: Vec<f32> = speech
        .samples
        .iter()
        .copied()
        .cycle()
        .take(SAMPLE_RATE * 180)
        .collect();
    let (input, output) = (dir.join("in.wav"), dir.join("out.wav"));
    Clip::from_samples(mic).save(&input).expect("save input");
    // SAFETY: set before any audio thread exists; no other thread reads the environment yet.
    unsafe {
        std::env::set_var(
            "MUMCORD_AUDIO_BACKEND",
            format!("file:{}:{}", input.display(), output.display()),
        );
    }

    // Nobody shares a channel with the user, so only a whisper can reach the bots.
    let target_name = unique("mc-wh-target");
    let (target, target_voice) = bot_in(&dir.join("target"), &target_name, "Raid").await;
    let (bystander, bystander_voice) =
        bot_in(&dir.join("bystander"), &unique("mc-wh-other"), "AFK").await;

    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    let name = unique("mc-wh-me");
    engine
        .identity_create(name.clone(), None)
        .await
        .expect("identity");
    let server = engine
        .server_add(ServerFields {
            label: "v15".into(),
            address: env::HOST.into(),
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
    let snapshot: Snapshot = wait_for(&mut rx, "synced", |e| match e {
        EngineEvent::CertTrustRequired { fingerprint, .. } => {
            engine
                .cert_trust_decide(server.id, fingerprint, true)
                .expect("trust");
            None
        }
        EngineEvent::ServerSynced(s) => Some((**s).clone()),
        _ => None,
    })
    .await;
    let lobby = snapshot
        .channels
        .iter()
        .find(|c| c.name == "Lobby")
        .expect("Lobby")
        .id;
    engine.join_channel(server.id, lobby).expect("join Lobby");
    let me = snapshot.self_session;
    // Joining a channel takes voice there; the server confirms the unmute.
    wait_for(&mut rx, "the own unmute", |e| match e {
        EngineEvent::UserUpserted(u)
            if u.session == me && u.channel_id == lobby && !u.self_mute && !u.self_deaf =>
        {
            Some(())
        }
        _ => None,
    })
    .await;
    let target_session = target.own_user().expect("target user").session;

    // Voice goes through the voice server only.
    assert_eq!(
        engine.whisper_start_to(server.id + 1000, &WhisperTo::Sessions(vec![target_session])),
        Err(WhisperDenied::NoVoiceServer)
    );

    // By session: the chosen person hears it, nobody else.
    engine
        .whisper_start_to(server.id, &WhisperTo::Sessions(vec![target_session]))
        .expect("whisper to the target");
    tokio::time::sleep(TALK).await;
    assert!(
        target_voice.packet_count(me) > 0,
        "the target hears the whisper"
    );
    assert_eq!(
        bystander_voice.packet_count(me),
        0,
        "nobody else hears the whisper"
    );

    // The target leaves: the whisper ends by itself.
    drop(target_voice);
    target.disconnect().await.expect("target leaves");
    wait_for(&mut rx, "the whisper to end", |e| {
        matches!(e, EngineEvent::WhisperEnded).then_some(())
    })
    .await;

    // Somebody else arrives under the same name: nothing of the old whisper reaches them.
    let (_newcomer, newcomer_voice) = bot_in(&dir.join("newcomer"), &target_name, "Raid").await;
    wait_for(&mut rx, "the newcomer to appear", |e| match e {
        EngineEvent::UserUpserted(u) if u.name == target_name => Some(()),
        _ => None,
    })
    .await;
    tokio::time::sleep(TALK).await;
    assert_eq!(
        newcomer_voice.packet_count(me),
        0,
        "a person with the same name is not the person the whisper was for"
    );

    // A saved target names the person: it is looked up when the whisper starts.
    engine
        .whisper_start(&WhisperReceivers::People {
            names: vec![target_name.clone()],
        })
        .expect("whisper by name");
    tokio::time::sleep(TALK).await;
    assert!(
        newcomer_voice.packet_count(me) > 0,
        "the person carrying the name now hears the new whisper"
    );
    assert_eq!(bystander_voice.packet_count(me), 0);

    // While that whisper runs, a third person logs in under the name. The server gives the
    // name to the new login and drops the older one, so the whisper ends; the person who
    // took the name over hears nothing.
    let (twin, twin_voice) = bot_in(&dir.join("twin"), &target_name, "Raid").await;
    wait_for(&mut rx, "the whisper to end", |e| {
        matches!(e, EngineEvent::WhisperEnded).then_some(())
    })
    .await;
    tokio::time::sleep(TALK).await;
    assert_eq!(
        twin_voice.packet_count(me),
        0,
        "taking a name over does not take the whisper over"
    );
    assert_eq!(bystander_voice.packet_count(me), 0);

    // Two people: when one of them leaves, the whisper goes on for the other.
    let twin_session = twin.own_user().expect("twin user").session;
    let bystander_session = bystander.own_user().expect("bystander user").session;
    engine
        .whisper_start_to(
            server.id,
            &WhisperTo::Sessions(vec![twin_session, bystander_session]),
        )
        .expect("whisper to two people");
    tokio::time::sleep(TALK).await;
    assert!(twin_voice.packet_count(me) > 0);
    drop(twin_voice);
    twin.disconnect().await.expect("twin leaves");
    wait_for(&mut rx, "the twin to leave", |e| match e {
        EngineEvent::WhisperEnded => panic!("the whisper still has a listener"),
        EngineEvent::UserRemoved(session) if *session == twin_session => Some(()),
        _ => None,
    })
    .await;
    let heard = bystander_voice.packet_count(me);
    tokio::time::sleep(TALK).await;
    assert!(
        bystander_voice.packet_count(me) > heard,
        "the person who stayed still hears the whisper"
    );

    engine.whisper_stop();
    // After the stop nothing more is addressed to them.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let heard = bystander_voice.packet_count(me);
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        bystander_voice.packet_count(me),
        heard,
        "the whisper stopped"
    );
    engine.disconnect(server.id);
}
