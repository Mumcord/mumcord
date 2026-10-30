//! Wrong server password, then a retry with the right one (FR-2.59, AC-2.40) against a local
//! test server that has a connect password.
//! `SERVER_PASSWORD=<pw> testenv/up.sh v14`, then:
//! `MUMCORD_LIVE=1 SERVER_PASSWORD=<pw> cargo test -p mumcord-engine --features test-backend --test live_wrong_password -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::time::Duration;

use mumbot::env;
use mumcord_engine::{
    ConnState, Engine, EngineEvent, ErrorKind, EventReceiver, PasswordChange, SecretStore,
    ServerFields,
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

/// Trusts the test server's certificate when asked, then waits for `done`.
async fn until<T>(
    engine: &Engine,
    rx: &mut EventReceiver,
    what: &str,
    mut done: impl FnMut(&EngineEvent) -> Option<T>,
) -> T {
    loop {
        let event = wait_for(rx, what, |e| match e {
            EngineEvent::CertTrustRequired { .. } => Some(Err(e.clone())),
            _ => done(e).map(Ok),
        })
        .await;
        match event {
            Ok(found) => return found,
            Err(EngineEvent::CertTrustRequired {
                server_id,
                fingerprint,
                ..
            }) => engine
                .cert_trust_decide(server_id, &fingerprint, true)
                .expect("trust"),
            Err(_) => unreachable!(),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v14 with SERVER_PASSWORD (MUMCORD_LIVE=1)"]
async fn wrong_password_is_rejected_and_the_retry_connects() {
    if !std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1") {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start the test servers");
        return;
    }
    let Some(right) = env::testenv_var("SERVER_PASSWORD") else {
        eprintln!("skipped: the test server needs a SERVER_PASSWORD");
        return;
    };
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("live_wrong_password");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create("Password Tester".into(), None)
        .await
        .expect("identity");
    let server = engine
        .server_add(ServerFields {
            label: "test password".into(),
            address: "127.0.0.1".into(),
            port: common::v14(),
            username: format!("b41v-{:06x}", std::process::id() & 0xff_ffff),
            password: PasswordChange::Set(format!("{right}-wrong")),
        })
        .await
        .expect("server");

    engine.connect(server.id, None).expect("connect");
    let kind = until(&engine, &mut rx, "rejection", |e| match e {
        EngineEvent::StateChanged(ConnState::Disconnected { error: Some(error) }) => {
            Some(error.kind)
        }
        _ => None,
    })
    .await;
    assert_eq!(kind, ErrorKind::WrongPassword);

    // The retry password is for this connect only; the saved (wrong) one stays untouched.
    engine
        .connect_with_password(server.id, None, Some(right))
        .expect("retry");
    until(&engine, &mut rx, "sync", |e| {
        matches!(e, EngineEvent::ServerSynced(_)).then_some(())
    })
    .await;
    engine.disconnect(server.id);
    until(&engine, &mut rx, "disconnected", |e| {
        matches!(e, EngineEvent::StateChanged(ConnState::Disconnected { .. })).then_some(())
    })
    .await;
}
