//! Engine bridge for plugins (spec 06, FR-6.16 … FR-6.19, FR-6.35, FR-6.38, FR-6.39, FR-6.77).
//!
//! The engine only carries plugin traffic and reports what happens on the server; the plugin
//! service (`crates/plugins`, glued in by the app) decides what each plugin may see. This module
//! turns session activity into [`PluginEngineEvent`]s on one bounded channel and sends plugin
//! messages through the protocol's low-priority lane, so Mumcord's own features always go first.
//!
//! Rules applied here, for every plugin alike:
//! - Nothing from or about a blocked person leaves this module; dropped messages are counted.
//! - Only `mumcord.p.<plugin-id>`, `mumcord.plugins` and `mumcord.srvplugins` are forwarded, each
//!   at most [`PLUGIN_DATA_MAX`] bytes.
//! - `mumcord.srvplugins` counts only from a registered user with a certificate, pinned per
//!   server entry and identity on first use; a relay pin of the same server is that pin.
//! - Servers before 1.4 (or ones refusing plugin data) have no shared plugin features.
//!
//! Payloads are never logged.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use mumcord_protocol::{
    CommandError, MumbleVersion, PLUGIN_DATA_MAX, PluginData, PluginTicket, ServerState,
    TextMessage,
};
use mumcord_store::{IdentityId, ServerId, Store};
use tokio::sync::mpsc;

use crate::blocking::BlockList;
use crate::inspector::Inspector;
use crate::session::{EngineEvent, SessionCtx, lock};
use crate::{Engine, EngineError};

/// Hello feature of clients that run plugins and exchange their messages (FR-6.35).
pub const FEATURE: &str = "plugins";
/// Hello feature of clients that accept `mumcord.srvplugins` (FR-6.77).
pub const SERVER_FEATURE: &str = "server-plugins";
/// Plugin data id of the list of plugins a client runs (FR-6.35).
pub const ANNOUNCE_ID: &str = "mumcord.plugins";
/// Plugin data id of the plugins a server bot suggests (FR-6.77).
pub const SERVER_PLUGINS_ID: &str = "mumcord.srvplugins";
/// Prefix of a plugin's own messages, followed by the plugin id (FR-6.34).
pub const MESSAGE_PREFIX: &str = "mumcord.p.";

/// Events waiting for the plugin service; more are dropped and counted, never awaited, so the
/// session and voice tasks are never held up by a slow consumer.
const EVENT_BUFFER: usize = 1024;
/// Inspector note for server-plugin lists of unregistered users (FR-6.77, AC-6.26).
const NOT_REGISTERED: &str = "sender not registered";
/// Inspector note for server-plugin lists of a registered user other than the pinned one.
const NOT_PINNED: &str = "sender not pinned";
/// Plugin ids: lowercase letter, then 2–31 lowercase letters, digits or dashes.
const PLUGIN_ID_MIN: usize = 3;
const PLUGIN_ID_MAX: usize = 32;

/// A person on a server as the plugin service sees them. The certificate hash is only for the
/// service's per-plugin key (FR-6.18); plugins never get it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPeer {
    pub session: u32,
    pub name: String,
    pub cert_hash: Option<String>,
}

/// What a bot says about itself in its `mumcord.hello` (untrusted, sanitised): its name and the
/// modules it runs, e.g. `mumbot` with `music`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginBot {
    pub name: String,
    pub modules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginChannel {
    pub id: u32,
    pub name: String,
}

/// The user's own state on a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSelf {
    pub me: PluginPeer,
    pub channel: Option<PluginChannel>,
    /// Muted by themselves, by an admin or suppressed.
    pub muted: bool,
    pub deafened: bool,
}

/// What the engine tells the plugin service. Each event names the server it happened on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginEngineEvent {
    /// After every sync and whenever the server turns out to refuse plugin data: whether
    /// shared plugin features work there (FR-6.38).
    SharedState {
        server_id: ServerId,
        supported: bool,
    },
    /// The connection ended; nothing more comes from this server until the next sync.
    Disconnected { server_id: ServerId },
    /// A person's hello now does (or no longer does, e.g. after leaving) list `plugins`.
    PeerCapable {
        server_id: ServerId,
        session: u32,
        plugins: bool,
    },
    /// A person's hello says they are a bot (or no longer is, e.g. after leaving the server).
    /// It can arrive before or after they join the user's channel.
    PeerBot {
        server_id: ServerId,
        session: u32,
        bot: Option<PluginBot>,
    },
    /// Someone joined the user's channel, or is there when the user arrives (FR-6.16).
    Joined {
        server_id: ServerId,
        peer: PluginPeer,
    },
    /// Someone left the user's channel or the server, or the user left their channel.
    Left {
        server_id: ServerId,
        peer: PluginPeer,
    },
    Talking {
        server_id: ServerId,
        peer: PluginPeer,
        talking: bool,
    },
    /// Someone in the user's channel started sharing their screen.
    ScreenShareStarted {
        server_id: ServerId,
        peer: PluginPeer,
    },
    /// Someone wrote in a channel; the text is not part of it.
    ChatActivity {
        server_id: ServerId,
        sender: PluginPeer,
        channel: PluginChannel,
    },
    /// A channel message whose first line starts with a trigger word such as `!poll`
    /// ([`crate::plain_text::chat_trigger`]); the user's own lines included, blocked senders
    /// never. Other chat text never reaches the plugin layer. The plugin service decides
    /// whether a plugin owns the word.
    ChatTrigger {
        server_id: ServerId,
        sender: PluginPeer,
        channel: PluginChannel,
        trigger: String,
        args: String,
    },
    /// Someone mentioned the user (the UI decides what a mention is); no text.
    Mentioned {
        server_id: ServerId,
        sender: PluginPeer,
    },
    SelfChanged {
        server_id: ServerId,
        state: PluginSelf,
    },
    /// A `mumcord.p.<id>` message or a `mumcord.plugins` announcement, raw.
    PeerData {
        server_id: ServerId,
        sender: PluginPeer,
        data_id: String,
        data: Vec<u8>,
    },
    /// A `mumcord.srvplugins` list from the pinned registered sender, raw.
    ServerPlugins {
        server_id: ServerId,
        sender: PluginPeer,
        data: Vec<u8>,
    },
}

/// Drop counters of one server for the diagnostics (AC-6.13).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PluginTransportStats {
    /// Plugin data of blocked people.
    pub blocked_dropped: u64,
    /// Plugin data over [`PLUGIN_DATA_MAX`] bytes.
    pub oversized_dropped: u64,
    /// `mumcord.srvplugins` of unregistered or unpinned senders.
    pub unregistered_dropped: u64,
    /// Events the plugin service did not take in time.
    pub events_dropped: u64,
    /// Outgoing plugin messages that waited too long in the low-priority lane (this connection).
    pub low_expired: u64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PluginSendError {
    #[error("not connected to this server")]
    NotConnected,
    #[error("this server doesn't support shared plugin features")]
    Unsupported,
    #[error("plugin message too large")]
    TooLarge,
    #[error("not a plugin data id")]
    InvalidId,
    #[error("no recipients")]
    NoRecipients,
}

#[derive(Debug, Clone)]
struct Seen {
    peer: PluginPeer,
    channel: u32,
    blocked: bool,
}

/// What the bridge knows of one synced server.
#[derive(Debug, Default)]
struct ServerView {
    supported: bool,
    own: Option<PluginSelf>,
    users: BTreeMap<u32, Seen>,
    sharers: BTreeSet<u32>,
    capable: BTreeSet<u32>,
    /// Bot info last reported per session.
    bots: BTreeMap<u32, PluginBot>,
    /// Server-plugin sender pinned for this connection when there is no identity to store it.
    sender_pin: Option<String>,
}

impl ServerView {
    fn own_session(&self) -> Option<u32> {
        self.own.as_ref().map(|o| o.me.session)
    }

    fn own_channel(&self) -> Option<u32> {
        self.own
            .as_ref()
            .and_then(|o| o.channel.as_ref())
            .map(|c| c.id)
    }

    /// People in the user's channel that plugins may know about.
    fn around(&self) -> BTreeMap<u32, &PluginPeer> {
        let (own, channel) = (self.own_session(), self.own_channel());
        self.users
            .iter()
            .filter(|(session, seen)| {
                Some(**session) != own && !seen.blocked && Some(seen.channel) == channel
            })
            .map(|(session, seen)| (*session, &seen.peer))
            .collect()
    }

    fn visible(&self, session: u32) -> Option<&PluginPeer> {
        self.users
            .get(&session)
            .filter(|seen| !seen.blocked)
            .map(|seen| &seen.peer)
    }
}

/// The bridge state shared by every connection; reached through the engine's event output.
pub struct PluginHub {
    tx: mpsc::Sender<PluginEngineEvent>,
    rx: Mutex<Option<mpsc::Receiver<PluginEngineEvent>>>,
    servers: Mutex<HashMap<ServerId, ServerView>>,
    stats: Mutex<HashMap<ServerId, PluginTransportStats>>,
}

impl Default for PluginHub {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel(EVENT_BUFFER);
        Self {
            tx,
            rx: Mutex::new(Some(rx)),
            servers: Mutex::default(),
            stats: Mutex::default(),
        }
    }
}

impl PluginHub {
    fn emit(&self, event: PluginEngineEvent) {
        let server_id = event.server_id();
        if self.tx.try_send(event).is_err() {
            self.count(server_id, |s| &mut s.events_dropped);
        }
    }

    fn emit_all(&self, events: Vec<PluginEngineEvent>) {
        events.into_iter().for_each(|e| self.emit(e));
    }

    fn count(
        &self,
        server_id: ServerId,
        field: impl FnOnce(&mut PluginTransportStats) -> &mut u64,
    ) {
        *field(lock(&self.stats).entry(server_id).or_default()) += 1;
    }

    /// Engine events that concern plugins but come from outside the session task (talking from
    /// the voice task, screen shares from their own module).
    pub(crate) fn observe(&self, server: Option<ServerId>, event: &EngineEvent) {
        let Some(server_id) = server else {
            return;
        };
        let events = match event {
            EngineEvent::TalkStateChanged { session, talking } => {
                let servers = lock(&self.servers);
                let Some(peer) = servers.get(&server_id).and_then(|v| v.visible(*session)) else {
                    return;
                };
                vec![PluginEngineEvent::Talking {
                    server_id,
                    peer: peer.clone(),
                    talking: *talking,
                }]
            }
            EngineEvent::ScreenSharesChanged(shares) => {
                let mut servers = lock(&self.servers);
                let Some(view) = servers.get_mut(&server_id) else {
                    return;
                };
                let now: BTreeSet<u32> = shares.iter().map(|s| s.session).collect();
                let around = view.around();
                let started = now
                    .difference(&view.sharers)
                    .filter_map(|s| around.get(s))
                    .map(|peer| PluginEngineEvent::ScreenShareStarted {
                        server_id,
                        peer: (*peer).clone(),
                    })
                    .collect();
                view.sharers = now;
                started
            }
            _ => return,
        };
        self.emit_all(events);
    }
}

impl PluginEngineEvent {
    fn server_id(&self) -> ServerId {
        match self {
            Self::SharedState { server_id, .. }
            | Self::Disconnected { server_id }
            | Self::PeerCapable { server_id, .. }
            | Self::PeerBot { server_id, .. }
            | Self::Joined { server_id, .. }
            | Self::Left { server_id, .. }
            | Self::Talking { server_id, .. }
            | Self::ScreenShareStarted { server_id, .. }
            | Self::ChatActivity { server_id, .. }
            | Self::ChatTrigger { server_id, .. }
            | Self::Mentioned { server_id, .. }
            | Self::SelfChanged { server_id, .. }
            | Self::PeerData { server_id, .. }
            | Self::ServerPlugins { server_id, .. } => *server_id,
        }
    }
}

/// Which plugin traffic a data id is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Message,
    Announce,
    ServerPlugins,
}

fn kind_of(data_id: &str) -> Option<Kind> {
    match data_id {
        ANNOUNCE_ID => Some(Kind::Announce),
        SERVER_PLUGINS_ID => Some(Kind::ServerPlugins),
        _ => data_id
            .strip_prefix(MESSAGE_PREFIX)
            .filter(|id| valid_plugin_id(id))
            .map(|_| Kind::Message),
    }
}

/// `^[a-z][a-z0-9-]{2,31}$`, the manifest's plugin id rule.
fn valid_plugin_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (PLUGIN_ID_MIN..=PLUGIN_ID_MAX).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// Whether a server shares plugin data at all (FR-6.38).
fn shares_plugin_data(version: Option<MumbleVersion>) -> bool {
    version.is_some_and(|v| v >= MumbleVersion::PLUGIN_DATA)
}

fn peer_of(user: &mumcord_protocol::User) -> PluginPeer {
    PluginPeer {
        session: user.session,
        name: user.name.clone(),
        cert_hash: user.cert_hash.clone(),
    }
}

fn channel_of(state: &ServerState, id: u32) -> PluginChannel {
    PluginChannel {
        id,
        name: state
            .channels
            .get(&id)
            .map(|c| c.name.clone())
            .unwrap_or_default(),
    }
}

/// The people and own state of a server as they are now.
fn read_state(
    state: &ServerState,
    server_id: ServerId,
    blocks: &[mumcord_store::BlockedUser],
) -> (Option<PluginSelf>, BTreeMap<u32, Seen>) {
    let own = state.own_user().map(|u| PluginSelf {
        me: peer_of(u),
        channel: Some(channel_of(state, u.channel_id)),
        muted: u.self_mute || u.mute || u.suppress,
        deafened: u.self_deaf || u.deaf,
    });
    let users = state
        .users
        .values()
        .map(|u| {
            let seen = Seen {
                peer: peer_of(u),
                channel: u.channel_id,
                blocked: crate::blocking::user_blocked(blocks, state, server_id, u),
            };
            (u.session, seen)
        })
        .collect();
    (own, users)
}

/// Replaces the people and own state of `view` and returns what plugins should hear about it.
fn apply(
    view: &mut ServerView,
    server_id: ServerId,
    own: Option<PluginSelf>,
    users: BTreeMap<u32, Seen>,
) -> Vec<PluginEngineEvent> {
    let before: BTreeMap<u32, PluginPeer> = view
        .around()
        .into_iter()
        .map(|(s, p)| (s, p.clone()))
        .collect();
    let own_changed = view.own != own;
    view.own = own;
    view.users = users;
    let after = view.around();
    let mut events = Vec::new();
    if own_changed && let Some(state) = &view.own {
        events.push(PluginEngineEvent::SelfChanged {
            server_id,
            state: state.clone(),
        });
    }
    events.extend(
        before
            .iter()
            .filter(|(s, _)| !after.contains_key(s))
            .map(|(_, peer)| PluginEngineEvent::Left {
                server_id,
                peer: peer.clone(),
            }),
    );
    events.extend(
        after
            .iter()
            .filter(|(s, _)| !before.contains_key(s))
            .map(|(_, peer)| PluginEngineEvent::Joined {
                server_id,
                peer: (*peer).clone(),
            }),
    );
    let gone: Vec<u32> = view
        .capable
        .iter()
        .filter(|s| view.visible(**s).is_none())
        .copied()
        .collect();
    for session in gone {
        view.capable.remove(&session);
        events.push(PluginEngineEvent::PeerCapable {
            server_id,
            session,
            plugins: false,
        });
    }
    events
}

/// Re-reads the server after people changed (joined, moved, left, renamed, got blocked).
fn resync(hub: &PluginHub, ctx: &SessionCtx, blocks: &BlockList) {
    let (own, users) = {
        let blocks = lock(blocks);
        ctx.handle
            .with_state(|s| read_state(s, ctx.server_id, &blocks))
    };
    let events = {
        let mut servers = lock(&hub.servers);
        let Some(view) = servers.get_mut(&ctx.server_id) else {
            // Not synced yet: the sync reads everything at once.
            return;
        };
        apply(view, ctx.server_id, own, users)
    };
    hub.emit_all(events);
}

/// The session (re)synced: shared features start afresh.
pub(crate) fn on_synced(hub: &PluginHub, ctx: &SessionCtx, blocks: &BlockList) {
    let supported = shares_plugin_data(ctx.handle.server_version());
    lock(&hub.servers).insert(
        ctx.server_id,
        ServerView {
            supported,
            ..ServerView::default()
        },
    );
    hub.emit(PluginEngineEvent::SharedState {
        server_id: ctx.server_id,
        supported,
    });
    resync(hub, ctx, blocks);
}

pub(crate) fn on_users_changed(hub: &PluginHub, ctx: &SessionCtx, blocks: &BlockList) {
    resync(hub, ctx, blocks);
}

pub(crate) fn on_disconnect(hub: &PluginHub, server_id: ServerId) {
    if lock(&hub.servers).remove(&server_id).is_some() {
        hub.emit(PluginEngineEvent::Disconnected { server_id });
    }
}

/// A hello changed what `session` can do; it is not blocked (blocked hellos never get here).
pub(crate) fn on_peer_known(
    hub: &PluginHub,
    peers: &crate::mumcord::ServerPeers,
    server_id: ServerId,
    session: u32,
) {
    let (plugins, bot) = {
        let servers = lock(peers);
        let known = servers.get(&server_id);
        (
            known
                .and_then(|p| p.features(session))
                .is_some_and(|f| f.iter().any(|x| x == FEATURE)),
            known.and_then(|p| p.bot(session)).cloned(),
        )
    };
    let (changed, bot_changed) = {
        let mut servers = lock(&hub.servers);
        let Some(view) = servers.get_mut(&server_id) else {
            return;
        };
        let visible = view.visible(session).is_some();
        let changed = if plugins && visible {
            view.capable.insert(session)
        } else {
            view.capable.remove(&session)
        };
        let bot = bot.filter(|_| visible);
        let bot_changed = match &bot {
            Some(bot) => view.bots.insert(session, bot.clone()).as_ref() != Some(bot),
            None => view.bots.remove(&session).is_some(),
        };
        (changed, bot_changed.then_some(bot))
    };
    if changed {
        hub.emit(PluginEngineEvent::PeerCapable {
            server_id,
            session,
            plugins,
        });
    }
    if let Some(bot) = bot_changed {
        hub.emit(PluginEngineEvent::PeerBot {
            server_id,
            session,
            bot,
        });
    }
}

/// Plugin data of a blocked person: dropped, and counted when it was plugin traffic (FR-6.19).
pub(crate) fn on_blocked_data(hub: &PluginHub, ctx: &SessionCtx, data: &PluginData) {
    if kind_of(&data.data_id).is_some() {
        hub.count(ctx.server_id, |s| &mut s.blocked_dropped);
    }
}

/// Plugin data of someone not blocked.
pub(crate) fn on_plugin_data(
    hub: &PluginHub,
    ctx: &SessionCtx,
    data: &PluginData,
    inspector: &Inspector,
) {
    let Some(kind) = kind_of(&data.data_id) else {
        return;
    };
    let Some(sender) = data.sender else {
        return;
    };
    if data.data.len() > PLUGIN_DATA_MAX {
        hub.count(ctx.server_id, |s| &mut s.oversized_dropped);
        return;
    }
    let found = ctx.handle.with_state(|s| {
        (s.session != Some(sender))
            .then(|| s.users.get(&sender))
            .flatten()
            .map(|u| (peer_of(u), u.user_id.is_some()))
    });
    let Some((peer, registered)) = found else {
        return;
    };
    let server_id = ctx.server_id;
    let event = match kind {
        Kind::Message | Kind::Announce => PluginEngineEvent::PeerData {
            server_id,
            sender: peer,
            data_id: data.data_id.clone(),
            data: data.data.to_vec(),
        },
        Kind::ServerPlugins => {
            let cert = match (&peer.cert_hash, registered) {
                (Some(cert), true) => cert.to_ascii_lowercase(),
                _ => {
                    inspector.count_ignored(server_id, SERVER_PLUGINS_ID, NOT_REGISTERED);
                    hub.count(server_id, |s| &mut s.unregistered_dropped);
                    return;
                }
            };
            if !accept_sender(hub, ctx, &cert) {
                inspector.count_ignored(server_id, SERVER_PLUGINS_ID, NOT_PINNED);
                hub.count(server_id, |s| &mut s.unregistered_dropped);
                return;
            }
            PluginEngineEvent::ServerPlugins {
                server_id,
                sender: peer,
                data: data.data.to_vec(),
            }
        }
    };
    hub.emit(event);
}

#[derive(Debug, PartialEq, Eq)]
enum PinVerdict {
    Accept,
    /// First registered sender: accept and pin it.
    Pin,
    Reject,
}

/// The relay bot's pin is the shared pin when there is one (FR-6.77: same rules as FR-4.8 …
/// FR-4.10); otherwise the first registered sender is pinned.
fn pin_verdict(relay_bot: Option<&str>, pinned: Option<&str>, cert: &str) -> PinVerdict {
    match relay_bot.or(pinned) {
        Some(pin) if pin.eq_ignore_ascii_case(cert) => PinVerdict::Accept,
        Some(_) => PinVerdict::Reject,
        None => PinVerdict::Pin,
    }
}

/// The pins stored for a server entry and the identity it connects with.
struct StoredPins {
    identity: IdentityId,
    relay_bot: Option<String>,
    sender: Option<String>,
}

fn stored_pins(store: &Store, server_id: ServerId) -> mumcord_store::Result<Option<StoredPins>> {
    let Some(identity) = crate::peer::pin_identity(store, server_id)? else {
        return Ok(None);
    };
    Ok(Some(StoredPins {
        identity,
        relay_bot: store
            .relay_pin(server_id, identity)?
            .map(|p| p.bot_cert_hash),
        sender: store.plugin_sender_pin(server_id, identity)?,
    }))
}

fn accept_sender(hub: &PluginHub, ctx: &SessionCtx, cert: &str) -> bool {
    let stored = match stored_pins(&ctx.store(), ctx.server_id) {
        Ok(stored) => stored,
        Err(e) => {
            tracing::warn!(error = %e, "reading the server plugin sender pin failed");
            return false;
        }
    };
    let Some(StoredPins {
        identity,
        relay_bot,
        sender,
    }) = stored
    else {
        // No identity to pin for: keep the pin for this connection only.
        let mut servers = lock(&hub.servers);
        let Some(view) = servers.get_mut(&ctx.server_id) else {
            return false;
        };
        return match pin_verdict(None, view.sender_pin.as_deref(), cert) {
            PinVerdict::Accept => true,
            PinVerdict::Pin => {
                view.sender_pin = Some(cert.to_owned());
                true
            }
            PinVerdict::Reject => false,
        };
    };
    match pin_verdict(relay_bot.as_deref(), sender.as_deref(), cert) {
        PinVerdict::Accept => true,
        PinVerdict::Reject => false,
        PinVerdict::Pin => {
            tracing::info!("server plugin sender pinned on first use");
            if let Err(e) = ctx.store().set_plugin_sender_pin(
                ctx.server_id,
                identity,
                cert,
                crate::chat::now_millis(),
            ) {
                tracing::warn!(error = %e, "storing the server plugin sender pin failed");
            }
            true
        }
    }
}

/// The channel a text message was written in, if it went to a channel the user reads.
fn text_channel(state: &ServerState, msg: &TextMessage) -> Option<u32> {
    let target = &msg.target;
    target.channels.first().copied().or_else(|| {
        (!target.trees.is_empty())
            .then(|| state.own_user().map(|u| u.channel_id))
            .flatten()
    })
}

/// Chat activity (FR-6.16): who wrote in which channel, never the text.
pub(crate) fn on_text(hub: &PluginHub, ctx: &SessionCtx, blocks: &BlockList, msg: &TextMessage) {
    let Some(actor) = msg.actor else {
        return;
    };
    let blocks = lock(blocks);
    let found = ctx.handle.with_state(|s| {
        let user = s.users.get(&actor)?;
        if s.session == Some(actor) {
            return None;
        }
        let blocked = crate::blocking::user_blocked(&blocks, s, ctx.server_id, user);
        Some((
            peer_of(user),
            text_channel(s, msg).map(|c| channel_of(s, c)),
            blocked,
        ))
    });
    drop(blocks);
    let Some((sender, Some(channel), blocked)) = found else {
        return;
    };
    if blocked {
        return;
    }
    emit_trigger(hub, ctx, &sender, &channel, &msg.message);
    hub.emit(PluginEngineEvent::ChatActivity {
        server_id: ctx.server_id,
        sender,
        channel,
    });
}

/// A `ChatTrigger` if the message has the trigger shape; nothing otherwise.
fn emit_trigger(
    hub: &PluginHub,
    ctx: &SessionCtx,
    sender: &PluginPeer,
    channel: &PluginChannel,
    html: &str,
) {
    if let Some((trigger, args)) = crate::plain_text::chat_trigger(html) {
        hub.emit(PluginEngineEvent::ChatTrigger {
            server_id: ctx.server_id,
            sender: sender.clone(),
            channel: channel.clone(),
            trigger,
            args,
        });
    }
}

/// Chat activity for the user's own channel message: the server does not echo it back, and
/// plugins (e.g. pets reacting to the user's chatting) count it as activity too.
pub(crate) fn on_own_text(hub: &PluginHub, ctx: &SessionCtx, channel_id: u32, html: &str) {
    let found = ctx.handle.with_state(|s| {
        let me = s.own_user()?;
        s.channels
            .contains_key(&channel_id)
            .then(|| (peer_of(me), channel_of(s, channel_id)))
    });
    if let Some((sender, channel)) = found {
        emit_trigger(hub, ctx, &sender, &channel, html);
        hub.emit(PluginEngineEvent::ChatActivity {
            server_id: ctx.server_id,
            sender,
            channel,
        });
    }
}

impl Engine {
    /// The plugin event stream. There is one consumer (the plugin service): the first call
    /// takes it, later calls get `None`.
    pub fn plugin_events(&self) -> Option<mpsc::Receiver<PluginEngineEvent>> {
        lock(&self.events().plugins().rx).take()
    }

    /// Sends a plugin message (`mumcord.p.<id>`) or the plugin announcement
    /// (`mumcord.plugins`) through the low-priority lane. Receivers that are gone, blocked, the
    /// user themselves or without the `plugins` feature are left out. Visibility and rate
    /// rules are the plugin service's.
    pub fn plugin_send(
        &self,
        server_id: ServerId,
        receivers: &[u32],
        data_id: &str,
        data: Vec<u8>,
    ) -> Result<PluginTicket, PluginSendError> {
        if !matches!(kind_of(data_id), Some(Kind::Message | Kind::Announce)) {
            return Err(PluginSendError::InvalidId);
        }
        if data.len() > PLUGIN_DATA_MAX {
            return Err(PluginSendError::TooLarge);
        }
        let ctx = self
            .session_ctx_for(server_id)
            .map_err(|_| PluginSendError::NotConnected)?;
        let hub = self.events().plugins();
        if !self.plugin_shared_supported(server_id) {
            return Err(PluginSendError::Unsupported);
        }
        let to: Vec<u32> = {
            let servers = lock(&hub.servers);
            let Some(view) = servers.get(&server_id) else {
                return Err(PluginSendError::NotConnected);
            };
            let mut seen = BTreeSet::new();
            receivers
                .iter()
                .copied()
                .filter(|s| {
                    view.capable.contains(s) && view.visible(*s).is_some() && seen.insert(*s)
                })
                .collect()
        };
        // The view may lag a moment behind a block; ask the block list itself as well.
        let to: Vec<u32> = to
            .into_iter()
            .filter(|s| !crate::blocking::is_blocked(&ctx, self.blocked_state(), *s))
            .collect();
        if to.is_empty() {
            return Err(PluginSendError::NoRecipients);
        }
        match ctx.handle.send_plugin_data_low(&to, data_id, data) {
            Ok(ticket) => Ok(ticket),
            Err(CommandError::Unsupported { .. }) => {
                mark_unsupported(hub, server_id);
                Err(PluginSendError::Unsupported)
            }
            Err(CommandError::NotConnected) => Err(PluginSendError::NotConnected),
        }
    }

    /// Who is in the user's channel right now, without blocked people (FR-6.17).
    pub fn plugin_presence(&self, server_id: ServerId) -> Vec<PluginPeer> {
        lock(&self.events().plugins().servers)
            .get(&server_id)
            .map(|v| v.around().into_values().cloned().collect())
            .unwrap_or_default()
    }

    /// Whether shared plugin features work on a connected server (FR-6.38).
    pub fn plugin_shared_supported(&self, server_id: ServerId) -> bool {
        lock(&self.events().plugins().servers)
            .get(&server_id)
            .is_some_and(|v| v.supported)
    }

    /// Drop counters of a server for the diagnostics.
    pub fn plugin_stats(&self, server_id: ServerId) -> PluginTransportStats {
        let mut stats = lock(&self.events().plugins().stats)
            .get(&server_id)
            .copied()
            .unwrap_or_default();
        if let Ok(ctx) = self.session_ctx_for(server_id) {
            stats.low_expired = ctx.handle.plugin_data_expired();
        }
        stats
    }

    /// The UI found a mention of the user in a message from `sender_session` (FR-6.16). Blocked
    /// senders, unknown sessions and the user's own messages are ignored.
    pub fn plugin_mentioned(
        &self,
        server_id: ServerId,
        sender_session: u32,
    ) -> Result<(), EngineError> {
        let ctx = self.session_ctx_for(server_id)?;
        if crate::blocking::is_blocked(&ctx, self.blocked_state(), sender_session) {
            return Ok(());
        }
        let hub = self.events().plugins();
        let sender = {
            let servers = lock(&hub.servers);
            servers
                .get(&server_id)
                .filter(|v| v.own_session() != Some(sender_session))
                .and_then(|v| v.visible(sender_session))
                .cloned()
        };
        if let Some(sender) = sender {
            hub.emit(PluginEngineEvent::Mentioned { server_id, sender });
        }
        Ok(())
    }

    /// The block list changed: people who became blocked vanish for plugins, unblocked ones
    /// come back.
    pub(crate) fn plugin_blocks_changed(&self) {
        let hub = self.events().plugins();
        for ctx in self.session_ctxs() {
            resync(hub, &ctx, self.blocked_state());
        }
    }
}

fn mark_unsupported(hub: &Arc<PluginHub>, server_id: ServerId) {
    let changed = lock(&hub.servers)
        .get_mut(&server_id)
        .is_some_and(|v| std::mem::replace(&mut v.supported, false));
    if changed {
        hub.emit(PluginEngineEvent::SharedState {
            server_id,
            supported: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(session: u32) -> PluginPeer {
        PluginPeer {
            session,
            name: format!("u{session}"),
            cert_hash: Some(format!("{session:040x}")),
        }
    }

    fn seen(session: u32, channel: u32, blocked: bool) -> (u32, Seen) {
        (
            session,
            Seen {
                peer: peer(session),
                channel,
                blocked,
            },
        )
    }

    fn me(channel: u32) -> Option<PluginSelf> {
        Some(PluginSelf {
            me: peer(1),
            channel: Some(PluginChannel {
                id: channel,
                name: format!("c{channel}"),
            }),
            muted: false,
            deafened: false,
        })
    }

    fn names(events: &[PluginEngineEvent]) -> Vec<String> {
        events
            .iter()
            .map(|e| match e {
                PluginEngineEvent::Joined { peer, .. } => format!("+{}", peer.session),
                PluginEngineEvent::Left { peer, .. } => format!("-{}", peer.session),
                PluginEngineEvent::SelfChanged { .. } => "self".into(),
                PluginEngineEvent::PeerCapable {
                    session, plugins, ..
                } => format!("cap{session}={plugins}"),
                other => format!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn plugin_ids_follow_the_manifest_rule() {
        assert_eq!(kind_of("mumcord.p.dice"), Some(Kind::Message));
        assert_eq!(kind_of("mumcord.p.echo-lings2"), Some(Kind::Message));
        assert_eq!(kind_of("mumcord.plugins"), Some(Kind::Announce));
        assert_eq!(kind_of("mumcord.srvplugins"), Some(Kind::ServerPlugins));
        for bad in [
            "mumcord.p.",
            "mumcord.p.ab",
            "mumcord.p.Dice",
            "mumcord.p.1dice",
            "mumcord.p.di_ce",
            "mumcord.p.a2345678901234567890123456789012x",
            "mumcord.hello",
            "mumcord.peer",
        ] {
            assert_eq!(kind_of(bad), None, "{bad}");
        }
        assert!(valid_plugin_id("a234567890123456789012345678901"));
    }

    #[test]
    fn plugin_data_needs_server_1_4() {
        assert!(!shares_plugin_data(None));
        assert!(!shares_plugin_data(Some(MumbleVersion::new(1, 3, 4))));
        assert!(shares_plugin_data(Some(MumbleVersion::new(1, 4, 0))));
        assert!(shares_plugin_data(Some(MumbleVersion::new(1, 5, 735))));
    }

    #[test]
    fn presence_follows_the_own_channel_without_blocked_people() {
        let mut view = ServerView::default();
        let users = BTreeMap::from([
            seen(1, 5, false),
            seen(2, 5, false),
            seen(3, 5, true),
            seen(4, 6, false),
        ]);
        let events = apply(&mut view, 7, me(5), users.clone());
        assert_eq!(names(&events), ["self", "+2"]);

        // Moving to channel 6: everyone of 5 left, everyone of 6 joined.
        let mut moved = users.clone();
        moved.insert(1, seen(1, 6, false).1);
        let events = apply(&mut view, 7, me(6), moved.clone());
        assert_eq!(names(&events), ["self", "-2", "+4"]);

        // 4 gets blocked: gone for plugins.
        moved.insert(4, seen(4, 6, true).1);
        let events = apply(&mut view, 7, me(6), moved.clone());
        assert_eq!(names(&events), ["-4"]);
        assert!(view.visible(4).is_none());
        assert!(view.around().is_empty());

        // Nothing changed: nothing to tell.
        assert!(apply(&mut view, 7, me(6), moved).is_empty());
    }

    #[test]
    fn capable_people_who_leave_or_get_blocked_are_reported() {
        let mut view = ServerView::default();
        apply(
            &mut view,
            7,
            me(5),
            BTreeMap::from([seen(1, 5, false), seen(2, 9, false)]),
        );
        view.capable.insert(2);
        let events = apply(
            &mut view,
            7,
            me(5),
            BTreeMap::from([seen(1, 5, false), seen(2, 9, true)]),
        );
        assert_eq!(names(&events), ["cap2=false"]);
        assert!(view.capable.is_empty());
    }

    #[test]
    fn talking_and_screen_shares_skip_blocked_people() {
        let hub = PluginHub::default();
        let mut rx = lock(&hub.rx).take().expect("receiver");
        let mut view = ServerView::default();
        apply(
            &mut view,
            7,
            me(5),
            BTreeMap::from([seen(1, 5, false), seen(2, 5, false), seen(3, 5, true)]),
        );
        lock(&hub.servers).insert(7, view);
        for session in [2, 3, 99] {
            hub.observe(
                Some(7),
                &EngineEvent::TalkStateChanged {
                    session,
                    talking: true,
                },
            );
        }
        hub.observe(
            None,
            &EngineEvent::TalkStateChanged {
                session: 2,
                talking: true,
            },
        );
        let talking = rx.try_recv().expect("talking");
        assert!(matches!(talking, PluginEngineEvent::Talking { peer, .. } if peer.session == 2));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_full_event_queue_drops_and_counts() {
        let hub = PluginHub::default();
        for _ in 0..EVENT_BUFFER + 3 {
            hub.emit(PluginEngineEvent::Disconnected { server_id: 4 });
        }
        assert_eq!(lock(&hub.stats)[&4].events_dropped, 3);
    }

    #[test]
    fn the_relay_pin_is_the_shared_server_plugin_pin() {
        let bot = "aa".repeat(20);
        let other = "bb".repeat(20);
        assert_eq!(pin_verdict(None, None, &bot), PinVerdict::Pin);
        assert_eq!(pin_verdict(None, Some(&bot), &bot), PinVerdict::Accept);
        assert_eq!(pin_verdict(None, Some(&bot), &other), PinVerdict::Reject);
        assert_eq!(
            pin_verdict(Some(&bot), None, &bot.to_uppercase()),
            PinVerdict::Accept
        );
        assert_eq!(pin_verdict(Some(&bot), None, &other), PinVerdict::Reject);
        // The relay pin wins over an older own pin.
        assert_eq!(
            pin_verdict(Some(&bot), Some(&other), &other),
            PinVerdict::Reject
        );
    }
}
