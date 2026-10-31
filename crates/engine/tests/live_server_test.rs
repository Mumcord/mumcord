//! Test connection of the add/edit dialog (issue #151; AC-3.22 – AC-3.24) against the local test
//! server v15. `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_server_test -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::Path;
use std::time::Duration;

use mumbot::env;
use mumcord_engine::{
    CertificateState, Engine, EngineEvent, ErrorKind, EventReceiver, PasswordChange, SecretStore,
    ServerFields, ServerTestInput,
};

const WAIT: Duration = Duration::from_secs(20);

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

fn input(port: u16, name: &str) -> ServerTestInput {
    ServerTestInput {
        address: "127.0.0.1".into(),
        port,
        username: name.into(),
        password: PasswordChange::Keep,
        server_id: None,
        identity_id: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn a_test_connection_reports_version_ping_and_certificate_state() {
    if !std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1") {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start the test server");
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("live_server_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    let name = format!("st-{:06x}", std::process::id() & 0xff_ffff);
    engine
        .identity_create(name.clone(), None)
        .await
        .expect("identity");

    // A new server (no trust decision yet) reports the version, a ping and "unknown".
    let result = engine
        .server_test_connection(input(common::v15(), &name))
        .await
        .expect("test connection");
    assert!(
        result.version.starts_with('1'),
        "version: {}",
        result.version
    );
    assert!(result.ping_ms < 5_000, "ping: {}", result.ping_ms);
    assert_eq!(result.certificate, CertificateState::Unknown);
    assert!(!result.fingerprint.is_empty());
    assert!(
        engine.connections().is_empty(),
        "a test must not become a connection"
    );

    // A saved, trusted server reports "trusted" and uses its saved password.
    let server = engine
        .server_add(ServerFields {
            label: "test".into(),
            address: "127.0.0.1".into(),
            port: common::v15(),
            username: name.clone(),
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server");
    engine.connect(server.id, None).expect("connect");
    loop {
        let event = wait_for(&mut rx, "sync", |event| match event {
            EngineEvent::CertTrustRequired { .. } => Some(event.clone()),
            EngineEvent::ServerSynced(snapshot) if snapshot.server_id == server.id => {
                Some(event.clone())
            }
            _ => None,
        })
        .await;
        match event {
            EngineEvent::CertTrustRequired { fingerprint, .. } => engine
                .cert_trust_decide(server.id, &fingerprint, true)
                .expect("trust"),
            _ => break,
        }
    }
    let trusted = engine
        .server_test_connection(ServerTestInput {
            server_id: Some(server.id),
            ..input(common::v15(), &name)
        })
        .await
        .expect("test trusted server");
    assert_eq!(trusted.certificate, CertificateState::Trusted);
    assert_eq!(trusted.version, result.version);
    engine.disconnect(server.id);

    // A wrong password is reported as such, with no figures.
    if let Some(password) = env::testenv_var("SERVER_PASSWORD") {
        let wrong = engine
            .server_test_connection(ServerTestInput {
                password: PasswordChange::Set(format!("wrong-{password}")),
                ..input(common::v15(), &name)
            })
            .await
            .expect_err("wrong password");
        assert_eq!(wrong.kind, ErrorKind::WrongPassword, "{wrong}");
    }

    // Nothing listens on this port: the result says the server is unreachable.
    let unreachable = engine
        .server_test_connection(input(common::v15() + 50, &name))
        .await
        .expect_err("unreachable");
    assert_eq!(
        unreachable.kind,
        ErrorKind::ServerUnreachable,
        "{unreachable}"
    );

    // An empty address is refused before any network access.
    let invalid = engine
        .server_test_connection(ServerTestInput {
            address: "  ".into(),
            ..input(common::v15(), &name)
        })
        .await
        .expect_err("invalid");
    assert_eq!(invalid.kind, ErrorKind::InvalidInput);

    engine.shutdown().await;
}
