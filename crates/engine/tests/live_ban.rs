//! What a ban looks like to the banned person, against the local test server with a SuperUser
//! bot as the admin. `testenv/up.sh v15`, then:
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_ban -- --ignored --test-threads=1`
//!
//! What stock servers (1.4 and 1.5) do, and what the engine can therefore say:
//! - banned while connected: the server names the ban (`UserRemove` with `ban`), so the
//!   connection ends as `ErrorKind::Banned` with the server's reason and is not retried;
//! - connecting from a banned address: the server drops the connection before the TLS handshake
//!   is through, without a word. That is `ServerUnreachable`, like any server that is not there;
//! - connecting with a banned certificate: the server lets the login succeed and closes right
//!   after it, again without a word. The engine retries, counts these short connections and
//!   gives up instead of logging in again and again.
//!
//! The quick ban covers the address every local client shares, so the list is emptied before
//! anything is asserted: a failing assertion must not leave the test server unreachable.
// FreeBSD: see the note on the mumbot dev-dependency in Cargo.toml.
#![cfg(all(feature = "test-backend", not(target_os = "freebsd")))]

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use mumbot::protocol::ban;
use mumbot::{Bot, BotConfig, env};
use mumcord_engine::{
    ConnState, Engine, EngineError, EngineEvent, ErrorKind, EventReceiver, PasswordChange,
    SecretStore, ServerFields, ServerId,
};

const WAIT: Duration = Duration::from_secs(20);
const REASON: &str = "live ban test";

fn live() -> bool {
    let on = std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start testenv/up.sh v15");
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

/// Connects and returns the own session once synced; `None` if the connection ended instead.
async fn connect(engine: &Engine, rx: &mut EventReceiver, server: ServerId) -> Option<u32> {
    engine.connect(server, None).expect("connect");
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(EngineEvent::CertTrustRequired { fingerprint, .. })) => engine
                .cert_trust_decide(server, &fingerprint, true)
                .expect("trust"),
            Ok(Some(EngineEvent::ServerSynced(s))) => return Some(s.self_session),
            Ok(Some(EngineEvent::StateChanged(ConnState::Disconnected { .. }))) => return None,
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => return None,
        }
    }
}

/// The connection states reported until `done` says so or `window` is over.
async fn states(
    rx: &mut EventReceiver,
    window: Duration,
    done: impl Fn(&ConnState) -> bool,
) -> Vec<ConnState> {
    let deadline = tokio::time::Instant::now() + window;
    let mut seen = Vec::new();
    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        if let EngineEvent::StateChanged(state) = event {
            let last = done(&state);
            seen.push(state);
            if last {
                break;
            }
        }
    }
    seen
}

fn ended(states: &[ConnState]) -> Option<&EngineError> {
    match states.last() {
        Some(ConnState::Disconnected { error }) => error.as_ref(),
        _ => None,
    }
}

fn is_disconnected(state: &ConnState) -> bool {
    matches!(state, ConnState::Disconnected { .. })
}

async fn save_bans(admin: &Bot, bans: &[ban::Ban]) {
    admin.handle().send_ban_list(bans).expect("ban list");
    admin.flush().await.expect("flush");
    // The server applies the list right after reading it; the next connect must see it.
    tokio::time::sleep(Duration::from_millis(500)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs testenv v15 (MUMCORD_LIVE=1)"]
async fn a_ban_is_named_only_where_the_server_names_it() {
    if !live() {
        return;
    }
    // SAFETY: set before any audio thread exists; tests run with --test-threads=1.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let dir = work_dir("engine-live-ban");
    let admin = Bot::connect(BotConfig::superuser(common::v15()))
        .await
        .expect("SuperUser bot (SUPERUSER_PASSWORD in testenv/.env)");
    save_bans(&admin, &[]).await;

    let (engine, mut rx) = Engine::new(
        &dir.join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    let name = unique("mc-ban-me");
    engine
        .identity_create(name.clone(), None)
        .await
        .expect("identity");
    let server = engine
        .server_add(ServerFields {
            label: "v15".into(),
            address: env::HOST.into(),
            port: common::v15(),
            username: name.clone(),
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server")
        .id;

    // Banned while connected: the server says so.
    let session = connect(&engine, &mut rx, server)
        .await
        .expect("first login");
    admin
        .wait_user(&name, &Default::default())
        .await
        .expect("the admin sees the user");
    let cert_hash = admin
        .state()
        .users
        .get(&session)
        .and_then(|u| u.cert_hash.clone())
        .expect("the admin sees the certificate hash");
    admin
        .handle()
        .remove_user(session, Some(REASON), true)
        .expect("ban");
    let while_connected = states(&mut rx, WAIT, is_disconnected).await;

    // Connecting from the banned address (the quick ban covers address and certificate).
    engine.connect(server, None).expect("connect");
    let from_banned_address = states(&mut rx, WAIT, is_disconnected).await;

    // Connecting with a banned certificate from an address that is not banned.
    save_bans(
        &admin,
        &[ban::Ban {
            address: "203.0.113.9".parse().expect("address"),
            mask: 32,
            name: Some(name.clone()),
            hash: Some(cert_hash),
            reason: Some(REASON.into()),
            start: Some(ban::format_start(SystemTime::now())),
            duration: 600,
        }],
    )
    .await;
    engine.connect(server, None).expect("connect");
    // Retries 1 to 3 come after 1, 2 and 4 seconds.
    let with_banned_certificate = states(&mut rx, WAIT, |state| {
        is_disconnected(state) || matches!(state, ConnState::Reconnecting { attempt: 3.., .. })
    })
    .await;
    engine.disconnect(server);

    save_bans(&admin, &[]).await;
    while rx.try_recv().is_ok() {}
    let after_unban = connect(&engine, &mut rx, server).await;
    engine.disconnect(server);

    let error = ended(&while_connected).expect("the ban ended the connection with an error");
    assert_eq!(error.kind, ErrorKind::Banned);
    assert_eq!(error.message, REASON, "the server's reason, as plain text");
    assert!(
        !while_connected
            .iter()
            .any(|s| matches!(s, ConnState::Reconnecting { .. })),
        "a ban is not retried: {while_connected:?}"
    );

    let error = ended(&from_banned_address).expect("the connect failed");
    assert_eq!(
        error.kind,
        ErrorKind::ServerUnreachable,
        "the server gives no reason to a banned address"
    );
    assert!(
        !from_banned_address
            .iter()
            .any(|s| matches!(s, ConnState::Reconnecting { .. })),
        "a failed first connect is not retried: {from_banned_address:?}"
    );

    // Every login succeeds and is closed at once; the attempts add up instead of starting
    // from one after each login, so the retries end (unit tests cover the giving up).
    let attempts: Vec<u32> = with_banned_certificate
        .iter()
        .filter_map(|s| match s {
            ConnState::Reconnecting { attempt, .. } => Some(*attempt),
            _ => None,
        })
        .collect();
    assert_eq!(attempts, vec![1, 2, 3], "{with_banned_certificate:?}");
    assert!(
        with_banned_certificate
            .iter()
            .filter(|s| matches!(s, ConnState::Connected { .. }))
            .count()
            >= 3,
        "the server accepts each login before closing: {with_banned_certificate:?}"
    );

    assert!(
        after_unban.is_some(),
        "without the ban the login works again"
    );
}
