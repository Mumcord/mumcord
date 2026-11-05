//! The own camera's stop rules (spec 08, FR-8.21 – FR-8.23, AC-8.10, AC-8.18, AC-8.22) against the
//! local test server: screen lock (with the setting on and off), leaving voice, moving to another
//! channel, switching the voice server, the camera disappearing, and quitting. The camera runs in
//! the preview with the test camera, so no relay is needed; the relay stops (FR-8.24) are covered
//! by the E2E tests with mumpeer's testenv. `testenv/up.sh v15 v15b`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_camera_stop -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mumbot::{Bot, BotConfig};
use mumcord_camera::{CameraError, TestCamera, TestCameraMode};
use mumcord_engine::camera::{CameraFailure, CameraState, CameraView};
use mumcord_engine::{
    Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields, ServerId,
};
// mumbot pins its own mumcord-protocol release, so bot-side messages use its types.
use bot_protocol::proto;

const WAIT: Duration = Duration::from_secs(20);

fn live() -> bool {
    let on = std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start testenv (v15, v15b)");
    }
    on
}

fn work_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    dir
}

fn unique(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{prefix}-{:06x}", (nanos ^ std::process::id()) & 0xff_ffff)
}

async fn connect(engine: &Engine, rx: &mut EventReceiver, port: u16) -> ServerId {
    let server = engine
        .server_add(ServerFields {
            label: unique("camstop"),
            address: "127.0.0.1".into(),
            port,
            username: unique("camstop"),
            password: PasswordChange::Keep,
        })
        .await
        .expect("server");
    engine.connect(server.id, None).expect("connect");
    loop {
        let event = next(rx, "sync").await;
        match event {
            EngineEvent::CertTrustRequired {
                server_id,
                fingerprint,
                ..
            } => engine
                .cert_trust_decide(server_id, &fingerprint, true)
                .expect("trust"),
            EngineEvent::ServerSynced(_) => return server.id,
            _ => {}
        }
    }
}

async fn next(rx: &mut EventReceiver, what: &str) -> EngineEvent {
    tokio::time::timeout(WAIT, rx.recv())
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
        .expect("event stream open")
}

/// The next `CameraChanged` that reports the camera off.
async fn camera_off(rx: &mut EventReceiver, what: &str) -> CameraView {
    loop {
        if let EngineEvent::CameraChanged(view) = next(rx, what).await
            && view.state == CameraState::Off
        {
            return view;
        }
    }
}

async fn poll(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn preview(engine: &Engine, server: ServerId) {
    engine.camera_preview_start(server).expect("preview opens");
    assert_eq!(
        engine.camera_view().map(|v| v.state),
        Some(CameraState::Preview)
    );
}

async fn set_off_on_lock(engine: &Engine, on: bool) {
    let mut settings = engine.settings().expect("settings");
    settings.camera_off_on_lock = on;
    engine.update_settings(settings).await.expect("settings");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs the local test server (MUMCORD_LIVE=1, testenv v15 and v15b)"]
async fn the_camera_turns_off_on_every_stop_rule() {
    if !live() {
        return;
    }
    let dir = work_dir("live-camera-stop");
    let (engine, mut rx) = Engine::new(
        &dir.join("a"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create("Camstop Tester".into(), None)
        .await
        .expect("identity");
    engine.camera_set_backend(Arc::new(TestCamera::new(TestCameraMode::Present)));
    let server = connect(&engine, &mut rx, common::v15()).await;
    engine.join_channel(server, 0).expect("voice in the root");
    poll("voice on the first server", || {
        engine.voice_server() == Some(server)
    })
    .await;

    // Screen lock with "Turn my camera off when my screen locks" on (the default).
    preview(&engine, server);
    engine.camera_screen_locked();
    let off = camera_off(&mut rx, "off after the lock").await;
    assert_eq!(off.error, None, "a lock shows no error");
    assert_eq!(engine.camera_view(), None);

    // With the setting off the camera stays on.
    set_off_on_lock(&engine, false).await;
    preview(&engine, server);
    engine.camera_screen_locked();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        engine.camera_view().map(|v| v.state),
        Some(CameraState::Preview)
    );
    engine.camera_turn_off(server);
    set_off_on_lock(&engine, true).await;

    // Leaving voice.
    preview(&engine, server);
    engine.leave_voice();
    camera_off(&mut rx, "off after leaving voice").await;
    engine.join_channel(server, 0).expect("voice again");
    poll("voice back", || engine.voice_server() == Some(server)).await;

    // Moving to another channel; a temporary channel stays while its creator is in it.
    let room = unique("camstop-room");
    let bot = Bot::connect(BotConfig::superuser(common::v15()))
        .await
        .expect("SuperUser bot (SUPERUSER_PASSWORD in testenv/.env)");
    bot.handle()
        .send_channel_state(proto::ChannelState {
            parent: Some(0),
            name: Some(room.clone()),
            temporary: Some(true),
            ..Default::default()
        })
        .expect("create channel");
    let mut room_id = None;
    poll("the temporary channel", || {
        room_id = bot
            .state()
            .channels
            .values()
            .find(|c| c.name == room)
            .map(|c| c.id);
        room_id.is_some()
    })
    .await;
    let room_id = room_id.unwrap_or_default();
    poll("the channel known to Mumcord", || {
        engine
            .snapshot_of(server)
            .is_some_and(|s| s.channels.iter().any(|c| c.id == room_id))
    })
    .await;
    preview(&engine, server);
    let moved = Instant::now();
    engine.join_channel(server, room_id).expect("move");
    camera_off(&mut rx, "off after the channel move").await;
    assert!(moved.elapsed() < Duration::from_secs(1), "within 1 s");

    // Switching the voice server.
    let second = connect(&engine, &mut rx, common::v15b()).await;
    preview(&engine, server);
    engine
        .join_channel(second, 0)
        .expect("voice on the second server");
    camera_off(&mut rx, "off after the server switch").await;
    engine.join_channel(server, 0).expect("voice back");
    poll("voice back", || engine.voice_server() == Some(server)).await;

    // The camera disappears: "Camera stopped: the camera was disconnected."
    engine.camera_set_backend(Arc::new(TestCamera::new(TestCameraMode::UnplugAfter(
        Duration::from_millis(300),
    ))));
    preview(&engine, server);
    let off = camera_off(&mut rx, "off after the unplug").await;
    assert_eq!(
        off.error,
        Some(CameraFailure::Camera(CameraError::DeviceGone))
    );

    // Quitting.
    engine.camera_set_backend(Arc::new(TestCamera::new(TestCameraMode::Present)));
    preview(&engine, server);
    let quit = engine.shutdown();
    assert_eq!(engine.camera_view(), None, "off before the drain");
    quit.await;
    drop(bot);
}
