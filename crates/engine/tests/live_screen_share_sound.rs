//! Screen share sound (FR-2.42, FR-2.45, AC-2.29) against the local test server: Alice shares
//! with a test tone as the app's sound, Bob watches and hears it at the level it was sent;
//! switching the sound off stops it. Real per-app capture needs a manual listening check.
//! `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_screen_share_sound -- --ignored --test-threads=1 --nocapture`
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mumbot::env;
use mumcord_engine::p2p::P2pConfig;
use mumcord_engine::screen_share::{ScreenShareView, TestTone};
use mumcord_engine::store::Consent;
use mumcord_engine::{
    Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields, Snapshot,
};

const WAIT: Duration = Duration::from_secs(30);
const TONE_HZ: f32 = 440.0;
const TONE_AMPLITUDE: f32 = 0.25;
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

/// Waits for a share list that `check` accepts.
async fn wait_shares(
    rx: &mut EventReceiver,
    what: &str,
    mut check: impl FnMut(&[ScreenShareView]) -> bool,
) -> Vec<ScreenShareView> {
    wait_for(rx, what, |e| match e {
        EngineEvent::ScreenSharesChanged(shares) if check(shares) => Some(shares.clone()),
        _ => None,
    })
    .await
}

fn share_of(shares: &[ScreenShareView], session: u32) -> Option<&ScreenShareView> {
    shares.iter().find(|s| s.session == session)
}

struct Client {
    engine: Engine,
    rx: EventReceiver,
    snapshot: Snapshot,
}

impl Client {
    fn server(&self) -> mumcord_engine::ServerId {
        self.engine
            .connections()
            .first()
            .and_then(mumcord_engine::ConnState::server_id)
            .unwrap_or(0)
    }

    fn session(&self) -> u32 {
        self.snapshot.self_session
    }
}

async fn start(dir: &Path, who: &str) -> Client {
    let (engine, mut rx) = Engine::new(
        &dir.join(who),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create(format!("{who} Tester"))
        .await
        .expect("identity");
    let mut settings = engine.settings().expect("settings");
    settings.allow_direct_connections = Consent::Yes;
    engine.update_settings(settings).await.expect("consent");
    // Engines on one machine reach each other over loopback; the router is never asked.
    engine.p2p_set_config(P2pConfig {
        connect_timeout: Duration::from_secs(10),
        port_mapping: false,
        allow_loopback: true,
    });
    let name = unique(who);
    let server = engine
        .server_add(ServerFields {
            label: format!("test {} {name}", common::v15()),
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
    let snapshot = loop {
        let event = wait_for(&mut rx, "sync", |e| match e {
            EngineEvent::CertTrustRequired { .. } | EngineEvent::ServerSynced(_) => Some(e.clone()),
            _ => None,
        })
        .await;
        match event {
            EngineEvent::CertTrustRequired {
                server_id,
                fingerprint,
                ..
            } => engine
                .cert_trust_decide(server_id, &fingerprint, true)
                .expect("trust"),
            EngineEvent::ServerSynced(s) => break *s,
            _ => unreachable!(),
        }
    };
    Client {
        engine,
        rx,
        snapshot,
    }
}

/// Waits until `client` sees `sharer`'s share and may watch it.
async fn wait_watchable(client: &mut Client, sharer: u32) {
    if client
        .engine
        .screen_shares(client.server())
        .iter()
        .any(|s| s.session == sharer && s.blocked.is_none())
    {
        return;
    }
    wait_shares(&mut client.rx, "a watchable share", |shares| {
        share_of(shares, sharer).is_some_and(|s| s.blocked.is_none())
    })
    .await;
}

async fn wait_until(what: &str, limit: Duration, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + limit;
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn the_viewer_hears_the_shared_sound_at_its_level() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_screen_share_sound");
    let alice = start(&dir, "ss5-alice").await;
    let mut bob = start(&dir, "ss5-bob").await;
    let a = alice.session();
    let server = alice.server();
    assert!(
        !alice.engine.screen_share_sound_available(),
        "no app sound on Linux (FR-2.42)"
    );

    // Deafened hears no share either; a fresh client starts deafened.
    bob.engine.set_self_deaf(false).expect("undeafen");
    alice
        .engine
        .screen_share_start(server, true)
        .await
        .expect("start");
    alice
        .engine
        .screen_share_set_sound_source(server, Box::new(TestTone::new(TONE_HZ, TONE_AMPLITUDE)))
        .expect("tone");
    wait_watchable(&mut bob, a).await;
    bob.engine
        .screen_share_watch(bob.server(), a)
        .await
        .expect("watch");

    let stats = || {
        bob.engine
            .screen_share_sound_stats(bob.server(), a)
            .expect("watching")
    };
    let deadline = Instant::now() + WAIT;
    while stats().played_samples < 2 * 48_000 {
        assert!(Instant::now() < deadline, "no steady sound: {:?}", stats());
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    // The level of a sine is A/√2; sample it a few times while it plays.
    let expected = TONE_AMPLITUDE / std::f32::consts::SQRT_2;
    let mut levels = Vec::new();
    for _ in 0..10 {
        levels.push(stats().rms);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let level = levels.iter().sum::<f32>() / levels.len() as f32;
    let s = stats();
    println!("level {level:.3} (expected {expected:.3}), {s:?}");
    assert!(
        (level - expected).abs() < expected * 0.1,
        "level {level}, expected {expected}: {levels:?}"
    );
    assert!(s.underruns <= 2, "steady playback: {s:?}");
    assert_eq!(s.overflow_samples, 0);

    // Sound off: no more packets come.
    alice
        .engine
        .screen_share_set_sound(server, false)
        .expect("sound off");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let off = stats().packets;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(stats().packets, off, "nothing sent while sound is off");

    // And back on.
    alice
        .engine
        .screen_share_set_sound(server, true)
        .expect("sound on");
    wait_until("sound again", WAIT, || stats().packets > off + 20).await;

    alice.engine.screen_share_stop(server).expect("stop");
}
