//! Servers reachable only through a DNS SRV record (issue #859) against the local test server v15
//! and a DNS fixture inside the test (no public DNS). `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_srv -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::time::Duration;

use hickory_resolver::proto::op::{Message, ResponseCode};
use hickory_resolver::proto::rr::rdata::SRV;
use hickory_resolver::proto::rr::{Name, RData, Record};
use hickory_resolver::proto::serialize::binary::{BinDecodable, BinEncodable};
use mumbot::env;
use mumcord_engine::{
    ConnState, Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields,
    ServerTestInput, ServerView,
};
use tokio::net::UdpSocket;

const WAIT: Duration = Duration::from_secs(30);
/// The zone the fixture serves; it exists nowhere else, so only the SRV record can lead to the
/// test server.
const ZONE: &str = "srv.mumcord.test";
const DEFAULT_PORT: u16 = 64738;

/// Answers `_mumble._tcp.<ZONE>.` with `records`, everything else with NXDOMAIN.
async fn dns_fixture(records: Vec<SRV>) -> SocketAddr {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind");
    let addr = socket.local_addr().expect("addr");
    let wanted = Name::from_ascii(format!("_mumble._tcp.{ZONE}.")).expect("name");
    tokio::spawn(async move {
        let mut buffer = [0u8; 1500];
        loop {
            let Ok((len, from)) = socket.recv_from(&mut buffer).await else {
                return;
            };
            let Ok(query) = Message::from_bytes(&buffer[..len]) else {
                continue;
            };
            let mut reply = Message::response(query.metadata.id, query.metadata.op_code);
            reply.metadata.recursion_desired = query.metadata.recursion_desired;
            reply.metadata.recursion_available = true;
            reply.add_queries(query.queries.clone());
            match query.queries.first().map(|q| q.name().clone()) {
                Some(name) if name == wanted => {
                    for record in &records {
                        reply.add_answer(Record::from_rdata(
                            name.clone(),
                            60,
                            RData::SRV(record.clone()),
                        ));
                    }
                }
                _ => reply.metadata.response_code = ResponseCode::NXDomain,
            }
            let bytes = reply.to_bytes().expect("encode");
            let _ = socket.send_to(&bytes, from).await;
        }
    });
    addr
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

/// Connects and accepts the certificate when asked; returns how many trust prompts came.
async fn connect(engine: &Engine, rx: &mut EventReceiver, server: &ServerView) -> usize {
    engine.connect(server.id, None).expect("connect");
    let mut prompts = 0;
    loop {
        let event = wait_for(rx, "sync", |e| match e {
            EngineEvent::CertTrustRequired { .. } | EngineEvent::ServerSynced(_) => Some(e.clone()),
            _ => None,
        })
        .await;
        if let EngineEvent::CertTrustRequired {
            server_id,
            fingerprint,
            ..
        } = event
        {
            prompts += 1;
            engine
                .cert_trust_decide(server_id, &fingerprint, true)
                .expect("trust");
        } else {
            return prompts;
        }
    }
}

async fn disconnect(engine: &Engine, rx: &mut EventReceiver, server: &ServerView) {
    engine.disconnect(server.id);
    wait_for(rx, "disconnected", |e| {
        matches!(e, EngineEvent::StateChanged(ConnState::Disconnected { .. })).then_some(())
    })
    .await;
}

fn password() -> PasswordChange {
    match env::testenv_var("SERVER_PASSWORD") {
        Some(p) => PasswordChange::Set(p),
        None => PasswordChange::Keep,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn a_server_behind_an_srv_record_can_be_joined() {
    if !std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1") {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start the test server");
        return;
    }
    // The first record points at a closed port (an unreachable target), the second at v15.
    let dns = dns_fixture(vec![
        SRV::new(0, 0, 1, Name::from_ascii("localhost.").expect("name")),
        SRV::new(
            10,
            0,
            common::v15(),
            Name::from_ascii("localhost.").expect("name"),
        ),
    ])
    .await;
    // SAFETY: set before any other thread reads the environment; tests run with --test-threads=1.
    unsafe {
        std::env::set_var("MUMCORD_AUDIO_BACKEND", "null");
        std::env::set_var("MUMCORD_TEST_DNS", dns.to_string());
    }
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("live_srv");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create("SRV Tester".into(), None)
        .await
        .expect("identity");
    let name = format!("srv-{:06x}", std::process::id() & 0xff_ffff);

    // "Test connection" in the dialog follows the record too.
    let probe = |port| ServerTestInput {
        address: ZONE.into(),
        port,
        username: name.clone(),
        password: password(),
        server_id: None,
        identity_id: None,
    };
    let result = engine
        .server_test_connection(probe(DEFAULT_PORT))
        .await
        .expect("test connection through the SRV record");
    assert!(
        result.version.starts_with('1'),
        "version: {}",
        result.version
    );

    // An explicit port wins: no lookup, and the zone itself does not resolve.
    assert!(
        engine.server_test_connection(probe(12345)).await.is_err(),
        "an explicit port must not follow the SRV record"
    );

    // A saved server with only the domain connects through the second record after the first
    // target refused, and the trust decision stays with the saved server on reconnect.
    let server = engine
        .server_add(ServerFields {
            label: format!("srv test {name}"),
            address: ZONE.into(),
            port: DEFAULT_PORT,
            username: name.clone(),
            password: password(),
        })
        .await
        .expect("server");
    assert_eq!(connect(&engine, &mut rx, &server).await, 1);
    disconnect(&engine, &mut rx, &server).await;
    assert_eq!(
        connect(&engine, &mut rx, &server).await,
        0,
        "the trusted certificate must still match after a reconnect through the record"
    );
    disconnect(&engine, &mut rx, &server).await;
}
