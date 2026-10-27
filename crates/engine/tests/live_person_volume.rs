//! Local volume of one person against the local 1.5 test server: the slider value (FR-1.18,
//! 0–200 %) scales that person's voice in the played-back mix.
//! `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_person_volume -- --ignored --nocapture`
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumbot::voice::{BotVoice, Clip, PlayOptions};
use mumbot::{Bot, BotConfig, env};
use mumcord_engine::EventReceiver;
use mumcord_engine::{EngineEvent, PasswordChange, SecretStore, ServerFields, Snapshot};

const WAIT: Duration = Duration::from_secs(20);
const SAMPLE_RATE: usize = 48_000;
/// Time per volume step; the recording's header is refreshed once a second, so the last
/// readable second always lies well inside the step.
const STEP: Duration = Duration::from_secs(4);

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

/// RMS of the last second recorded so far.
fn recent_rms(recording: &Path) -> f32 {
    let samples = Clip::load(recording).expect("recording").samples;
    let tail = &samples[samples.len().saturating_sub(SAMPLE_RATE)..];
    (tail.iter().map(|s| s * s).sum::<f32>() / tail.len().max(1) as f32).sqrt()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn person_volume_scales_their_voice() {
    if !live() {
        return;
    }
    let dir = work_dir("engine-live-person-volume");
    let name = unique("mc-me");
    let (input, output) = (dir.join("in.wav"), dir.join("out.wav"));
    Clip::from_samples(vec![0.0; SAMPLE_RATE * 60])
        .save(&input)
        .expect("save input");
    // SAFETY: set before any audio thread exists; no other thread reads the environment yet.
    unsafe {
        std::env::set_var(
            "MUMCORD_AUDIO_BACKEND",
            format!("file:{}:{}", input.display(), output.display()),
        );
    }

    let speaker = Bot::connect(BotConfig {
        data_dir: Some(dir.join("speaker")),
        ..BotConfig::new(&unique("mc-tone"), common::v15())
    })
    .await
    .expect("speaker");
    speaker.join("AFK").await.expect("speaker joins");
    let speaker_voice = BotVoice::start(&speaker).await.expect("speaker voice");
    // A steady 440 Hz tone at -14 dBFS: 200 % stays below the limiter ceiling.
    let tone = Clip::from_samples(
        (0..SAMPLE_RATE)
            .map(|i| 0.2 * (i as f32 * 440.0 * std::f32::consts::TAU / SAMPLE_RATE as f32).sin())
            .collect(),
    );

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
            label: "v15".into(),
            address: env::HOST.into(),
            port: common::v15(),
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
    engine.join_channel(server.id, afk).expect("join AFK");
    let session = speaker.own_user().expect("speaker user").session;

    let play = speaker_voice.play(
        &tone,
        PlayOptions {
            forever: true,
            max_duration: Some(STEP * 5),
            ..PlayOptions::default()
        },
    );
    let measure = async {
        let mut levels = Vec::new();
        for volume in [100, 25, 200, 100] {
            engine
                .person_set_volume(server.id, session, volume)
                .expect("set volume");
            tokio::time::sleep(STEP).await;
            let rms = recent_rms(&output);
            eprintln!("volume {volume} %: rms {rms:.4}");
            levels.push(rms);
        }
        levels
    };
    let (played, levels) = tokio::join!(play, measure);
    played.expect("speaker plays");

    let normal = levels[0];
    assert!(normal > 0.05, "tone audible at 100 %: {normal}");
    assert!(
        (levels[3] / normal - 1.0).abs() < 0.15,
        "back to 100 %: {levels:?}"
    );
    // The slider follows a cubic curve (F9): 25 % is 0.25³ of the level, about −36 dB.
    assert!(
        (levels[1] / normal - 0.25f32.powi(3)).abs() < 0.005,
        "25 %: {levels:?}"
    );
    // 200 % would be 2³ = 8×, but the output limiter caps the loud tone; at least +9 dB must remain.
    assert!(levels[2] / normal > 2.8, "200 %: {levels:?}");
}
