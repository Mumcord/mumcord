//! The camera path choice and the fallback matrix (spec 08, FR-8.94, FR-8.95, AC-8.29 – AC-8.33)
//! with engines on this machine: a new relay carries the camera over its new-format broadcast, an
//! old relay keeps the old-format broadcast for 0.8.x viewers while 0.9.0 viewers get a direct
//! stream, `direct_only` and a forgotten relay run without any broadcast, a viewer without
//! `media-v2` is refused, and `relay_only` without a relay reports the FR-8.4 reason.
//!
//! Start mumpeer's `testenv/up.sh` with the profile the test needs and name it:
//! `MUMPEER_PROFILE=all testenv/up.sh` with `MUMCORD_CAMERA_PROFILE=all` (new relay, mumpeer
//! 0.3.0), `MUMPEER_PROFILE=old testenv/up.sh` with `MUMCORD_CAMERA_PROFILE=old` (relay without
//! the media profile), then:
//! `MUMCORD_LIVE=1 MUMCORD_TEST_CAMERA=1 MUMCORD_CAMERA_PORT=44758 MUMCORD_CAMERA_PROFILE=all \
//!  cargo test -p mumcord-engine --features test-backend --test live_camera_paths -- --ignored
//!  --test-threads=1`
#![cfg(feature = "test-backend")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mumcord_engine::camera::avail::CameraUnavailable;
use mumcord_engine::camera::signal::{self, CameraMessage, Codec};
use mumcord_engine::camera::{CameraFailure, CameraPath, CameraState, TileState};
use mumcord_engine::p2p::P2pConfig;
use mumcord_engine::store::{CameraShow, Consent, P2pMode};
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

/// The relay profile the testenv runs: `all` (a new relay) or `old` (no media profile).
fn profile() -> String {
    std::env::var("MUMCORD_CAMERA_PROFILE").unwrap_or_else(|_| "all".to_owned())
}

/// Skips a test whose cell needs another relay profile.
fn wants(profile_name: &str) -> bool {
    let running = profile();
    if running == profile_name {
        return true;
    }
    eprintln!("skipped: this cell runs with MUMCORD_CAMERA_PROFILE={profile_name}, not {running}");
    false
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
            label: format!("camera paths {}", unique(who)),
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

async fn set_mode(client: &Client, mode: P2pMode) {
    let mut settings = client.engine.settings().expect("settings");
    settings.p2p_mode = mode;
    client.engine.update_settings(settings).await.expect("mode");
}

async fn set_show(client: &Client, show: CameraShow) {
    let mut settings = client.engine.settings().expect("settings");
    settings.camera_show = show;
    client.engine.update_settings(settings).await.expect("show");
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

/// Watches cameras on `viewer`: its room is open before the camera turns on.
async fn watch_cameras(viewer: &Client) {
    viewer.engine.camera_room_set_open(viewer.server(), true);
    wait_relay(viewer).await;
}

/// Waits until `sharer`'s camera plays on `viewer`.
async fn plays(viewer: &Client, sharer: u32) {
    let server = viewer.server();
    poll("the sharer's tile playing", || {
        viewer
            .engine
            .camera_room(server)
            .tiles
            .iter()
            .find(|t| t.sharer == sharer && t.state == TileState::Playing)
            .map(|_| ())
    })
    .await;
}

/// Waits until `sharer` is in the own camera's viewer list and returns the list.
async fn viewers_with(client: &Client, session: u32) -> Vec<mumcord_engine::camera::CameraViewer> {
    poll("the viewer list", || {
        client
            .engine
            .camera_view()
            .filter(|v| v.viewers.iter().any(|x| x.session == session))
            .map(|v| v.viewers)
    })
    .await
}

/// Sends a `cam-join` for `id` by hand, as a 0.8.x client would: its hello has no `media-v2`, so
/// its `cam-join` carries no `mv: 2` (FR-8.95).
fn join_without_media(viewer: &Client, owner: u32, id: &str) {
    let join = CameraMessage::CamJoin {
        id: id.to_owned(),
        codecs: vec![Codec::Av1, Codec::Vp9],
        media: false,
    };
    let document = signal::encode(&join).expect("a valid cam-join");
    let ctx = viewer
        .engine
        .session_ctx_for(viewer.server())
        .expect("connected");
    ctx.handle
        .send_plugin_data(&[owner], signal::DATA_ID, document)
        .expect("the scripted cam-join goes out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs mumpeer's testenv (MUMCORD_LIVE=1, MUMCORD_TEST_CAMERA=1)"]
async fn a_new_relay_carries_the_camera_to_a_09_viewer() {
    if !live() || !wants("all") {
        return;
    }
    let dir = work_dir("live-camera-paths-new");
    let a = start(&dir, "a").await;
    let b = start(&dir, "b").await;
    wait_relay(&a).await;
    watch_cameras(&b).await;
    let (server_a, server_b) = (a.server(), b.server());

    a.engine
        .camera_preview_start(server_a)
        .expect("preview opens");
    a.engine
        .camera_turn_on(server_a)
        .await
        .expect("the camera goes on air");
    let view = a.engine.camera_view().expect("on air");
    assert_eq!(view.state, CameraState::OnAir);
    // FR-8.94: a new relay carries the camera, so there is a broadcast…
    let id = view.broadcast.expect("a relay broadcast on a new relay");
    assert_eq!(id.len(), 16);

    // …and the 0.9.0 viewer is served through it, not through a stream of its own.
    plays(&b, a.session()).await;
    assert!(
        !b.engine.camera_test_direct(server_b, a.session()),
        "the relay broadcast serves a 0.9.0 viewer on a new relay"
    );
    let viewers = viewers_with(&a, b.session()).await;
    assert_eq!(
        viewers[0].path,
        CameraPath::Relay,
        "the viewer list names the relay path (FR-8.62)"
    );

    a.engine.camera_turn_off(server_a);
    assert_eq!(a.engine.camera_view(), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs mumpeer's testenv (MUMCORD_LIVE=1, MUMCORD_TEST_CAMERA=1)"]
async fn an_old_relay_serves_a_09_viewer_directly() {
    if !live() || !wants("old") {
        return;
    }
    let dir = work_dir("live-camera-paths-old");
    let a = start(&dir, "a").await;
    let b = start(&dir, "b").await;
    wait_relay(&a).await;
    watch_cameras(&b).await;
    let (server_a, server_b) = (a.server(), b.server());

    a.engine
        .camera_preview_start(server_a)
        .expect("preview opens");
    a.engine
        .camera_turn_on(server_a)
        .await
        .expect("the camera goes on air");
    // The old-format broadcast still exists for 0.8.x viewers (FR-8.94)…
    assert!(a.engine.camera_view().and_then(|v| v.broadcast).is_some());

    // …while a 0.9.0 viewer gets its own `STREAM_CAMERA`.
    plays(&b, a.session()).await;
    poll("the direct stream", || {
        b.engine
            .camera_test_direct(server_b, a.session())
            .then_some(())
    })
    .await;
    let viewers = viewers_with(&a, b.session()).await;
    assert_eq!(viewers[0].path, CameraPath::Direct);

    a.engine.camera_turn_off(server_a);
    assert_eq!(a.engine.camera_view(), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs mumpeer's testenv (MUMCORD_LIVE=1, MUMCORD_TEST_CAMERA=1)"]
async fn an_old_relay_serves_a_08_viewer_over_its_old_broadcast() {
    if !live() || !wants("old") {
        return;
    }
    let dir = work_dir("live-camera-paths-old-viewer");
    let a = start(&dir, "a").await;
    let b = start(&dir, "b").await;
    // B never joins on its own, so its one `cam-join` allowance stays free for the scripted one.
    set_show(&b, CameraShow::Never).await;
    wait_relay(&a).await;
    let (server_a, server_b) = (a.server(), b.server());

    a.engine
        .camera_preview_start(server_a)
        .expect("preview opens");
    a.engine
        .camera_turn_on(server_a)
        .await
        .expect("the camera goes on air");
    let id = a
        .engine
        .camera_view()
        .and_then(|v| v.broadcast)
        .expect("the old-format broadcast");

    // The old broadcast is what a 0.8.x viewer reads, so it is admitted over the relay and gets
    // no stream and no `needs` (FR-8.95).
    join_without_media(&b, a.session(), &id);
    let viewers = viewers_with(&a, b.session()).await;
    assert_eq!(viewers[0].path, CameraPath::Relay);
    assert!(!b.engine.camera_test_direct(server_b, a.session()));

    a.engine.camera_turn_off(server_a);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs mumpeer's testenv (MUMCORD_LIVE=1, MUMCORD_TEST_CAMERA=1)"]
async fn a_viewer_without_the_new_format_is_refused() {
    if !live() || !wants("all") {
        return;
    }
    let dir = work_dir("live-camera-paths-needs");
    let a = start(&dir, "a").await;
    let b = start(&dir, "b").await;
    set_show(&b, CameraShow::Never).await;
    wait_relay(&a).await;
    let (server_a, server_b) = (a.server(), b.server());

    a.engine
        .camera_preview_start(server_a)
        .expect("preview opens");
    a.engine
        .camera_turn_on(server_a)
        .await
        .expect("the camera goes on air");
    let id = a
        .engine
        .camera_view()
        .and_then(|v| v.broadcast)
        .expect("a new-format relay broadcast");

    // A 0.8.x `cam-join` cannot read the new format, so the sender serves it nothing at all:
    // no admission, no stream, no second broadcast (FR-8.95).
    join_without_media(&b, a.session(), &id);
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        a.engine.camera_view().map(|v| v.viewers),
        Some(Vec::new()),
        "nobody is admitted for a viewer that cannot read the format"
    );
    assert!(
        !b.engine.camera_test_direct(server_b, a.session()),
        "no STREAM_CAMERA is opened to it"
    );

    a.engine.camera_turn_off(server_a);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs mumpeer's testenv (MUMCORD_LIVE=1, MUMCORD_TEST_CAMERA=1)"]
async fn direct_only_runs_without_any_broadcast() {
    if !live() {
        return;
    }
    let dir = work_dir("live-camera-paths-direct-only");
    let a = start(&dir, "a").await;
    let b = start(&dir, "b").await;
    set_mode(&a, P2pMode::DirectOnly).await;
    watch_cameras(&b).await;
    let (server_a, server_b) = (a.server(), b.server());

    a.engine
        .camera_preview_start(server_a)
        .expect("preview opens");
    a.engine
        .camera_turn_on(server_a)
        .await
        .expect("the direct camera goes on air");
    let view = a.engine.camera_view().expect("on air");
    // FR-8.94: `direct_only` never opens a relay broadcast.
    assert_eq!(view.broadcast, None);
    assert_eq!(view.state, CameraState::OnAir);

    plays(&b, a.session()).await;
    poll("the direct stream", || {
        b.engine
            .camera_test_direct(server_b, a.session())
            .then_some(())
    })
    .await;
    let viewers = viewers_with(&a, b.session()).await;
    assert_eq!(viewers[0].path, CameraPath::Direct);

    a.engine.camera_turn_off(server_a);
    assert_eq!(a.engine.camera_view(), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs mumpeer's testenv (MUMCORD_LIVE=1, MUMCORD_TEST_CAMERA=1)"]
async fn without_a_relay_the_camera_runs_directly() {
    if !live() {
        return;
    }
    let dir = work_dir("live-camera-paths-no-relay");
    let a = start(&dir, "a").await;
    let b = start(&dir, "b").await;
    let (server_a, server_b) = (a.server(), b.server());
    // Forget the announced relay: with no relay the direct path is the only one (FR-8.96).
    a.engine.relay_forget(server_a).expect("forget the relay");
    poll("the relay gone", || {
        a.engine.relay_info(server_a).host.is_none().then_some(())
    })
    .await;
    b.engine.camera_room_set_open(server_b, true);

    a.engine
        .camera_preview_start(server_a)
        .expect("preview opens");
    a.engine
        .camera_turn_on(server_a)
        .await
        .expect("the camera goes on air without a relay");
    assert_eq!(a.engine.camera_view().map(|v| v.broadcast), Some(None));

    plays(&b, a.session()).await;
    poll("the direct stream", || {
        b.engine
            .camera_test_direct(server_b, a.session())
            .then_some(())
    })
    .await;

    a.engine.camera_turn_off(server_a);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs mumpeer's testenv (MUMCORD_LIVE=1, MUMCORD_TEST_CAMERA=1)"]
async fn relay_only_without_a_relay_is_unavailable() {
    if !live() {
        return;
    }
    let dir = work_dir("live-camera-paths-relay-only");
    let a = start(&dir, "a").await;
    wait_relay(&a).await;
    let server = a.server();
    set_mode(&a, P2pMode::RelayOnly).await;
    a.engine.relay_forget(server).expect("forget the relay");
    let reason = poll("the FR-8.4 reason", || a.engine.camera_unavailable(server)).await;
    assert_eq!(reason, CameraUnavailable::NoRelay);

    a.engine
        .camera_preview_start(server)
        .expect("the preview still opens");
    let refused = a.engine.camera_turn_on(server).await;
    assert!(
        matches!(
            refused,
            Err(CameraFailure::RelayDown) | Err(CameraFailure::UseNotAllowed)
        ),
        "a camera in relay_only without a relay cannot go on air, got {refused:?}"
    );
    assert_eq!(
        a.engine.camera_view().map(|v| v.state),
        Some(CameraState::Preview),
        "a refusal keeps the preview"
    );
    a.engine.camera_turn_off(server);
}
