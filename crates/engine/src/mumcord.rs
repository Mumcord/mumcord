//! Recognising other Mumcord users (FR-1.61, QA.45).
//!
//! On connect every Mumcord client sends a small "hello" as plugin data to everyone on the
//! server, and to each person who joins later. A client that receives a hello from someone it
//! has not greeted yet answers once, so two clients that connected at the same moment still end
//! up knowing each other, without ping-pong. Official clients ignore the unknown data id, so the
//! badge never appears for them. Hellos come from other clients and are untrusted: the decoder
//! caps size, feature count and feature names.
//!
//! A bot (e.g. mumbot) adds a `bot` object with its name, product, version, modules and whether
//! it uses voice. The UI labels and places bots with it (issues #458, #459); plugins use it to
//! offer bot-specific actions only on bots (see [`crate::plugins::PluginBot`]).
//!
//! Features gate Mumcord-only behaviour: ask [`Engine::mumcord_user_has_feature`] before sending
//! another Mumcord-specific plugin payload to someone.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

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
/// Feature name for sending and receiving files (spec 07, section 6.4).
pub const FEATURE_FILES: &str = crate::files::FEATURE;
/// Feature name of clients that can watch a screen share through the relay broadcast (spec 07,
/// FR-7.48); viewers without it get a direct copy.
pub const FEATURE_SCREEN_BROADCAST: &str = "screen-broadcast";
/// What this client can do for other Mumcord clients.
pub const FEATURES: &[&str] = &[
    "now-playing",
    "now-gaming",
    crate::full_image::FEATURE,
    crate::history_sync::images::FEATURE,
    FEATURE_FILES,
    crate::p2p::FEATURE,
    crate::watch_party::FEATURE,
    crate::screen_share::signal::FEATURE,
    crate::camera::signal::FEATURE,
    FEATURE_SCREEN_BROADCAST,
    crate::pm_rules::FEATURE,
    crate::plugins::FEATURE,
    crate::plugins::SERVER_FEATURE,
];
const MAX_FEATURES: usize = 16;
const MAX_FEATURE_LEN: usize = 32;
/// Caps for the untrusted `bot` object: modules per bot, and the length of names and modules.
const MAX_BOT_MODULES: usize = 16;
const MAX_BOT_NAME_LEN: usize = 32;
/// Caps for the bot's product name ("Mumbot") and version ("0.4.1"), shown as plain text.
const MAX_BOT_PRODUCT_LEN: usize = 32;
const MAX_BOT_VERSION_LEN: usize = 32;
/// The module that makes a bot speak; older bots without the `voice` field are voice bots when
/// they run it (issue #458).
const VOICE_MODULE: &str = "music";

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
    /// "Please answer" (issue #647): sent to someone who never said hello. Older clients ignore
    /// the field and treat it as a normal hello.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    ask: bool,
}

/// Optional bot fields stay raw: a wrong type drops only that field, not the whole bot.
#[derive(Deserialize)]
struct WireBot {
    name: String,
    #[serde(default)]
    product: Option<serde_json::Value>,
    #[serde(default)]
    version: Option<serde_json::Value>,
    #[serde(default)]
    modules: Vec<String>,
    #[serde(default)]
    voice: Option<serde_json::Value>,
}

/// What a bot says about itself in its hello, sanitised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bot {
    pub name: String,
    /// Display name of the bot software, e.g. "Mumbot". Older bots send none; then it is the
    /// capitalised `name` (decision on issue #459).
    pub product: String,
    /// Version of the bot software, e.g. "0.4.1", when it sent a valid one.
    pub version: Option<String>,
    /// Sorted, deduplicated module names, e.g. `history`, `peer`.
    pub modules: Vec<String>,
    /// Whether the bot speaks in its channel; such bots stay visible there (issue #458).
    pub voice: bool,
}

impl Bot {
    /// The part plugins see.
    pub fn plugin(&self) -> PluginBot {
        PluginBot {
            name: self.name.clone(),
            modules: self.modules.clone(),
        }
    }
}

/// A decoded hello.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub features: Vec<String>,
    /// Set when the sender says it is a bot; a malformed `bot` object is ignored.
    pub bot: Option<Bot>,
    /// The sender has no hello from us yet and asks for one (issue #647).
    pub ask: bool,
}

/// Lowercase letters, digits, `-` and `.`, the alphabet of feature, bot and module names.
fn valid_name(name: &str, max: usize) -> bool {
    !name.is_empty()
        && name.len() <= max
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
}

/// Display text from another client: printable ASCII letters, digits, spaces, `-` and `.`, with
/// no space at either end, so it can neither spoof markup nor break the layout.
fn valid_product(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_BOT_PRODUCT_LEN
        && text.trim() == text
        && !text.contains("  ")
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '.')
}

/// A version like `0.4.1` or `1.0.0-rc.1+abc`.
fn valid_version(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_BOT_VERSION_LEN
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
}

fn string_if(value: Option<serde_json::Value>, valid: fn(&str) -> bool) -> Option<String> {
    match value {
        Some(serde_json::Value::String(s)) if valid(&s) => Some(s),
        _ => None,
    }
}

/// `mumbot` → `Mumbot`; the name is already limited to lowercase ASCII.
fn capitalised(name: &str) -> String {
    let mut chars = name.chars();
    chars
        .next()
        .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
        .unwrap_or_default()
}

fn bot_of(value: serde_json::Value) -> Option<Bot> {
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
    let voice = match wire.voice {
        Some(serde_json::Value::Bool(voice)) => voice,
        // Bots from before the field: those with music are the ones that speak.
        _ => modules.iter().any(|m| m == VOICE_MODULE),
    };
    Some(Bot {
        product: string_if(wire.product, valid_product).unwrap_or_else(|| capitalised(&wire.name)),
        version: string_if(wire.version, valid_version),
        name: wire.name,
        modules,
        voice,
    })
}

/// The features this client announces, as the engine reports them for ourselves too.
fn own_features() -> Vec<String> {
    FEATURES.iter().map(|f| (*f).to_owned()).collect()
}

/// The hello this client sends; `ask` asks the receiver to answer with its own (issue #647).
pub fn encode(ask: bool) -> Vec<u8> {
    let wire = Wire {
        app: APP.to_owned(),
        v: VERSION,
        features: own_features(),
        bot: None,
        ask,
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
        ask: wire.ask,
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
    bots: HashMap<u32, Bot>,
    /// Sessions that already got our hello on this connection.
    greeted: HashSet<u32>,
    /// Whether the connection finished syncing; joins before that are covered by the first send.
    synced: bool,
    /// People who have not said hello yet, and when to ask them (issue #647).
    asks: HashMap<u32, Ask>,
    /// People asked three times without an answer; not asked again until they leave.
    given_up: HashSet<u32>,
    /// Session → when we last replied to its hello; see [`ANSWER_INTERVAL`].
    answered: HashMap<u32, Instant>,
}

/// Asking someone who never said hello: when next, and how often already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ask {
    next: Instant,
    sent: usize,
}

/// The gaps before asking someone who never said hello (issue #647): three requests in about two
/// minutes, then they count as a plain Mumble client until they reconnect.
const ASK_DELAYS: [Duration; 3] = [
    Duration::from_secs(10),
    Duration::from_secs(30),
    Duration::from_secs(90),
];

/// The shortest gap between two replies to hellos of the same sender. Every reply is a plugin
/// message that counts against our own budget on the server (pluginmessagelimit); without this
/// gap anyone could send requests in a loop and make our file, camera and plugin messages drop.
const ANSWER_INTERVAL: Duration = Duration::from_secs(30);

impl Peers {
    pub(crate) fn features(&self, session: u32) -> Option<&[String]> {
        self.known.get(&session).map(Vec::as_slice)
    }

    pub(crate) fn bot(&self, session: u32) -> Option<&Bot> {
        self.bots.get(&session)
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    /// Records a hello; true when the peer is new or its features changed.
    fn learn(&mut self, session: u32, features: Vec<String>) -> bool {
        self.asks.remove(&session);
        self.known.insert(session, features.clone()) != Some(features)
    }

    /// Starts waiting for the hello of `session`; it is asked for one if none comes.
    fn expect(&mut self, session: u32, now: Instant) {
        if !self.known.contains_key(&session) && !self.given_up.contains(&session) {
            self.asks.entry(session).or_insert(Ask {
                next: now + ASK_DELAYS[0],
                sent: 0,
            });
        }
    }

    /// The people to ask for their hello now; each is asked at most `ASK_DELAYS.len()` times.
    fn due(&mut self, now: Instant) -> Vec<u32> {
        let mut due = Vec::new();
        let mut done = 0usize;
        let given_up = &mut self.given_up;
        self.asks.retain(|session, ask| {
            if ask.next > now {
                return true;
            }
            due.push(*session);
            ask.sent += 1;
            match ASK_DELAYS.get(ask.sent) {
                Some(delay) => {
                    ask.next = now + *delay;
                    true
                }
                None => {
                    done += 1;
                    given_up.insert(*session);
                    false
                }
            }
        });
        if done > 0 {
            // Most likely plain Mumble clients; only the count, never a name (FR-1.62).
            tracing::debug!(people = done, "no mumcord hello after 3 requests");
        }
        due.sort_unstable();
        due
    }

    /// Records the bot info of a hello; true when it changed.
    fn learn_bot(&mut self, session: u32, bot: Option<Bot>) -> bool {
        match bot {
            Some(bot) => self.bots.insert(session, bot.clone()) != Some(bot),
            None => self.bots.remove(&session).is_some(),
        }
    }

    /// Marks `session` as greeted; true when it was not yet (so a hello must be sent).
    fn greet(&mut self, session: u32) -> bool {
        self.greeted.insert(session)
    }

    /// Whether a hello from `session` is answered now: always the first one (`first`), a request
    /// only when the last reply to this sender is older than [`ANSWER_INTERVAL`]. Entries are
    /// keyed by server-assigned sessions and dropped in `forget`, so the map stays bounded by the
    /// users on the server.
    fn should_answer(&mut self, session: u32, first: bool, ask: bool, now: Instant) -> bool {
        let recent = self
            .answered
            .get(&session)
            .is_some_and(|last| now.saturating_duration_since(*last) < ANSWER_INTERVAL);
        if first || (ask && !recent) {
            self.answered.insert(session, now);
            true
        } else {
            false
        }
    }

    fn forget(&mut self, session: u32) -> bool {
        self.answered.remove(&session);
        self.greeted.remove(&session);
        self.asks.remove(&session);
        self.given_up.remove(&session);
        self.bots.remove(&session);
        self.known.remove(&session).is_some()
    }
}

fn send(handle: &Handle, receivers: &[u32], ask: bool) {
    if receivers.is_empty() {
        return;
    }
    // Old servers (< 1.4) cannot forward plugin data; there the badge simply does not appear.
    if let Err(e) = handle.send_plugin_data(receivers, DATA_ID, encode(ask)) {
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
        let now = Instant::now();
        for session in &everyone {
            peers.expect(*session, now);
        }
    }
    send(&ctx.handle, &everyone, false);
}

/// Someone joined after the sync: they get our hello.
pub(crate) fn on_user_joined(state: &ServerPeers, ctx: &SessionCtx, session: u32) {
    if ctx.handle.with_state(|s| s.session) == Some(session) {
        return;
    }
    let first = lock(state).get_mut(&ctx.server_id).is_some_and(|peers| {
        if peers.synced {
            peers.expect(session, Instant::now());
        }
        peers.synced && peers.greet(session)
    });
    if first {
        send(&ctx.handle, &[session], false);
    }
}

/// Forgets a person who left; true when they were a known Mumcord user.
pub(crate) fn on_user_removed(state: &ServerPeers, server_id: ServerId, session: u32) -> bool {
    lock(state)
        .get_mut(&server_id)
        .is_some_and(|peers| peers.forget(session))
}

/// The session's tick: asks the people who still have not said hello (issue #647).
pub(crate) fn on_tick(state: &ServerPeers, ctx: &SessionCtx) {
    let due = lock(state)
        .get_mut(&ctx.server_id)
        .filter(|peers| peers.synced)
        .map(|peers| peers.due(Instant::now()))
        .unwrap_or_default();
    send(&ctx.handle, &due, true);
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
        let first = peers.synced && peers.greet(sender);
        (
            peers.learn(sender, hello.features) | peers.learn_bot(sender, hello.bot),
            // A request is always answered, without asking back, so there is no ping-pong.
            peers.synced && peers.should_answer(sender, first, hello.ask, Instant::now()),
        )
    };
    if reply {
        send(&ctx.handle, &[sender], false);
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
    #[test]
    fn a_request_carries_ask_and_older_hellos_do_not() {
        assert!(decode(&encode(true)).expect("request").ask);
        assert!(!decode(&encode(false)).expect("hello").ask);
        // A hello from an older Mumcord has no `ask` field at all.
        let old = br#"{"app":"mumcord","v":1,"features":["files"]}"#;
        assert!(!decode(old).expect("old hello").ask);
        assert!(
            !String::from_utf8(encode(false))
                .expect("utf8")
                .contains("ask")
        );
    }

    #[test]
    fn a_request_is_answered_at_most_once_per_interval_and_sender() {
        let mut peers = Peers::default();
        let start = Instant::now();
        // The first hello of a new peer is answered.
        assert!(peers.should_answer(7, true, true, start));
        // Five more requests in a row get no reply.
        for i in 1..=5 {
            assert!(!peers.should_answer(7, false, true, start + Duration::from_secs(i)));
        }
        // A different sender is still answered, once.
        assert!(peers.should_answer(8, false, true, start + Duration::from_secs(2)));
        assert!(!peers.should_answer(8, false, true, start + Duration::from_secs(3)));
        // After the interval the same sender is answered again.
        assert!(peers.should_answer(7, false, true, start + ANSWER_INTERVAL));
        assert!(!peers.should_answer(7, false, true, start + ANSWER_INTERVAL));
        // A plain hello without a request is never answered unless it is the first.
        assert!(!peers.should_answer(9, false, false, start));
        // Leaving clears the entry.
        peers.forget(7);
        assert!(peers.should_answer(7, false, true, start + ANSWER_INTERVAL));
        assert!(peers.answered.len() <= 3);
    }

    #[test]
    fn someone_without_a_hello_is_asked_three_times_then_left_alone() {
        let mut peers = Peers::default();
        let start = Instant::now();
        peers.expect(7, start);
        assert!(peers.due(start + Duration::from_secs(9)).is_empty());
        assert_eq!(peers.due(start + Duration::from_secs(10)), vec![7]);
        assert!(peers.due(start + Duration::from_secs(39)).is_empty());
        assert_eq!(peers.due(start + Duration::from_secs(40)), vec![7]);
        assert_eq!(peers.due(start + Duration::from_secs(130)), vec![7]);
        // Three requests: a plain Mumble client is not asked again during this stay.
        assert!(peers.due(start + Duration::from_secs(10_000)).is_empty());
        peers.expect(7, start);
        assert!(peers.due(start + Duration::from_secs(20_000)).is_empty());
        // Only a new stay starts over.
        peers.forget(7);
        peers.expect(7, start);
        assert_eq!(peers.due(start + Duration::from_secs(20_000)), vec![7]);
    }

    #[test]
    fn a_hello_ends_the_asking_and_known_people_are_never_asked() {
        let mut peers = Peers::default();
        let start = Instant::now();
        peers.expect(7, start);
        peers.expect(8, start);
        peers.learn(7, vec!["files".into()]);
        peers.expect(7, start);
        assert_eq!(peers.due(start + Duration::from_secs(60)), vec![8]);
        peers.forget(8);
        assert!(peers.due(start + Duration::from_secs(600)).is_empty());
    }

    use super::*;

    #[test]
    fn file_and_broadcast_features_are_valid_names_and_announced() {
        for name in [FEATURE_FILES, FEATURE_SCREEN_BROADCAST] {
            assert!(!name.is_empty() && name.len() <= MAX_FEATURE_LEN);
            assert!(FEATURES.contains(&name));
        }
    }

    #[test]
    fn hello_round_trips() {
        let hello = decode(&encode(false)).expect("own hello decodes");
        assert_eq!(hello.bot, None);
        let features = hello.features;
        assert_eq!(
            features,
            [
                "camera",
                "chat-request",
                "files",
                "full-image",
                "histimg",
                "now-gaming",
                "now-playing",
                "p2p",
                "plugins",
                "screen-broadcast",
                "screen-share",
                "server-plugins",
                "watch-party"
            ]
        );
        assert!(encode(true).len() <= PLUGIN_DATA_MAX);
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
            Some(Bot {
                name: "mumbot".into(),
                product: "Mumbot".into(),
                version: Some("2.1.0".into()),
                modules: vec!["music".into(), "radio".into()],
                voice: true,
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

    fn bot_of_hello(bot: &str) -> Option<Bot> {
        let data = format!(r#"{{"app":"mumcord","v":1,"features":[],"bot":{bot}}}"#);
        decode(data.as_bytes()).expect("the hello stays valid").bot
    }

    #[test]
    fn new_bot_fields_are_read() {
        let bot = bot_of_hello(
            r#"{"name":"mumbot","product":"Mumbot","version":"0.4.1",
                "modules":["history","peer","webhook"],"voice":false,"look":"bot"}"#,
        )
        .expect("bot");
        assert_eq!(bot.product, "Mumbot");
        assert_eq!(bot.version.as_deref(), Some("0.4.1"));
        assert_eq!(bot.modules, ["history", "peer", "webhook"]);
        assert!(!bot.voice);
        let dj = bot_of_hello(r#"{"name":"mumbot","product":"Mumbot DJ","voice":true}"#);
        assert_eq!(
            dj.as_ref().map(|b| (b.product.as_str(), b.voice)),
            Some(("Mumbot DJ", true))
        );
        assert_eq!(dj.and_then(|b| b.version), None);
    }

    #[test]
    fn old_bots_get_a_product_from_their_name_and_voice_from_music() {
        let relay = bot_of_hello(
            r#"{"name":"mumbot","version":"0.4.1","modules":["history","peer","webhook"]}"#,
        )
        .expect("bot");
        assert_eq!(relay.product, "Mumbot");
        assert_eq!(relay.version.as_deref(), Some("0.4.1"));
        assert!(!relay.voice);
        let dj = bot_of_hello(r#"{"name":"mumbot","modules":["music"]}"#).expect("bot");
        assert!(dj.voice, "music without the voice field means it speaks");
        let quiet =
            bot_of_hello(r#"{"name":"mumbot","modules":["music"],"voice":false}"#).expect("bot");
        assert!(!quiet.voice, "an explicit voice field wins");
    }

    #[test]
    fn garbage_bot_fields_drop_only_that_field() {
        let long = "x".repeat(MAX_BOT_PRODUCT_LEN + 1);
        for product in [
            "\"<b>Evil</b>\"".to_owned(),
            "\"Mum\\nbot\"".to_owned(),
            "\" Mumbot\"".to_owned(),
            "\"Mum  bot\"".to_owned(),
            "\"\"".to_owned(),
            "\"Mümbot\"".to_owned(),
            format!("\"{long}\""),
            "42".to_owned(),
            "null".to_owned(),
            "[\"Mumbot\"]".to_owned(),
        ] {
            let bot = bot_of_hello(&format!(r#"{{"name":"mumbot","product":{product}}}"#))
                .expect("the bot stays valid");
            assert_eq!(bot.product, "Mumbot", "{product}");
        }
        let long = "1".repeat(MAX_BOT_VERSION_LEN + 1);
        for version in [
            "\"1.0 beta\"".to_owned(),
            "\"<1>\"".to_owned(),
            "\"\"".to_owned(),
            format!("\"{long}\""),
            "1".to_owned(),
            "{}".to_owned(),
        ] {
            let bot = bot_of_hello(&format!(r#"{{"name":"mumbot","version":{version}}}"#))
                .expect("the bot stays valid");
            assert_eq!(bot.version, None, "{version}");
        }
        let bot = bot_of_hello(r#"{"name":"mumbot","version":"1.0.0-rc.1+abc"}"#);
        assert_eq!(
            bot.and_then(|b| b.version).as_deref(),
            Some("1.0.0-rc.1+abc")
        );
        for voice in ["\"yes\"", "1", "null"] {
            let bot = bot_of_hello(&format!(r#"{{"name":"mumbot","voice":{voice}}}"#))
                .expect("the bot stays valid");
            assert!(!bot.voice, "{voice}");
        }
    }

    #[test]
    fn hellos_without_a_bot_are_people() {
        let data = br#"{"app":"mumcord","v":1,"features":["files"]}"#;
        assert_eq!(decode(data).expect("valid").bot, None);
    }

    #[test]
    fn bots_are_learned_and_forgotten() {
        let mut peers = Peers::default();
        let bot = Bot {
            name: "mumbot".into(),
            product: "Mumbot".into(),
            version: None,
            modules: vec!["music".into()],
            voice: true,
        };
        assert_eq!(
            bot.plugin(),
            PluginBot {
                name: "mumbot".into(),
                modules: vec!["music".into()],
            }
        );
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
