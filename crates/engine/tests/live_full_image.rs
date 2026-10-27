//! Full-size images (FR-1.60c) against the local test server: Alice keeps the full image of a
//! picture and sends its thumbnail with the reference; the unmodified server passes the
//! attribute through, Bob's engine fetches the full image from Alice over plugin data (with
//! progress) and gets exactly her bytes. Once Alice is gone, Bob's fetch of another image
//! yields `None` (the viewer keeps the thumbnail). `testenv/up.sh v14`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_full_image -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use image::{ImageFormat, Rgb, RgbImage};
use mumbot::env;
use mumcord_engine::EventReceiver;
use mumcord_engine::full_image::FullRef;
use mumcord_engine::store::Consent;
use mumcord_engine::{Engine, EngineEvent, PasswordChange, SecretStore, ServerFields, ServerView};

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

async fn engine(dir: &Path, name: &str) -> (Engine, EventReceiver, ServerView) {
    let (engine, rx) = Engine::new(
        &dir.join(name),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine.identity_create(name.into()).await.expect("identity");
    // Plugin data only: the direct path is covered by live_p2p, and an unanswered consent
    // question would hold the fetch for a minute before it falls back.
    let mut settings = engine.settings().expect("settings");
    settings.allow_direct_connections = Consent::No;
    engine.update_settings(settings).await.expect("no direct");
    let server = engine
        .server_add(ServerFields {
            label: format!("test {} {name}", common::v14()),
            address: "127.0.0.1".into(),
            port: common::v14(),
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

/// A noisy PNG (does not compress), a few chunks large.
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

const THUMBNAIL: &str = "data:image/png;base64,iVBORw0KGgo=";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v14 (MUMCORD_LIVE=1)"]
async fn full_image_is_fetched_from_the_sender() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("live_full_image");
    let alice_name = unique("b66-alice");
    let bob_name = unique("b66-bob");
    let (alice, mut alice_rx, alice_server) = engine(&dir, &alice_name).await;
    let (bob, mut bob_rx, bob_server) = engine(&dir, &bob_name).await;
    connect(&alice, &mut alice_rx, &alice_server).await;
    connect(&bob, &mut bob_rx, &bob_server).await;

    let original = picture(40, 30, 7);
    let reference = alice.full_image_keep(original.clone()).await.expect("kept");
    assert_eq!(
        FullRef::parse(&reference, usize::MAX),
        Some(FullRef::of(&original)),
        "a small valid image is kept as it is"
    );
    let html = format!("<img src=\"{THUMBNAIL}\" data-mumcord-full=\"{reference}\" />");
    alice
        .send_channel_message(only_server(&alice), 0, &html)
        .await
        .expect("sent");

    let message = wait_for(&mut bob_rx, "the image message", |e| match e {
        EngineEvent::MessageReceived(m) if m.sender_name == alice_name => Some((**m).clone()),
        _ => None,
    })
    .await;
    assert!(
        message
            .body
            .contains(&format!("data-mumcord-full=\"{reference}\"")),
        "the server passes the reference through: {}",
        message.body
    );

    let fetch = {
        let bob = bob.clone();
        let reference = reference.clone();
        let cert = message.sender_cert_hash.clone();
        tokio::spawn(async move {
            bob.full_image_fetch(only_server(&bob), &reference, message.sender_session, cert)
                .await
        })
    };
    let total = u32::try_from(original.len()).expect("size");
    wait_for(&mut bob_rx, "the last progress event", |e| match e {
        EngineEvent::FullImageProgress {
            reference: r,
            received,
            total: t,
        } if *r == reference && *t == total && *received == total => Some(()),
        _ => None,
    })
    .await;
    let data_url = fetch.await.expect("task").expect("full image");
    assert!(data_url.starts_with("data:image/png;base64,"), "{data_url}");
    // The second open comes from the cache, without asking Alice again.
    assert_eq!(
        bob.full_image_fetch(only_server(&bob), &reference, None, None)
            .await,
        Some(data_url)
    );

    // Alice is gone: an image Bob does not have yet stays a thumbnail.
    let other = alice
        .full_image_keep(picture(20, 20, 9))
        .await
        .expect("kept");
    alice.disconnect(only_server(&alice));
    let started = tokio::time::Instant::now();
    assert_eq!(
        bob.full_image_fetch(only_server(&bob), &other, message.sender_session, None)
            .await,
        None
    );
    assert!(started.elapsed() < WAIT, "gives up quickly");
    bob.disconnect(only_server(&bob));
}

/// The test's only connected server (0, which no call accepts, when none is).
fn only_server(engine: &Engine) -> mumcord_engine::ServerId {
    engine
        .connections()
        .first()
        .and_then(mumcord_engine::ConnState::server_id)
        .unwrap_or(0)
}
