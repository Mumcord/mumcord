//! The direct camera path (spec 08, FR-8.92, FR-8.93) with two engines on this machine: A runs
//! `direct_only`, so it opens one `STREAM_CAMERA` to B; B's tile plays it without any relay
//! broadcast, follows B's tile height on the reverse stream, and closes when B blocks A or A
//! turns the camera off.
//!
//! `testenv/up.sh` with the camera profile (see `live_camera.rs`), then:
//! `MUMCORD_LIVE=1 MUMCORD_TEST_CAMERA=1 MUMCORD_CAMERA_PORT=44758 cargo test -p mumcord-engine --features test-backend --test live_camera_direct -- --ignored --test-threads=1 --nocapture`
#![cfg(feature = "test-backend")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mumcord_engine::camera::{CameraState, TileState};
use mumcord_engine::p2p::P2pConfig;
use mumcord_engine::screen_share::wire::{Codec, Frame, FrameHeader};
use mumcord_engine::screen_share::{I420Frame, ScreenFrameSink};
use mumcord_engine::store::{Consent, P2pMode};
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

/// Waits for `check` while the engines work; the camera room ticks once a second.
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
    // Engines on one machine reach each other over loopback; the router is never asked.
    engine.p2p_set_config(P2pConfig {
        connect_timeout: Duration::from_secs(10),
        port_mapping: false,
        allow_loopback: true,
    });
    let server = engine
        .server_add(ServerFields {
            label: format!("camera direct {}", unique(who)),
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

/// The frames a viewer's tile gets, newest last.
#[derive(Default)]
struct Received(Mutex<Vec<FrameHeader>>);

impl ScreenFrameSink for Received {
    fn on_encoded(&self, _: ServerId, _: u32, frame: &Frame) {
        self.0.lock().expect("sink").push(frame.header);
    }

    fn on_decoded(&self, _: ServerId, _: u32, _: Codec, frame: &I420Frame) {
        self.0.lock().expect("sink").push(FrameHeader {
            kind: mumcord_engine::screen_share::wire::FrameKind::Video(Codec::Vp9),
            keyframe: false,
            temporal_layer: 0,
            timestamp_us: 0,
            width: u16::try_from(frame.width()).unwrap_or(0),
            height: u16::try_from(frame.height()).unwrap_or(0),
            payload_len: 0,
        });
    }
}

impl Received {
    fn frames(&self) -> usize {
        self.0.lock().expect("sink").len()
    }

    fn newest_size(&self) -> (u16, u16) {
        self.0
            .lock()
            .expect("sink")
            .last()
            .map_or((0, 0), |header| (header.width, header.height))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs mumpeer's testenv (MUMCORD_LIVE=1, MUMCORD_TEST_CAMERA=1)"]
async fn the_camera_reaches_a_viewer_over_the_direct_stream() {
    if !live() {
        return;
    }
    let dir = work_dir("live-camera-direct");
    let a = start(&dir, "a").await;
    let b = start(&dir, "b").await;
    let (server_a, server_b) = (a.server(), b.server());

    // `direct_only` keeps the camera off the relay broadcast whatever format the testenv's
    // relay speaks, so this test always exercises the direct path (FR-8.94).
    let mut settings = a.engine.settings().expect("settings");
    settings.p2p_mode = P2pMode::DirectOnly;
    a.engine.update_settings(settings).await.expect("mode");

    // B watches the room before A goes on air.
    let frames = Arc::new(Received::default());
    b.engine.camera_set_frame_sink(frames.clone());
    b.engine.camera_room_set_open(server_b, true);
    poll("B's relay announcement", || {
        b.engine.relay_info(server_b).host.is_some().then_some(())
    })
    .await;

    a.engine
        .camera_preview_start(server_a)
        .expect("preview opens");
    a.engine
        .camera_turn_on(server_a)
        .await
        .expect("the camera goes on air");
    assert_eq!(
        a.engine.camera_view().map(|v| v.state),
        Some(CameraState::OnAir)
    );

    // B's tile binds the sender's own stream, not the relay broadcast (FR-8.92).
    let sharer = a.session();
    poll("B's tile playing", || {
        b.engine
            .camera_room(server_b)
            .tiles
            .iter()
            .find(|t| t.sharer == sharer && t.state == TileState::Playing)
            .map(|_| ())
    })
    .await;
    poll("the direct stream", || {
        b.engine.camera_test_direct(server_b, sharer).then_some(())
    })
    .await;
    poll("camera frames", || (frames.frames() > 5).then_some(())).await;

    // B's tile is 720 high: `want` travels back and the encoder follows within a second, with a
    // keyframe at the new size (FR-8.93, FR-8.45).
    b.engine.camera_tile_set_height(server_b, sharer, 720);
    poll("720p after want", || {
        (frames.newest_size() == (1280, 720)).then_some(())
    })
    .await;

    // B hides the tile: the stream closes and nothing plays any more (FR-8.57).
    b.engine.camera_tile_set_hidden(server_b, sharer, true);
    poll("the tile hidden", || {
        b.engine
            .camera_room(server_b)
            .tiles
            .iter()
            .find(|t| t.sharer == sharer)
            .filter(|t| t.state == TileState::Hidden)
            .map(|_| ())
    })
    .await;
    assert!(
        !b.engine.camera_test_direct(server_b, sharer),
        "stream closed"
    );

    // Blocking B's side is refused the same way (FR-8.59).
    b.engine
        .block_session(server_b, sharer)
        .expect("block the camera's sender");
    poll("the blocked tile gone", || {
        b.engine
            .camera_room(server_b)
            .tiles
            .iter()
            .all(|t| t.sharer != sharer)
            .then_some(())
    })
    .await;

    // Off ends the camera.
    a.engine.camera_turn_off(server_a);
    assert_eq!(a.engine.camera_view(), None);
}
