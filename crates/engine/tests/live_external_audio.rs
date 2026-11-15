//! The platform-fed audio backend against the local 1.5 test server (Epic 5, FR-5.25, FR-5.26):
//! two engines whose audio is supplied by the app, as on a phone. What one app's microphone
//! callback delivers leaves as voice packets and comes out of the other app's speaker callback,
//! with a bounded delay; the runtime bitrate limit shows in the connection info and lifts again.
//! `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --test live_external_audio -- --ignored --nocapture`
#![cfg(not(target_os = "freebsd"))]

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::host::{Host, SAMPLE_RATE, rms, speech_like};
use mumbot::env;
use mumcord_engine::{
    Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields, ServerId,
    Snapshot,
};

const WAIT: Duration = Duration::from_secs(20);
/// Longest time from the microphone callback of one app to the speaker callback of the other
/// on a local server. The real figure is around 100 ms (20 ms packets, jitter buffer, playback
/// queue); the bound leaves room for a loaded test machine.
const MAX_DELAY: Duration = Duration::from_millis(500);
/// The limit of "Reduced voice quality" on mobile data (FR-5.26).
const MOBILE_DATA_KBPS: u32 = 24;

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
        .map_or(0, |d| d.subsec_nanos());
    format!("{prefix}-{nanos:x}")
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

/// One "phone": an engine fed by a [`Host`], connected and in the Lobby.
struct Phone {
    engine: Engine,
    events: EventReceiver,
    host: Host,
    server: ServerId,
    session: u32,
}

async fn phone(dir: &Path, prefix: &str) -> Phone {
    let name = unique(prefix);
    let (host, audio) = Host::start();
    let (engine, mut events) = Engine::with_external_audio(
        &dir.join(&name),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
        audio,
    )
    .expect("engine");
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
    let snapshot: Snapshot = wait_for(&mut events, "synced", |e| match e {
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
    Phone {
        engine,
        events,
        host,
        server: server.id,
        session: snapshot.self_session,
    }
}

/// Polls the connection info until the send bitrate satisfies `ok`, returns it in kbit/s.
async fn bitrate_kbps_when(phone: &Phone, ok: impl Fn(u32) -> bool) -> Option<u32> {
    let start = Instant::now();
    loop {
        let kbps = phone
            .engine
            .connection_stats(phone.server)
            .map_or(0, |s| s.send_bitrate_bps / 1000);
        if ok(kbps) {
            return Some(kbps);
        }
        if start.elapsed() > WAIT {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn speech_travels_between_two_apps_that_supply_the_audio() {
    if !live() {
        return;
    }
    let dir = work_dir("engine-live-external-audio");
    let speaker = phone(&dir, "mc-ext-a").await;
    let mut listener = phone(&dir, "mc-ext-b").await;
    // Both are in the Lobby and the voice paths are up.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // The speaker's microphone callback delivers 3 s of speech.
    let spoken_at = listener.host.played_len();
    speaker.host.speak(&speech_like(SAMPLE_RATE * 3, 0.5));
    let talker = speaker.session;
    wait_for(&mut listener.events, "the speaker talking", |e| match e {
        EngineEvent::TalkStateChanged {
            session,
            talking: true,
        } if *session == talker => Some(()),
        _ => None,
    })
    .await;
    tokio::time::sleep(Duration::from_secs(4)).await;

    // It comes out of the listener's speaker callback, soon and at a speech level.
    let heard = listener.host.played_from(spoken_at);
    let onset = heard
        .iter()
        .position(|s| s.abs() > 0.01)
        .expect("the listener heard nothing");
    let delay = Duration::from_secs_f64(onset as f64 / SAMPLE_RATE as f64);
    eprintln!("microphone callback to speaker callback: {delay:?}");
    assert!(delay <= MAX_DELAY, "speech arrived after {delay:?}");
    let level = rms(&heard[onset..onset + 2 * SAMPLE_RATE]);
    assert!(level > 0.02, "speech arrived too quiet (level {level})");

    // "Reduced voice quality" on mobile data: the limit applies and lifts again.
    speaker
        .engine
        .set_bitrate_limit_kbps(Some(MOBILE_DATA_KBPS));
    let limited = bitrate_kbps_when(&speaker, |kbps| (1..=MOBILE_DATA_KBPS).contains(&kbps)).await;
    assert!(limited.is_some(), "the bitrate limit did not apply");
    speaker.engine.set_bitrate_limit_kbps(None);
    let lifted = bitrate_kbps_when(&speaker, |kbps| kbps > MOBILE_DATA_KBPS).await;
    assert!(lifted.is_some(), "the bitrate limit did not lift");
}
