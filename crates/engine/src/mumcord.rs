//! Recognising other Mumcord users (FR-1.61, QA.45).
//!
//! On connect every Mumcord client sends a small "hello" as plugin data to everyone on the
//! server, and to each person who joins later. A client that receives a hello from someone it
//! has not greeted yet answers once, so two clients that connected at the same moment still end
//! up knowing each other, without ping-pong. Official clients ignore the unknown data id, so the
//! badge never appears for them. Hellos come from other clients and are untrusted: the decoder
//! caps size, feature count and feature names.
//!
//! Features gate Mumcord-only behaviour: ask [`Engine::mumcord_user_has_feature`] before sending
//! another Mumcord-specific plugin payload to someone.

use std::collections::{HashMap, HashSet};

use mumcord_protocol::{Handle, PLUGIN_DATA_MAX, PluginData};
use serde::{Deserialize, Serialize};

use crate::session::{Engine, SessionCtx, lock};

/// Plugin data id of the hello; other ids are ignored.
pub const DATA_ID: &str = "mumcord.hello";
/// The hello version this client sends. Higher versions are still accepted (see [`decode`]).
pub const VERSION: u32 = 1;
/// The `app` value of every hello.
const APP: &str = "mumcord";
/// What this client can do for other Mumcord clients.
pub const FEATURES: &[&str] = &["now-playing", "now-gaming"];
const MAX_FEATURES: usize = 16;
const MAX_FEATURE_LEN: usize = 32;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HelloError {
    #[error("payload too large")]
    TooLarge,
    #[error("payload is not a Mumcord hello")]
    Malformed,
}

#[derive(Serialize, Deserialize)]
struct Wire {
    app: String,
    v: u32,
    #[serde(default)]
    features: Vec<String>,
}

/// The hello this client sends.
pub fn encode() -> Vec<u8> {
    let wire = Wire {
        app: APP.to_owned(),
        v: VERSION,
        features: FEATURES.iter().map(|f| (*f).to_owned()).collect(),
    };
    serde_json::to_vec(&wire).unwrap_or_default()
}

/// The features announced in a hello. A higher version is still a Mumcord client: the fields
/// read here are kept stable, newer versions only add to them.
pub fn decode(data: &[u8]) -> Result<Vec<String>, HelloError> {
    if data.len() > PLUGIN_DATA_MAX {
        return Err(HelloError::TooLarge);
    }
    let wire: Wire = serde_json::from_slice(data).map_err(|_| HelloError::Malformed)?;
    if wire.app != APP || wire.v == 0 {
        return Err(HelloError::Malformed);
    }
    let mut features: Vec<String> = wire
        .features
        .into_iter()
        .filter(|f| {
            !f.is_empty()
                && f.len() <= MAX_FEATURE_LEN
                && f.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
        })
        .take(MAX_FEATURES)
        .collect();
    features.sort();
    features.dedup();
    Ok(features)
}

/// Who is known as a Mumcord user on the current connection.
#[derive(Default)]
pub(crate) struct Peers {
    /// Session → announced features.
    known: HashMap<u32, Vec<String>>,
    /// Sessions that already got our hello on this connection.
    greeted: HashSet<u32>,
    /// Whether the connection finished syncing; joins before that are covered by the first send.
    synced: bool,
}

impl Peers {
    pub(crate) fn features(&self, session: u32) -> Option<&[String]> {
        self.known.get(&session).map(Vec::as_slice)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    /// Records a hello; true when the peer is new or its features changed.
    fn learn(&mut self, session: u32, features: Vec<String>) -> bool {
        self.known.insert(session, features.clone()) != Some(features)
    }

    /// Marks `session` as greeted; true when it was not yet (so a hello must be sent).
    fn greet(&mut self, session: u32) -> bool {
        self.greeted.insert(session)
    }

    fn forget(&mut self, session: u32) -> bool {
        self.greeted.remove(&session);
        self.known.remove(&session).is_some()
    }
}

fn send(handle: &Handle, receivers: &[u32]) {
    if receivers.is_empty() {
        return;
    }
    // Old servers (< 1.4) cannot forward plugin data; there the badge simply does not appear.
    if let Err(e) = handle.send_plugin_data(receivers, DATA_ID, encode()) {
        tracing::debug!(error = %e, "mumcord hello not sent");
    }
}

fn others(handle: &Handle) -> Vec<u32> {
    handle.with_state(|s| {
        let own = s.session;
        s.users
            .keys()
            .copied()
            .filter(|id| Some(*id) != own)
            .collect()
    })
}

/// The connection synced: greet everyone who is already here.
pub(crate) fn on_synced(state: &std::sync::Mutex<Peers>, ctx: &SessionCtx) {
    let everyone = others(&ctx.handle);
    {
        let mut peers = lock(state);
        peers.reset();
        peers.synced = true;
        peers.greeted.extend(everyone.iter().copied());
    }
    send(&ctx.handle, &everyone);
}

/// Someone joined after the sync: they get our hello.
pub(crate) fn on_user_joined(state: &std::sync::Mutex<Peers>, ctx: &SessionCtx, session: u32) {
    if ctx.handle.with_state(|s| s.session) == Some(session) {
        return;
    }
    let first = {
        let mut peers = lock(state);
        peers.synced && peers.greet(session)
    };
    if first {
        send(&ctx.handle, &[session]);
    }
}

/// Forgets a person who left; true when they were a known Mumcord user.
pub(crate) fn on_user_removed(state: &std::sync::Mutex<Peers>, session: u32) -> bool {
    lock(state).forget(session)
}

pub(crate) fn on_disconnect(state: &std::sync::Mutex<Peers>) {
    lock(state).reset();
}

/// A hello arrived (session task). Returns the sender when its Mumcord state changed.
pub(crate) fn on_plugin_data(
    state: &std::sync::Mutex<Peers>,
    ctx: &SessionCtx,
    data: &PluginData,
) -> Option<u32> {
    if data.data_id != DATA_ID {
        return None;
    }
    let sender = data.sender?;
    let (own, known) = ctx
        .handle
        .with_state(|s| (s.session, s.users.contains_key(&sender)));
    if own == Some(sender) || !known {
        return None;
    }
    let features = match decode(&data.data) {
        Ok(features) => features,
        Err(e) => {
            tracing::debug!(error = %e, "ignoring a mumcord hello");
            return None;
        }
    };
    let (changed, reply) = {
        let mut peers = lock(state);
        (
            peers.learn(sender, features),
            peers.synced && peers.greet(sender),
        )
    };
    if reply {
        send(&ctx.handle, &[sender]);
    }
    changed.then_some(sender)
}

impl Engine {
    /// Whether `session` is known to be another Mumcord client (it sent a hello).
    pub fn is_mumcord_user(&self, session: u32) -> bool {
        lock(self.mumcord_state()).features(session).is_some()
    }

    /// Whether `session` is a Mumcord client that announced `feature`. Gate Mumcord-only
    /// behaviour (extra plugin payloads, shared history, …) on this.
    pub fn mumcord_user_has_feature(&self, session: u32, feature: &str) -> bool {
        lock(self.mumcord_state())
            .features(session)
            .is_some_and(|f| f.iter().any(|x| x == feature))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_round_trips() {
        let features = decode(&encode()).expect("own hello decodes");
        assert_eq!(features, ["now-gaming", "now-playing"]);
        assert!(encode().len() <= PLUGIN_DATA_MAX);
    }

    #[test]
    fn newer_versions_and_unknown_features_are_accepted() {
        let data = br#"{"app":"mumcord","v":7,"features":["x-new","history-sync"],"extra":1}"#;
        assert_eq!(decode(data).expect("valid"), ["history-sync", "x-new"]);
    }

    #[test]
    fn foreign_or_broken_payloads_are_rejected() {
        assert_eq!(decode(b"nope"), Err(HelloError::Malformed));
        assert_eq!(
            decode(br#"{"app":"other","v":1}"#),
            Err(HelloError::Malformed)
        );
        assert_eq!(
            decode(br#"{"app":"mumcord","v":0}"#),
            Err(HelloError::Malformed)
        );
        assert_eq!(
            decode(&vec![b' '; PLUGIN_DATA_MAX + 1]),
            Err(HelloError::TooLarge)
        );
    }

    #[test]
    fn features_are_sanitised_and_capped() {
        let mut names: Vec<String> = (0..40).map(|i| format!("f{i}")).collect();
        names.extend([
            "BAD NAME".into(),
            String::new(),
            "x".repeat(33),
            "ok".into(),
        ]);
        let json = serde_json::json!({ "app": "mumcord", "v": 1, "features": names });
        let features = decode(json.to_string().as_bytes()).expect("valid");
        assert!(features.len() <= MAX_FEATURES);
        assert!(!features.iter().any(|f| f == "BAD NAME" || f.is_empty()));
    }

    #[test]
    fn peers_track_join_leave_and_reconnect() {
        let mut peers = Peers::default();
        assert!(peers.learn(5, vec!["a".into()]));
        assert!(!peers.learn(5, vec!["a".into()]), "same hello is no change");
        assert!(peers.learn(5, vec!["a".into(), "b".into()]));
        assert!(peers.greet(5));
        assert!(!peers.greet(5), "greeted only once");
        assert!(peers.features(5).is_some());
        assert!(peers.forget(5));
        assert!(!peers.forget(5));
        assert!(
            peers.greet(5),
            "a new person with the same session is greeted again"
        );
        peers.learn(6, vec![]);
        peers.reset();
        assert!(peers.features(6).is_none());
        assert!(!peers.synced);
    }
}
