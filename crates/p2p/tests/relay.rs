//! Two clients on this machine connecting through an in-process fake relay that implements the
//! relay side of spec 04 section 6.5: QUIC and TLS-over-TCP listeners, client certificates,
//! pairing by token, `waiting`/`ready`, forwarding and error codes.

use std::time::{Duration, Instant};

use mumcord_p2p::{
    ConnectionPath, P2p, P2pConfig, P2pError, PeerConnection, RelayAnnouncement, RelayRefusal,
    RelayState, RelayToken, RelayTransport, Side,
};
use mumcord_protocol::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use mumcord_protocol::{ClientIdentity, cert_hash};

const TIMEOUT: Duration = Duration::from_secs(5);

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

fn token(fill: char) -> RelayToken {
    RelayToken::parse(&fill.to_string().repeat(43)).expect("token")
}

/// Connects both sides at once and returns (offering, answering).
async fn connect_pair(
    alice: (&ClientIdentity, &RelayAnnouncement),
    bob: (&ClientIdentity, &RelayAnnouncement),
    hashes: (&str, &str),
    token: &RelayToken,
) -> (
    Result<PeerConnection, P2pError>,
    Result<PeerConnection, P2pError>,
) {
    let (alice_p2p, bob_p2p) = (p2p(alice.0), p2p(bob.0));
    tokio::join!(
        alice_p2p.connect_via_relay(alice.1, token, hashes.1, Side::Offering),
        bob_p2p.connect_via_relay(bob.1, token, hashes.0, Side::Answering),
    )
}

async fn ping_pong(a: &PeerConnection, b: &PeerConnection) {
    let (mut send, mut recv) = a.connection().open_bi().await.expect("open");
    send.write_all(b"ping").await.expect("write");
    send.finish().expect("finish");
    let (mut b_send, mut b_recv) = b.connection().accept_bi().await.expect("accept");
    assert_eq!(b_recv.read_to_end(64).await.expect("read"), b"ping");
    b_send.write_all(b"pong").await.expect("write back");
    b_send.finish().expect("finish back");
    assert_eq!(recv.read_to_end(64).await.expect("read back"), b"pong");
}

#[tokio::test]
async fn connects_over_udp_and_carries_a_stream() {
    let relay = fake::Relay::start().await;
    let ((alice, alice_hash), (bob, bob_hash)) = (identity(), identity());
    let token = token('a');
    relay.pair(&token, &alice_hash, &bob_hash);

    let (a, b) = connect_pair(
        (&alice, &relay.announce()),
        (&bob, &relay.announce()),
        (&alice_hash, &bob_hash),
        &token,
    )
    .await;
    let (a, b) = (a.expect("offering side"), b.expect("answering side"));
    assert_eq!(a.path(), ConnectionPath::Relay(RelayTransport::Udp));
    assert_eq!(b.path(), ConnectionPath::Relay(RelayTransport::Udp));
    assert_eq!(a.peer_hash(), bob_hash);
    assert_eq!(b.peer_hash(), alice_hash);
    ping_pong(&a, &b).await;
}

#[tokio::test]
async fn falls_back_to_tcp_when_udp_is_blocked() {
    let relay = fake::Relay::start().await;
    let ((alice, alice_hash), (bob, bob_hash)) = (identity(), identity());
    let token = token('b');
    relay.pair(&token, &alice_hash, &bob_hash);

    let started = Instant::now();
    let (a, b) = connect_pair(
        (&alice, &relay.announce_udp_blocked()),
        (&bob, &relay.announce_udp_blocked()),
        (&alice_hash, &bob_hash),
        &token,
    )
    .await;
    let (a, b) = (a.expect("offering side"), b.expect("answering side"));
    assert_eq!(a.path(), ConnectionPath::Relay(RelayTransport::Tcp));
    assert_eq!(b.path(), ConnectionPath::Relay(RelayTransport::Tcp));
    // TCP starts after QUIC's 2 s head start.
    assert!(started.elapsed() >= Duration::from_secs(2));
    ping_pong(&a, &b).await;
}

#[tokio::test]
async fn connects_one_side_over_udp_and_the_other_over_tcp() {
    let relay = fake::Relay::start().await;
    let ((alice, alice_hash), (bob, bob_hash)) = (identity(), identity());
    let token = token('c');
    relay.pair(&token, &alice_hash, &bob_hash);

    let (a, b) = connect_pair(
        (&alice, &relay.announce()),
        (&bob, &relay.announce_udp_blocked()),
        (&alice_hash, &bob_hash),
        &token,
    )
    .await;
    let (a, b) = (a.expect("offering side"), b.expect("answering side"));
    assert_eq!(a.path(), ConnectionPath::Relay(RelayTransport::Udp));
    assert_eq!(b.path(), ConnectionPath::Relay(RelayTransport::Tcp));
    ping_pong(&b, &a).await;
}

#[tokio::test]
async fn closes_when_the_relay_link_is_lost() {
    let relay = fake::Relay::start().await;
    let ((alice, alice_hash), (bob, bob_hash)) = (identity(), identity());
    let token = token('d');
    relay.pair(&token, &alice_hash, &bob_hash);

    let (a, b) = connect_pair(
        (&alice, &relay.announce_udp_blocked()),
        (&bob, &relay.announce_udp_blocked()),
        (&alice_hash, &bob_hash),
        &token,
    )
    .await;
    let (a, _b) = (a.expect("offering side"), b.expect("answering side"));
    drop(relay);
    tokio::time::timeout(Duration::from_secs(2), a.connection().closed())
        .await
        .expect("closed soon after the relay went away");
}

#[tokio::test]
async fn refuses_a_relay_with_another_certificate() {
    let relay = fake::Relay::start().await;
    let (alice, _) = identity();
    let (_, bob_hash) = identity();
    let mut announcement = relay.announce();
    announcement.fingerprint = [0; 32];

    let started = Instant::now();
    let result = p2p(&alice)
        .connect_via_relay(&announcement, &token('e'), &bob_hash, Side::Offering)
        .await;
    assert!(
        matches!(result, Err(P2pError::RelayCertMismatch)),
        "{:?}",
        result.err()
    );
    assert!(started.elapsed() < Duration::from_secs(2), "fails at once");
}

#[tokio::test]
async fn reports_the_relay_error_code() {
    let relay = fake::Relay::start().await;
    let ((alice, alice_hash), (_, bob_hash)) = (identity(), identity());
    let (carol, _) = identity();
    let token = token('f');
    relay.pair(&token, &alice_hash, &bob_hash);

    let unknown = p2p(&alice)
        .connect_via_relay(
            &relay.announce(),
            &self::token('g'),
            &bob_hash,
            Side::Offering,
        )
        .await;
    assert!(
        matches!(
            unknown,
            Err(P2pError::RelayRefused(RelayRefusal::UnknownToken))
        ),
        "{:?}",
        unknown.err()
    );

    let stranger = p2p(&carol)
        .connect_via_relay(
            &relay.announce_udp_blocked(),
            &token,
            &bob_hash,
            Side::Offering,
        )
        .await;
    assert!(
        matches!(
            stranger,
            Err(P2pError::RelayRefused(RelayRefusal::NotAllowed))
        ),
        "{:?}",
        stranger.err()
    );
}

#[tokio::test]
async fn reports_an_unreachable_relay_within_the_setup_timeout() {
    let blackhole = std::net::UdpSocket::bind("127.0.0.1:0").expect("udp");
    let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("tcp");
    let announcement = RelayAnnouncement {
        host: "127.0.0.1".to_owned(),
        udp: blackhole.local_addr().expect("addr").port(),
        tcp: closed.local_addr().expect("addr").port(),
        fingerprint: [0; 32],
        state: RelayState::Up,
        seq: 1,
    };
    drop(closed);
    let (alice, _) = identity();
    let (_, bob_hash) = identity();

    let started = Instant::now();
    let result = p2p(&alice)
        .connect_via_relay(&announcement, &token('h'), &bob_hash, Side::Offering)
        .await;
    assert!(
        matches!(result, Err(P2pError::RelayUnreachable)),
        "{:?}",
        result.err()
    );
    assert!(started.elapsed() < Duration::from_secs(11));
}

/// Test-only relay: just enough of mumpeer to exercise the client.
mod fake {
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use bytes::Bytes;
    use mumcord_p2p::quinn;
    use mumcord_p2p::{RelayAnnouncement, RelayState, RelayToken};
    use mumcord_protocol::cert_hash;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, UnixTime};
    use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
    use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};
    use sha2::{Digest, Sha256};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::{mpsc, oneshot};
    use tokio::task::JoinSet;

    const CONTROL: u8 = 0x00;
    const INNER: u8 = 0x01;

    /// What the relay sends to one bound client: control JSON or an inner packet.
    type Out = mpsc::Sender<(u8, Bytes)>;

    struct Pair {
        hashes: [String; 2],
        bound: HashSet<String>,
        /// First side to bind: where to reach it, and how to hand it the partner.
        waiting: Option<(Out, oneshot::Sender<Out>)>,
    }

    #[derive(Default)]
    struct State {
        pairs: Mutex<HashMap<String, Pair>>,
    }

    pub struct Relay {
        udp: u16,
        tcp: u16,
        /// UDP port that swallows everything, to make QUIC hang like on a blocking network.
        blackhole: std::net::UdpSocket,
        fingerprint: [u8; 32],
        state: Arc<State>,
        // Dropping the relay stops every listener and session.
        _tasks: JoinSet<()>,
    }

    impl Relay {
        pub async fn start() -> Self {
            let generated =
                rcgen::generate_simple_self_signed(vec!["relay".to_owned()]).expect("relay cert");
            let fingerprint = Sha256::digest(generated.cert.der()).into();
            let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .expect("tls 1.3")
            .with_client_cert_verifier(Arc::new(AnyClientCert))
            .with_single_cert(
                vec![CertificateDer::clone(generated.cert.der())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                    generated.signing_key.serialize_der(),
                )),
            )
            .expect("server config");
            config.alpn_protocols = vec![b"mumpeer/1".to_vec()];
            let config = Arc::new(config);

            let quic = quinn::ServerConfig::with_crypto(Arc::new(
                quinn::crypto::rustls::QuicServerConfig::try_from(config.clone())
                    .expect("quic config"),
            ));
            let endpoint =
                quinn::Endpoint::server(quic, "127.0.0.1:0".parse().expect("addr")).expect("quic");
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("tcp");
            let state = Arc::new(State::default());
            let mut relay = Self {
                udp: endpoint.local_addr().expect("addr").port(),
                tcp: listener.local_addr().expect("addr").port(),
                blackhole: std::net::UdpSocket::bind("127.0.0.1:0").expect("blackhole"),
                fingerprint,
                state: state.clone(),
                _tasks: JoinSet::new(),
            };
            let quic_state = state.clone();
            relay._tasks.spawn(async move {
                let mut sessions = JoinSet::new();
                while let Some(incoming) = endpoint.accept().await {
                    sessions.spawn(serve_quic(incoming, quic_state.clone()));
                }
            });
            let acceptor = tokio_rustls::TlsAcceptor::from(config);
            relay._tasks.spawn(async move {
                let mut sessions = JoinSet::new();
                while let Ok((stream, _)) = listener.accept().await {
                    sessions.spawn(serve_tcp(stream, acceptor.clone(), state.clone()));
                }
            });
            relay
        }

        pub fn pair(&self, token: &RelayToken, a: &str, b: &str) {
            self.state.pairs.lock().expect("lock").insert(
                token.expose().to_owned(),
                Pair {
                    hashes: [a.to_owned(), b.to_owned()],
                    bound: HashSet::new(),
                    waiting: None,
                },
            );
        }

        pub fn announce(&self) -> RelayAnnouncement {
            RelayAnnouncement {
                host: "127.0.0.1".to_owned(),
                udp: self.udp,
                tcp: self.tcp,
                fingerprint: self.fingerprint,
                state: RelayState::Up,
                seq: 1,
            }
        }

        pub fn announce_udp_blocked(&self) -> RelayAnnouncement {
            RelayAnnouncement {
                udp: self.blackhole.local_addr().expect("addr").port(),
                ..self.announce()
            }
        }
    }

    enum Bound {
        Ready(Out),
        Waiting(oneshot::Receiver<Out>),
    }

    /// Pairs a `bind`; returns the partner's sender, or the error code to send.
    fn bind(state: &State, message: &[u8], hash: &str, me: &Out) -> Result<Bound, &'static str> {
        let message: serde_json::Value =
            serde_json::from_slice(message).map_err(|_| "bad_request")?;
        if message["t"] != "bind" {
            return Err("bad_request");
        }
        if message["v"] != 1 {
            return Err("version");
        }
        let token = message["tok"].as_str().ok_or("bad_request")?;
        let mut pairs = state.pairs.lock().expect("lock");
        let pair = pairs.get_mut(token).ok_or("unknown_token")?;
        if !pair.hashes.iter().any(|h| h == hash) || !pair.bound.insert(hash.to_owned()) {
            return Err("not_allowed");
        }
        Ok(match pair.waiting.take() {
            Some((first, hand_over)) => {
                // `ready` goes out before any forwarded packet on both links.
                let ready = control(r#"{"v":1,"t":"ready"}"#);
                first.try_send(ready.clone()).expect("queue");
                me.try_send(ready).expect("queue");
                drop(hand_over.send(me.clone()));
                Bound::Ready(first)
            }
            None => {
                let (hand_over, partner) = oneshot::channel();
                pair.waiting = Some((me.clone(), hand_over));
                me.try_send(control(r#"{"v":1,"t":"waiting"}"#))
                    .expect("queue");
                Bound::Waiting(partner)
            }
        })
    }

    fn control(json: &str) -> (u8, Bytes) {
        (CONTROL, Bytes::copy_from_slice(json.as_bytes()))
    }

    fn error(code: &str) -> (u8, Bytes) {
        control(&format!(r#"{{"v":1,"t":"error","code":"{code}"}}"#))
    }

    async fn partner(bound: Bound) -> Option<Out> {
        match bound {
            Bound::Ready(partner) => Some(partner),
            Bound::Waiting(partner) => partner.await.ok(),
        }
    }

    async fn serve_quic(incoming: quinn::Incoming, state: Arc<State>) {
        let Ok(connection) = incoming.await else {
            return;
        };
        let hash = connection
            .peer_identity()
            .and_then(|id| id.downcast::<Vec<CertificateDer<'static>>>().ok())
            .and_then(|chain| chain.first().map(|c| cert_hash(c.as_ref())))
            .expect("client certificate");
        let Ok((mut send, recv)) = connection.accept_bi().await else {
            return;
        };
        let mut line = Vec::new();
        let mut recv = BufReader::new(recv);
        if recv.read_until(b'\n', &mut line).await.is_err() {
            return;
        }
        let (me, mut out) = mpsc::channel::<(u8, Bytes)>(256);
        let bound = bind(&state, line.trim_ascii_end(), &hash, &me);
        let writer = connection.clone();
        let mut tasks = JoinSet::new();
        tasks.spawn(async move {
            while let Some((kind, payload)) = out.recv().await {
                if kind == CONTROL {
                    let mut line = payload.to_vec();
                    line.push(b'\n');
                    if send.write_all(&line).await.is_err() {
                        return;
                    }
                } else {
                    drop(writer.send_datagram(payload));
                }
            }
            drop(send.finish());
        });
        let bound = match bound {
            Ok(bound) => bound,
            Err(code) => {
                me.send(error(code)).await.expect("queue");
                drop(me);
                // Give the client time to read the error before the connection closes.
                drop(tokio::time::timeout(Duration::from_secs(2), connection.closed()).await);
                return;
            }
        };
        let Some(partner) = partner(bound).await else {
            return;
        };
        while let Ok(packet) = connection.read_datagram().await {
            if partner.send((INNER, packet)).await.is_err() {
                break;
            }
        }
    }

    async fn serve_tcp(stream: TcpStream, acceptor: tokio_rustls::TlsAcceptor, state: Arc<State>) {
        let Ok(stream) = acceptor.accept(stream).await else {
            return;
        };
        let hash = stream
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|chain| chain.first().map(|c| cert_hash(c.as_ref())))
            .expect("client certificate");
        let (mut read, mut write) = tokio::io::split(stream);
        let Some((CONTROL, message)) = read_frame(&mut read).await else {
            return;
        };
        let (me, mut out) = mpsc::channel::<(u8, Bytes)>(256);
        let bound = bind(&state, &message, &hash, &me);
        let mut tasks = JoinSet::new();
        tasks.spawn(async move {
            while let Some((kind, payload)) = out.recv().await {
                let len = u16::try_from(payload.len())
                    .expect("small frame")
                    .to_be_bytes();
                let frame = [&[kind][..], &len, &payload].concat();
                if write.write_all(&frame).await.is_err() {
                    return;
                }
            }
            drop(write.shutdown().await);
        });
        let bound = match bound {
            Ok(bound) => bound,
            Err(code) => {
                me.send(error(code)).await.expect("queue");
                drop(me);
                drop(tasks.join_next().await);
                return;
            }
        };
        let Some(partner) = partner(bound).await else {
            return;
        };
        while let Some((kind, packet)) = read_frame(&mut read).await {
            if kind == INNER && partner.send((INNER, packet.into())).await.is_err() {
                break;
            }
        }
    }

    async fn read_frame<R: AsyncReadExt + Unpin>(reader: &mut R) -> Option<(u8, Vec<u8>)> {
        let mut head = [0u8; 3];
        reader.read_exact(&mut head).await.ok()?;
        let mut payload = vec![0; usize::from(u16::from_be_bytes([head[1], head[2]]))];
        reader.read_exact(&mut payload).await.ok()?;
        Some((head[0], payload))
    }

    /// Requires a client certificate and accepts any (proof of key possession comes from the
    /// handshake); the relay checks the hash at `bind`.
    #[derive(Debug)]
    struct AnyClientCert;

    impl ClientCertVerifier for AnyClientCert {
        fn root_hint_subjects(&self) -> &[DistinguishedName] {
            &[]
        }

        fn verify_client_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _now: UnixTime,
        ) -> Result<ClientCertVerified, rustls::Error> {
            Ok(ClientCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Err(rustls::Error::General("TLS 1.2 is not used".to_owned()))
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls13_signature(
                message,
                cert,
                dss,
                &rustls::crypto::ring::default_provider().signature_verification_algorithms,
            )
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }
}
