//! The sync flow between several in-memory clients over a simulated server (AC-1.51,
//! AC-1.51b, AC-1.51c): each client has its own store; packets are passed on as the server
//! would forward plugin data.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, MutexGuard};

use mumcord_store::{Conversation, NewChatMessage, NewServer, ServerId, Store, UnixMillis};

use super::core::{Core, Env, MAX_AGE_MS, RequestError};
use super::hash::{Content, content_hash};
use super::wire::{self, Packet, WireMessage};
use crate::chat::ChatMessageView;
use crate::session::EngineEvent;

const START: UnixMillis = 1_700_000_000_000;
const LOBBY: u32 = 1;
const RAID: u32 = 2;
const ROOT: u32 = 0;

struct TestEnv {
    _dir: tempfile::TempDir,
    store: Mutex<Store>,
    server: ServerId,
    sharing: bool,
    cert: String,
}

impl Env for TestEnv {
    fn server_id(&self) -> ServerId {
        self.server
    }
    fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().expect("store")
    }
    fn sharing(&self) -> bool {
        self.sharing
    }
    fn own_cert(&self) -> Option<String> {
        Some(self.cert.clone())
    }
    fn channel_key(&self, channel: u32) -> Option<String> {
        match channel {
            ROOT => Some(String::new()),
            LOBBY => Some("Lobby".into()),
            RAID => Some("Raid".into()),
            _ => None,
        }
    }
}

struct Node {
    name: String,
    core: Core,
    env: TestEnv,
    events: Vec<EngineEvent>,
}

/// Clients by session plus a clock; [`Net::run`] ticks everyone and forwards packets.
struct Net {
    nodes: BTreeMap<u32, Node>,
    now: UnixMillis,
    /// Every forwarded packet as (sender, receivers, packet).
    log: Vec<(u32, Vec<u32>, Packet)>,
}

fn key(channel: u32) -> &'static str {
    if channel == LOBBY { "Lobby" } else { "Raid" }
}

fn cert(session: u32) -> String {
    format!("{session:040x}")
}

impl Net {
    fn new(sessions: &[u32]) -> Self {
        let mut nodes = BTreeMap::new();
        for &session in sessions {
            let dir = tempfile::tempdir().expect("tempdir");
            let store = Store::open(dir.path()).expect("store");
            let server = store
                .add_server(&NewServer {
                    label: "Test".into(),
                    address: "localhost".into(),
                    port: 64738,
                    username: format!("user{session}"),
                    save_history: true,
                })
                .expect("server")
                .id;
            nodes.insert(
                session,
                Node {
                    name: format!("user{session}"),
                    core: Core::new(session, START),
                    env: TestEnv {
                        _dir: dir,
                        store: Mutex::new(store),
                        server,
                        sharing: true,
                        cert: cert(session),
                    },
                    events: Vec::new(),
                },
            );
        }
        let mut net = Self {
            nodes,
            now: START,
            log: Vec::new(),
        };
        for &session in sessions {
            net.place(session, &[ROOT]);
        }
        net
    }

    /// Everyone sees `session` in / following `channels` now.
    fn place(&mut self, session: u32, channels: &[u32]) {
        let name = self.nodes[&session].name.clone();
        for node in self.nodes.values_mut() {
            node.core
                .user_seen(session, &name, channels.iter().copied(), self.now);
        }
    }

    fn node(&mut self, session: u32) -> &mut Node {
        self.nodes.get_mut(&session).expect("node")
    }

    /// A channel message from `sender` as stored by `holder`.
    fn hold(&mut self, holder: u32, channel: u32, sender: u32, sent_at: UnixMillis, body: &str) {
        let node = self.node(holder);
        node.env
            .store()
            .append_message(
                node.env.server,
                &NewChatMessage {
                    conversation: Conversation::Channel(key(channel).into()),
                    sender_name: format!("user{sender}"),
                    sender_cert_hash: Some(cert(sender)),
                    sent_at,
                    body: body.into(),
                },
            )
            .expect("append");
    }

    fn deliver(&mut self, from: u32, to: &[u32], data: &[u8]) {
        for receiver in to {
            if let Some(node) = self.nodes.get_mut(receiver) {
                node.core.on_packet(&node.env, from, data, self.now);
                node.events.extend(node.core.take_events());
            }
        }
    }

    /// Runs for `ms` in 100 ms steps.
    fn run(&mut self, ms: i64) {
        let end = self.now + ms;
        while self.now < end {
            self.now += 100;
            let mut sent = Vec::new();
            for (&session, node) in &mut self.nodes {
                node.core.tick(&node.env, self.now);
                node.events.extend(node.core.take_events());
                for (to, data) in node.core.take_outgoing(self.now) {
                    sent.push((session, to, data));
                }
            }
            for (from, to, data) in sent {
                self.log
                    .push((from, to.clone(), wire::decode(&data).expect("valid packet")));
                self.deliver(from, &to, &data);
            }
        }
    }

    fn synced(&self, session: u32) -> Vec<ChatMessageView> {
        self.nodes[&session]
            .events
            .iter()
            .filter_map(|e| match e {
                EngineEvent::HistorySynced { messages, .. } => Some(messages.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    fn finished(&self, session: u32, channel: u32) -> Option<u32> {
        self.nodes[&session].events.iter().find_map(|e| match e {
            EngineEvent::HistorySyncStatus {
                channel_id,
                running: false,
                added,
            } if *channel_id == channel => Some(*added),
            _ => None,
        })
    }

    fn stored(&mut self, session: u32, channel: u32) -> Vec<(UnixMillis, String)> {
        let node = self.node(session);
        node.env
            .store()
            .messages(
                node.env.server,
                &Conversation::Channel(key(channel).into()),
                1000,
            )
            .expect("messages")
            .into_iter()
            .map(|m| (m.sent_at, m.body))
            .collect()
    }

    fn fetches(&self, from: u32, to: u32) -> usize {
        self.log
            .iter()
            .filter(|(f, t, p)| *f == from && t == &[to] && matches!(p, Packet::Fetch { .. }))
            .count()
    }
}

fn bodies(messages: &[ChatMessageView]) -> Vec<&str> {
    messages.iter().map(|m| m.body.as_str()).collect()
}

#[test]
fn a_late_joiner_gets_missed_channel_messages_but_no_private_ones() {
    let mut net = Net::new(&[1, 2]);
    net.place(1, &[LOBBY]);
    for (i, body) in ["one", "two", "three"].iter().enumerate() {
        net.hold(
            1,
            LOBBY,
            3,
            START - 60_000 + i64::try_from(i).unwrap_or(0) * 1000,
            body,
        );
    }
    let server = net.nodes[&1].env.server;
    net.node(1)
        .env
        .store()
        .append_message(
            server,
            &NewChatMessage {
                conversation: Conversation::Private(cert(3)),
                sender_name: "user3".into(),
                sender_cert_hash: Some(cert(3)),
                sent_at: START - 30_000,
                body: "secret".into(),
            },
        )
        .expect("private");
    // Too old to sync (more than 7 days).
    net.hold(1, LOBBY, 3, START - MAX_AGE_MS - 1000, "ancient");

    net.place(2, &[LOBBY]);
    net.run(10_000);

    let synced = net.synced(2);
    assert_eq!(bodies(&synced), ["one", "two", "three"]);
    assert!(
        synced
            .iter()
            .all(|m| m.synced_from.as_deref() == Some("user1"))
    );
    assert!(synced.iter().all(|m| !m.own));
    assert_eq!(net.finished(2, LOBBY), Some(3));
    let stored = net.stored(2, LOBBY);
    assert_eq!(stored.len(), 3);
    assert!(stored.windows(2).all(|w| w[0].0 <= w[1].0), "time order");
    let node = net.node(2);
    let private = node
        .env
        .store()
        .messages(node.env.server, &Conversation::Private(cert(3)), 10)
        .expect("private");
    assert!(private.is_empty(), "private chats are never shared");
}

#[test]
fn nothing_is_shared_while_sharing_is_off() {
    let mut net = Net::new(&[1, 2]);
    net.node(1).env.sharing = false;
    net.place(1, &[LOBBY]);
    net.hold(1, LOBBY, 3, START - 5000, "hidden");
    net.place(2, &[LOBBY]);
    net.run(5000);
    assert!(net.synced(2).is_empty());
    assert_eq!(net.finished(2, LOBBY), Some(0));
}

#[test]
fn overlapping_holders_deliver_each_missing_message_once() {
    let mut net = Net::new(&[1, 2, 3]);
    net.place(1, &[LOBBY]);
    net.place(3, &[LOBBY]);
    for i in 0..6 {
        let t = START - 60_000 + i * 1000;
        let body = format!("m{i}");
        net.hold(1, LOBBY, 9, t, &body);
        // Holder 3 saw the same messages a few milliseconds later (its own clock).
        net.hold(3, LOBBY, 9, t + 7, &body);
    }
    net.hold(3, LOBBY, 9, START - 30_000, "only at 3");
    // The joiner already has two of them (received 300 ms apart from holder 1).
    net.hold(2, LOBBY, 9, START - 60_000 + 300, "m0");
    net.hold(2, LOBBY, 9, START - 59_000 + 300, "m1");

    net.place(2, &[LOBBY]);
    net.run(15_000);

    let synced = net.synced(2);
    let mut got = bodies(&synced);
    got.sort_unstable();
    assert_eq!(got, ["m2", "m3", "m4", "m5", "only at 3"], "no duplicates");
    assert_eq!(net.stored(2, LOBBY).len(), 7);
    assert!(
        net.fetches(2, 1) == 1 && net.fetches(2, 3) == 1,
        "work is spread"
    );
    let from_1 = synced
        .iter()
        .filter(|m| m.synced_from.as_deref() == Some("user1"))
        .count();
    let from_3 = synced
        .iter()
        .filter(|m| m.synced_from.as_deref() == Some("user3"))
        .count();
    assert!(from_1 >= 1 && from_3 >= 1, "{from_1} / {from_3}");
}

#[test]
fn only_messages_before_join_or_follow_are_synced() {
    let mut net = Net::new(&[1, 2]);
    net.place(1, &[LOBBY, RAID]);
    net.hold(1, LOBBY, 3, START - 5000, "lobby before");
    net.run(1000);
    let joined = net.now;
    net.place(2, &[LOBBY]);
    // Arrives live at both after the join; must not be synced again.
    net.hold(1, LOBBY, 3, joined + 200, "lobby after");
    net.hold(2, LOBBY, 3, joined + 201, "lobby after");
    net.hold(1, RAID, 3, joined + 300, "raid before follow");
    net.run(5000);
    let followed = net.now;
    net.place(2, &[LOBBY, RAID]);
    net.hold(1, RAID, 3, followed + 100, "raid after follow");
    net.run(5000);

    let synced = net.synced(2);
    assert_eq!(bodies(&synced), ["lobby before", "raid before follow"]);
    assert_eq!(net.stored(2, LOBBY).len(), 2);
}

#[test]
fn holders_clamp_requests_reaching_past_the_requesters_cutoff() {
    let mut net = Net::new(&[1, 2]);
    net.place(1, &[LOBBY]);
    net.run(1000);
    net.place(2, &[LOBBY]);
    let seen_joining = net.now;
    net.hold(1, LOBBY, 3, seen_joining - 500, "before");
    net.hold(1, LOBBY, 3, seen_joining + 500, "after");
    net.now += 2000;
    // A hand-made request that asks for everything up to now.
    let request = wire::encode(&Packet::HashRequest {
        req: 77,
        channel: LOBBY,
        from: 0,
        to: net.now,
    });
    net.deliver(2, &[1], &request);
    let later = net.now + 1000;
    let outgoing = net.node(1).core.take_outgoing(later);
    let hashes: Vec<_> = outgoing
        .iter()
        .filter_map(|(_, d)| match wire::decode(d) {
            Ok(Packet::Hashes { hashes, .. }) => Some(hashes),
            _ => None,
        })
        .flatten()
        .collect();
    let before = content_hash(&Content {
        channel: "Lobby",
        sender_name: "user3",
        sender_cert_hash: Some(&cert(3)),
        sent_at: seen_joining - 500,
        body: "before",
    });
    assert_eq!(hashes, [before]);
}

#[test]
fn unsolicited_and_tampered_data_is_ignored() {
    let mut net = Net::new(&[1, 2]);
    net.place(2, &[LOBBY]);
    // Data nobody asked for.
    let stream = wire::encode_messages(&[WireMessage {
        ts: START - 1000,
        n: "evil".into(),
        h: None,
        b: "spam".into(),
    }]);
    for packet in wire::chunk_stream(1, &stream)
        .into_iter()
        .chain(wire::chunk_hashes(1, &[[1; 8]]))
        .chain([Packet::Availability {
            req: 1,
            channel: LOBBY,
            count: 5,
            oldest: 0,
            newest: 1,
        }])
    {
        net.deliver(1, &[2], &wire::encode(&packet));
    }
    net.run(1000);
    assert!(net.synced(2).is_empty());
    assert!(net.stored(2, LOBBY).is_empty());

    // A holder that sends something other than what its hash promised.
    let mut net = Net::new(&[1, 2]);
    net.place(1, &[LOBBY]);
    net.hold(1, LOBBY, 3, START - 5000, "genuine");
    net.place(2, &[LOBBY]);
    let fetch_req = loop {
        net.run(100);
        let fetch = net.log.iter().find_map(|(_, _, p)| match p {
            Packet::Fetch { req, .. } => Some(*req),
            _ => None,
        });
        if let Some(req) = fetch {
            break req;
        }
        assert!(net.now < START + 10_000, "no fetch sent");
    };
    // The genuine answer never leaves the holder.
    net.node(1).core = Core::new(1, net.now);
    let forged = wire::encode_messages(&[WireMessage {
        ts: START - 5000,
        n: "user3".into(),
        h: Some(cert(3)),
        b: "forged".into(),
    }]);
    for packet in wire::chunk_stream(fetch_req, &forged) {
        net.deliver(1, &[2], &wire::encode(&packet));
    }
    assert!(net.synced(2).is_empty());
}

#[test]
fn requests_and_sending_are_rate_limited() {
    let mut net = Net::new(&[1, 2]);
    net.place(1, &[LOBBY]);
    net.place(2, &[LOBBY]);
    net.hold(1, LOBBY, 3, START - 5000, "x");
    let request = wire::encode(&Packet::AvailabilityRequest {
        req: 5,
        channel: LOBBY,
        from: 0,
        to: START,
    });
    for _ in 0..50 {
        net.deliver(2, &[1], &request);
    }
    let later = net.now + 60_000;
    let answers = net.node(1).core.take_outgoing(later).len();
    assert_eq!(answers, 6, "per-person request budget");

    let mut core = Core::new(1, START);
    core.user_seen(1, "me", [LOBBY], START);
    core.user_seen(2, "other", [LOBBY], START);
    let env = &net.nodes[&1].env;
    for _ in 0..20 {
        core.on_packet(env, 2, &request, START);
    }
    let first = core.take_outgoing(START).len();
    assert!(first <= 10, "burst {first}");
}

#[test]
fn load_earlier_needs_a_live_channel_and_reaches_before_the_given_time() {
    let mut net = Net::new(&[1, 2]);
    net.place(1, &[LOBBY]);
    net.place(2, &[LOBBY]);
    net.run(5000);
    net.hold(1, LOBBY, 3, START - 10_000, "older");
    net.hold(1, LOBBY, 3, START - 2_000, "newer");
    assert_eq!(
        net.node(2).core.request(RAID, None, START),
        Err(RequestError::NotLive)
    );
    net.node(2)
        .core
        .request(LOBBY, Some(START - 5_000), START)
        .expect("request");
    net.run(5000);
    let got: HashMap<_, _> = net
        .synced(2)
        .into_iter()
        .map(|m| (m.body, m.synced_from))
        .collect();
    assert_eq!(got.len(), 1);
    assert!(got.contains_key("older"));
}

#[test]
fn copies_stamped_seconds_apart_by_skewed_clocks_are_not_duplicated() {
    let mut net = Net::new(&[1, 2]);
    net.place(1, &[LOBBY]);
    let t = START - 60_000;
    // Holder 1's clock runs 3 s ahead of the joiner's.
    net.hold(1, LOBBY, 9, t + 3_000, "<b>seen</b> by both");
    net.hold(1, LOBBY, 9, t + 20_000, "only at 1");
    net.hold(2, LOBBY, 9, t, "seen by both");
    // Same text, but too far apart to be the same message.
    net.hold(1, LOBBY, 9, t + 40_000, "again");
    net.hold(2, LOBBY, 9, t + 30_000, "again");

    net.place(2, &[LOBBY]);
    net.run(10_000);

    let synced = net.synced(2);
    let mut got = bodies(&synced);
    got.sort_unstable();
    assert_eq!(got, ["again", "only at 1"]);
}

#[test]
fn skewed_holders_deliver_each_message_once_but_keep_repeats() {
    let mut net = Net::new(&[1, 2, 3]);
    net.place(1, &[LOBBY]);
    net.place(3, &[LOBBY]);
    let t = START - 60_000;
    for (i, body) in ["a", "b", "ok", "ok"].iter().enumerate() {
        let at = t + i64::try_from(i).unwrap_or(0) * 2_000;
        net.hold(1, LOBBY, 9, at, body);
        // Holder 3's clock runs 4 s behind holder 1's.
        net.hold(3, LOBBY, 9, at - 4_000, body);
    }

    net.place(2, &[LOBBY]);
    net.run(15_000);

    let synced = net.synced(2);
    let mut got = bodies(&synced);
    got.sort_unstable();
    assert_eq!(
        got,
        ["a", "b", "ok", "ok"],
        "one copy each, the repeat kept"
    );
    assert_eq!(net.stored(2, LOBBY).len(), 4);
}

#[test]
fn a_holder_leaving_mid_transfer_hands_its_share_to_another() {
    let mut net = Net::new(&[1, 2, 3]);
    net.place(1, &[LOBBY]);
    net.place(3, &[LOBBY]);
    for i in 0..6 {
        let t = START - 60_000 + i * 1000;
        let body = format!("m{i}");
        net.hold(1, LOBBY, 9, t, &body);
        net.hold(3, LOBBY, 9, t, &body);
    }
    net.place(2, &[LOBBY]);
    let mut waited = 0;
    while net.fetches(2, 3) == 0 {
        assert!(waited < 10_000, "holder 3 was never asked");
        net.run(100);
        waited += 100;
    }
    // Holder 3 disconnects before sending anything.
    net.nodes.remove(&3);
    for node in net.nodes.values_mut() {
        node.core.user_left(3);
    }
    net.run(15_000);

    let synced = net.synced(2);
    let mut got = bodies(&synced);
    got.sort_unstable();
    assert_eq!(got, ["m0", "m1", "m2", "m3", "m4", "m5"]);
    assert!(
        synced
            .iter()
            .all(|m| m.synced_from.as_deref() == Some("user1"))
    );
    assert_eq!(net.fetches(2, 1), 2, "the share of 3 came from 1");
    assert_eq!(net.finished(2, LOBBY), Some(6));
}

#[test]
fn a_silent_holder_times_out_and_its_share_is_fetched_elsewhere() {
    let mut net = Net::new(&[1, 2, 3]);
    net.place(1, &[LOBBY]);
    net.place(3, &[LOBBY]);
    for i in 0..4 {
        let t = START - 60_000 + i * 1000;
        let body = format!("m{i}");
        net.hold(1, LOBBY, 9, t, &body);
        net.hold(3, LOBBY, 9, t, &body);
    }
    net.place(2, &[LOBBY]);
    let mut waited = 0;
    while net.fetches(2, 3) == 0 {
        assert!(waited < 10_000, "holder 3 was never asked");
        net.run(100);
        waited += 100;
    }
    // Holder 3 stays connected but stops answering.
    let silent = net.nodes.remove(&3).expect("node 3");
    net.run(40_000);
    net.nodes.insert(3, silent);

    let mut got: Vec<String> = net.synced(2).into_iter().map(|m| m.body).collect();
    got.sort_unstable();
    assert_eq!(got, ["m0", "m1", "m2", "m3"]);
    assert_eq!(net.finished(2, LOBBY), Some(4));
}
