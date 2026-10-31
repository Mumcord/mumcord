//! Two endpoints on this machine: the offer/answer flow, the certificate pin and the failure
//! paths. Router port mapping (PCP, NAT-PMP, UPnP) needs a real router and is tested by hand.

use std::time::{Duration, Instant};

use mumcord_p2p::{DeclineReason, P2p, P2pConfig, P2pError, SessionId, Signal, SignalError};
use mumcord_protocol::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use mumcord_protocol::{ClientIdentity, cert_hash};

const TIMEOUT: Duration = Duration::from_secs(3);

fn identity() -> (ClientIdentity, String) {
    let generated =
        rcgen::generate_simple_self_signed(vec!["mumcord-test".to_owned()]).expect("cert");
    let hash = cert_hash(generated.cert.der());
    let identity = ClientIdentity {
        chain: vec![CertificateDer::clone(generated.cert.der())],
        key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            generated.signing_key.serialize_der(),
        )),
    };
    (identity, hash)
}

fn p2p(identity: &ClientIdentity) -> P2p {
    P2p::new(
        identity,
        P2pConfig {
            connect_timeout: TIMEOUT,
            port_mapping: false,
            allow_loopback: true,
        },
    )
}

/// Sends a signal the way plugin data would: encoded, then decoded on the other side.
fn carry(signal: &Signal) -> Signal {
    Signal::decode(&signal.encode().expect("encode")).expect("decode")
}

#[tokio::test]
async fn connects_and_carries_a_stream() {
    let (alice, alice_hash) = identity();
    let (bob, bob_hash) = identity();

    let offer = p2p(&alice).offer(&bob_hash).await.expect("offer");
    let answer = p2p(&bob)
        .answer(&carry(offer.signal()), &alice_hash)
        .await
        .expect("answer");
    let reply = carry(answer.signal());

    let (a, b) = tokio::join!(offer.complete(&reply), answer.connect());
    let (a, b) = (a.expect("offering side"), b.expect("answering side"));
    assert_eq!(a.peer_hash(), bob_hash);
    assert_eq!(b.peer_hash(), alice_hash);

    let (mut send, mut recv) = a.connection().open_bi().await.expect("open");
    send.write_all(b"ping").await.expect("write");
    send.finish().expect("finish");
    let (mut back, mut incoming) = b.connection().accept_bi().await.expect("accept");
    let got = incoming.read_to_end(64).await.expect("read");
    assert_eq!(got, b"ping");
    back.write_all(b"pong").await.expect("write back");
    back.finish().expect("finish back");
    assert_eq!(recv.read_to_end(64).await.expect("read back"), b"pong");
}

#[tokio::test]
async fn wrong_certificate_is_rejected() {
    let (alice, alice_hash) = identity();
    let (bob, _) = identity();
    let (_, someone_else) = identity();

    // Alice expects someone else; Bob answers with his own certificate.
    let offer = p2p(&alice).offer(&someone_else).await.expect("offer");
    let answer = p2p(&bob)
        .answer(&carry(offer.signal()), &alice_hash)
        .await
        .expect("answer");
    let reply = carry(answer.signal());

    let started = Instant::now();
    let (a, b) = tokio::join!(offer.complete(&reply), answer.connect());
    assert!(matches!(a, Err(P2pError::CertMismatch)), "{:?}", a.err());
    assert!(b.is_err());
    assert!(
        started.elapsed() < TIMEOUT / 2,
        "a wrong certificate ends the attempt without waiting for the timeout"
    );
}

/// Identities as Mumcord creates them (RSA, like most real Mumble certificates).
fn rsa_identity(name: &str) -> (ClientIdentity, String) {
    let generated = mumcord_store::Identity::generate(name).expect("rsa identity");
    let identity = ClientIdentity {
        chain: vec![CertificateDer::from(generated.cert_der().to_vec())],
        key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(generated.key_pkcs8_der().to_vec())),
    };
    (identity, generated.cert_hash())
}

#[tokio::test]
async fn rsa_certificates_connect() {
    let (alice, alice_hash) = rsa_identity("alice");
    let (bob, bob_hash) = rsa_identity("bob");

    let offer = p2p(&alice).offer(&bob_hash).await.expect("offer");
    let answer = p2p(&bob)
        .answer(&carry(offer.signal()), &alice_hash)
        .await
        .expect("answer");
    let reply = carry(answer.signal());

    let (a, b) = tokio::join!(offer.complete(&reply), answer.connect());
    assert_eq!(a.expect("offering side").peer_hash(), bob_hash);
    assert_eq!(b.expect("answering side").peer_hash(), alice_hash);
}

#[tokio::test]
async fn unreachable_peer_gives_no_direct_path_in_time() {
    let (alice, _) = identity();
    let (_, bob_hash) = identity();
    let offer = p2p(&alice).offer(&bob_hash).await.expect("offer");
    // TEST-NET-1: never answers.
    let reply = Signal::Answer {
        session: offer.signal().session(),
        candidates: vec!["192.0.2.1:9".parse().expect("addr")],
        relay: false,
    };
    let started = Instant::now();
    let result = offer.complete(&reply).await;
    assert!(
        matches!(result, Err(P2pError::NoDirectPath)),
        "{:?}",
        result.err()
    );
    assert!(started.elapsed() < TIMEOUT + Duration::from_secs(2));
}

#[tokio::test]
async fn decline_and_foreign_replies_end_the_attempt() {
    let (alice, _) = identity();
    let (_, bob_hash) = identity();
    let node = p2p(&alice);

    let offer = node.offer(&bob_hash).await.expect("offer");
    let decline = Signal::Decline {
        session: offer.signal().session(),
        reason: DeclineReason::Declined,
    };
    let result = offer.complete(&decline).await;
    assert!(matches!(
        result,
        Err(P2pError::Declined(DeclineReason::Declined))
    ));

    let offer = node.offer(&bob_hash).await.expect("offer");
    let other_session = Signal::Answer {
        session: SessionId(offer.signal().session().0.wrapping_add(1)),
        candidates: vec![],
        relay: false,
    };
    let result = offer.complete(&other_session).await;
    assert!(matches!(
        result,
        Err(P2pError::BadSignal(SignalError::Unexpected))
    ));

    let not_an_offer = Signal::Answer {
        session: SessionId(1),
        candidates: vec![],
        relay: false,
    };
    let result = node.answer(&not_an_offer, &bob_hash).await;
    assert!(matches!(
        result,
        Err(P2pError::BadSignal(SignalError::Unexpected))
    ));
}

#[tokio::test]
async fn invalid_peer_hash_is_refused() {
    let (alice, _) = identity();
    for hash in ["", "abc", &"g".repeat(40), &"a".repeat(41)] {
        let result = p2p(&alice).offer(hash).await;
        assert!(matches!(result, Err(P2pError::BadPeerHash)), "{hash}");
    }
}
