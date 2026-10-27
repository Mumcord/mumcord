//! Talking indicators against the local 1.4 test server: only the person actually speaking is
//! reported as talking (FR-1.15, AC-1.7), never the quiet people or listeners in the channel.
//! `testenv/up.sh v14`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_talk -- --ignored --nocapture`
#![cfg(feature = "test-backend")]

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mumbot::voice::{BotVoice, Clip, PlayOptions};
use mumbot::{Bot, BotConfig, env};
use mumcord_engine::EventReceiver;
use mumcord_engine::{EngineEvent, PasswordChange, SecretStore, ServerFields, Snapshot};

const WAIT: Duration = Duration::from_secs(20);
const SAMPLE_RATE: usize = 48_000;

fn live() -> bool {
    let on = std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start testenv/up.sh v14");
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

/// Collects the sessions reported as talking during `window`.
async fn talkers(rx: &mut EventReceiver, window: Duration) -> BTreeSet<u32> {
    let deadline = tokio::time::Instant::now() + window;
    let mut seen = BTreeSet::new();
    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        if let EngineEvent::TalkStateChanged { session, talking } = event {
            eprintln!("talk: session {session} talking {talking}");
            if talking {
                seen.insert(session);
            }
        }
    }
    seen
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v14 (MUMCORD_LIVE=1)"]
async fn only_the_speaker_is_reported_talking() {
    if !live() {
        return;
    }
    let dir = work_dir("engine-live-talk");
    let name = unique("mc-me");
    let speech =
        Clip::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testenv/audio/speech.wav"))
            .expect("speech clip");

    // Microphone: 12 s silence, 3 s speech, then silence.
    let mut mic = vec![0.0; SAMPLE_RATE * 12];
    mic.extend(speech.samples.iter().take(SAMPLE_RATE * 3));
    mic.extend(std::iter::repeat_n(0.0, SAMPLE_RATE * 60));
    let (input, output) = (dir.join("in.wav"), dir.join("out.wav"));
    Clip::from_samples(mic).save(&input).expect("save input");
    // SAFETY: set before any audio thread exists; no other thread reads the environment yet.
    unsafe {
        std::env::set_var(
            "MUMCORD_AUDIO_BACKEND",
            format!("file:{}:{}", input.display(), output.display()),
        );
    }

    let bot = |n: &str| {
        Bot::connect(BotConfig {
            data_dir: Some(dir.join(n)),
            ..BotConfig::new(n, common::v14())
        })
    };
    let talker = bot(&unique("mc-talker")).await.expect("talker");
    let quiet = bot(&unique("mc-quiet")).await.expect("quiet");
    let follower = bot(&unique("mc-follow")).await.expect("follower");
    talker.join("AFK").await.expect("talker joins");
    quiet.join("AFK").await.expect("quiet joins");
    follower.listen(&["AFK"]).expect("follower listens");
    let talker_voice = BotVoice::start(&talker).await.expect("talker voice");
    let _quiet_voice = BotVoice::start(&quiet).await.expect("quiet voice");
    let _follower_voice = BotVoice::start(&follower).await.expect("follower voice");

    let (engine, mut rx) = mumcord_engine::Engine::new(
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
        .server_add(ServerFields {
            label: "v14".into(),
            address: env::HOST.into(),
            port: common::v14(),
            username: name.clone(),
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
    let afk = snapshot
        .channels
        .iter()
        .find(|c| c.name == "AFK")
        .expect("AFK")
        .id;
    engine
        .join_channel(only_server(&engine), afk)
        .expect("join AFK");
    let me = snapshot.self_session;
    let talker_session = talker.own_user().expect("talker user").session;

    // A remote person talks: only that person is flagged.
    let play = talker_voice.play(
        &speech,
        PlayOptions {
            max_duration: Some(Duration::from_secs(2)),
            ..PlayOptions::default()
        },
    );
    let (played, remote) = tokio::join!(play, talkers(&mut rx, Duration::from_secs(5)));
    played.expect("talker plays");
    assert_eq!(remote, BTreeSet::from([talker_session]), "remote talk");

    // We talk (mic speech after 12 s): only we are flagged.
    let own = talkers(&mut rx, Duration::from_secs(12)).await;
    assert_eq!(own, BTreeSet::from([me]), "own talk");
}

/// The test's only connected server (0, which no call accepts, when none is).
fn only_server(engine: &mumcord_engine::Engine) -> mumcord_engine::ServerId {
    engine
        .connections()
        .first()
        .and_then(mumcord_engine::ConnState::server_id)
        .unwrap_or(0)
}
