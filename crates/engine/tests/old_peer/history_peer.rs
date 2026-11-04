//! A Mumcord 0.6.2 client for the history-image E2E (AC-1.51f, issue #402). It is not built by
//! this crate: `scripts/build-old-peer.sh` copies it into a checkout of the v0.6.2 tag as an
//! example of that release's engine and builds it there.
//!
//! `history_peer <name> <channel> <data dir>` connects to the local v15 test server
//! (`MUMCORD_PORT_BASE`, `SERVER_PASSWORD` as in the live tests), joins the channel by name,
//! waits until its history sync ends and prints every synced message as one JSON line
//! `{"body": …}`, then `{"done": <added>}`.

use std::path::Path;
use std::time::Duration;

use mumbot::env;
use mumcord_engine::{
    Engine, EngineEvent, EventReceiver, PasswordChange, SecretStore, ServerFields,
};

const WAIT: Duration = Duration::from_secs(60);

fn port() -> u16 {
    std::env::var("MUMCORD_PORT_BASE")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(64738)
}

async fn next(rx: &mut EventReceiver, deadline: tokio::time::Instant) -> EngineEvent {
    tokio::time::timeout_at(deadline, rx.recv())
        .await
        .expect("old peer timed out")
        .expect("event stream open")
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, name, channel_name, dir] = args.as_slice() else {
        panic!("usage: history_peer <name> <channel> <data dir>");
    };
    // SAFETY: set before the engine starts any thread.
    unsafe { std::env::set_var("MUMCORD_AUDIO_BACKEND", "null") };
    let (engine, mut rx) = Engine::new(
        &Path::new(dir).join("data"),
        SecretStore::in_memory(),
        tokio::runtime::Handle::current(),
    )
    .expect("engine");
    engine
        .identity_create(name.clone(), None)
        .await
        .expect("identity");
    let server = engine
        .server_add(ServerFields {
            label: "test".into(),
            address: "127.0.0.1".into(),
            port: port(),
            username: name.clone(),
            password: match env::testenv_var("SERVER_PASSWORD") {
                Some(p) => PasswordChange::Set(p),
                None => PasswordChange::Keep,
            },
        })
        .await
        .expect("server");

    let deadline = tokio::time::Instant::now() + WAIT;
    engine.connect(server.id, None).expect("connect");
    let snapshot = loop {
        match next(&mut rx, deadline).await {
            EngineEvent::CertTrustRequired {
                server_id,
                fingerprint,
                ..
            } => engine
                .cert_trust_decide(server_id, &fingerprint, true)
                .expect("trust"),
            EngineEvent::ServerSynced(s) => break *s,
            _ => {}
        }
    };
    let channel = snapshot
        .channels
        .iter()
        .find(|c| &c.name == channel_name)
        .unwrap_or_else(|| panic!("channel {channel_name}"))
        .id;
    engine
        .join_channel(snapshot.server_id, channel)
        .expect("join");

    loop {
        match next(&mut rx, deadline).await {
            EngineEvent::HistorySynced {
                channel_id,
                messages,
            } if channel_id == channel => {
                for message in messages {
                    println!("{}", serde_json::json!({ "body": message.body }));
                }
            }
            EngineEvent::HistorySyncStatus {
                channel_id,
                running: false,
                added,
            } if channel_id == channel => {
                println!("{}", serde_json::json!({ "done": added }));
                break;
            }
            _ => {}
        }
    }
    engine.disconnect(snapshot.server_id);
    tokio::time::sleep(Duration::from_millis(300)).await;
}
