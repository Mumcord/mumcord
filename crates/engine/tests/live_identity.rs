//! Several identities and the per-server choice (issue #148; AC-3.9, AC-3.10) against the local
//! test servers v15 and v15b. `testenv/up.sh v15 v15b`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_identity -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::Path;
use std::time::Duration;

use mumbot::env;
use mumcord_engine::{
    ConnState, Engine, EngineEvent, EventReceiver, IdentityView, PasswordChange, SecretStore,
    ServerFields, ServerId, Snapshot,
};

const WAIT: Duration = Duration::from_secs(20);

/// Waits for the first event `pick` accepts.
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

/// Connects and answers the trust question of a local test server.
async fn connect(engine: &Engine, rx: &mut EventReceiver, server_id: ServerId) -> Snapshot {
    engine.connect(server_id, None).expect("connect");
    loop {
        let event = wait_for(rx, "sync", |event| match event {
            EngineEvent::CertTrustRequired { server_id: id, .. } if *id == server_id => {
                Some(event.clone())
            }
            EngineEvent::ServerSynced(snapshot) if snapshot.server_id == server_id => {
                Some(event.clone())
            }
            _ => None,
        })
        .await;
        match event {
            EngineEvent::CertTrustRequired { fingerprint, .. } => engine
                .cert_trust_decide(server_id, &fingerprint, true)
                .expect("trust"),
            EngineEvent::ServerSynced(snapshot) => return *snapshot,
            _ => unreachable!(),
        }
    }
}

/// Certificate hash the server has for the user, from the synced snapshot.
fn own_hash(snapshot: &Snapshot) -> String {
    snapshot
        .users
        .iter()
        .find(|u| u.session == snapshot.self_session)
        .and_then(|u| u.cert_hash.clone())
        .expect("own certificate hash")
}

async fn add_server(engine: &Engine, label: &str, port: u16, name: &str) -> ServerId {
    engine
        .server_add(ServerFields {
            label: label.into(),
            address: "127.0.0.1".into(),
            port,
            username: name.into(),
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server")
        .id
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs testenv v15 and v15b (MUMCORD_LIVE=1)"]
async fn two_servers_connect_with_their_own_identities() {
    if !std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1") {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start the test servers");
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("live_identity");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");

    let name = format!("id-{:06x}", std::process::id() & 0xff_ffff);
    let main: IdentityView = engine
        .identity_create(format!("{name} Main"), Some("Main".into()))
        .await
        .expect("identity main");
    let work = engine
        .identity_create(format!("{name} Work"), Some("Work".into()))
        .await
        .expect("identity work");
    assert!(main.is_default && !work.is_default);

    let a = add_server(&engine, "test A", common::v15(), &name).await;
    let b = add_server(&engine, "test B", common::v15b(), &name).await;
    engine
        .server_set_identity(b, Some(work.id))
        .await
        .expect("identity for B");

    // Both servers see the certificate they were given (AC-3.9).
    let snap_a = connect(&engine, &mut rx, a).await;
    let snap_b = connect(&engine, &mut rx, b).await;
    assert_eq!(own_hash(&snap_a), main.cert_hash, "A uses the default");
    assert_eq!(own_hash(&snap_b), work.cert_hash, "B uses its own identity");
    assert_eq!(engine.connections().len(), 2);

    // Switching B back to the default reconnects only B (AC-3.10).
    engine
        .server_set_identity(b, None)
        .await
        .expect("default for B");
    engine.disconnect(b);
    wait_for(&mut rx, "B disconnected", |event| {
        matches!(
            event,
            EngineEvent::StateChanged(ConnState::Disconnected { .. })
        )
        .then_some(())
    })
    .await;
    assert!(matches!(
        engine.connection_state_of(a),
        ConnState::Connected { .. }
    ));
    let snap_b = connect(&engine, &mut rx, b).await;
    assert_eq!(own_hash(&snap_b), main.cert_hash, "B back on the default");

    // The identity list knows which server uses which identity.
    let identities = engine.identity_list().await.expect("identities");
    let main_after = identities
        .iter()
        .find(|i| i.id == main.id)
        .expect("main identity");
    assert!(main_after.server_ids.is_empty(), "B is back on the default");
    assert!(
        identities
            .iter()
            .find(|i| i.id == work.id)
            .is_some_and(|i| i.server_ids.is_empty())
    );

    engine.shutdown().await;
}
