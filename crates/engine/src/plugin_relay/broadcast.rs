//! Plugin data to the own channel through one relay broadcast per sender (FR-9.59 channel part,
//! FR-9.60 … FR-9.62).
//!
//! The sender asks the relay bot for its own broadcast with `bcast-req k=plg` at the first
//! channel send and admits each receiver the plugin service names (`bcast-admit`). The service
//! names only members that announced the plugin, so the sender's host is where receivers are
//! filtered. The bot then gives each receiver a token (`bcast-tok`) and it joins on its own.
//! The broadcast reuses the screen-share broadcast machinery of spec 07: the certificate-signed
//! key exchange (`keyx`) and the SFrame encryption (`crypto`), so the relay and the bot see only
//! the clear unit header (kind `data`, size, sequence), never the plugin id or payload. A
//! receiver that leaves the channel or the server makes the sender switch keys (FR-7.44); a
//! plugin-data unit can switch at once, as every unit stands alone.
//!
//! One broadcast carries the data of every plugin of the sender: the bot keeps one broadcast per
//! sender and kind. The receiving host delivers a message only to a plugin it runs with the
//! relay permission.
//!
//! The plaintext of a unit is a piece of the relay-data envelope ([`super::envelope`]): two
//! bytes (piece index, piece count) and the piece. The relay takes at most 64 KiB + 64 bytes
//! per unit, so a full 64 KiB message with its envelope needs two pieces. The relay forwards in
//! order and only drops under load, so a receiver keeps at most one message in pieces.
//!
//! Receivers count as joined from their admission on; the plugin service sends fallback-sized
//! messages to the members that did not join through Mumble plugin data (FR-9.61) and learns
//! the joined ones from [`EngineEvent::PluginRelayJoined`]. Messages sent while a receiver is
//! still binding are lost for it rather than delivered twice.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mumcord_p2p::broadcast::{
    BindRole, Content, ControlKind, ENCRYPTION_OVERHEAD, MAX_DATA_PAYLOAD, split_slot,
};
use mumcord_p2p::{BroadcastEnd, BroadcastId, BroadcastLink, RelayToken, Unit};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::envelope::{self, Envelope, MAX_ENVELOPE};
use super::pair::{Key, Order};
use super::rate::{DEFAULT_KBPS, Pacer};
use crate::ServerId;
use crate::peer::RelayUse;
use crate::screen_share::broadcast::cert_key;
use crate::screen_share::broadcast::crypto::{FrameOpener, FrameSealer, TAG_LEN};
use crate::screen_share::broadcast::keyx::{
    self, CertKey, PendingRequest, Reassembler, SharerChannel, ViewerChannel,
};
use crate::screen_share::broadcast::link::send_chunks;
use crate::session::{Engine, EngineEvent, SessionCtx, WeakEngine, lock};

/// After a broadcast could not start or was lost, channel sends use the fallback this long
/// before the next try.
const RETRY_AFTER: Duration = Duration::from_secs(30);
/// Receivers a broadcast asks room for when the relay announces no broadcast limit.
const DEFAULT_RECEIVERS: u16 = 16;
/// A receiver without the keys asks again after this long.
const KEY_RETRY: Duration = Duration::from_secs(3);
/// Bytes of the piece header (index, count) in a unit's plaintext.
const PIECE_HEADER: usize = 2;
/// Largest piece of an envelope in one unit: what the relay takes, minus the tag and header.
const MAX_PIECE: usize = MAX_DATA_PAYLOAD as usize - TAG_LEN - PIECE_HEADER;
// The encryption has to fit in the room the relay leaves for it.
const _: () = assert!(TAG_LEN + PIECE_HEADER <= ENCRYPTION_OVERHEAD as usize);

/// Splits an encoded envelope into unit plaintexts of at most `max_piece` bytes each.
fn pieces(envelope: &[u8], max_piece: usize) -> Vec<Vec<u8>> {
    let parts: Vec<&[u8]> = if envelope.is_empty() {
        vec![&[]]
    } else {
        envelope.chunks(max_piece).collect()
    };
    let count = parts.len() as u8;
    parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            let mut plain = Vec::with_capacity(PIECE_HEADER + part.len());
            plain.extend_from_slice(&[index as u8, count]);
            plain.extend_from_slice(part);
            plain
        })
        .collect()
}

/// Puts an envelope back together from the pieces of one sender, in arrival order.
#[derive(Debug, Default)]
struct Pieces {
    buf: Vec<u8>,
    next: u8,
    count: u8,
}

impl Pieces {
    /// Adds one unit plaintext; the whole envelope once its last piece came. A piece out of
    /// turn (one before it was dropped by the relay) drops the message.
    fn push(&mut self, plain: &[u8]) -> Option<Vec<u8>> {
        let (&[index, count], piece) = plain.split_first_chunk::<PIECE_HEADER>()?;
        if count == 0 || index >= count {
            *self = Self::default();
            return None;
        }
        if index == 0 {
            self.buf.clear();
            self.count = count;
        } else if index != self.next || count != self.count {
            *self = Self::default();
            return None;
        }
        if self.buf.len() + piece.len() > MAX_ENVELOPE {
            *self = Self::default();
            return None;
        }
        self.buf.extend_from_slice(piece);
        self.next = index + 1;
        if self.next == self.count {
            let whole = std::mem::take(&mut self.buf);
            *self = Self::default();
            Some(whole)
        } else {
            None
        }
    }
}

/// The receivers admitted to the own broadcast and their certificate hashes.
#[derive(Debug, Default)]
struct Members {
    hashes: BTreeMap<u32, String>,
}

impl Members {
    /// Admits the `receivers` not admitted yet that have a certificate (`hash_of`); returns
    /// them for `bcast-admit`.
    fn admit(
        &mut self,
        receivers: impl IntoIterator<Item = u32>,
        hash_of: impl Fn(u32) -> Option<String>,
    ) -> Vec<(u32, String)> {
        let mut new = Vec::new();
        for session in receivers {
            if self.hashes.contains_key(&session) {
                continue;
            }
            let Some(hash) = hash_of(session).map(|h| h.to_ascii_lowercase()) else {
                continue;
            };
            self.hashes.insert(session, hash.clone());
            new.push((session, hash));
        }
        new
    }

    /// `session` left or was refused: its hash, for the rekey.
    fn remove(&mut self, session: u32) -> Option<String> {
        self.hashes.remove(&session)
    }

    fn joined(&self) -> Vec<u32> {
        self.hashes.keys().copied().collect()
    }
}

/// The own plugin-data broadcast on one server.
enum Own {
    /// Asking the bot and binding; the receivers to admit once it runs.
    Starting { wanted: Vec<u32> },
    Live {
        publisher: Publisher,
        members: Members,
    },
    /// Not available (refused, lost, no certificate …) until `until`.
    Off { until: Instant },
}

/// Plugin-data broadcasts of this client: its own per server and the ones it receives.
#[derive(Default)]
pub(super) struct Broadcasts {
    own: HashMap<ServerId, Own>,
    /// The broadcasts this client receives, by sender.
    receiving: HashMap<Key, Subscriber>,
    /// The next envelope sequence per server and plugin: one sender, every receiver.
    seqs: HashMap<(ServerId, String), u64>,
    /// What was delivered already, per sender and plugin.
    order: Order,
}

impl std::fmt::Debug for Broadcasts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Broadcasts")
            .field("own", &self.own.len())
            .field("receiving", &self.receiving.len())
            .finish_non_exhaustive()
    }
}

impl Broadcasts {
    /// Ends everything on `server_id` (or everywhere); the servers whose joined list emptied.
    fn end(&mut self, server_id: Option<ServerId>) -> Vec<ServerId> {
        let matches = |id: &ServerId| server_id.is_none_or(|s| s == *id);
        let emptied = self
            .own
            .iter()
            .filter(|(id, own)| {
                matches(id)
                    && matches!(own, Own::Live { members, .. } if !members.hashes.is_empty())
            })
            .map(|(id, _)| *id)
            .collect();
        self.own.retain(|id, _| !matches(id));
        // Sequences and delivery order stay: senders keep counting, so nothing is delivered
        // twice after a rejoin.
        self.receiving.retain(|(id, _), _| !matches(id));
        emptied
    }
}

fn cert_hash(ctx: &SessionCtx, session: u32) -> Option<String> {
    ctx.handle
        .with_state(|s| s.users.get(&session).and_then(|u| u.cert_hash.clone()))
}

/// Whether `session` is in the user's channel.
fn in_own_channel(ctx: &SessionCtx, session: u32) -> bool {
    ctx.handle.with_state(|s| {
        let own = s.session.and_then(|own| s.users.get(&own));
        let other = s.users.get(&session);
        matches!((own, other), (Some(own), Some(other)) if own.session != session && own.channel_id == other.channel_id)
    })
}

impl Engine {
    /// Sends one plugin message to the own channel through the own plugin-data broadcast,
    /// started at the first send. `receivers` are the members the plugin service picked (they
    /// announced the plugin and pass the reach rules); the ones not admitted yet are admitted
    /// now. The caller checked permission, size and the plugin's rate, and sent the fallback to
    /// the members that had not joined (FR-9.61).
    pub fn plugin_relay_broadcast(
        &self,
        server_id: ServerId,
        plugin: &str,
        api: (u8, u8),
        data: Vec<u8>,
        receivers: &[u32],
    ) {
        let relay = self.direct().plugin_relay();
        if !relay.enabled() {
            return;
        }
        let Ok(ctx) = self.session_ctx_for(server_id) else {
            return;
        };
        let mut state = lock(&relay.broadcasts);
        let seq = state
            .seqs
            .entry((server_id, plugin.to_owned()))
            .or_insert(0);
        *seq += 1;
        let bytes = match envelope::encode(&Envelope {
            api_major: api.0,
            api_minor: api.1,
            seq: *seq,
            plugin: plugin.to_owned(),
            data,
        }) {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::debug!(error = %e, "plugin broadcast message not sent");
                return;
            }
        };
        let now = Instant::now();
        let own = state
            .own
            .entry(server_id)
            .or_insert(Own::Off { until: now });
        let admitted = match own {
            Own::Off { until } if *until <= now => {
                // Nobody has joined yet, so the message went to everyone as fallback.
                *own = Own::Starting {
                    wanted: receivers.to_vec(),
                };
                drop(state);
                self.start_plugin_broadcast(server_id);
                return;
            }
            Own::Off { .. } => return,
            Own::Starting { wanted } => {
                for receiver in receivers {
                    if !wanted.contains(receiver) {
                        wanted.push(*receiver);
                    }
                }
                return;
            }
            Own::Live { publisher, members } => {
                let new: Vec<u32> = members
                    .admit(receivers.iter().copied(), |s| cert_hash(&ctx, s))
                    .into_iter()
                    .map(|(session, hash)| {
                        publisher.admit(&hash);
                        session
                    })
                    .collect();
                publisher.send(bytes);
                (!new.is_empty()).then(|| (publisher.id, new, members.joined()))
            }
        };
        drop(state);
        if let Some((id, new, joined)) = admitted {
            self.admit_plugin_receivers(server_id, id, new);
            ctx.events.emit(EngineEvent::PluginRelayJoined { joined });
        }
    }

    /// Sends `bcast-admit` for each newly admitted receiver; a refusal takes the receiver out
    /// again, so it gets the fallback.
    fn admit_plugin_receivers(&self, server_id: ServerId, id: BroadcastId, new: Vec<u32>) {
        for receiver in new {
            let engine = self.clone();
            self.runtime().spawn(async move {
                let Err(reason) = engine
                    .relay_broadcast_admit(server_id, RelayUse::PluginData, id, receiver)
                    .await
                else {
                    return;
                };
                tracing::info!(?reason, "receiver not admitted to the plugin broadcast");
                engine.plugin_receiver_gone(server_id, receiver, false);
            });
        }
    }

    fn start_plugin_broadcast(&self, server_id: ServerId) {
        let engine = self.clone();
        self.runtime().spawn(async move {
            let opened = engine.open_plugin_broadcast(server_id).await;
            engine.plugin_broadcast_opened(server_id, opened);
        });
    }

    async fn open_plugin_broadcast(
        &self,
        server_id: ServerId,
    ) -> Option<(BroadcastId, CertKey, BroadcastLink)> {
        if let Err(reason) = self
            .relays()
            .availability_for(server_id, RelayUse::PluginData)
        {
            tracing::info!(?reason, "no relay for a plugin broadcast");
            return None;
        }
        if !self.relay_mode_allows_broadcast(server_id) {
            return None;
        }
        let (p2p, identity) = self.p2p_with_identity().await?;
        let me = cert_key(&identity)?;
        let max = self
            .relays()
            .plugin_broadcast_announcement(server_id)
            .ok()?
            .broadcast
            .filter(|max| *max > 0)
            .unwrap_or(DEFAULT_RECEIVERS);
        let (ann, id, token) = match self
            .relay_broadcast_request(server_id, RelayUse::PluginData, max)
            .await
        {
            Ok(answer) => answer,
            Err(reason) => {
                tracing::info!(?reason, "plugin broadcast not started");
                return None;
            }
        };
        match p2p.broadcast(&ann, &token, BindRole::Publisher).await {
            Ok(link) => Some((id, me, link)),
            Err(e) => {
                tracing::info!(error = %e, "plugin broadcast link not opened");
                None
            }
        }
    }

    fn plugin_broadcast_opened(
        &self,
        server_id: ServerId,
        opened: Option<(BroadcastId, CertKey, BroadcastLink)>,
    ) {
        let Ok(ctx) = self.session_ctx_for(server_id) else {
            return;
        };
        let relay = self.direct().plugin_relay();
        let mut state = lock(&relay.broadcasts);
        let Some(own) = state.own.get_mut(&server_id) else {
            // Switched off or the user moved meanwhile; dropping the link ends the broadcast.
            return;
        };
        let Own::Starting { wanted } = own else {
            return;
        };
        let Some((id, me, link)) = opened else {
            *own = Own::Off {
                until: Instant::now() + RETRY_AFTER,
            };
            return;
        };
        let wanted = std::mem::take(wanted);
        let weak = self.downgrade();
        let on_end: OnEnd = Box::new(move |end| {
            if let Some(engine) = weak.upgrade() {
                engine.plugin_broadcast_lost(server_id, id, end);
            }
        });
        let kbps = self
            .relays()
            .plugin_data_kbps(server_id)
            .unwrap_or(DEFAULT_KBPS);
        let publisher = match Publisher::start(self.runtime(), id, me, link, kbps, on_end) {
            Ok(publisher) => publisher,
            Err(e) => {
                tracing::warn!(error = %e, "plugin broadcast not started");
                *own = Own::Off {
                    until: Instant::now() + RETRY_AFTER,
                };
                return;
            }
        };
        let mut members = Members::default();
        // Who is still here of the receivers asked for meanwhile.
        let wanted = wanted.into_iter().filter(|s| in_own_channel(&ctx, *s));
        for (_, hash) in members.admit(wanted, |s| cert_hash(&ctx, s)) {
            publisher.admit(&hash);
        }
        let joined = members.joined();
        tracing::info!(
            receivers = joined.len(),
            "plugin data sent once via the relay"
        );
        *own = Own::Live { publisher, members };
        drop(state);
        if !joined.is_empty() {
            self.admit_plugin_receivers(server_id, id, joined.clone());
            ctx.events.emit(EngineEvent::PluginRelayJoined { joined });
        }
    }

    /// The own broadcast `id` ended by itself: channel sends fall back for a while.
    fn plugin_broadcast_lost(&self, server_id: ServerId, id: BroadcastId, end: BroadcastEnd) {
        let relay = self.direct().plugin_relay();
        let mut state = lock(&relay.broadcasts);
        let Some(own) = state.own.get_mut(&server_id) else {
            return;
        };
        if !matches!(own, Own::Live { publisher, .. } if publisher.id == id) {
            return;
        }
        tracing::info!(?end, "plugin broadcast ended");
        *own = Own::Off {
            until: Instant::now() + RETRY_AFTER,
        };
        drop(state);
        if let Ok(ctx) = self.session_ctx_for(server_id) {
            ctx.events
                .emit(EngineEvent::PluginRelayJoined { joined: Vec::new() });
        }
    }

    /// `session` left the own broadcast (left the channel or server, or the bot refused it):
    /// it gets no keys any more and the broadcast switches keys. `kick` tells the bot.
    fn plugin_receiver_gone(&self, server_id: ServerId, session: u32, kick: bool) {
        let relay = self.direct().plugin_relay();
        let mut state = lock(&relay.broadcasts);
        let Some(Own::Live { publisher, members }) = state.own.get_mut(&server_id) else {
            return;
        };
        let Some(hash) = members.remove(session) else {
            return;
        };
        publisher.remove(&hash);
        let id = publisher.id;
        let joined = members.joined();
        drop(state);
        if kick {
            self.relay_broadcast_kick(server_id, RelayUse::PluginData, id, session);
        }
        if let Ok(ctx) = self.session_ctx_for(server_id) {
            ctx.events.emit(EngineEvent::PluginRelayJoined { joined });
        }
    }

    /// Someone left the server or the user's channel: their place in the own broadcast and the
    /// broadcast received from them go.
    pub(crate) fn plugin_relay_left(&self, server_id: ServerId, session: u32) {
        self.plugin_receiver_gone(server_id, session, true);
        let key = (server_id, session);
        let mut state = lock(&self.direct().plugin_relay().broadcasts);
        state.receiving.remove(&key);
        state.order.forget(key);
    }

    /// `session` moved: out of the user's channel it is gone for plugin broadcasts.
    pub(crate) fn plugin_relay_moved(&self, ctx: &SessionCtx, session: u32) {
        if !in_own_channel(ctx, session) {
            self.plugin_relay_left(ctx.server_id, session);
        }
    }

    /// The user changed channel or the connection ended: the own broadcast and all received
    /// ones on `server_id` end; the next channel send starts anew.
    pub(crate) fn plugin_relay_reset(&self, server_id: ServerId) {
        self.end_plugin_broadcasts(Some(server_id));
    }

    /// Ends own and received broadcasts on `server_id`, or everywhere (FR-9.63).
    pub(super) fn end_plugin_broadcasts(&self, server_id: Option<ServerId>) {
        let relay = self.direct().plugin_relay();
        let emptied = lock(&relay.broadcasts).end(server_id);
        for server in emptied {
            if let Ok(ctx) = self.session_ctx_for(server) {
                ctx.events
                    .emit(EngineEvent::PluginRelayJoined { joined: Vec::new() });
            }
        }
    }

    /// The bot's `bcast-tok` that the screen share did not take: `sharer` admitted this client
    /// to its plugin-data broadcast `id`. Joined when relay plugin data is on and allowed here
    /// and the sender is in the user's channel.
    pub(crate) fn plugin_relay_broadcast_token(
        &self,
        ctx: &SessionCtx,
        sharer: u32,
        id: BroadcastId,
        token: RelayToken,
    ) {
        if !self.direct().plugin_relay().enabled() || !in_own_channel(ctx, sharer) {
            return;
        }
        let Some(hash) = cert_hash(ctx, sharer) else {
            return;
        };
        let server_id = ctx.server_id;
        let engine = self.clone();
        self.runtime().spawn(async move {
            let Some((me, link)) = engine.join_plugin_broadcast(server_id, &token).await else {
                return;
            };
            let key = (server_id, sharer);
            let weak = engine.downgrade();
            let on_end: OnEnd = Box::new(move |end| {
                tracing::info!(?end, "received plugin broadcast ended");
                if let Some(engine) = weak.upgrade() {
                    let mut state = lock(&engine.direct().plugin_relay().broadcasts);
                    if state.receiving.get(&key).is_some_and(|s| s.id == id) {
                        state.receiving.remove(&key);
                    }
                }
            });
            let relay = engine.direct().plugin_relay();
            // Switched off or the sender left while joining.
            if !relay.enabled() {
                return;
            }
            let Ok(ctx) = engine.session_ctx_for(server_id) else {
                return;
            };
            if !in_own_channel(&ctx, sharer) {
                return;
            }
            let subscriber = Subscriber::start(&engine, key, id, me, hash, link, on_end);
            lock(&relay.broadcasts).receiving.insert(key, subscriber);
        });
    }

    async fn join_plugin_broadcast(
        &self,
        server_id: ServerId,
        token: &RelayToken,
    ) -> Option<(CertKey, BroadcastLink)> {
        self.relays()
            .availability_for(server_id, RelayUse::PluginData)
            .ok()?;
        if !self.relay_mode_allows_broadcast(server_id) {
            return None;
        }
        let ann = self
            .relays()
            .plugin_broadcast_announcement(server_id)
            .ok()?;
        let (p2p, identity) = self.p2p_with_identity().await?;
        let me = cert_key(&identity)?;
        match p2p.broadcast(&ann, token, BindRole::Subscriber).await {
            Ok(link) => Some((me, link)),
            Err(e) => {
                tracing::info!(error = %e, "plugin broadcast not joined");
                None
            }
        }
    }

    /// One decrypted envelope from `key`'s broadcast: delivered once and in order per sender
    /// and plugin (FR-9.62).
    fn plugin_broadcast_received(&self, key: Key, bytes: &[u8]) {
        let relay = self.direct().plugin_relay();
        if !relay.enabled() {
            return;
        }
        let Ok(Envelope {
            api_major,
            api_minor,
            seq,
            plugin,
            data,
        }) = envelope::decode(bytes)
        else {
            tracing::debug!("dropping an invalid plugin broadcast envelope");
            return;
        };
        if !lock(&relay.broadcasts).order.admit(key, &plugin, seq) {
            return;
        }
        if let Ok(ctx) = self.session_ctx_for(key.0) {
            ctx.events.emit(EngineEvent::PluginRelayData {
                session: key.1,
                plugin,
                api: (api_major, api_minor),
                data: super::Opaque(data),
            });
        }
    }
}

/// Called once when a link ends on its own (not when it is dropped).
type OnEnd = Box<dyn FnOnce(BroadcastEnd) + Send>;

// ---------------------------------------------------------------------------------------------
// Publisher
// ---------------------------------------------------------------------------------------------

enum PubCommand {
    /// An encoded envelope for every receiver.
    Data(Vec<u8>),
    /// A receiver left: drop its key channel and switch keys.
    Removed(String),
}

/// The sender's end; dropping it closes the link, which ends the broadcast.
struct Publisher {
    id: BroadcastId,
    /// Certificate hashes of the admitted receivers; only they get keys.
    admitted: Arc<Mutex<HashSet<String>>>,
    commands: mpsc::UnboundedSender<PubCommand>,
    task: JoinHandle<()>,
}

impl Publisher {
    fn start(
        rt: &tokio::runtime::Handle,
        id: BroadcastId,
        me: CertKey,
        link: BroadcastLink,
        kbps: u32,
        on_end: OnEnd,
    ) -> Result<Self, crate::screen_share::broadcast::crypto::CryptoError> {
        let admitted = Arc::new(Mutex::new(HashSet::new()));
        let (commands, command_rx) = mpsc::unbounded_channel();
        let task = rt.spawn(
            PublisherTask {
                id,
                me,
                sealer: FrameSealer::new()?,
                admitted: admitted.clone(),
                channels: HashMap::new(),
                reassembler: Reassembler::new(),
                set_id: 0,
                pacer: Pacer::new(kbps, Instant::now()),
            }
            .run(link, command_rx, on_end),
        );
        Ok(Self {
            id,
            admitted,
            commands,
            task,
        })
    }

    fn admit(&self, hash: &str) {
        lock(&self.admitted).insert(hash.to_owned());
    }

    fn remove(&self, hash: &str) {
        lock(&self.admitted).remove(hash);
        // The task is gone only with the link; then nothing needs a new key.
        let _ = self.commands.send(PubCommand::Removed(hash.to_owned()));
    }

    fn send(&self, envelope: Vec<u8>) {
        if self.commands.send(PubCommand::Data(envelope)).is_err() {
            tracing::debug!("plugin broadcast gone");
        }
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct PublisherTask {
    id: BroadcastId,
    me: CertKey,
    sealer: FrameSealer,
    admitted: Arc<Mutex<HashSet<String>>>,
    channels: HashMap<u16, ViewerChannel>,
    reassembler: Reassembler<u16>,
    set_id: u8,
    pacer: Pacer,
}

impl PublisherTask {
    async fn run(
        mut self,
        mut link: BroadcastLink,
        mut commands: mpsc::UnboundedReceiver<PubCommand>,
        on_end: OnEnd,
    ) {
        let units = link.sender();
        let end = loop {
            let alive = tokio::select! {
                unit = link.recv() => match unit {
                    Ok(unit) => self.on_unit(&unit, &units).await,
                    Err(end) => break end,
                },
                command = commands.recv() => match command {
                    Some(PubCommand::Data(envelope)) => self.send(&envelope, &units).await,
                    Some(PubCommand::Removed(hash)) => {
                        self.channels.retain(|_, ch| ch.cert_hash() != hash);
                        self.rekey(&units).await
                    }
                    None => break BroadcastEnd::Closed,
                },
            };
            if !alive {
                break BroadcastEnd::Lost;
            }
        };
        on_end(end);
    }

    /// Seals the envelope in pieces and sends them at the relay's per-sender rate.
    async fn send(&mut self, envelope: &[u8], units: &mpsc::Sender<Vec<u8>>) -> bool {
        for plain in pieces(envelope, MAX_PIECE) {
            let mut unit = Vec::new();
            if let Err(e) = self.sealer.seal(Content::Data, &plain, &mut unit) {
                tracing::warn!(error = %e, "plugin broadcast unit not sealed");
                return true;
            }
            let now = Instant::now();
            let wait = self.pacer.delay(unit.len(), now);
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
            if units.send(unit).await.is_err() {
                return false;
            }
        }
        true
    }

    /// Handles one unit from the relay; false once the link is gone.
    async fn on_unit(&mut self, unit: &Unit, units: &mpsc::Sender<Vec<u8>>) -> bool {
        match unit.header.content {
            Content::Control(ControlKind::Gone) => {
                if let Some((slot, _)) = split_slot(unit.payload()) {
                    self.reassembler.forget(&slot);
                    if self.channels.remove(&slot).is_some() {
                        return self.rekey(units).await;
                    }
                }
                true
            }
            Content::Control(ControlKind::KeyRequest) => {
                let Some((slot, chunk)) = split_slot(unit.payload()) else {
                    return true;
                };
                match self.reassembler.push(slot, chunk, Instant::now()) {
                    Ok(Some(request)) => self.answer(slot, &request, units).await,
                    Ok(None) => true,
                    Err(e) => {
                        tracing::debug!(error = %e, "plugin broadcast key request chunk dropped");
                        true
                    }
                }
            }
            // Keyframes, layers and media are not for a plugin-data publisher.
            _ => true,
        }
    }

    async fn answer(&mut self, slot: u16, request: &[u8], units: &mpsc::Sender<Vec<u8>>) -> bool {
        let keys = self.sealer.keys();
        let admitted = self.admitted.clone();
        match keyx::answer_request(
            self.id,
            &self.me,
            request,
            |hash| lock(&admitted).contains(hash),
            &keys,
        ) {
            Ok((channel, answer)) => {
                self.channels.insert(slot, channel);
                self.set_id = self.set_id.wrapping_add(1);
                send_chunks(units, ControlKind::Key, Some(slot), &answer, self.set_id).await
            }
            Err(e) => {
                tracing::info!(error = %e, "plugin broadcast key request refused");
                true
            }
        }
    }

    /// Stages a new key and hands it to every remaining receiver; the next unit uses it. The
    /// relay forwards control units before data, so the key arrives first.
    async fn rekey(&mut self, units: &mpsc::Sender<Vec<u8>>) -> bool {
        if let Err(e) = self.sealer.rekey() {
            tracing::warn!(error = %e, "plugin broadcast not rekeyed");
            return true;
        }
        let keys = self.sealer.keys();
        tracing::info!(receivers = self.channels.len(), "plugin broadcast rekeyed");
        let mut updates = Vec::new();
        for (slot, channel) in &mut self.channels {
            match channel.update(&keys) {
                Ok(update) => updates.push((*slot, update)),
                Err(e) => tracing::warn!(error = %e, "plugin broadcast key update not sealed"),
            }
        }
        for (slot, update) in updates {
            self.set_id = self.set_id.wrapping_add(1);
            if !send_chunks(units, ControlKind::Key, Some(slot), &update, self.set_id).await {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------------------------
// Subscriber
// ---------------------------------------------------------------------------------------------

/// A receiver's end of a sender's broadcast; dropping it leaves the broadcast.
pub(super) struct Subscriber {
    id: BroadcastId,
    task: JoinHandle<()>,
}

impl Subscriber {
    fn start(
        engine: &Engine,
        key: Key,
        id: BroadcastId,
        me: CertKey,
        sharer_hash: String,
        link: BroadcastLink,
        on_end: OnEnd,
    ) -> Self {
        let task = engine.runtime().spawn(
            SubscriberTask {
                engine: engine.downgrade(),
                key,
                id,
                me,
                sharer_hash,
                pending: None,
                channel: None,
                opener: FrameOpener::new(),
                reassembler: Reassembler::new(),
                set_id: 0,
                pieces: Pieces::default(),
            }
            .run(link, on_end),
        );
        Self { id, task }
    }
}

impl Drop for Subscriber {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct SubscriberTask {
    engine: WeakEngine,
    key: Key,
    id: BroadcastId,
    me: CertKey,
    sharer_hash: String,
    pending: Option<PendingRequest>,
    channel: Option<SharerChannel>,
    opener: FrameOpener,
    reassembler: Reassembler<()>,
    set_id: u8,
    pieces: Pieces,
}

impl SubscriberTask {
    async fn run(mut self, mut link: BroadcastLink, on_end: OnEnd) {
        let units = link.sender();
        if !self.request_keys(&units).await {
            on_end(BroadcastEnd::Lost);
            return;
        }
        let mut retry =
            tokio::time::interval_at(tokio::time::Instant::now() + KEY_RETRY, KEY_RETRY);
        let end = loop {
            tokio::select! {
                unit = link.recv() => match unit {
                    Ok(mut unit) => self.on_unit(&mut unit),
                    Err(end) => break end,
                },
                _ = retry.tick(), if self.channel.is_none() => {
                    if !self.request_keys(&units).await {
                        break BroadcastEnd::Lost;
                    }
                }
            }
        };
        on_end(end);
    }

    async fn request_keys(&mut self, units: &mpsc::Sender<Vec<u8>>) -> bool {
        let (pending, request) = match keyx::request(self.id, &self.me) {
            Ok(request) => request,
            Err(e) => {
                tracing::warn!(error = %e, "plugin broadcast key request not made");
                return true;
            }
        };
        self.pending = Some(pending);
        self.set_id = self.set_id.wrapping_add(1);
        send_chunks(units, ControlKind::KeyRequest, None, &request, self.set_id).await
    }

    fn on_unit(&mut self, unit: &mut Unit) {
        match unit.header.content {
            Content::Control(ControlKind::Key) => {
                match self.reassembler.push((), unit.payload(), Instant::now()) {
                    Ok(Some(message)) => self.on_key_message(&message),
                    Ok(None) => {}
                    Err(e) => tracing::debug!(error = %e, "plugin broadcast key chunk dropped"),
                }
            }
            Content::Data if self.channel.is_some() => {
                let whole = match self.opener.open(&mut unit.bytes) {
                    Ok((_, plain)) => self.pieces.push(plain),
                    Err(e) => {
                        tracing::debug!(error = %e, "plugin broadcast unit not decrypted");
                        self.pieces = Pieces::default();
                        None
                    }
                };
                if let (Some(whole), Some(engine)) = (whole, self.engine.upgrade()) {
                    engine.plugin_broadcast_received(self.key, &whole);
                }
            }
            _ => {}
        }
    }

    fn on_key_message(&mut self, message: &[u8]) {
        let keys = if let Some(pending) = self.pending.take() {
            match pending.accept(message, &self.sharer_hash) {
                Ok((channel, keys)) => {
                    self.channel = Some(channel);
                    keys
                }
                Err(e) => {
                    // Asked again on the next retry tick.
                    tracing::warn!(error = %e, "plugin broadcast key answer refused");
                    return;
                }
            }
        } else if let Some(channel) = self.channel.as_mut() {
            match channel.open_update(message) {
                Ok(keys) => keys,
                Err(e) => {
                    tracing::warn!(error = %e, "plugin broadcast key update refused");
                    return;
                }
            }
        } else {
            return;
        };
        for key in &keys {
            self.opener.add_key(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope_of(len: usize) -> Vec<u8> {
        envelope::encode(&Envelope {
            api_major: 1,
            api_minor: 1,
            seq: 3,
            plugin: "x".repeat(255),
            data: (0..len).map(|i| i as u8).collect(),
        })
        .expect("valid")
    }

    #[test]
    fn a_full_message_fits_the_relay_in_two_sealed_units() {
        let whole = envelope_of(envelope::MAX_DATA);
        let parts = pieces(&whole, MAX_PIECE);
        assert_eq!(parts.len(), 2);
        let mut sealer = FrameSealer::new().expect("sealer");
        let mut opener = FrameOpener::new();
        for key in sealer.keys() {
            opener.add_key(&key);
        }
        let mut received = Pieces::default();
        let mut out = None;
        for plain in parts {
            let mut unit = Vec::new();
            let header = sealer
                .seal(Content::Data, &plain, &mut unit)
                .expect("sealed");
            // The clear header passes the relay's size check.
            assert!(header.payload_len <= MAX_DATA_PAYLOAD);
            let (_, opened) = opener.open(&mut unit).expect("opened");
            out = received.push(opened);
        }
        assert_eq!(out, Some(whole));
    }

    #[test]
    fn a_small_or_empty_envelope_is_one_piece() {
        let whole = envelope_of(10);
        let mut received = Pieces::default();
        let parts = pieces(&whole, MAX_PIECE);
        assert_eq!(parts.len(), 1);
        assert_eq!(received.push(&parts[0]), Some(whole));
        assert_eq!(pieces(&[], MAX_PIECE), vec![vec![0, 1]]);
    }

    #[test]
    fn a_dropped_piece_drops_only_its_message() {
        let first = envelope_of(100);
        let second = envelope_of(50);
        let a = pieces(&first, 40);
        let b = pieces(&second, 40);
        let mut received = Pieces::default();
        // The relay dropped `a[1]` under load.
        assert_eq!(received.push(&a[0]), None);
        assert_eq!(received.push(&a[2]), None);
        for piece in &a[3..] {
            assert_eq!(received.push(piece), None);
        }
        let mut out = None;
        for piece in &b {
            out = received.push(piece);
        }
        assert_eq!(out, Some(second));
        // Nonsense headers are refused.
        assert_eq!(received.push(&[0, 0, 1]), None);
        assert_eq!(received.push(&[3, 2, 1]), None);
        assert_eq!(received.push(&[0]), None);
    }

    #[test]
    fn only_named_receivers_with_a_certificate_are_admitted_once() {
        let mut members = Members::default();
        let hash_of = |s: u32| (s != 9).then(|| format!("AB{s}"));
        assert_eq!(
            members.admit([3, 4, 9], hash_of),
            vec![(3, "ab3".to_owned()), (4, "ab4".to_owned())]
        );
        // Already admitted ones are not asked again.
        assert_eq!(members.admit([3, 5], hash_of), vec![(5, "ab5".to_owned())]);
        assert_eq!(members.joined(), vec![3, 4, 5]);
        // Leaving returns the hash the rekey shuts out.
        assert_eq!(members.remove(4), Some("ab4".to_owned()));
        assert_eq!(members.remove(4), None);
        assert_eq!(members.joined(), vec![3, 5]);
    }

    #[test]
    fn receivers_deliver_each_sequence_once_and_in_order_per_sender() {
        let mut order = Order::default();
        let alice: Key = (1, 5);
        assert!(order.admit(alice, "dice", 1));
        assert!(order.admit(alice, "dice", 3));
        assert!(!order.admit(alice, "dice", 2));
        assert!(!order.admit(alice, "dice", 3));
    }
}
