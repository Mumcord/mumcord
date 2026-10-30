//! The public query behind the `mumble://` link card (issue #175) against the local test server:
//! `testenv/up.sh v15`, then
//! `MUMCORD_LIVE=1 cargo test -p mumcord-engine --features test-backend --test live_server_query -- --ignored --test-threads=1`
#![cfg(feature = "test-backend")]

mod common;

use std::path::PathBuf;

use mumcord_engine::{Engine, QueryOutcome, SecretStore};

fn live() -> bool {
    let on = std::env::var("MUMCORD_LIVE").is_ok_and(|v| v == "1");
    if !on {
        eprintln!("skipped: set MUMCORD_LIVE=1 and start the test servers");
    }
    on
}

fn work_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    dir
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the local test server (testenv/up.sh v15) and MUMCORD_LIVE=1"]
async fn the_test_server_answers_the_card_query() {
    if !live() {
        return;
    }
    let dir = work_dir("live_server_query");
    let (engine, _rx) = Engine::new(
        &dir,
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");

    // The privacy switch decides before anything is asked: with link previews off no packet leaves.
    let mut settings = engine.settings().expect("settings");
    settings.link_previews = false;
    engine
        .update_settings(settings.clone())
        .await
        .expect("save");
    assert_eq!(
        engine.server_query("127.0.0.1", common::v15(), false).await,
        QueryOutcome::NotAllowed,
        "nothing is asked while the switch is off"
    );

    settings.link_previews = true;
    engine.update_settings(settings).await.expect("save");
    let first = engine.server_query("127.0.0.1", common::v15(), false).await;
    let QueryOutcome::Answered(answer) = first else {
        panic!("the test server did not answer the query: {first:?}");
    };
    assert_eq!(answer.version.major, 1, "server major version");
    assert_eq!(answer.version.minor, 5, "server minor version");
    assert!(
        answer.ping_ms < 1000,
        "round trip on the loopback: {answer:?}"
    );

    // The stored answer is what the card shows again, so a message rendered twice asks once.
    let again = engine.server_query("127.0.0.1", common::v15(), false).await;
    assert_eq!(
        again,
        QueryOutcome::Answered(answer.clone()),
        "the answer is reused"
    );

    // A refresh asks the server again; the round trip is measured again and can differ.
    let refreshed = engine.server_query("127.0.0.1", common::v15(), true).await;
    let QueryOutcome::Answered(refreshed) = refreshed else {
        panic!("the refreshed query got no answer: {refreshed:?}");
    };
    assert_eq!(refreshed.users, answer.users, "the same server answered");

    // A port nothing listens on is the card's "not reachable".
    assert_eq!(
        engine.server_query("127.0.0.1", 64999, false).await,
        QueryOutcome::Unreachable
    );
}
