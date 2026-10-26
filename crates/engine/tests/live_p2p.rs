//! Direct connections (FR-2.35 – FR-2.37, AC-2.25, AC-2.26) against the local test server: two
//! engines on this machine (loopback allowed, no router port mapping). Bob opens Alice's full
//! image: the first time he is asked for consent; with yes the image comes over the direct
//! connection, with "no" on Alice's side or without a working direct path it still arrives
//! over plugin data. `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_p2p -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use image::{ImageFormat, Rgb, RgbImage};
use mumcord_bot::env;
use mumcord_engine::p2p::P2pConfig;
use mumcord_engine::store::Consent;
use mumcord_engine::{
    ChatMessageView, Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields,
    ServerView,
};

const WAIT: Duration = Duration::from_secs(30);
const THUMBNAIL: &str = "data:image/png;base64,iVBORw0KGgo=";

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

/// Two engines on one machine reach each other over loopback; the router is never asked.
fn local_config(connect_timeout: Duration) -> P2pConfig {
    P2pConfig {
        connect_timeout,
        port_mapping: false,
        allow_loopback: true,
    }
}

async fn engine(dir: &Path, name: &str, consent: Consent) -> (Engine, EventReceiver, ServerView) {
    let (engine, rx) = Engine::new(
        &dir.join(name),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine.identity_create(name.into()).await.expect("identity");
    let mut settings = engine.settings().expect("settings");
    settings.allow_direct_connections = consent;
    engine.update_settings(settings).await.expect("consent");
    engine.p2p_set_config(local_config(Duration::from_secs(10)));
    let server = engine
        .server_add(ServerFields {
            label: format!("test {} {name}", common::v15()),
            address: "127.0.0.1".into(),
            port: common::v15(),
            username: name.into(),
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server");
    (engine, rx, server)
}

async fn connect(engine: &Engine, rx: &mut EventReceiver, server: &ServerView) {
    engine.connect(server.id, None).expect("connect");
    loop {
        let event = wait_for(rx, "sync", |e| match e {
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
            EngineEvent::ServerSynced(_) => return,
            _ => unreachable!(),
        }
    }
}

/// A noisy PNG (does not compress).
fn picture(width: u32, height: u32, seed: u32) -> Vec<u8> {
    let mut state = seed | 1;
    let image = RgbImage::from_fn(width, height, |_, _| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let [r, g, b, _] = state.to_le_bytes();
        Rgb([r, g, b])
    });
    let mut out = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .expect("png");
    out
}

/// The test's only connected server (0, which no call accepts, when none is).
fn only_server(engine: &Engine) -> mumcord_engine::ServerId {
    engine
        .connections()
        .first()
        .and_then(mumcord_engine::ConnState::server_id)
        .unwrap_or(0)
}

struct Pair {
    alice: Engine,
    bob: Engine,
    bob_rx: EventReceiver,
}

async fn pair(test: &str, alice_consent: Consent, bob_consent: Consent) -> (Pair, String) {
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir(test);
    let alice_name = unique("b23-alice");
    let (alice, mut alice_rx, alice_server) = engine(&dir, &alice_name, alice_consent).await;
    let (bob, mut bob_rx, bob_server) = engine(&dir, &unique("b23-bob"), bob_consent).await;
    connect(&alice, &mut alice_rx, &alice_server).await;
    connect(&bob, &mut bob_rx, &bob_server).await;
    (Pair { alice, bob, bob_rx }, alice_name)
}

/// Alice sends a picture of that size; returns its reference and Bob's copy of the message.
async fn send_picture(
    pair: &mut Pair,
    alice_name: &str,
    original: Vec<u8>,
) -> (String, ChatMessageView) {
    let reference = pair.alice.full_image_keep(original).await.expect("kept");
    let html = format!("<img src=\"{THUMBNAIL}\" data-mumcord-full=\"{reference}\" />");
    pair.alice
        .send_channel_message(only_server(&pair.alice), 0, &html)
        .await
        .expect("sent");
    let message = wait_for(&mut pair.bob_rx, "the image message", |e| match e {
        EngineEvent::MessageReceived(m) if m.sender_name == alice_name => Some((**m).clone()),
        _ => None,
    })
    .await;
    (reference, message)
}

fn fetch(
    bob: &Engine,
    reference: &str,
    message: &ChatMessageView,
) -> tokio::task::JoinHandle<Option<String>> {
    let bob = bob.clone();
    let reference = reference.to_owned();
    let session = message.sender_session;
    let cert = message.sender_cert_hash.clone();
    tokio::spawn(async move {
        bob.full_image_fetch(only_server(&bob), &reference, session, cert)
            .await
    })
}

fn disconnect(pair: &Pair) {
    pair.alice.disconnect(only_server(&pair.alice));
    pair.bob.disconnect(only_server(&pair.bob));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn consent_yes_loads_the_image_directly() {
    if !live() {
        return;
    }
    let (mut pair, alice_name) = pair("live_p2p_yes", Consent::Yes, Consent::Unset).await;
    // About 350 plugin-data chunks: minutes over plugin data, a moment directly.
    let original = picture(300, 200, 3);
    assert!(original.len() > 150_000, "{}", original.len());
    let (reference, message) = send_picture(&mut pair, &alice_name, original).await;
    let alice_session = message.sender_session.expect("sender session");

    let loading = fetch(&pair.bob, &reference, &message);
    wait_for(&mut pair.bob_rx, "the consent question", |e| {
        matches!(e, EngineEvent::DirectConsentRequired).then_some(())
    })
    .await;
    let started = tokio::time::Instant::now();
    pair.bob.p2p_consent_decide(true).await.expect("decided");
    let data_url = loading.await.expect("task").expect("full image");
    assert!(data_url.starts_with("data:image/png;base64,"));
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "came directly: {:?}",
        started.elapsed()
    );
    let server = only_server(&pair.bob);
    assert!(pair.bob.p2p_connected(server, alice_session));
    assert_eq!(
        pair.bob
            .settings()
            .expect("settings")
            .allow_direct_connections,
        Consent::Yes,
        "the answer is remembered"
    );

    // Switching direct connections off closes the connection.
    let mut settings = pair.bob.settings().expect("settings");
    settings.allow_direct_connections = Consent::No;
    pair.bob.update_settings(settings).await.expect("off");
    assert!(!pair.bob.p2p_connected(server, alice_session));
    disconnect(&pair);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn declined_direct_connection_falls_back_to_plugin_data() {
    if !live() {
        return;
    }
    let (mut pair, alice_name) = pair("live_p2p_no", Consent::No, Consent::Yes).await;
    let (reference, message) = send_picture(&mut pair, &alice_name, picture(40, 30, 5)).await;
    let data_url = fetch(&pair.bob, &reference, &message)
        .await
        .expect("task")
        .expect("full image over plugin data");
    assert!(data_url.starts_with("data:image/png;base64,"));
    let alice_session = message.sender_session.expect("sender session");
    assert!(
        !pair
            .bob
            .p2p_connected(only_server(&pair.bob), alice_session)
    );
    disconnect(&pair);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn no_direct_path_falls_back_to_plugin_data() {
    if !live() {
        return;
    }
    let (mut pair, alice_name) = pair("live_p2p_blocked", Consent::Yes, Consent::Yes).await;
    // No time to reach Alice: as if every direct path were blocked.
    pair.bob.p2p_set_config(local_config(Duration::ZERO));
    let (reference, message) = send_picture(&mut pair, &alice_name, picture(40, 30, 6)).await;
    let data_url = fetch(&pair.bob, &reference, &message)
        .await
        .expect("task")
        .expect("full image over plugin data");
    assert!(data_url.starts_with("data:image/png;base64,"));
    let alice_session = message.sender_session.expect("sender session");
    assert!(
        !pair
            .bob
            .p2p_connected(only_server(&pair.bob), alice_session)
    );
    disconnect(&pair);
}
