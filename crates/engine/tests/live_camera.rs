//! Camera send through the relay (spec 08, FR-8.42 – FR-8.51) against mumpeer's `testenv/`
//! (unmodified Mumble server, mumpeer, mumbot `peer` with camera broadcasts). A opens the
//! preview with the test camera, turns it on (`bcast-req k=cam`), B asks to join (`cam-join`),
//! A admits it through the bot, B binds, gets the key and receives decrypted camera frames. With
//! the `no-camera` profile turning on fails with "use-not-allowed" and the camera stays in the
//! preview.
//!
//! Start the testenv with a mumbot image from main:
//! `MUMPEER_PORT_BASE=46300 MUMPEER_PROJECT=camera MUMBOT_IMAGE=<mumbot from main> testenv/up.sh`
//! (profile `all`), then:
//! `MUMCORD_LIVE=1 MUMCORD_TEST_CAMERA=1 MUMCORD_CAMERA_PORT=46300 cargo test -p mumcord-engine --features test-backend --test live_camera -- --ignored --test-threads=1 --nocapture`
//! For the refusal run `MUMPEER_PROFILE=no-camera testenv/up.sh` and add
//! `MUMCORD_CAMERA_PROFILE=no-camera`.
#![cfg(feature = "test-backend")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mumcord_engine::camera::{CameraFailure, CameraState};
use mumcord_engine::p2p::P2pConfig;
use mumcord_engine::screen_share::wire::FrameKind;
use mumcord_engine::store::Consent;
use mumcord_engine::{
    Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields, ServerId,
    Snapshot,
};

const WAIT: Duration = Duration::from_secs(30);

fn live() -> bool {
    let on = std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1")
        && std::env::var("MUMCORD_TEST_CAMERA").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set MUMCORD_LIVE=1 MUMCORD_TEST_CAMERA=1 and start mumpeer's testenv");
    }
    on
}

fn port() -> u16 {
    std::env::var("MUMCORD_CAMERA_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(44758)
}

fn camera_profile() -> bool {
    std::env::var("MUMCORD_CAMERA_PROFILE").map_or(true, |p| p != "no-camera")
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

async fn poll<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(found) = check() {
            return found;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

struct Client {
    engine: Engine,
    _rx: EventReceiver,
    snapshot: Snapshot,
}

impl Client {
    fn server(&self) -> ServerId {
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
        .identity_create(format!("{who} Tester"), None)
        .await
        .expect("identity");
    let mut settings = engine.settings().expect("settings");
    settings.allow_direct_connections = Consent::Yes;
    engine.update_settings(settings).await.expect("consent");
    engine.p2p_set_config(P2pConfig {
        connect_timeout: Duration::from_secs(10),
        port_mapping: false,
        allow_loopback: true,
    });
    let server = engine
        .server_add(ServerFields {
            label: format!("camera {}", unique(who)),
            address: "127.0.0.1".into(),
            port: port(),
            username: unique(who),
            password: PasswordChange::Keep,
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
    // The relay bot serves registered users only (its default).
    engine.register_self(server.id).await.expect("register");
    Client {
        engine,
        _rx: rx,
        snapshot,
    }
}

async fn wait_relay(client: &Client) {
    let server = client.server();
    poll("the relay announcement", || {
        client
            .engine
            .relay_info(server)
            .host
            .is_some()
            .then_some(())
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs mumpeer's testenv (MUMCORD_LIVE=1, MUMCORD_TEST_CAMERA=1)"]
async fn the_test_camera_goes_on_air_and_reaches_a_viewer() {
    if !live() {
        return;
    }
    let dir = work_dir("live-camera");
    let a = start(&dir, "a").await;
    let b = start(&dir, "b").await;
    wait_relay(&a).await;
    wait_relay(&b).await;
    let (server_a, server_b) = (a.server(), b.server());

    // Preview: the camera runs, nothing is sent.
    a.engine
        .camera_preview_start(server_a)
        .expect("preview opens");
    assert_eq!(
        a.engine.camera_view().map(|v| v.state),
        Some(CameraState::Preview)
    );

    if !camera_profile() {
        let refused = a.engine.camera_turn_on(server_a).await;
        assert_eq!(refused, Err(CameraFailure::UseNotAllowed));
        let view = a.engine.camera_view().expect("still in the preview");
        assert_eq!(
            view.state,
            CameraState::Preview,
            "a refusal keeps the preview"
        );
        assert_eq!(view.error, Some(CameraFailure::UseNotAllowed));
        a.engine.camera_turn_off(server_a);
        assert_eq!(a.engine.camera_view(), None);
        return;
    }

    a.engine
        .camera_turn_on(server_a)
        .await
        .expect("the camera goes on air");
    let view = a.engine.camera_view().expect("on air");
    assert_eq!(view.state, CameraState::OnAir);
    let id = view.broadcast.expect("broadcast id");
    assert_eq!(id.len(), 16);

    // B joins: A admits it through the bot, B binds, gets the key and the frames.
    let mut frames = b
        .engine
        .camera_test_watch(server_b, a.session(), &id)
        .await
        .expect("B joins A's camera");
    let first = tokio::time::timeout(WAIT, frames.recv())
        .await
        .expect("a camera frame within 30 s")
        .expect("frame stream open");
    assert!(matches!(first.header.kind, FrameKind::Video(_)));
    assert!(first.header.keyframe, "the first frame is a keyframe");
    // Nobody reported a tile yet: the room's usual 360p.
    assert_eq!((first.header.width, first.header.height), (640, 360));
    let mut count = 1;
    let until = Instant::now() + Duration::from_secs(2);
    while Instant::now() < until {
        if let Ok(Some(_)) = tokio::time::timeout(Duration::from_millis(500), frames.recv()).await {
            count += 1;
        }
    }
    assert!(count >= 20, "about 30 fps arrive, got {count} in ~2 s");
    let viewers = poll("B among A's viewers", || {
        a.engine
            .camera_view()
            .filter(|v| v.viewers.contains(&b.session()))
            .map(|v| v.viewers)
    })
    .await;
    assert_eq!(viewers, [b.session()]);

    // Off: the broadcast ends and the camera is released.
    a.engine.camera_turn_off(server_a);
    assert_eq!(a.engine.camera_view(), None);
}
