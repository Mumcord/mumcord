//! The history sync state machine of one connection, both sides (FR-1.60, QA.48, QA.49).
//!
//! **Requesting** (one channel at a time, queued): ask everyone who holds the channel's
//! messages in `[now − 7 days, cutoff)` (`areq`; starting later after the user cleared the
//! channel or within the automatic deletion window, see `sync_floor`), collect summaries for
//! [`DISCOVER_MS`], ask the [`MAX_HOLDERS`] largest holders for their hash lists (`hreq`),
//! subtract what is already stored (±1 s, see [`super::hash`]), assign every missing hash to
//! exactly one holder (spread evenly) and fetch it (`get`). Received messages are checked
//! against the requested hashes, sanitized, deduplicated again (by hash, and by sender and
//! text within [`SKEW_MS`] for clients with skewed clocks) and inserted. When a holder drops
//! out mid-transfer, the messages it still owed are fetched from another holder that has
//! them (after asking it for a fresh hash list).
//!
//! **Answering** (only while the "share history" setting is on): channel messages only,
//! never private chats, and only up to the moment the requester started receiving that
//! channel live as seen here (join or follow); requests reaching later are clamped.
//!
//! Abuse limits: a per-person request budget, at most [`MAX_SERVES`] open answers, size
//! caps on everything, and data that does not answer an own open request is ignored.
//! Outgoing packets are paced below the server's plugin message rate limit and sent round
//! robin per receiver, so one long answer does not block the others.
//!
//! Pure logic: time comes in as Unix milliseconds and packets go out through
//! [`Core::take_outgoing`], so all of it runs in unit tests without a server.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::MutexGuard;

use mumcord_store::{ChatMessage, Conversation, NewChatMessage, ServerId, Store, UnixMillis};

use super::hash::{Content, content_hash, text_key, variants};
use super::wire::{self, Hash, MAX_MESSAGES, Packet, WireMessage};
use crate::chat::{ChatMessageView, sanitize_message_html, transient_id};
use crate::maintenance::history_cutoff;
use crate::session::{ConversationRef, EngineEvent};

/// How far back a sync reaches (FR-1.60).
pub const MAX_AGE_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// Messages with the same channel, sender and text stamped at most this far apart are the
/// same message: clients stamp messages with their own clocks, which may be off by seconds.
pub const SKEW_MS: i64 = 5000;
/// How long summaries are collected after asking.
pub const DISCOVER_MS: i64 = 1500;
/// A holder that stops sending its hash list for this long is given up.
const LIST_TIMEOUT_MS: i64 = 15_000;
/// A holder that stops sending messages for this long is given up.
const FETCH_IDLE_MS: i64 = 30_000;
/// An answer whose hash list was not followed by `get` within this time is dropped.
const SERVE_TTL_MS: i64 = 60_000;
/// Most holders asked for one channel.
pub const MAX_HOLDERS: usize = 3;
/// Most answers prepared at the same time.
pub const MAX_SERVES: usize = 3;
/// Messages with a larger stored body (e.g. inline images) are not synced.
pub const MAX_BODY: usize = 8 * 1024;
/// Largest message stream of one answer.
pub const MAX_STREAM: usize = 256 * 1024;
/// Longest sender name kept (characters).
const MAX_NAME: usize = 128;
/// Own messages loaded to compare hashes against.
const MAX_KNOWN: u32 = 5000;
/// Requests per person: refill rate per second and burst.
const PEER_RATE: f64 = 0.2;
const PEER_BURST: f64 = 6.0;
/// Outgoing packets per second and burst, below murmur's default plugin message limit
/// (4 per second, burst 15).
const SEND_RATE: f64 = 3.0;
const SEND_BURST: f64 = 10.0;

/// What the state machine needs from the connection.
pub(crate) trait Env {
    fn server_id(&self) -> ServerId;
    fn store(&self) -> MutexGuard<'_, Store>;
    /// The "share history" setting. Never called while [`Env::store`] is held.
    fn sharing(&self) -> bool;
    fn own_cert(&self) -> Option<String>;
    /// Store key (channel path) of a channel.
    fn channel_key(&self, channel: u32) -> Option<String>;
    /// The "delete history after N days" setting. Never called while [`Env::store`] is held.
    fn history_max_age_days(&self) -> Option<u32>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestError {
    /// The user neither is in nor follows the channel.
    NotLive,
}

/// A token bucket over Unix milliseconds.
struct Bucket {
    tokens: f64,
    at: UnixMillis,
    rate: f64,
    burst: f64,
}

impl Bucket {
    fn new(rate: f64, burst: f64, now: UnixMillis) -> Self {
        Self {
            tokens: burst,
            at: now,
            rate,
            burst,
        }
    }

    fn take(&mut self, now: UnixMillis) -> bool {
        let elapsed = now.saturating_sub(self.at).max(0) as f64 / 1000.0;
        self.tokens = (self.tokens + elapsed * self.rate).min(self.burst);
        self.at = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Outgoing packets, one queue per receiver list, served round robin.
#[derive(Default)]
struct Outbox {
    queues: VecDeque<(Vec<u32>, VecDeque<Vec<u8>>)>,
}

impl Outbox {
    fn push(&mut self, to: Vec<u32>, packet: &Packet) {
        let data = wire::encode(packet);
        match self.queues.iter_mut().find(|(t, _)| *t == to) {
            Some((_, queue)) => queue.push_back(data),
            None => self.queues.push_back((to, VecDeque::from([data]))),
        }
    }

    fn pop(&mut self) -> Option<(Vec<u32>, Vec<u8>)> {
        let (to, mut queue) = self.queues.pop_front()?;
        let data = queue.pop_front()?;
        if !queue.is_empty() {
            self.queues.push_back((to.clone(), queue));
        }
        Some((to, data))
    }

    fn forget(&mut self, session: u32) {
        self.queues.retain(|(to, _)| to != &[session]);
    }
}

struct Peer {
    name: String,
    /// Channels this person receives live (in it or following it) and since when.
    live: HashMap<u32, UnixMillis>,
    budget: Bucket,
}

struct Pending {
    channel: u32,
    to: UnixMillis,
}

struct Job {
    channel: u32,
    key: String,
    from: UnixMillis,
    to: UnixMillis,
    added: u32,
    stage: Stage,
}

enum Stage {
    Discover {
        req: u32,
        until: UnixMillis,
        offers: Vec<(u32, u32)>,
    },
    Transfer {
        holders: Vec<Holder>,
        planned: bool,
        /// The messages already stored or received.
        known: Known,
        /// Hashes owed by holders that dropped out, waiting for another holder.
        orphans: HashSet<Hash>,
    },
}

/// What is already here, for deduplication.
#[derive(Default)]
struct Known {
    /// Hashes (all ±1 s variants).
    hashes: HashSet<Hash>,
    /// Send times by [`text_key`], to match copies stamped up to [`SKEW_MS`] apart.
    texts: HashMap<Hash, Vec<UnixMillis>>,
}

impl Known {
    fn add(&mut self, content: &Content<'_>) {
        self.hashes.extend(variants(content));
        self.add_text(text_key(content), content.sent_at);
    }

    fn add_text(&mut self, key: Hash, sent_at: UnixMillis) {
        self.texts.entry(key).or_default().push(sent_at);
    }

    fn contains(&self, hash: &Hash, content: &Content<'_>) -> bool {
        self.hashes.contains(hash)
            || self.texts.get(&text_key(content)).is_some_and(|times| {
                times
                    .iter()
                    .any(|t| t.abs_diff(content.sent_at) <= SKEW_MS.unsigned_abs())
            })
    }
}

struct Holder {
    session: u32,
    req: u32,
    deadline: UnixMillis,
    state: HolderState,
    /// Its latest complete hash list, to find a replacement for a holder that dropped out.
    list: Vec<Hash>,
}

impl Holder {
    /// Gives up on this holder; what it still owed goes to another one.
    fn drop_out(&mut self) {
        let orphans = match std::mem::replace(
            &mut self.state,
            HolderState::Dropped {
                orphans: HashSet::new(),
            },
        ) {
            HolderState::Fetching { wanted, .. } => wanted,
            _ => HashSet::new(),
        };
        self.state = HolderState::Dropped { orphans };
    }
}

enum HolderState {
    Listing {
        hashes: Vec<Hash>,
        next: u16,
    },
    Listed {
        hashes: Vec<Hash>,
    },
    Fetching {
        wanted: HashSet<Hash>,
        stream: Vec<u8>,
        next: u16,
    },
    /// Delivered everything asked for (or had nothing to give).
    Done,
    /// Left, timed out, denied or sent garbage; `orphans` are the hashes it still owed.
    Dropped {
        orphans: HashSet<Hash>,
    },
}

/// An answer waiting for the requester's `get`.
struct Serve {
    requester: u32,
    req: u32,
    created: UnixMillis,
    messages: Vec<ChatMessage>,
}

pub(crate) struct Core {
    own: u32,
    peers: HashMap<u32, Peer>,
    next_req: u32,
    pending: VecDeque<Pending>,
    job: Option<Job>,
    serves: Vec<Serve>,
    outbox: Outbox,
    pacing: Bucket,
    events: Vec<EngineEvent>,
}

impl Core {
    pub(crate) fn new(own: u32, now: UnixMillis) -> Self {
        Self {
            own,
            peers: HashMap::new(),
            next_req: 1,
            pending: VecDeque::new(),
            job: None,
            serves: Vec::new(),
            outbox: Outbox::default(),
            pacing: Bucket::new(SEND_RATE, SEND_BURST, now),
            events: Vec::new(),
        }
    }

    /// A person's current state: `channels` are the channel they are in plus the ones they
    /// follow. For the own user, every channel that starts arriving live queues a sync up to
    /// that moment (QA.49).
    pub(crate) fn user_seen(
        &mut self,
        session: u32,
        name: &str,
        channels: impl IntoIterator<Item = u32>,
        now: UnixMillis,
    ) {
        let peer = self.peers.entry(session).or_insert_with(|| Peer {
            name: String::new(),
            live: HashMap::new(),
            budget: Bucket::new(PEER_RATE, PEER_BURST, now),
        });
        name.clone_into(&mut peer.name);
        let channels: HashSet<u32> = channels.into_iter().collect();
        peer.live.retain(|c, _| channels.contains(c));
        let mut added: Vec<u32> = channels
            .into_iter()
            .filter(|c| !peer.live.contains_key(c))
            .collect();
        added.sort_unstable();
        for &channel in &added {
            peer.live.insert(channel, now);
        }
        if session == self.own {
            for channel in added {
                self.queue(channel, now);
            }
        }
    }

    pub(crate) fn user_left(&mut self, session: u32) {
        self.peers.remove(&session);
        self.serves.retain(|s| s.requester != session);
        self.outbox.forget(session);
        if let Some(Job {
            stage: Stage::Transfer { holders, .. },
            ..
        }) = self.job.as_mut()
        {
            for holder in holders.iter_mut().filter(|h| h.session == session) {
                holder.drop_out();
            }
        }
    }

    /// "Load earlier messages": sync the channel up to `before` (or its cutoff).
    pub(crate) fn request(
        &mut self,
        channel: u32,
        before: Option<UnixMillis>,
        _now: UnixMillis,
    ) -> Result<(), RequestError> {
        let since = self
            .peers
            .get(&self.own)
            .and_then(|p| p.live.get(&channel))
            .copied()
            .ok_or(RequestError::NotLive)?;
        self.queue(channel, before.map_or(since, |b| b.min(since)));
        Ok(())
    }

    fn queue(&mut self, channel: u32, to: UnixMillis) {
        let busy = self.job.as_ref().is_some_and(|j| j.channel == channel)
            || self.pending.iter().any(|p| p.channel == channel);
        if !busy {
            self.pending.push_back(Pending { channel, to });
        }
    }

    fn take_req(&mut self) -> u32 {
        let req = self.next_req;
        self.next_req = self.next_req.wrapping_add(1).max(1);
        req
    }

    pub(crate) fn take_events(&mut self) -> Vec<EngineEvent> {
        std::mem::take(&mut self.events)
    }

    /// Packets that may be sent now, as `(receivers, data)`.
    pub(crate) fn take_outgoing(&mut self, now: UnixMillis) -> Vec<(Vec<u32>, Vec<u8>)> {
        let mut out = Vec::new();
        while !self.outbox.queues.is_empty() && self.pacing.take(now) {
            out.extend(self.outbox.pop());
        }
        out
    }

    /// Timeouts and the next queued channel.
    pub(crate) fn tick(&mut self, env: &impl Env, now: UnixMillis) {
        self.serves
            .retain(|s| now.saturating_sub(s.created) < SERVE_TTL_MS);
        if self.job.is_none() {
            self.start_next(env, now);
        }
        let discover_due = match self.job.as_mut().map(|j| &mut j.stage) {
            None => return,
            Some(Stage::Discover { until, .. }) => Some(now >= *until),
            Some(Stage::Transfer { holders, .. }) => {
                for holder in holders.iter_mut().filter(|h| now >= h.deadline) {
                    // Listed holders wait for the others; nothing more is expected from them.
                    if matches!(
                        holder.state,
                        HolderState::Listing { .. } | HolderState::Fetching { .. }
                    ) {
                        holder.drop_out();
                    }
                }
                None
            }
        };
        match discover_due {
            Some(true) => self.ask_holders(env, now),
            Some(false) => {}
            None => self.advance(now),
        }
    }

    fn start_next(&mut self, env: &impl Env, now: UnixMillis) {
        while let Some(pending) = self.pending.pop_front() {
            let Some(key) = env.channel_key(pending.channel) else {
                continue;
            };
            let from = sync_floor(env, &key, now);
            if from >= pending.to {
                // Nothing to ask for (e.g. cleared since): "load earlier" still gets its answer.
                self.events.push(EngineEvent::HistorySyncStatus {
                    channel_id: pending.channel,
                    running: false,
                    added: 0,
                });
                continue;
            }
            self.events.push(EngineEvent::HistorySyncStatus {
                channel_id: pending.channel,
                running: true,
                added: 0,
            });
            let req = self.take_req();
            let others: Vec<u32> = self
                .peers
                .keys()
                .copied()
                .filter(|&s| s != self.own)
                .collect();
            self.job = Some(Job {
                channel: pending.channel,
                key,
                from,
                to: pending.to,
                added: 0,
                stage: Stage::Discover {
                    req,
                    until: now + DISCOVER_MS,
                    offers: Vec::new(),
                },
            });
            if others.is_empty() {
                self.finish();
                continue;
            }
            tracing::debug!(
                receivers = others.len(),
                "history sync: asking for summaries"
            );
            self.outbox.push(
                others,
                &Packet::AvailabilityRequest {
                    req,
                    channel: pending.channel,
                    from,
                    to: pending.to,
                },
            );
            return;
        }
    }

    fn finish(&mut self) {
        if let Some(job) = self.job.take() {
            tracing::info!(added = job.added, "history sync: channel done");
            self.events.push(EngineEvent::HistorySyncStatus {
                channel_id: job.channel,
                running: false,
                added: job.added,
            });
        }
    }

    /// After the discovery window: ask the largest holders for their hash lists.
    fn ask_holders(&mut self, env: &impl Env, now: UnixMillis) {
        let Some(Job {
            channel,
            key,
            from,
            to,
            stage: Stage::Discover { offers, .. },
            ..
        }) = self.job.as_mut()
        else {
            return;
        };
        let mut offers = std::mem::take(offers);
        let (channel, key, from, to) = (*channel, key.clone(), *from, *to);
        offers.sort_by_key(|&(session, count)| (std::cmp::Reverse(count), session));
        offers.truncate(MAX_HOLDERS);
        if offers.is_empty() {
            self.finish();
            return;
        }
        let known = known_messages(env, &key, from, to);
        let mut holders = Vec::new();
        for (session, _) in offers {
            let req = self.take_req();
            self.outbox.push(
                vec![session],
                &Packet::HashRequest {
                    req,
                    channel,
                    from,
                    to,
                },
            );
            holders.push(Holder {
                session,
                req,
                deadline: now + LIST_TIMEOUT_MS,
                state: HolderState::Listing {
                    hashes: Vec::new(),
                    next: 0,
                },
                list: Vec::new(),
            });
        }
        if let Some(job) = self.job.as_mut() {
            job.stage = Stage::Transfer {
                holders,
                planned: false,
                known,
                orphans: HashSet::new(),
            };
        }
    }

    /// Plans the fetch once every hash list is in, hands what dropped-out holders owed to
    /// another holder, and ends the job when all are done.
    fn advance(&mut self, now: UnixMillis) {
        let mut fetches = Vec::new();
        let mut relist = None;
        let done = match self.job.as_mut().map(|j| &mut j.stage) {
            Some(Stage::Transfer {
                holders,
                planned,
                known,
                orphans,
            }) => {
                if !*planned
                    && holders
                        .iter()
                        .all(|h| !matches!(h.state, HolderState::Listing { .. }))
                {
                    *planned = true;
                    fetches = plan_fetch(holders, &known.hashes, now);
                }
                if *planned {
                    fetches.extend(reassign(holders, orphans, now));
                    relist = replacement(holders, orphans);
                }
                *planned
                    && relist.is_none()
                    && holders
                        .iter()
                        .all(|h| matches!(h.state, HolderState::Done | HolderState::Dropped { .. }))
            }
            _ => false,
        };
        for (holder, packet) in fetches {
            self.outbox.push(vec![holder], &packet);
        }
        if let Some(at) = relist {
            self.relist(at, now);
        }
        if done {
            self.finish();
        }
    }

    /// Asks the holder at `at` for a fresh hash list, to fetch the orphans it has.
    fn relist(&mut self, at: usize, now: UnixMillis) {
        let req = self.take_req();
        let Some(Job {
            channel,
            from,
            to,
            stage: Stage::Transfer { holders, .. },
            ..
        }) = self.job.as_mut()
        else {
            return;
        };
        let holder = &mut holders[at];
        tracing::debug!("history sync: asking another holder for what a dropped one owed");
        holder.req = req;
        holder.deadline = now + LIST_TIMEOUT_MS;
        holder.state = HolderState::Listing {
            hashes: Vec::new(),
            next: 0,
        };
        let packet = Packet::HashRequest {
            req,
            channel: *channel,
            from: *from,
            to: *to,
        };
        let session = holder.session;
        self.outbox.push(vec![session], &packet);
    }

    /// A packet from another person.
    pub(crate) fn on_packet(&mut self, env: &impl Env, sender: u32, data: &[u8], now: UnixMillis) {
        if sender == self.own || !self.peers.contains_key(&sender) {
            return;
        }
        let packet = match wire::decode(data) {
            Ok(packet) => packet,
            Err(e) => {
                tracing::debug!(error = %e, "ignoring a history sync packet");
                return;
            }
        };
        match packet {
            Packet::AvailabilityRequest {
                req,
                channel,
                from,
                to,
            } => self.answer_summary(env, sender, req, channel, from, to, now),
            Packet::HashRequest {
                req,
                channel,
                from,
                to,
            } => self.answer_hashes(env, sender, req, channel, from, to, now),
            Packet::Fetch { req, wanted } => self.answer_fetch(env, sender, req, &wanted, now),
            Packet::Availability {
                req,
                channel,
                count,
                ..
            } => {
                if let Some(Job {
                    channel: c,
                    stage: Stage::Discover { req: r, offers, .. },
                    ..
                }) = self.job.as_mut()
                    && *r == req
                    && *c == channel
                    && count > 0
                    && !offers.iter().any(|&(s, _)| s == sender)
                {
                    offers.push((sender, count));
                }
            }
            Packet::Hashes {
                req,
                index,
                last,
                hashes,
            } => {
                if let Some(holder) = self.holder(sender, req) {
                    receive_hashes(holder, index, last, hashes, now);
                }
                self.advance(now);
            }
            Packet::Part {
                req,
                index,
                last,
                data,
            } => {
                let complete = self
                    .holder(sender, req)
                    .and_then(|holder| receive_part(holder, index, last, data, now));
                if let Some((wanted, stream)) = complete {
                    self.deliver(env, sender, wanted, &stream);
                }
                self.advance(now);
            }
            Packet::Deny { req } => {
                if let Some(holder) = self.holder(sender, req) {
                    holder.drop_out();
                }
                self.advance(now);
            }
        }
    }

    /// The open transfer of `req` with `sender`, if any.
    fn holder(&mut self, sender: u32, req: u32) -> Option<&mut Holder> {
        match self.job.as_mut() {
            Some(Job {
                stage: Stage::Transfer { holders, .. },
                ..
            }) => holders
                .iter_mut()
                .find(|h| h.session == sender && h.req == req),
            _ => None,
        }
    }

    /// Checks, sanitizes, deduplicates and stores the messages of one holder.
    fn deliver(&mut self, env: &impl Env, sender: u32, mut wanted: HashSet<Hash>, stream: &[u8]) {
        let holder_name = self
            .peers
            .get(&sender)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let own_cert = env.own_cert();
        let Some(job) = self.job.as_mut() else {
            return;
        };
        // The user may have cleared the channel while this sync was running.
        let from = cleared_floor(env, &job.key).map_or(job.from, |f| f.max(job.from));
        let Stage::Transfer { known, .. } = &mut job.stage else {
            return;
        };
        let messages = match wire::decode_messages(stream) {
            Ok(messages) => messages,
            Err(e) => {
                tracing::debug!(error = %e, "history sync: unreadable message stream");
                return;
            }
        };
        let mut accepted: Vec<(Hash, NewChatMessage)> = Vec::new();
        // Text keys of this batch join `known` only afterwards: one holder's repeated
        // message ("ok", "ok") is two messages, while another source's copy is a duplicate.
        let mut texts = Vec::new();
        for m in messages.into_iter().take(wanted.len()) {
            // Also guards against holders that ignore the requested range.
            if m.ts < from || m.ts >= job.to || m.b.len() > MAX_BODY {
                continue;
            }
            if m.h.as_deref().is_some_and(|h| !is_cert_hash(h)) {
                continue;
            }
            let content = Content {
                channel: &job.key,
                sender_name: &m.n,
                sender_cert_hash: m.h.as_deref(),
                sent_at: m.ts,
                body: &m.b,
            };
            let hash = content_hash(&content);
            // Only what was asked for, each at most once; and nothing already here.
            if !wanted.remove(&hash) || known.contains(&hash, &content) {
                continue;
            }
            known.hashes.extend(variants(&content));
            texts.push((text_key(&content), m.ts));
            let Some(name) = crate::web::meta::clean(&m.n, MAX_NAME) else {
                continue;
            };
            accepted.push((
                hash,
                NewChatMessage {
                    conversation: Conversation::Channel(job.key.clone()),
                    sender_name: name,
                    sender_cert_hash: m.h,
                    sent_at: m.ts,
                    body: sanitize_message_html(&m.b),
                },
            ));
        }
        for (key, sent_at) in texts {
            known.add_text(key, sent_at);
        }
        if accepted.is_empty() {
            return;
        }
        accepted.sort_by_key(|(hash, m)| (m.sent_at, *hash));
        let messages: Vec<ChatMessageView> = {
            let store = env.store();
            accepted
                .into_iter()
                .map(|(_, new)| {
                    let id = store
                        .insert_synced_message(env.server_id(), &new, &holder_name)
                        .unwrap_or_else(|e| {
                            tracing::warn!(error = %e, "storing a synced message failed");
                            None
                        })
                        .map_or_else(transient_id, |id| id.to_string());
                    ChatMessageView {
                        id,
                        conversation: ConversationRef::Channel {
                            channel_id: job.channel,
                        },
                        own: own_cert.is_some() && new.sender_cert_hash == own_cert,
                        sender_name: new.sender_name,
                        sender_cert_hash: new.sender_cert_hash,
                        sender_session: None,
                        sent_at: new.sent_at,
                        body: new.body,
                        dismissed_previews: Vec::new(),
                        synced_from: Some(holder_name.clone()),
                    }
                })
                .collect()
        };
        job.added = job
            .added
            .saturating_add(u32::try_from(messages.len()).unwrap_or(u32::MAX));
        self.events.push(EngineEvent::HistorySynced {
            channel_id: job.channel,
            messages,
        });
    }

    /// The store key and clamped range for answering `sender`, or `None` if not allowed.
    fn answer_range(
        &self,
        env: &impl Env,
        sender: u32,
        channel: u32,
        from: UnixMillis,
        to: UnixMillis,
        now: UnixMillis,
    ) -> Option<(String, UnixMillis, UnixMillis)> {
        if !env.sharing() {
            return None;
        }
        // Only people who receive the channel live, and only up to when they started to.
        let since = *self.peers.get(&sender)?.live.get(&channel)?;
        let to = to.min(since).min(now);
        let from = from.max(now - MAX_AGE_MS);
        if from >= to {
            return None;
        }
        Some((env.channel_key(channel)?, from, to))
    }

    fn spend(&mut self, sender: u32, now: UnixMillis) -> bool {
        let allowed = self
            .peers
            .get_mut(&sender)
            .is_some_and(|p| p.budget.take(now));
        if !allowed {
            tracing::debug!("history sync: request budget of a person exhausted");
        }
        allowed
    }

    #[allow(clippy::too_many_arguments)]
    fn answer_summary(
        &mut self,
        env: &impl Env,
        sender: u32,
        req: u32,
        channel: u32,
        from: UnixMillis,
        to: UnixMillis,
        now: UnixMillis,
    ) {
        if !self.spend(sender, now) {
            return;
        }
        // Asked everyone at once: stay silent rather than deny.
        let Some((key, from, to)) = self.answer_range(env, sender, channel, from, to, now) else {
            return;
        };
        let summary =
            match env
                .store()
                .range_summary(env.server_id(), &Conversation::Channel(key), from, to)
            {
                Ok(summary) => summary,
                Err(e) => {
                    tracing::warn!(error = %e, "history sync: reading the summary failed");
                    return;
                }
            };
        let (Some(oldest), Some(newest)) = (summary.oldest, summary.newest) else {
            return;
        };
        self.outbox.push(
            vec![sender],
            &Packet::Availability {
                req,
                channel,
                count: u32::try_from(summary.count).unwrap_or(u32::MAX),
                oldest,
                newest,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn answer_hashes(
        &mut self,
        env: &impl Env,
        sender: u32,
        req: u32,
        channel: u32,
        from: UnixMillis,
        to: UnixMillis,
        now: UnixMillis,
    ) {
        if !self.spend(sender, now) {
            return;
        }
        self.serves.retain(|s| s.requester != sender);
        let range = self.answer_range(env, sender, channel, from, to, now);
        let Some((key, from, to)) = range.filter(|_| self.serves.len() < MAX_SERVES) else {
            self.outbox.push(vec![sender], &Packet::Deny { req });
            return;
        };
        let rows = env.store().messages_between(
            env.server_id(),
            &Conversation::Channel(key.clone()),
            from,
            to,
            u32::try_from(MAX_MESSAGES).unwrap_or(u32::MAX),
        );
        let messages: Vec<ChatMessage> = match rows {
            Ok(rows) => rows
                .into_iter()
                .filter(|m| m.body.len() <= MAX_BODY)
                .collect(),
            Err(e) => {
                tracing::warn!(error = %e, "history sync: reading messages failed");
                self.outbox.push(vec![sender], &Packet::Deny { req });
                return;
            }
        };
        let hashes: Vec<Hash> = messages
            .iter()
            .map(|m| content_hash(&stored_content(&key, m)))
            .collect();
        tracing::debug!(count = hashes.len(), "history sync: sending a hash list");
        for packet in wire::chunk_hashes(req, &hashes) {
            self.outbox.push(vec![sender], &packet);
        }
        self.serves.push(Serve {
            requester: sender,
            req,
            created: now,
            messages,
        });
    }

    fn answer_fetch(
        &mut self,
        env: &impl Env,
        sender: u32,
        req: u32,
        wanted: &[u8],
        now: UnixMillis,
    ) {
        if !self.spend(sender, now) {
            return;
        }
        let Some(at) = self
            .serves
            .iter()
            .position(|s| s.requester == sender && s.req == req)
        else {
            return;
        };
        let serve = self.serves.swap_remove(at);
        if !env.sharing() {
            self.outbox.push(vec![sender], &Packet::Deny { req });
            return;
        }
        let mut size = 0;
        let mut messages = Vec::new();
        for k in wire::selected(wanted, serve.messages.len()) {
            let m = &serve.messages[k];
            let message = WireMessage {
                ts: m.sent_at,
                n: m.sender_name.clone(),
                h: m.sender_cert_hash.clone(),
                b: m.body.clone(),
            };
            size += wire::encode_messages(std::slice::from_ref(&message)).len();
            if size > MAX_STREAM {
                break;
            }
            messages.push(message);
        }
        tracing::debug!(count = messages.len(), "history sync: sending messages");
        for packet in wire::chunk_stream(req, &wire::encode_messages(&messages)) {
            self.outbox.push(vec![sender], &packet);
        }
    }
}

fn stored_content<'a>(key: &'a str, m: &'a ChatMessage) -> Content<'a> {
    Content {
        channel: key,
        sender_name: &m.sender_name,
        sender_cert_hash: m.sender_cert_hash.as_deref(),
        sent_at: m.sent_at,
        body: &m.body,
    }
}

/// The oldest send time a sync of the channel `key` takes (issue #119): at most
/// [`MAX_AGE_MS`] back, nothing the automatic history deletion would delete right away, and
/// nothing from before the user last cleared the channel.
fn sync_floor(env: &impl Env, key: &str, now: UnixMillis) -> UnixMillis {
    let mut floor = now - MAX_AGE_MS;
    if let Some(days) = env.history_max_age_days() {
        floor = floor.max(history_cutoff(now, days));
    }
    cleared_floor(env, key).map_or(floor, |f| f.max(floor))
}

/// The first send time after the user last cleared the channel `key`, if they did. Holders
/// stamp messages with their own clocks, so a copy of a deleted message may carry a time up to
/// [`SKEW_MS`] after the clear; those few seconds are skipped too.
fn cleared_floor(env: &impl Env, key: &str) -> Option<UnixMillis> {
    match env.store().history_cleared_at(env.server_id(), key) {
        Ok(at) => at.map(|at| at.saturating_add(SKEW_MS + 1)),
        Err(e) => {
            tracing::warn!(error = %e, "history sync: reading when history was cleared failed");
            None
        }
    }
}

/// The stored messages around `[from, to)`.
fn known_messages(env: &impl Env, key: &str, from: UnixMillis, to: UnixMillis) -> Known {
    let rows = env.store().messages_between(
        env.server_id(),
        &Conversation::Channel(key.to_owned()),
        from - SKEW_MS - 1000,
        to.saturating_add(SKEW_MS + 1000),
        MAX_KNOWN,
    );
    let mut known = Known::default();
    match rows {
        Ok(rows) => {
            for m in &rows {
                known.add(&stored_content(key, m));
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "history sync: reading stored messages failed");
        }
    }
    known
}

fn is_cert_hash(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn receive_hashes(holder: &mut Holder, index: u16, last: bool, chunk: Vec<Hash>, now: UnixMillis) {
    let HolderState::Listing { hashes, next } = &mut holder.state else {
        return;
    };
    if index != *next || hashes.len() + chunk.len() > MAX_MESSAGES {
        holder.drop_out();
        return;
    }
    hashes.extend(chunk);
    *next = next.saturating_add(1);
    holder.deadline = now + LIST_TIMEOUT_MS;
    if last {
        holder.state = HolderState::Listed {
            hashes: std::mem::take(hashes),
        };
    }
}

/// Adds a chunk; returns the wanted hashes and the whole stream once complete.
fn receive_part(
    holder: &mut Holder,
    index: u16,
    last: bool,
    data: Vec<u8>,
    now: UnixMillis,
) -> Option<(HashSet<Hash>, Vec<u8>)> {
    let HolderState::Fetching {
        wanted,
        stream,
        next,
    } = &mut holder.state
    else {
        return None;
    };
    if index != *next || stream.len() + data.len() > MAX_STREAM {
        holder.drop_out();
        return None;
    }
    stream.extend(data);
    *next = next.saturating_add(1);
    holder.deadline = now + FETCH_IDLE_MS;
    if !last {
        return None;
    }
    let complete = (std::mem::take(wanted), std::mem::take(stream));
    holder.state = HolderState::Done;
    Some(complete)
}

/// Assigns every missing hash (newest first, at most [`MAX_MESSAGES`]) to the holder with
/// the fewest assignments among those that have it; returns the `get` packets to send.
fn plan_fetch(
    holders: &mut [Holder],
    known: &HashSet<Hash>,
    now: UnixMillis,
) -> Vec<(u32, Packet)> {
    let mut lists: Vec<Vec<Hash>> = holders
        .iter()
        .map(|h| match &h.state {
            HolderState::Listed { hashes } => hashes.clone(),
            _ => Vec::new(),
        })
        .collect();
    let mut where_: HashMap<Hash, Vec<(usize, usize)>> = HashMap::new();
    let mut order: Vec<Hash> = Vec::new();
    let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
    // Interleave the lists from their newest end so the cap keeps the newest messages.
    for back in 0..longest {
        for (h, list) in lists.iter().enumerate() {
            let Some(k) = list.len().checked_sub(back + 1) else {
                continue;
            };
            let hash = list[k];
            let places = where_.entry(hash).or_default();
            if places.is_empty() {
                order.push(hash);
            }
            places.push((h, k));
        }
    }
    let mut assigned: Vec<Vec<usize>> = vec![Vec::new(); holders.len()];
    let mut wanted: Vec<HashSet<Hash>> = vec![HashSet::new(); holders.len()];
    for hash in order
        .into_iter()
        .filter(|h| !known.contains(h))
        .take(MAX_MESSAGES)
    {
        if let Some(&(h, k)) = where_[&hash].iter().min_by_key(|(h, _)| assigned[*h].len()) {
            assigned[h].push(k);
            wanted[h].insert(hash);
        }
    }
    let mut packets = Vec::new();
    for (h, holder) in holders.iter_mut().enumerate() {
        holder.list = std::mem::take(&mut lists[h]);
        if assigned[h].is_empty() {
            holder.state = HolderState::Done;
            continue;
        }
        packets.push((
            holder.session,
            Packet::Fetch {
                req: holder.req,
                wanted: wire::bitmap(holder.list.len(), assigned[h].iter().copied()),
            },
        ));
        holder.state = HolderState::Fetching {
            wanted: std::mem::take(&mut wanted[h]),
            stream: Vec::new(),
            next: 0,
        };
        holder.deadline = now + FETCH_IDLE_MS;
    }
    packets
}

/// Collects what dropped-out holders owed into `orphans`, and fetches the orphans that a
/// re-listed holder has from it.
fn reassign(
    holders: &mut [Holder],
    orphans: &mut HashSet<Hash>,
    now: UnixMillis,
) -> Vec<(u32, Packet)> {
    let mut packets = Vec::new();
    for holder in holders.iter_mut() {
        match &mut holder.state {
            HolderState::Dropped { orphans: owed } => orphans.extend(owed.drain()),
            HolderState::Listed { hashes } => {
                holder.list = std::mem::take(hashes);
                let picks: Vec<usize> = (0..holder.list.len())
                    .filter(|&k| orphans.remove(&holder.list[k]))
                    .collect();
                if picks.is_empty() {
                    holder.state = HolderState::Done;
                    continue;
                }
                packets.push((
                    holder.session,
                    Packet::Fetch {
                        req: holder.req,
                        wanted: wire::bitmap(holder.list.len(), picks.iter().copied()),
                    },
                ));
                holder.state = HolderState::Fetching {
                    wanted: picks.iter().map(|&k| holder.list[k]).collect(),
                    stream: Vec::new(),
                    next: 0,
                };
                holder.deadline = now + FETCH_IDLE_MS;
            }
            _ => {}
        }
    }
    packets
}

/// The finished holder that has most of the orphans, to ask for a fresh hash list (its
/// earlier answer is used up). `None` while another holder is being re-listed or no
/// finished holder has any of them; orphans nobody can serve are given up with the job.
fn replacement(holders: &[Holder], orphans: &HashSet<Hash>) -> Option<usize> {
    if orphans.is_empty()
        || holders
            .iter()
            .any(|h| matches!(h.state, HolderState::Listing { .. }))
    {
        return None;
    }
    holders
        .iter()
        .enumerate()
        .filter(|(_, h)| matches!(h.state, HolderState::Done))
        .map(|(at, h)| (h.list.iter().filter(|x| orphans.contains(*x)).count(), at))
        .filter(|&(count, _)| count > 0)
        .max()
        .map(|(_, at)| at)
}
