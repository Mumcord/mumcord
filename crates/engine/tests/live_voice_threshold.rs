//! Manual voice-activation threshold against the local 1.4 test server (FR-1.57, AC-1.47): with
//! the threshold between a quiet and a loud tone, the bot hears only the loud one.
//! `testenv/up.sh v14`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_voice_threshold -- --ignored --nocapture`
#![cfg(feature = "test-backend")]

mod common;

use std::f32::consts::PI;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mumcord_bot::voice::{BotVoice, Clip};
use mumcord_bot::{Bot, BotConfig, env};
use mumcord_engine::EventReceiver;
use mumcord_engine::{EngineEvent, PasswordChange, SecretStore, ServerFields, Snapshot};
use mumcord_store::VoiceActivation;

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

fn tone(seconds: usize, amplitude: f32) -> impl Iterator<Item = f32> {
    (0..SAMPLE_RATE * seconds)
        .map(move |i| amplitude * (2.0 * PI * 440.0 * i as f32 / SAMPLE_RATE as f32).sin())
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v14 (MUMCORD_LIVE=1)"]
async fn manual_threshold_lets_only_the_loud_tone_through() {
    if !live() {
        return;
    }
    let dir = work_dir("engine-live-voice-threshold");
    let name = unique("mc-me");

    // Microphone: 1 s silence, 3 s quiet tone (-46 dBFS), 3 s silence, 3 s loud tone (-12 dBFS).
    let mut mic = vec![0.0; SAMPLE_RATE];
    mic.extend(tone(3, 0.005));
    mic.extend(std::iter::repeat_n(0.0, SAMPLE_RATE * 3));
    mic.extend(tone(3, 0.25));
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

    let listener = Bot::connect(BotConfig {
        data_dir: Some(dir.join("listener")),
        ..BotConfig::new(&unique("mc-listen"), common::v14())
    })
    .await
    .expect("listener");
    listener.join("AFK").await.expect("listener joins");
    let listener_voice = BotVoice::start(&listener).await.expect("listener voice");

    let (engine, mut rx) = mumcord_engine::Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    // Level of the processed signal decides, so the model-based stages stay out of the way.
    let mut settings = engine.settings().expect("settings");
    settings.voice_activation = VoiceActivation::Manual;
    settings.voice_threshold_dbfs = -30;
    settings.auto_gain = false;
    settings.noise_suppression = false;
    settings.echo_cancellation = false;
    engine.update_settings(settings).await.expect("settings");
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
    let started = Instant::now();
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

    // The loud tone arrives; the quiet one before it never opened the gate.
    let hit = listener_voice
        .wait_audio(Some(&name), -30.0, started, Duration::from_secs(30))
        .await
        .expect("loud tone heard");
    assert_eq!(hit.session, me);
    tokio::time::sleep(Duration::from_secs(5)).await;
    let heard = listener_voice.received_duration(me);
    eprintln!("heard {heard:?} of voice");
    assert!(
        heard < Duration::from_millis(5_000),
        "quiet tone leaked: {heard:?}"
    );
    assert!(
        heard > Duration::from_millis(2_000),
        "loud tone cut: {heard:?}"
    );
}

/// The test's only connected server (0, which no call accepts, when none is).
fn only_server(engine: &mumcord_engine::Engine) -> mumcord_engine::ServerId {
    engine
        .connections()
        .first()
        .and_then(mumcord_engine::ConnState::server_id)
        .unwrap_or(0)
}
