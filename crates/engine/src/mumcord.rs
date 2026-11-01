//! Recognising other Mumcord users (FR-1.61, QA.45).
//!
//! On connect every Mumcord client sends a small "hello" as plugin data to everyone on the
//! server, and to each person who joins later. A client that receives a hello from someone it
//! has not greeted yet answers once, so two clients that connected at the same moment still end
//! up knowing each other, without ping-pong. Official clients ignore the unknown data id, so the
//! badge never appears for them. Hellos come from other clients and are untrusted: the decoder
//! caps size, feature count and feature names.
//!
//! A bot (e.g. mumbot) adds a `bot` object with its name, version and modules; plugins use it to
//! offer bot-specific actions only on bots (see [`crate::plugins::PluginBot`]).
//!
//! Features gate Mumcord-only behaviour: ask [`Engine::mumcord_user_has_feature`] before sending
//! another Mumcord-specific plugin payload to someone.

use std::collections::{HashMap, HashSet};

use mumcord_protocol::{Handle, PLUGIN_DATA_MAX, PluginData};
use serde::{Deserialize, Serialize};

use crate::ServerId;
use crate::plugins::PluginBot;
use crate::session::{Engine, SessionCtx, lock};

/// Plugin data id of the hello; other ids are ignored.
pub const DATA_ID: &str = "mumcord.hello";
/// The hello version this client sends. Higher versions are still accepted (see [`decode`]).
pub const VERSION: u32 = 1;
/// The `app` value of every hello.
const APP: &str = "mumcord";
/// What this client can do for other Mumcord clients.
pub const FEATURES: &[&str] = &[
    "now-playing",
    "now-gaming",
    crate::full_image::FEATURE,
    crate::p2p::FEATURE,
    crate::watch_party::FEATURE,
    crate::screen_share::signal::FEATURE,
    crate::pm_rules::FEATURE,
    crate::plugins::FEATURE,
    crate::plugins::SERVER_FEATURE,
];
const MAX_FEATURES: usize = 16;
const MAX_FEATURE_LEN: usize = 32;
/// Caps for the untrusted `bot` object: modules per bot, and the length of names and modules.
const MAX_BOT_MODULES: usize = 16;
const MAX_BOT_NAME_LEN: usize = 32;

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
    /// Kept raw so a malformed bot object does not discard the whole hello.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bot: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct WireBot {
    name: String,
    // `version` is part of the object but not used by this client.
    #[serde(default)]
    modules: Vec<String>,
}

/// A decoded hello.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub features: Vec<String>,
    /// Set when the sender says it is a bot; a malformed `bot` object is ignored.
    pub bot: Option<PluginBot>,
}

/// Lowercase letters, digits, `-` and `.`, the alphabet of feature, bot and module names.
fn valid_name(name: &str, max: usize) -> bool {
    !name.is_empty()
        && name.len() <= max
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
}

fn bot_of(value: serde_json::Value) -> Option<PluginBot> {
    let wire: WireBot = serde_json::from_value(value).ok()?;
    if !valid_name(&wire.name, MAX_BOT_NAME_LEN) {
        return None;
    }
    let mut modules: Vec<String> = wire
        .modules
        .into_iter()
        .filter(|m| valid_name(m, MAX_BOT_NAME_LEN))
        .take(MAX_BOT_MODULES)
        .collect();
    modules.sort();
    modules.dedup();
    Some(PluginBot {
        name: wire.name,
        modules,
    })
}

/// The features this client announces, as the engine reports them for ourselves too.
fn own_features() -> Vec<String> {
    FEATURES.iter().map(|f| (*f).to_owned()).collect()
}

/// The hello this client sends.
pub fn encode() -> Vec<u8> {
    let wire = Wire {
        app: APP.to_owned(),
        v: VERSION,
        features: own_features(),
        bot: None,
    };
    serde_json::to_vec(&wire).unwrap_or_default()
}

/// The features (and bot info) announced in a hello. A higher version is still a Mumcord client:
/// the fields read here are kept stable, newer versions only add to them.
pub fn decode(data: &[u8]) -> Result<Hello, HelloError> {
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
        .filter(|f| valid_name(f, MAX_FEATURE_LEN))
        .take(MAX_FEATURES)
        .collect();
    features.sort();
    features.dedup();
    Ok(Hello {
        features,
        bot: wire.bot.and_then(bot_of),
    })
}

/// Who is known as a Mumcord user, per connected server.
pub(crate) type ServerPeers = std::sync::Mutex<HashMap<ServerId, Peers>>;

/// Who is known as a Mumcord user on one connection.
#[derive(Default)]
pub(crate) struct Peers {
    /// Session → announced features.
    known: HashMap<u32, Vec<String>>,
    /// Session → bot info, for those whose hello says they are a bot.
    bots: HashMap<u32, PluginBot>,
    /// Sessions that already got our hello on this connection.
    greeted: HashSet<u32>,
    /// Whether the connection finished syncing; joins before that are covered by the first send.
    synced: bool,
}

impl Peers {
    pub(crate) fn features(&self, session: u32) -> Option<&[String]> {
        self.known.get(&session).map(Vec::as_slice)
    }

    pub(crate) fn bot(&self, session: u32) -> Option<&PluginBot> {
        self.bots.get(&session)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    /// Records a hello; true when the peer is new or its features changed.
    fn learn(&mut self, session: u32, features: Vec<String>) -> bool {
        self.known.insert(session, features.clone()) != Some(features)
    }

    /// Records the bot info of a hello; true when it changed.
    fn learn_bot(&mut self, session: u32, bot: Option<PluginBot>) -> bool {
        match bot {
            Some(bot) => self.bots.insert(session, bot.clone()) != Some(bot),
            None => self.bots.remove(&session).is_some(),
        }
    }

    /// Marks `session` as greeted; true when it was not yet (so a hello must be sent).
    fn greet(&mut self, session: u32) -> bool {
        self.greeted.insert(session)
    }

    fn forget(&mut self, session: u32) -> bool {
        self.greeted.remove(&session);
        self.bots.remove(&session);
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
pub(crate) fn on_synced(state: &ServerPeers, ctx: &SessionCtx) {
    let own = ctx.handle.with_state(|s| s.session);
    let everyone = others(&ctx.handle);
    {
        let mut servers = lock(state);
        let peers = servers.entry(ctx.server_id).or_default();
        peers.reset();
        peers.synced = true;
        // We are a Mumcord client too: the badge must show on our own entry everywhere.
        if let Some(own) = own {
            peers.known.insert(own, own_features());
        }
        peers.greeted.extend(everyone.iter().copied());
    }
    send(&ctx.handle, &everyone);
}

/// Someone joined after the sync: they get our hello.
pub(crate) fn on_user_joined(state: &ServerPeers, ctx: &SessionCtx, session: u32) {
    if ctx.handle.with_state(|s| s.session) == Some(session) {
        return;
    }
    let first = lock(state)
        .get_mut(&ctx.server_id)
        .is_some_and(|peers| peers.synced && peers.greet(session));
    if first {
        send(&ctx.handle, &[session]);
    }
}

/// Forgets a person who left; true when they were a known Mumcord user.
pub(crate) fn on_user_removed(state: &ServerPeers, server_id: ServerId, session: u32) -> bool {
    lock(state)
        .get_mut(&server_id)
        .is_some_and(|peers| peers.forget(session))
}

pub(crate) fn on_disconnect(state: &ServerPeers, server_id: ServerId) {
    lock(state).remove(&server_id);
}

/// A hello arrived (session task). Returns the sender when its Mumcord state changed.
pub(crate) fn on_plugin_data(
    state: &ServerPeers,
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
    let hello = match decode(&data.data) {
        Ok(hello) => hello,
        Err(e) => {
            tracing::debug!(error = %e, "ignoring a mumcord hello");
            return None;
        }
    };
    let (changed, reply) = {
        let mut servers = lock(state);
        let peers = servers.entry(ctx.server_id).or_default();
        (
            peers.learn(sender, hello.features) | peers.learn_bot(sender, hello.bot),
            peers.synced && peers.greet(sender),
        )
    };
    if reply {
        send(&ctx.handle, &[sender]);
    }
    changed.then_some(sender)
}

impl Engine {
    /// Whether `session` on `server_id` is known to be another Mumcord client (it sent a hello).
    pub fn is_mumcord_user(&self, server_id: ServerId, session: u32) -> bool {
        lock(self.mumcord_state())
            .get(&server_id)
            .and_then(|peers| peers.features(session))
            .is_some()
    }

    /// Whether `session` on `server_id` is a Mumcord client that announced `feature`. Gate
    /// Mumcord-only behaviour (extra plugin payloads, shared history, …) on this.
    pub fn mumcord_user_has_feature(
        &self,
        server_id: ServerId,
        session: u32,
        feature: &str,
    ) -> bool {
        lock(self.mumcord_state())
            .get(&server_id)
            .and_then(|peers| peers.features(session))
            .is_some_and(|f| f.iter().any(|x| x == feature))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_round_trips() {
        let hello = decode(&encode()).expect("own hello decodes");
        assert_eq!(hello.bot, None);
        let features = hello.features;
        assert_eq!(
            features,
            [
                "chat-request",
                "full-image",
                "now-gaming",
                "now-playing",
                "p2p",
                "plugins",
                "screen-share",
                "server-plugins",
                "watch-party"
            ]
        );
        assert!(encode().len() <= PLUGIN_DATA_MAX);
    }

    #[test]
    fn newer_versions_and_unknown_features_are_accepted() {
        let data = br#"{"app":"mumcord","v":7,"features":["x-new","history-sync"],"extra":1}"#;
        assert_eq!(
            decode(data).expect("valid").features,
            ["history-sync", "x-new"]
        );
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
        let features = decode(json.to_string().as_bytes()).expect("valid").features;
        assert!(features.len() <= MAX_FEATURES);
        assert!(!features.iter().any(|f| f == "BAD NAME" || f.is_empty()));
    }

    #[test]
    fn bot_info_is_read_and_sanitised() {
        let data = br#"{"app":"mumcord","v":1,"features":["plugins"],
            "bot":{"name":"mumbot","version":"2.1.0","modules":["music","BAD","music","radio"]}}"#;
        assert_eq!(
            decode(data).expect("valid").bot,
            Some(PluginBot {
                name: "mumbot".into(),
                modules: vec!["music".into(), "radio".into()],
            })
        );
        for bad in [
            r#"{"name":"Mumbot"}"#,
            r#"{"name":""}"#,
            r#"{"modules":["music"]}"#,
            r#""mumbot""#,
        ] {
            let data = format!(r#"{{"app":"mumcord","v":1,"bot":{bad}}}"#);
            let hello = decode(data.as_bytes()).expect("the hello itself stays valid");
            assert_eq!(hello.bot, None, "{bad}");
        }
        let many: Vec<String> = (0..40).map(|i| format!("m{i}")).collect();
        let json = serde_json::json!({"app":"mumcord","v":1,"bot":{"name":"b","modules":many}});
        let bot = decode(json.to_string().as_bytes()).expect("valid").bot;
        assert_eq!(bot.map(|b| b.modules.len()), Some(MAX_BOT_MODULES));
    }

    #[test]
    fn bots_are_learned_and_forgotten() {
        let mut peers = Peers::default();
        let bot = PluginBot {
            name: "mumbot".into(),
            modules: vec!["music".into()],
        };
        assert!(peers.learn_bot(5, Some(bot.clone())));
        assert!(!peers.learn_bot(5, Some(bot.clone())));
        assert_eq!(peers.bot(5), Some(&bot));
        assert!(peers.learn_bot(5, None));
        assert!(!peers.learn_bot(5, None));
        peers.learn_bot(5, Some(bot));
        peers.forget(5);
        assert!(peers.bot(5).is_none());
    }

    #[test]
    fn own_entry_reports_our_features_after_sync() {
        let mut peers = Peers::default();
        assert!(peers.features(7).is_none());
        peers.known.insert(7, own_features());
        assert_eq!(peers.features(7), Some(own_features().as_slice()));
        peers.reset();
        assert!(peers.features(7).is_none(), "a reconnect starts without it");
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
