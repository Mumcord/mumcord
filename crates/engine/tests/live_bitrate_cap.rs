//! Own bitrate cap against the local 1.4 test server (FR-2.64, AC-2.45): the slider maximum is
//! reported, a 32 kbit/s cap shows in the connection info, and "Max" lifts it again.
//! `testenv/up.sh v14`, then (the Max test needs `BANDWIDTH_V14=400000 testenv/up.sh v14`):
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_bitrate_cap -- --ignored --nocapture`
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mumbot::env;
use mumbot::voice::Clip;
use mumcord_engine::{
    Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields, ServerId,
};

const WAIT: Duration = Duration::from_secs(20);
const SAMPLE_RATE: usize = 48_000;

fn live() -> bool {
    let on = std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start testenv/up.sh v14");
    }
    on
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

/// Polls the connection info until the send bitrate satisfies `ok`, returns it in kbit/s.
async fn bitrate_kbps_when(
    engine: &Engine,
    server: ServerId,
    timeout: Duration,
    ok: impl Fn(u32) -> bool,
) -> Option<u32> {
    let start = Instant::now();
    loop {
        let kbps = engine
            .connection_stats(server)
            .map_or(0, |s| s.send_bitrate_bps / 1000);
        if ok(kbps) {
            return Some(kbps);
        }
        if start.elapsed() > timeout {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v14 (MUMCORD_LIVE=1)"]
async fn own_cap_shows_in_the_connection_info_and_max_lifts_it() {
    if !live() {
        return;
    }
    let dir = work_dir("engine-live-bitrate-cap");
    let (input, output) = (dir.join("in.wav"), dir.join("out.wav"));
    Clip::from_samples(vec![0.0; SAMPLE_RATE * 120])
        .save(&input)
        .expect("save input");
    // SAFETY: set before any audio thread exists; no other thread reads the environment yet.
    unsafe {
        std::env::set_var(
            "MUMCORD_AUDIO_BACKEND",
            format!("file:{}:{}", input.display(), output.display()),
        );
    }

    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    let mut settings = engine.settings().expect("settings");
    settings.bitrate_kbps = Some(32);
    engine.update_settings(settings).await.expect("settings");
    engine
        .identity_create("mc-bitrate".into())
        .await
        .expect("identity");
    let server = engine
        .server_add(ServerFields {
            label: "v14".into(),
            address: env::HOST.into(),
            port: common::v14(),
            username: "mc-bitrate".into(),
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server");
    engine.connect(server.id, None).expect("connect");
    wait_for(&mut rx, "synced", |e| match e {
        EngineEvent::CertTrustRequired { fingerprint, .. } => {
            engine
                .cert_trust_decide(server.id, fingerprint, true)
                .expect("trust");
            None
        }
        EngineEvent::ServerSynced(_) => Some(()),
        _ => None,
    })
    .await;

    // The slider maximum is reported and never above the 510 kbit/s Opus limit.
    let max_bps = wait_for(&mut rx, "bitrate maximum", |e| match e {
        EngineEvent::BitrateMaxChanged(Some(max)) => Some(*max),
        _ => None,
    })
    .await;
    eprintln!("slider maximum {} kbit/s", max_bps / 1000);
    assert!((16_000..=510_000).contains(&max_bps), "max {max_bps}");
    assert_eq!(engine.bitrate_max(), Some(max_bps));

    // 32 kbit/s shows in the connection info within 2 s of it being applied.
    let kbps = bitrate_kbps_when(&engine, server.id, Duration::from_secs(10), |k| k == 32)
        .await
        .expect("capped at 32 kbit/s");
    assert_eq!(kbps, 32);

    // "Max" with music quality off lets it climb again, up to the maximum and no further; above
    // the 96 kbit/s speech cap when the server allows it (F20).
    let mut settings = engine.settings().expect("settings");
    settings.bitrate_kbps = None;
    settings.music_quality = false;
    engine.update_settings(settings).await.expect("settings");
    let above = if max_bps > 96_000 { 96 } else { 32 };
    let kbps = bitrate_kbps_when(&engine, server.id, Duration::from_secs(60), |k| k > above)
        .await
        .unwrap_or_else(|| {
            panic!(
                "rises above {above} kbit/s (maximum {} kbit/s)",
                max_bps / 1000
            )
        });
    assert!(kbps * 1000 <= max_bps, "{kbps} above the maximum");
}
