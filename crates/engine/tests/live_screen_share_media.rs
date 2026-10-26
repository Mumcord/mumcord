//! Screen share media (FR-2.44, AC-2.29) against the local test server: Alice shares the test
//! pattern with three viewers over loopback direct connections. Every viewer starts at a
//! keyframe and gets 720p30; a throttled viewer loses temporal layers while the others keep the
//! full frame rate; throttling Alice's whole upload drops quality to 480p15 and lifting it brings
//! 720p30 back; the Rust decoder fallback hands out pictures. Prints fps, bitrate and CPU.
//! `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_screen_share_media -- --ignored --test-threads=1 --nocapture`
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mumcord_bot::env;
use mumcord_engine::p2p::P2pConfig;
use mumcord_engine::screen_share::wire::{Frame, FrameHeader};
use mumcord_engine::screen_share::{I420Frame, ScreenFrameSink, ScreenShareView};
use mumcord_engine::store::Consent;
use mumcord_engine::{
    Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields, ServerId,
    Snapshot,
};

const WAIT: Duration = Duration::from_secs(30);

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

/// What one viewer received.
#[derive(Default)]
struct Received {
    encoded: Vec<(Instant, FrameHeader)>,
    decoded: Vec<(Instant, u32, u32)>,
}

#[derive(Default)]
struct Sink(Mutex<Received>);

impl ScreenFrameSink for Sink {
    fn on_encoded(&self, _: ServerId, _: u32, frame: &Frame) {
        self.0
            .lock()
            .expect("sink")
            .encoded
            .push((Instant::now(), frame.header));
    }

    fn on_decoded(&self, _: ServerId, _: u32, frame: &I420Frame) {
        self.0
            .lock()
            .expect("sink")
            .decoded
            .push((Instant::now(), frame.width(), frame.height()));
    }
}

impl Sink {
    /// Encoded frames per second over the last `window`, and the size of the newest frame.
    fn recent(&self, window: Duration) -> (f64, (u16, u16)) {
        let got = self.0.lock().expect("sink");
        let since = Instant::now() - window;
        let count = got.encoded.iter().filter(|(at, _)| *at >= since).count();
        let size = got
            .encoded
            .last()
            .map_or((0, 0), |(_, h)| (h.width, h.height));
        (count as f64 / window.as_secs_f64(), size)
    }

    fn decoded_recent(&self, window: Duration) -> (f64, (u32, u32)) {
        let got = self.0.lock().expect("sink");
        let since = Instant::now() - window;
        let count = got.decoded.iter().filter(|(at, ..)| *at >= since).count();
        let size = got.decoded.last().map_or((0, 0), |(_, w, h)| (*w, *h));
        (count as f64 / window.as_secs_f64(), size)
    }
}

/// CPU time of this whole process (all engines) in seconds.
fn cpu_seconds() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat
        .rsplit_once(')')
        .map(|(_, rest)| rest.split_whitespace().collect())
        .unwrap_or_default();
    let ticks = |i: usize| {
        fields
            .get(i)
            .and_then(|f| f.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    // utime and stime are fields 14 and 15; `fields` starts at field 3.
    (ticks(11) + ticks(12)) / 100.0
}

async fn wait_until(what: &str, limit: Duration, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + limit;
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

const WINDOW: Duration = Duration::from_secs(2);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn screen_share_media_reaches_every_viewer() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_screen_share_media");
    let alice = start(&dir, "ss4b-alice").await;
    let mut viewers = Vec::new();
    for i in 0..3 {
        viewers.push(start(&dir, &format!("ss4b-v{i}")).await);
    }
    let a = alice.session();
    let server = alice.server();
    let sinks: Vec<Arc<Sink>> = (0..3).map(|_| Arc::new(Sink::default())).collect();
    for (v, sink) in viewers.iter().zip(&sinks) {
        v.engine.set_screen_frame_sink(sink.clone());
    }

    alice
        .engine
        .screen_share_start(server, false)
        .await
        .expect("start");
    for v in &mut viewers {
        wait_watchable(v, a).await;
    }
    // Everyone clicks "Watch" at once; resent requests get past the server's rate limit.
    let [v0, v1, v2] = [&viewers[0], &viewers[1], &viewers[2]];
    let (w0, w1, w2) = tokio::join!(
        v0.engine.screen_share_watch(v0.server(), a),
        v1.engine.screen_share_watch(v1.server(), a),
        v2.engine.screen_share_watch(v2.server(), a),
    );
    for result in [w0, w1, w2] {
        result.expect("watch");
    }

    // Every viewer starts at a keyframe and gets 720p30.
    tokio::time::sleep(Duration::from_secs(4)).await;
    let cpu_before = cpu_seconds();
    tokio::time::sleep(WINDOW).await;
    let cpu = (cpu_seconds() - cpu_before) / WINDOW.as_secs_f64();
    for (i, sink) in sinks.iter().enumerate() {
        let first = sink
            .0
            .lock()
            .expect("sink")
            .encoded
            .first()
            .map(|(_, h)| h.keyframe);
        assert_eq!(first, Some(true), "viewer {i} starts at a keyframe");
        let (fps, size) = sink.recent(WINDOW);
        assert_eq!(size, (1280, 720), "viewer {i} size");
        assert!(fps >= 25.0, "viewer {i} gets {fps} fps");
    }
    let stats = alice
        .engine
        .screen_share_media_stats(server)
        .expect("stats");
    println!(
        "720p30: sender {} fps, {} kbps, encode {:.1} ms/frame, {} keyframes; process CPU {:.0}% of one core",
        stats.fps,
        stats.kbps,
        stats.encode_ms,
        stats.keyframes,
        cpu * 100.0
    );
    assert_eq!(stats.step, 0);
    assert_eq!(stats.viewers.len(), 3);
    let full_kbps = stats.kbps.max(100);

    // The Rust decoder fallback hands out pictures.
    let v2 = &viewers[2];
    v2.engine
        .screen_share_set_decode_fallback(v2.server(), a, true)
        .expect("fallback");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let (fps, size) = sinks[2].decoded_recent(WINDOW);
    println!("decode fallback: {fps:.1} fps at {size:?}");
    assert_eq!(size, (1280, 720));
    assert!(fps >= 25.0, "decoded {fps} fps");
    v2.engine
        .screen_share_set_decode_fallback(v2.server(), a, false)
        .expect("fallback off");

    // One slow viewer loses layers; the others keep 30 fps and the quality stays.
    let v0 = viewers[0].session();
    let slow = full_kbps * 7 / 10;
    alice
        .engine
        .screen_share_set_upload_limit(server, Some(v0), Some(slow))
        .expect("throttle one");
    tokio::time::sleep(Duration::from_secs(6)).await;
    let stats = alice
        .engine
        .screen_share_media_stats(server)
        .expect("stats");
    let layer = stats
        .viewers
        .iter()
        .find(|v| v.session == v0)
        .map(|v| v.max_layer);
    let (slow_fps, _) = sinks[0].recent(WINDOW);
    let (fast_fps, _) = sinks[1].recent(WINDOW);
    println!(
        "one viewer at {slow} kbps: layer {layer:?}, {slow_fps:.1} fps; others {fast_fps:.1} fps; step {}",
        stats.step
    );
    assert!(layer.is_some_and(|l| l < 2), "the slow viewer lost a layer");
    assert!(slow_fps < 25.0, "the slow viewer gets fewer frames");
    assert!(fast_fps >= 25.0, "the others keep 30 fps");
    assert_eq!(stats.step, 0, "one slow viewer keeps the quality");

    // A slow upload for everyone steps down to 480p15 ...
    alice
        .engine
        .screen_share_set_upload_limit(server, Some(v0), None)
        .expect("unthrottle one");
    let low = full_kbps * 3 / 10;
    alice
        .engine
        .screen_share_set_upload_limit(server, None, Some(low))
        .expect("throttle all");
    let began = Instant::now();
    wait_until("480p15", Duration::from_secs(30), || {
        alice
            .engine
            .screen_share_media_stats(server)
            .is_some_and(|s| s.step == 2)
    })
    .await;
    let down = began.elapsed();
    tokio::time::sleep(Duration::from_secs(3)).await;
    let stats = alice
        .engine
        .screen_share_media_stats(server)
        .expect("stats");
    let (fps, size) = sinks[1].recent(WINDOW);
    println!(
        "upload {low} kbps: 480p15 after {:.1} s; sender {}x{} {} fps {} kbps; viewer {fps:.1} fps at {size:?}",
        down.as_secs_f64(),
        stats.width,
        stats.height,
        stats.fps,
        stats.kbps
    );
    assert_eq!((stats.width, stats.height), (854, 480));
    assert_eq!(size, (854, 480));

    // ... and back up once the upload recovers.
    alice
        .engine
        .screen_share_set_upload_limit(server, None, None)
        .expect("unthrottle all");
    let began = Instant::now();
    wait_until("720p30 again", Duration::from_secs(60), || {
        alice
            .engine
            .screen_share_media_stats(server)
            .is_some_and(|s| s.step == 0)
    })
    .await;
    let up = began.elapsed();
    tokio::time::sleep(Duration::from_secs(4)).await;
    for (i, sink) in sinks.iter().enumerate() {
        let (fps, size) = sink.recent(WINDOW);
        assert_eq!(size, (1280, 720), "viewer {i} back at 720p");
        assert!(fps >= 20.0, "viewer {i} back at {fps} fps");
    }
    println!("recovered to 720p30 after {:.1} s", up.as_secs_f64());

    alice.engine.screen_share_stop(server).expect("stop");
}
