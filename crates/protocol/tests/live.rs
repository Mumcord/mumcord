//! Integration tests against the local test servers from `testenv/` (never public servers).
//! Start the matching server first, e.g. `testenv/up.sh v15`, then run
//! `cargo test -p mumcord-protocol --test live -- --ignored --test-threads=1`.
//! Note: the servers' default autoban (10 connections in 120 s) bans the host for 5 minutes, so
//! running the suite several times in a row fails with "tls handshake eof" until it expires.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use mumcord_protocol::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use mumcord_protocol::{
    ClientIdentity, CommandError, ConnectConfig, ConnectError, Connection, DenyKind,
    DisconnectReason, Event, TextTarget, connect,
};
use tokio::sync::mpsc::UnboundedReceiver;

const V15: u16 = 64738;
const V14: u16 = 64739;
const V13: u16 = 64740;
const LIMIT: u16 = 64741;
const WAIT: Duration = Duration::from_secs(10);

fn identity() -> ClientIdentity {
    let generated =
        rcgen::generate_simple_self_signed(vec!["mumcord-test".to_owned()]).expect("cert");
    ClientIdentity {
        chain: vec![CertificateDer::clone(generated.cert.der())],
        key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            generated.signing_key.serialize_der(),
        )),
    }
}

fn unique(prefix: &str) -> String {
    format!("{prefix}-{:08x}", rand::random::<u32>())
}

fn config(port: u16, name: &str, accept: bool) -> ConnectConfig {
    ConnectConfig {
        host: "127.0.0.1".to_owned(),
        port,
        username: name.to_owned(),
        password: std::env::var("MUMCORD_TEST_SERVER_PASSWORD")
            .ok()
            .filter(|p| !p.is_empty()),
        tokens: Vec::new(),
        identity: Some(identity()),
        verify_cert: Arc::new(move |_| accept),
        login_timeout: WAIT,
    }
}

async fn login(port: u16, prefix: &str) -> Connection {
    connect(config(port, &unique(prefix), true))
        .await
        .expect("login")
}

/// Waits for the first event matching `pred`, skipping others.
async fn wait_for<T>(
    events: &mut UnboundedReceiver<Event>,
    what: &str,
    mut pred: impl FnMut(&Event) -> Option<T>,
) -> T {
    tokio::time::timeout(WAIT, async {
        loop {
            let event = events.recv().await.expect("event stream ended");
            if let Some(found) = pred(&event) {
                return found;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for: {what}"))
}

async fn wait_own(
    conn: &mut Connection,
    what: &str,
    pred: impl Fn(&mumcord_protocol::User) -> bool,
) -> mumcord_protocol::User {
    let own = conn.handle.state().session.expect("session");
    wait_for(&mut conn.events, what, |e| match e {
        Event::UserUpdated(c) if c.user.session == own && pred(&c.user) => Some(c.user.clone()),
        Event::PermissionDenied(d) => panic!("denied: {d:?}"),
        _ => None,
    })
    .await
}

fn channel_id(conn: &Connection, name: &str) -> u32 {
    conn.handle
        .with_state(|s| s.child_by_name(0, name).map(|c| c.id))
        .expect(name)
}

async fn full_scenario(port: u16, server_stores_volume: bool) {
    let mut a = login(port, "alice").await;
    let mut b = login(port, "bob").await;
    let a_session = a.handle.state().session.expect("session");

    // Channel tree.
    let names: Vec<String> = a
        .handle
        .with_state(|s| s.children(0).iter().map(|c| c.name.clone()).collect());
    for expected in ["Lobby", "Raid", "AFK"] {
        assert!(
            names.iter().any(|n| n == expected),
            "missing {expected} in {names:?}"
        );
    }
    assert!(a.handle.state().server.supports_listening());
    assert!(a.handle.state().crypt.is_some_and(|c| c.key.is_some()));
    let raid = channel_id(&a, "Raid");
    let lobby = channel_id(&a, "Lobby");

    // Join Raid.
    a.handle.join_channel(raid).expect("send");
    wait_own(&mut a, "a joins Raid", |u| u.channel_id == raid).await;
    b.handle.join_channel(raid).expect("send");
    wait_own(&mut b, "b joins Raid", |u| u.channel_id == raid).await;

    // Channel text between two connections, and a private reply.
    a.handle
        .send_text(&TextTarget::channel(raid), "hello raid")
        .expect("send");
    let got = wait_for(&mut b.events, "b gets channel text", |e| match e {
        Event::TextMessage(m) => Some(m.clone()),
        _ => None,
    })
    .await;
    assert_eq!(got.actor, Some(a_session));
    assert_eq!(got.message, "hello raid");
    assert_eq!(got.target.channels, vec![raid]);
    b.handle
        .send_text(&TextTarget::user(a_session), "psst")
        .expect("send");
    let got = wait_for(&mut a.events, "a gets private text", |e| match e {
        Event::TextMessage(m) => Some(m.clone()),
        _ => None,
    })
    .await;
    assert_eq!(got.message, "psst");
    assert_eq!(got.target.sessions, vec![a_session]);

    // Follow Lobby: listener added, channel text arrives without joining, then removed.
    a.handle.listen(&[lobby]).expect("send");
    wait_own(&mut a, "a listens to Lobby", |u| {
        u.listening.contains(&lobby)
    })
    .await;
    if server_stores_volume {
        a.handle.set_listener_volume(lobby, 0.0).expect("send");
        wait_own(&mut a, "listener volume echoed", |u| {
            u.listening_volume.get(&lobby) == Some(&0.0)
        })
        .await;
    } else {
        assert!(matches!(
            a.handle.set_listener_volume(lobby, 0.0),
            Err(CommandError::Unsupported { .. })
        ));
    }
    wait_for(&mut b.events, "b sees a listening", |e| match e {
        Event::UserUpdated(c)
            if c.user.session == a_session && c.user.listening.contains(&lobby) =>
        {
            Some(())
        }
        _ => None,
    })
    .await;
    b.handle
        .send_text(&TextTarget::channel(lobby), "to lobby")
        .expect("send");
    let got = wait_for(
        &mut a.events,
        "a gets Lobby text as listener",
        |e| match e {
            Event::TextMessage(m) => Some(m.clone()),
            _ => None,
        },
    )
    .await;
    assert_eq!(got.message, "to lobby");
    a.handle.unlisten(&[lobby]).expect("send");
    wait_own(&mut a, "a stops listening", |u| {
        !u.listening.contains(&lobby)
    })
    .await;

    // Self deafen implies mute; comment and avatar.
    a.handle.set_self_deaf(true).expect("send");
    wait_own(&mut a, "self deaf", |u| u.self_deaf && u.self_mute).await;
    a.handle.set_comment("my comment".to_owned()).expect("send");
    wait_own(&mut a, "comment", |u| {
        u.comment.as_deref() == Some("my comment")
    })
    .await;
    a.handle
        .set_texture(vec![0x89, b'P', b'N', b'G'])
        .expect("send");
    wait_for(&mut b.events, "b sees avatar", |e| match e {
        Event::UserUpdated(c)
            if c.user.session == a_session
                && (c.user.texture.is_some() || c.user.texture_hash.is_some()) =>
        {
            Some(())
        }
        _ => None,
    })
    .await;

    // Self-registration.
    a.handle.register_self().expect("send");
    wait_own(&mut a, "self registration", |u| u.user_id.is_some()).await;

    // Disconnect is seen on both sides.
    a.handle.disconnect();
    let reason = wait_for(&mut a.events, "a disconnected", |e| match e {
        Event::Disconnected(r) => Some(r.clone()),
        _ => None,
    })
    .await;
    assert_eq!(reason, DisconnectReason::Requested);
    assert_eq!(a.handle.join_channel(raid), Err(CommandError::NotConnected));
    wait_for(&mut b.events, "b sees a leave", |e| match e {
        Event::UserRemoved { user, .. } if user.session == a_session => Some(()),
        _ => None,
    })
    .await;
    b.handle.disconnect();
}

#[tokio::test]
#[ignore = "needs testenv/up.sh v15"]
async fn v15_full_flow() {
    full_scenario(V15, true).await;
}

#[tokio::test]
#[ignore = "needs testenv/up.sh v14"]
async fn v14_full_flow() {
    full_scenario(V14, false).await;
}

#[tokio::test]
#[ignore = "needs testenv/up.sh v13"]
async fn v13_has_no_listening() {
    let conn = login(V13, "old").await;
    let version = conn.handle.server_version().expect("version");
    assert_eq!((version.major, version.minor), (1, 3));
    assert!(!conn.handle.state().server.supports_listening());
    assert!(matches!(
        conn.handle.listen(&[1]),
        Err(CommandError::Unsupported { .. })
    ));
    conn.handle.disconnect();
}

#[tokio::test]
#[ignore = "needs testenv/up.sh limit"]
async fn listener_limit_is_a_typed_denial() {
    let mut conn = login(LIMIT, "limited").await;
    let lobby = channel_id(&conn, "Lobby");
    conn.handle.listen(&[lobby]).expect("send");
    let denied = wait_for(&mut conn.events, "listener denial", |e| match e {
        Event::PermissionDenied(d) => Some(d.clone()),
        _ => None,
    })
    .await;
    assert!(
        matches!(
            denied.kind,
            DenyKind::ChannelListenerLimit | DenyKind::UserListenerLimit
        ),
        "{denied:?}"
    );
    conn.handle.disconnect();
}

#[tokio::test]
#[ignore = "needs testenv/up.sh v15"]
async fn rejected_certificate_reports_fingerprint() {
    let seen = Arc::new(Mutex::new(None));
    let mut cfg = config(V15, &unique("tofu"), false);
    let seen_in_cb = seen.clone();
    cfg.verify_cert = Arc::new(move |cert| {
        *seen_in_cb.lock().expect("lock") = Some(cert.sha256_hex.clone());
        false
    });
    match connect(cfg).await {
        Err(ConnectError::CertificateRejected(cert)) => {
            assert_eq!(cert.sha1_hex.len(), 40);
            assert_eq!(Some(cert.sha256_hex), seen.lock().expect("lock").clone());
        }
        other => panic!("expected CertificateRejected, got {:?}", other.err()),
    }
}

/// The server warns clients it believes predate 1.4 ("ChannelListener feature enabled but your
/// client version does not support it"); that happens when it handles Authenticate before our
/// Version message, so the warning must never arrive.
async fn no_old_client_warning(port: u16) {
    let mut conn = login(port, "ver").await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, conn.events.recv()).await {
        if let Event::TextMessage(m) = &event {
            assert!(
                !m.message.contains("ChannelListener"),
                "server treated us as an old client: {}",
                m.message
            );
        }
    }
    conn.handle.disconnect();
}

#[tokio::test]
#[ignore = "needs testenv/up.sh v15"]
async fn v15_sees_client_version_before_authenticate() {
    no_old_client_warning(V15).await;
}

#[tokio::test]
#[ignore = "needs testenv/up.sh v14"]
async fn v14_sees_client_version_before_authenticate() {
    no_old_client_warning(V14).await;
}
