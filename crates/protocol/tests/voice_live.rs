//! Voice transport against the local test servers from `testenv/` (never public servers).
//! Start the servers first (`testenv/up.sh v15`, `testenv/up.sh v14`), then run
//! `MUMCORD_LIVE=1 cargo test -p mumcord-protocol --test voice_live -- --ignored --test-threads=1`.
//! The fallback tests start `tools/netsim` themselves (via `cargo run`) on ports 64800–64803.

use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use mumcord_protocol::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use mumcord_protocol::{
    ClientIdentity, ConnectConfig, Event, Handle, OutgoingAudio, Transport, VoiceEvent,
    VoicePacket, VoiceTransport, connect,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc::UnboundedReceiver;

const V15: u16 = 64738;
const V14: u16 = 64739;
const WAIT: Duration = Duration::from_secs(10);

fn live() -> bool {
    let on = std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start testenv/up.sh v15 / v14");
    }
    on
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

/// A logged-in client with a running voice transport.
struct Client {
    handle: Handle,
    voice: Arc<VoiceTransport>,
    events: UnboundedReceiver<VoiceEvent>,
    session: u32,
}

async fn client(port: u16, prefix: &str) -> Client {
    let conn = connect(ConnectConfig {
        host: "127.0.0.1".to_owned(),
        port,
        username: format!("{prefix}-{:08x}", rand::random::<u32>()),
        password: std::env::var("MUMCORD_TEST_SERVER_PASSWORD")
            .ok()
            .filter(|p| !p.is_empty()),
        identity: Some(identity()),
        verify_cert: Arc::new(|_| true),
        login_timeout: WAIT,
    })
    .await
    .expect("login");
    let server: SocketAddr = ([127, 0, 0, 1], port).into();
    let (voice, events) = VoiceTransport::start(conn.handle.clone(), server)
        .await
        .expect("voice start");
    let voice = Arc::new(voice);
    let session = conn.handle.state().session.expect("session");

    // What the engine does: forward the voice-related control events.
    let forward = Arc::clone(&voice);
    let mut control = conn.events;
    tokio::spawn(async move {
        while let Some(event) = control.recv().await {
            match event {
                Event::CryptSetup(c) => forward.on_crypt_setup(c),
                Event::UdpTunnel(p) => forward.on_tunnel(p),
                _ => {}
            }
        }
    });
    Client {
        handle: conn.handle,
        voice,
        events,
        session,
    }
}

impl Client {
    async fn wait_transport(&mut self, want: Transport, limit: Duration) {
        if self.voice.transport() == want {
            return;
        }
        tokio::time::timeout(limit, async {
            loop {
                match self.events.recv().await.expect("voice stopped") {
                    VoiceEvent::TransportChanged(t) if t == want => return,
                    _ => {}
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("transport did not become {want:?}"));
    }

    /// Sends a burst of `n` packets (20 ms each) whose payload encodes `tag` and the index.
    async fn talk(&self, tag: u8, n: u8) {
        for i in 0..n {
            let payload = Bytes::from(vec![tag, i, 0xAB, 0xCD]);
            self.voice
                .send_audio(OutgoingAudio::new(payload, 2, i + 1 == n));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Collects the packets of `tag` from `from` until the terminator or a timeout.
    async fn hear(&mut self, from: u32, tag: u8) -> Vec<VoicePacket> {
        let mut got = Vec::new();
        let _ = tokio::time::timeout(WAIT, async {
            loop {
                if let VoiceEvent::Packet(p) = self.events.recv().await.expect("voice stopped")
                    && p.session == from
                    && p.payload.first() == Some(&tag)
                {
                    let last = p.is_last;
                    got.push(p);
                    if last {
                        return;
                    }
                }
            }
        })
        .await;
        got
    }

    fn drain(&mut self) {
        while self.events.try_recv().is_ok() {}
    }
}

/// Checks a received burst: (almost) complete, ordered seq numbers, all on `transport`.
fn check_burst(got: &[VoicePacket], n: u8, transport: Transport, via_listener: bool) {
    assert!(
        got.len() >= usize::from(n) * 8 / 10,
        "only {} of {n} packets arrived",
        got.len()
    );
    assert!(got.last().is_some_and(|p| p.is_last), "terminator missing");
    assert!(
        got.windows(2).all(|w| w[1].seq > w[0].seq),
        "seq not increasing"
    );
    assert!(
        got.windows(2).all(|w| (w[1].seq - w[0].seq) % 2 == 0),
        "seq not in frames"
    );
    for p in got {
        assert_eq!(p.transport, transport, "packet seq {}", p.seq);
        assert_eq!(p.via_listener, via_listener, "packet seq {}", p.seq);
        assert_eq!(&p.payload[2..], &[0xAB, 0xCD]);
    }
}

async fn udp_audio(port: u16) {
    let mut a = client(port, "va").await;
    let mut b = client(port, "vb").await;
    a.wait_transport(Transport::Udp, WAIT).await;
    b.wait_transport(Transport::Udp, WAIT).await;
    assert!(a.voice.stats().rtt.is_some());

    b.drain();
    a.talk(2, 50).await;
    let got = b.hear(a.session, 2).await;
    check_burst(&got, 50, Transport::Udp, false);
    let stats = b.voice.stats();
    assert!(stats.loss < 0.05, "loss {}", stats.loss);
    assert!(stats.crypt.good >= 50);
}

async fn listener(port: u16) {
    let mut a = client(port, "la").await;
    let mut b = client(port, "lb").await;
    let lobby = a
        .handle
        .with_state(|s| s.child_by_name(0, "Lobby").map(|c| c.id))
        .expect("Lobby");
    a.handle.join_channel(lobby).expect("join");
    b.handle.listen(&[lobby]).expect("listen");
    let ready = || {
        b.handle.with_state(|s| {
            s.users
                .get(&a.session)
                .is_some_and(|u| u.channel_id == lobby)
                && s.users
                    .get(&b.session)
                    .is_some_and(|u| u.listening.contains(&lobby))
        })
    };
    tokio::time::timeout(WAIT, async {
        while !ready() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("join + listen applied");

    a.wait_transport(Transport::Udp, WAIT).await;
    b.wait_transport(Transport::Udp, WAIT).await;
    b.drain();
    a.talk(3, 20).await;
    let got = b.hear(a.session, 3).await;
    check_burst(&got, 20, Transport::Udp, true);

    // Same channel again: normal delivery, not via the listener.
    b.handle.join_channel(lobby).expect("join");
    tokio::time::timeout(WAIT, async {
        while !b.handle.with_state(|s| {
            s.users
                .get(&b.session)
                .is_some_and(|u| u.channel_id == lobby)
        }) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("b joins Lobby");
    b.drain();
    a.talk(4, 20).await;
    let got = b.hear(a.session, 4).await;
    check_burst(&got, 20, Transport::Udp, false);
}

/// The netsim proxy as a child process; killed on drop.
struct Netsim {
    child: Child,
    proxy: u16,
    control: SocketAddr,
}

impl Netsim {
    async fn start(target: u16, proxy: u16, control: u16) -> Self {
        let child = Command::new(env!("CARGO"))
            .args(["run", "-q", "-p", "netsim", "--", "run"])
            .args(["--listen", &format!("127.0.0.1:{proxy}")])
            .args(["--target", &format!("127.0.0.1:{target}")])
            .args(["--control", &format!("127.0.0.1:{control}")])
            .stdout(Stdio::null())
            .spawn()
            .expect("spawn netsim");
        let netsim = Self {
            child,
            proxy,
            control: ([127, 0, 0, 1], control).into(),
        };
        // Allow for a first build of netsim.
        tokio::time::timeout(Duration::from_secs(180), async {
            while netsim.ctl("status").await.is_none() {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        })
        .await
        .expect("netsim did not start");
        netsim
    }

    async fn ctl(&self, command: &str) -> Option<String> {
        let mut stream = TcpStream::connect(self.control).await.ok()?;
        stream
            .write_all(format!("{command}\n").as_bytes())
            .await
            .ok()?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).await.ok()?;
        line.starts_with("ok").then_some(line)
    }
}

impl Drop for Netsim {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn fallback(port: u16, proxy: u16, control: u16) {
    let net = Netsim::start(port, proxy, control).await;
    let mut a = client(net.proxy, "fa").await;
    let mut b = client(port, "fb").await;
    a.wait_transport(Transport::Udp, WAIT).await;
    b.wait_transport(Transport::Udp, WAIT).await;

    // UDP blocked: a falls back to the tunnel, audio still flows both ways.
    net.ctl("block-udp on").await.expect("block");
    a.wait_transport(Transport::Tcp, Duration::from_secs(8))
        .await;
    assert_eq!(a.voice.stats().rtt, None);
    b.drain();
    a.talk(6, 20).await;
    check_burst(&b.hear(a.session, 6).await, 20, Transport::Udp, false);
    a.drain();
    b.talk(7, 20).await;
    check_burst(&a.hear(b.session, 7).await, 20, Transport::Tcp, false);

    // UDP back: a returns to UDP; the server keeps tunnelling to a until a talked over UDP.
    net.ctl("block-udp off").await.expect("unblock");
    a.wait_transport(Transport::Udp, Duration::from_secs(8))
        .await;
    b.drain();
    a.talk(8, 20).await;
    check_burst(&b.hear(a.session, 8).await, 20, Transport::Udp, false);
    a.drain();
    b.talk(9, 20).await;
    check_burst(&a.hear(b.session, 9).await, 20, Transport::Udp, false);
}

#[tokio::test]
#[ignore = "needs testenv v15 and MUMCORD_LIVE=1"]
async fn v15_udp_audio() {
    if live() {
        udp_audio(V15).await;
    }
}

#[tokio::test]
#[ignore = "needs testenv v14 and MUMCORD_LIVE=1"]
async fn v14_udp_audio() {
    if live() {
        udp_audio(V14).await;
    }
}

#[tokio::test]
#[ignore = "needs testenv v15 and MUMCORD_LIVE=1"]
async fn v15_listener() {
    if live() {
        listener(V15).await;
    }
}

#[tokio::test]
#[ignore = "needs testenv v14 and MUMCORD_LIVE=1"]
async fn v14_listener() {
    if live() {
        listener(V14).await;
    }
}

#[tokio::test]
#[ignore = "needs testenv v15, MUMCORD_LIVE=1 and tools/netsim"]
async fn v15_fallback_to_tcp_and_back() {
    if live() {
        fallback(V15, 64800, 64801).await;
    }
}

#[tokio::test]
#[ignore = "needs testenv v14, MUMCORD_LIVE=1 and tools/netsim"]
async fn v14_fallback_to_tcp_and_back() {
    if live() {
        fallback(V14, 64802, 64803).await;
    }
}
