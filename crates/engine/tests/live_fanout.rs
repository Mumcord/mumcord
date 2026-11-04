//! Screen share fan-out through the relay broadcast (spec 07, FR-7.40 – FR-7.52) against
//! mumpeer's `testenv/` (unmodified Mumble server, mumpeer, mumbot `peer` with broadcast
//! admission). A shares the test pattern, B and C watch through the relay, frames arrive
//! decrypted (all three register, as the bot serves registered users only), a viewer leaving makes A rekey without breaking the other viewer, C's frame-rate
//! cap goes to the relay as a temporal layer, and C's reports reach A. With the `no-broadcast`
//! profile the share falls back to direct copies with the reason for the notice.
//!
//! Start the testenv (a mumbot image from main, which has broadcast admission):
//! `MUMPEER_PORT_BASE=46100 MUMPEER_PROJECT=fanout MUMBOT_IMAGE=<mumbot from main> testenv/up.sh`
//! (profile `all`), then:
//! `MUMCORD_LIVE=1 MUMCORD_FANOUT_PORT=46100 cargo test -p mumcord-engine --features test-backend --test live_fanout -- --ignored --test-threads=1 --nocapture`
//! For the fallback run `MUMPEER_PROFILE=no-broadcast testenv/up.sh` and add
//! `MUMCORD_FANOUT_PROFILE=no-broadcast`. `MUMCORD_FANOUT_STOP=<mumpeer container>` also stops
//! the relay mid-share and checks that the viewers continue on direct copies (FR-7.49).
#![cfg(feature = "test-backend")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mumcord_engine::p2p::P2pConfig;
use mumcord_engine::screen_share::wire::{Codec, Frame, FrameHeader, FrameKind};
use mumcord_engine::screen_share::{
    FanoutUnavailable, FanoutView, I420Frame, ScreenFrameSink, StartOutcome, SwitchReason,
    ViewerPath,
};
use mumcord_engine::store::{Consent, ScreenCodec};
use mumcord_engine::{
    Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields, ServerId,
    Snapshot,
};

const WAIT: Duration = Duration::from_secs(30);

fn live() -> bool {
    let on = std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start mumpeer's testenv");
    }
    on
}

fn port() -> u16 {
    std::env::var("MUMCORD_FANOUT_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(44758)
}

fn broadcast_profile() -> bool {
    std::env::var("MUMCORD_FANOUT_PROFILE").map_or(true, |p| p != "no-broadcast")
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

/// Polls `check` until it gives a value or `WAIT` is over.
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
    /// Kept open so the engine's events have somewhere to go.
    _rx: EventReceiver,
    snapshot: Snapshot,
    sink: Arc<Sink>,
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
    let sink = Arc::new(Sink::default());
    engine.set_screen_frame_sink(sink.clone());
    let server = engine
        .server_add(ServerFields {
            label: format!("fanout {}", unique(who)),
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
        sink,
    }
}

#[derive(Default)]
struct Sink(Mutex<Vec<(Instant, FrameHeader)>>);

impl ScreenFrameSink for Sink {
    fn on_encoded(&self, _: ServerId, _: u32, frame: &Frame) {
        self.0
            .lock()
            .expect("sink")
            .push((Instant::now(), frame.header));
    }

    fn on_decoded(&self, _: ServerId, _: u32, _: Codec, _: &I420Frame) {}
}

impl Sink {
    /// Video frames per second over the last `window`, and the first video frame's keyframe
    /// flag.
    fn fps(&self, window: Duration) -> f64 {
        let since = Instant::now() - window;
        let got = self.0.lock().expect("sink");
        got.iter()
            .filter(|(at, h)| *at >= since && matches!(h.kind, FrameKind::Video(_)))
            .count() as f64
            / window.as_secs_f64()
    }

    fn first_is_keyframe(&self) -> Option<bool> {
        self.0
            .lock()
            .expect("sink")
            .iter()
            .find(|(_, h)| matches!(h.kind, FrameKind::Video(_)))
            .map(|(_, h)| h.keyframe)
    }

    /// The codecs of the video frames that arrived over the last `window`.
    fn codecs(&self, window: Duration) -> Vec<Codec> {
        let since = Instant::now() - window;
        let mut codecs: Vec<Codec> = self
            .0
            .lock()
            .expect("sink")
            .iter()
            .filter(|(at, _)| *at >= since)
            .filter_map(|(_, h)| match h.kind {
                FrameKind::Video(codec) => Some(codec),
                FrameKind::Audio => None,
            })
            .collect();
        codecs.dedup();
        codecs
    }

    fn clear(&self) {
        self.0.lock().expect("sink").clear();
    }

    fn count(&self) -> usize {
        self.0.lock().expect("sink").len()
    }
}

/// Waits until the relay bot announced itself to `client`.
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

async fn watch(client: &mut Client, sharer: u32) {
    let server = client.server();
    poll("a watchable share", || {
        client
            .engine
            .screen_shares(server)
            .iter()
            .any(|s| s.session == sharer && s.blocked.is_none())
            .then_some(())
    })
    .await;
    client
        .engine
        .screen_share_watch(server, sharer)
        .await
        .expect("watch");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs mumpeer's testenv (MUMCORD_LIVE=1)"]
async fn fanout_through_the_relay_broadcast() {
    if !live() {
        return;
    }
    let dir = work_dir("live_fanout");
    let a = start(&dir, "fan-a").await;
    let mut b = start(&dir, "fan-b").await;
    let mut c = start(&dir, "fan-c").await;
    for client in [&a, &b, &c] {
        wait_relay(client).await;
    }
    // The bot learns the registrations from the server.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let (server_a, sharer) = (a.server(), a.session());
    assert_eq!(
        a.engine
            .screen_share_start(server_a, false)
            .await
            .expect("start"),
        StartOutcome::Started
    );
    let view = poll("the fan-out decision", || {
        a.engine
            .screen_share_fanout(server_a)
            .map(|f| f.view)
            .filter(|v| *v != FanoutView::Starting)
    })
    .await;

    if !broadcast_profile() {
        // FR-7.47: direct copies, with the reason for the notice.
        assert_eq!(
            view,
            FanoutView::Direct {
                reason: FanoutUnavailable::NotOffered
            }
        );
        watch(&mut b, sharer).await;
        assert_eq!(
            b.engine.screen_share_watch_via_relay(b.server(), sharer),
            Some(false)
        );
        poll("direct frames", || {
            (b.sink.fps(Duration::from_secs(2)) > 10.0).then_some(())
        })
        .await;
        return;
    }

    let FanoutView::Relay { limit } = view else {
        panic!("no relay fan-out: {view:?}");
    };
    assert!(limit >= 2, "limit {limit}");
    watch(&mut b, sharer).await;
    watch(&mut c, sharer).await;
    for viewer in [&b, &c] {
        assert_eq!(
            viewer
                .engine
                .screen_share_watch_via_relay(viewer.server(), sharer),
            Some(true),
            "watching through the relay"
        );
        poll("decrypted frames from the relay", || {
            (viewer.sink.fps(Duration::from_secs(2)) > 10.0).then_some(())
        })
        .await;
        assert_eq!(viewer.sink.first_is_keyframe(), Some(true));
    }
    // Both are in the sharer's list, through the relay (FR-7.52).
    let shares = a.engine.screen_shares(server_a);
    let own = shares
        .iter()
        .find(|s| s.session == sharer)
        .expect("own share");
    assert!(own.viewers.contains(&b.session()) && own.viewers.contains(&c.session()));

    // C reports what it receives; it keeps up, so it is not slow.
    let report = poll("C's report", || {
        a.engine.screen_share_fanout(server_a).and_then(|f| {
            f.relay_viewers
                .into_iter()
                .find(|v| v.session == c.session() && v.fps > 0)
        })
    })
    .await;
    assert!(!report.slow, "C is not slow: {report:?}");

    // B leaves: A rekeys at the next keyframe and C keeps decrypting (FR-7.44).
    b.engine
        .screen_share_unwatch(b.server(), sharer)
        .expect("unwatch");
    let before = c.sink.count();
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(c.sink.count() > before + 40, "C continues after the rekey");
    assert!(c.sink.fps(Duration::from_secs(2)) > 10.0);

    // The sharer's stats list C as a broadcast viewer (FR-7.52), and the start dialog's path
    // shows the relay's limit (FR-7.36).
    let stats = a.engine.screen_share_media_stats(server_a).expect("stats");
    assert!(
        stats
            .viewers
            .iter()
            .any(|v| v.session == c.session() && v.path == ViewerPath::Broadcast),
        "C on the broadcast: {:?}",
        stats.viewers
    );
    assert_eq!(
        a.engine.screen_broadcast_max(server_a).map(u16::from),
        Some(limit.min(255))
    );

    // C caps itself at 15 fps; the relay drops the top layer (FR-7.45, FR-7.51): C's own
    // report counts what arrives from the relay, before C's local layer filter.
    c.engine
        .screen_share_set_viewer_fps(c.server(), sharer, Some(15))
        .expect("max fps");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let capped = c.sink.fps(Duration::from_secs(2));
    assert!(capped > 3.0 && capped <= 17.0, "capped fps {capped}");
    poll("C's report at the capped rate", || {
        a.engine.screen_share_fanout(server_a).and_then(|f| {
            f.relay_viewers
                .into_iter()
                .find(|v| v.session == c.session() && v.fps > 3 && v.fps <= 17)
        })
    })
    .await;

    if let Ok(container) = std::env::var("MUMCORD_FANOUT_STOP") {
        // FR-7.49: the relay goes away; C continues on a direct copy within 10 s.
        let stopped = std::process::Command::new("docker")
            .args(["stop", "-t", "0", &container])
            .status()
            .expect("docker");
        assert!(stopped.success());
        let stopped_at = Instant::now();
        poll("C on a direct copy", || {
            (c.engine.screen_share_watch_via_relay(c.server(), sharer) == Some(false)).then_some(())
        })
        .await;
        poll("direct frames", || {
            (c.sink.fps(Duration::from_secs(1)) > 10.0).then_some(())
        })
        .await;
        let took = stopped_at.elapsed();
        assert!(
            took <= Duration::from_secs(10),
            "direct copy after {took:?}"
        );
        assert_eq!(
            a.engine.screen_share_fanout(server_a).map(|f| f.view),
            Some(FanoutView::Direct {
                reason: FanoutUnavailable::Lost
            })
        );
    }

    a.engine.screen_share_stop(server_a).expect("stop");
}

/// An AV1 share watched through the relay by an AV1 viewer (B) and a VP9-only one (C): the
/// share runs VP9 while C watches, and both get playable frames; after C leaves it returns to
/// AV1 (FR-7.58 on the broadcast path).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs mumpeer's testenv (MUMCORD_LIVE=1)"]
async fn relay_viewers_choose_the_codec() {
    if !live() || !broadcast_profile() {
        return;
    }
    let dir = work_dir("live_fanout_codec");
    let a = start(&dir, "fan-av1").await;
    let mut b = start(&dir, "fan-b").await;
    let mut c = start(&dir, "fan-vp9").await;
    for client in [&a, &b, &c] {
        wait_relay(client).await;
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let mut settings = a.engine.settings().expect("settings");
    settings.screen_codec = ScreenCodec::Av1;
    a.engine.update_settings(settings).await.expect("AV1");
    c.engine
        .screen_share_set_playable_codecs(Some(&[Codec::Vp9]));

    let (server_a, sharer) = (a.server(), a.session());
    assert_eq!(
        a.engine
            .screen_share_start(server_a, false)
            .await
            .expect("start"),
        StartOutcome::Started
    );
    let view = poll("the fan-out decision", || {
        a.engine
            .screen_share_fanout(server_a)
            .map(|f| f.view)
            .filter(|v| *v != FanoutView::Starting)
    })
    .await;
    assert!(matches!(view, FanoutView::Relay { .. }), "{view:?}");

    watch(&mut b, sharer).await;
    poll("AV1 frames at B through the relay", || {
        (b.engine.screen_share_watch_via_relay(b.server(), sharer) == Some(true)
            && b.sink.codecs(Duration::from_secs(1)) == [Codec::Av1])
        .then_some(())
    })
    .await;

    // The VP9-only viewer joins the broadcast: the share switches to VP9 for it.
    watch(&mut c, sharer).await;
    assert_eq!(
        c.engine.screen_share_watch_via_relay(c.server(), sharer),
        Some(true)
    );
    let switched = poll("the switch to VP9", || {
        a.engine
            .screen_share_media_stats(server_a)
            .and_then(|s| s.switched)
    })
    .await;
    assert_eq!(
        switched.reason,
        SwitchReason::Viewer {
            session: c.session()
        }
    );
    for viewer in [&b, &c] {
        poll("VP9 frames through the relay", || {
            (viewer.sink.codecs(Duration::from_secs(1)) == [Codec::Vp9]
                && viewer.sink.fps(Duration::from_secs(1)) > 10.0)
                .then_some(())
        })
        .await;
    }
    assert!(
        !c.sink.codecs(WAIT).contains(&Codec::Av1),
        "C never got AV1"
    );
    assert_eq!(c.sink.first_is_keyframe(), Some(true));

    // C leaves: the share goes back to AV1 for B.
    c.engine
        .screen_share_unwatch(c.server(), sharer)
        .expect("unwatch");
    b.sink.clear();
    poll("AV1 frames at B again", || {
        (b.sink.codecs(Duration::from_secs(1)) == [Codec::Av1]).then_some(())
    })
    .await;

    a.engine.screen_share_stop(server_a).expect("stop");
}
