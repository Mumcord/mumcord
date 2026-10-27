//! Screen share state (FR-2.42 – FR-2.46, AC-2.29) against the local test server: ten engines
//! in one channel (loopback direct connections). Alice's share shows as "Live" everywhere,
//! eight people watch it over direct streams and the ninth gets `shareFull`; a second share is
//! watched at the same time; leaving the channel ends watching and sharing. No video yet.
//! `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_screen_share -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use mumbot::env;
use mumcord_engine::p2p::P2pConfig;
use mumcord_engine::screen_share::{ScreenShareView, TestTone};
use mumcord_engine::store::Consent;
use mumcord_engine::{
    Engine, EngineEvent, ErrorKind, EventReceiver, PasswordChange, SecretStore, ServerFields,
    Snapshot,
};

const WAIT: Duration = Duration::from_secs(30);
/// The test server allows 4 plugin messages per second per user (burst 15); a sharer sends
/// three per new viewer (direct connection answer, accept, viewer list).
const WATCH_PACE: Duration = Duration::from_millis(800);

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn screen_shares_between_mumcord_clients() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_screen_share");
    let mut alice = start(&dir, "ss4a-alice").await;
    let mut viewers = Vec::new();
    for i in 0..9 {
        viewers.push(start(&dir, &format!("ss4a-v{i}")).await);
    }
    let a = alice.session();
    let server = alice.server();

    // Alice shares: everyone sees her "Live", nobody watches yet.
    alice
        .engine
        .screen_share_start(server, true)
        .await
        .expect("start");
    assert!(
        alice.engine.screen_share_start(server, true).await.is_err(),
        "one share per server"
    );
    let own = alice.engine.screen_shares(server);
    assert_eq!(
        (own[0].sound, own[0].sound_available),
        (false, false),
        "no app sound on Linux (FR-2.42)"
    );
    // A share with sound, as on Windows/macOS, so the sound switch can be seen by the viewers.
    alice
        .engine
        .screen_share_set_sound_source(server, Box::new(TestTone::new(440.0, 0.1)))
        .expect("tone");
    let own = alice.engine.screen_shares(server);
    assert_eq!(own.len(), 1);
    assert_eq!((own[0].session, own[0].sound), (a, true));
    for v in &mut viewers {
        wait_watchable(v, a).await;
        // The share may first be announced before its sound was switched on.
        let with_sound = |shares: &[ScreenShareView]| share_of(shares, a).is_some_and(|s| s.sound);
        if !with_sound(&v.engine.screen_shares(v.server())) {
            wait_shares(&mut v.rx, "the share with sound", with_sound).await;
        }
        let share = v.engine.screen_shares(v.server());
        let share = share_of(&share, a).expect("Alice's share");
        assert!(share.sound && !share.paused && share.viewers.is_empty());
    }

    // Eight watch over direct streams; the ninth gets "This share is full".
    for v in &viewers[..8] {
        v.engine
            .screen_share_watch(v.server(), a)
            .await
            .expect("watch");
        assert!(v.engine.p2p_connected(v.server(), a), "direct connection");
        tokio::time::sleep(WATCH_PACE).await;
    }
    let expected: Vec<u32> = {
        let mut list: Vec<u32> = viewers[..8].iter().map(Client::session).collect();
        list.sort_unstable();
        list
    };
    wait_shares(&mut alice.rx, "eight viewers", |shares| {
        share_of(shares, a).is_some_and(|s| s.viewers == expected)
    })
    .await;
    let ninth = &viewers[8];
    let full = ninth
        .engine
        .screen_share_watch(ninth.server(), a)
        .await
        .expect_err("full");
    assert_eq!(full.kind, ErrorKind::ShareFull);
    wait_shares(&mut viewers[0].rx, "the viewer list", |shares| {
        share_of(shares, a).is_some_and(|s| s.viewers == expected)
    })
    .await;

    // Pause and sound reach the viewers.
    alice
        .engine
        .screen_share_set_paused(server, true)
        .expect("pause");
    alice
        .engine
        .screen_share_set_sound(server, false)
        .expect("sound off");
    wait_shares(&mut viewers[1].rx, "paused without sound", |shares| {
        share_of(shares, a).is_some_and(|s| s.paused && !s.sound)
    })
    .await;

    // A second share is watched at the same time.
    let b = viewers[8].session();
    viewers[8]
        .engine
        .screen_share_start(viewers[8].server(), false)
        .await
        .expect("second share");
    wait_watchable(&mut viewers[0], b).await;
    viewers[0]
        .engine
        .screen_share_watch(viewers[0].server(), b)
        .await
        .expect("watch the second share");
    let v0 = viewers[0].session();
    let both = wait_shares(&mut viewers[0].rx, "watching both", |shares| {
        share_of(shares, b).is_some_and(|s| s.viewers == [v0])
    })
    .await;
    assert!(share_of(&both, a).is_some_and(|s| s.viewers.contains(&v0)));

    // A viewer leaving the channel stops watching.
    let afk = alice
        .snapshot
        .channels
        .iter()
        .find(|ch| ch.name == "AFK")
        .expect("AFK")
        .id;
    let v1 = viewers[1].session();
    viewers[1]
        .engine
        .join_channel(viewers[1].server(), afk)
        .expect("move viewer");
    wait_shares(
        &mut viewers[1].rx,
        "no shares outside the channel",
        <[_]>::is_empty,
    )
    .await;
    wait_shares(&mut alice.rx, "the viewer gone", |shares| {
        share_of(shares, a).is_some_and(|s| !s.viewers.contains(&v1))
    })
    .await;
    wait_shares(
        &mut viewers[2].rx,
        "the viewer gone for others too",
        |shares| share_of(shares, a).is_some_and(|s| !s.viewers.contains(&v1)),
    )
    .await;

    // The freed place can be taken again; unwatching frees it once more.
    let ninth = &viewers[8];
    ninth
        .engine
        .screen_share_watch(ninth.server(), a)
        .await
        .expect("a free place");
    ninth
        .engine
        .screen_share_unwatch(ninth.server(), a)
        .expect("unwatch");
    wait_shares(&mut alice.rx, "the unwatch", |shares| {
        share_of(shares, a).is_some_and(|s| s.viewers.len() == 7 && !s.viewers.contains(&b))
    })
    .await;

    // The sharer leaving the channel ends her share for everyone.
    alice.engine.join_channel(server, afk).expect("move Alice");
    wait_shares(&mut viewers[0].rx, "Alice's share gone", |shares| {
        share_of(shares, a).is_none() && share_of(shares, b).is_some()
    })
    .await;
    assert!(
        viewers[1]
            .engine
            .screen_share_watch(viewers[1].server(), a)
            .await
            .is_err(),
        "a share outside the channel cannot be watched"
    );

    // Stopping ends the second share.
    viewers[8]
        .engine
        .screen_share_stop(viewers[8].server())
        .expect("stop");
    wait_shares(&mut viewers[0].rx, "no shares left", <[_]>::is_empty).await;
}
