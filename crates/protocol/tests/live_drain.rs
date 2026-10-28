//! Disconnecting right after sending must not lose what is still in the rate-limited queue
//! (issue #74). Runs against the local v15 test server (`testenv/up.sh v15`, never public servers);
//! `MUMCORD_PORT_BASE` selects another testenv instance, as in `testenv/env.sh`:
//! `cargo test -p mumcord-protocol --test live_drain -- --ignored`.

use std::sync::Arc;
use std::time::Duration;

use mumcord_protocol::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use mumcord_protocol::{ClientIdentity, ConnectConfig, Connection, Event, TextTarget, connect};

const WAIT: Duration = Duration::from_secs(10);
/// One more than the client's burst of 4, so the last one is still paced when we disconnect.
const SENT: usize = 5;

fn v15() -> u16 {
    std::env::var("MUMCORD_PORT_BASE")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(64738)
}

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

async fn login(prefix: &str) -> Connection {
    connect(ConnectConfig {
        host: "127.0.0.1".to_owned(),
        port: v15(),
        username: format!("{prefix}-{:08x}", rand::random::<u32>()),
        password: std::env::var("MUMCORD_TEST_SERVER_PASSWORD")
            .ok()
            .filter(|p| !p.is_empty()),
        tokens: Vec::new(),
        identity: Some(identity()),
        verify_cert: Arc::new(|_| true),
        login_timeout: WAIT,
    })
    .await
    .expect("login")
}

#[tokio::test]
#[ignore = "needs testenv/up.sh v15"]
async fn disconnect_right_after_sending_delivers_the_queue() {
    let sender = login("drain-a").await;
    let mut receiver = login("drain-b").await;
    let sender_session = sender.handle.state().session.expect("session");
    let target = TextTarget::user(receiver.handle.state().session.expect("session"));

    for i in 0..SENT {
        sender
            .handle
            .send_text(&target, &format!("queued {i}"))
            .expect("send");
    }
    sender.handle.disconnect();

    // The server handles a client's messages in order, so everything it got arrives before the
    // sender's leave.
    let received = tokio::time::timeout(WAIT, async {
        let mut received = Vec::new();
        loop {
            match receiver.events.recv().await.expect("event stream ended") {
                Event::TextMessage(m) if m.actor == Some(sender_session) => {
                    received.push(m.message);
                }
                Event::UserRemoved { user, .. } if user.session == sender_session => {
                    return received;
                }
                _ => {}
            }
        }
    })
    .await
    .expect("timed out waiting for the sender to leave");
    let expected: Vec<String> = (0..SENT).map(|i| format!("queued {i}")).collect();
    assert_eq!(received, expected);
    receiver.handle.disconnect();
}
