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
//! **Locked channels** (the channel or a parent restricts entering, issue #382): following
//! is not enough. Only requesters physically in the channel are answered, checked again for
//! every request and before sending messages; open answers end when the requester leaves.
//! An answerer that holds access tokens also asks for the password proof ([`super::proof`])
//! and answers only while its own Enter permission there is confirmed by a fresh
//! PermissionQuery. The own user asks for a locked channel's history only while inside it.
//!
//! Abuse limits: a per-person request budget, at most [`MAX_SERVES`] open answers, size
//! caps on everything, and data that does not answer an own open request is ignored.
//! Outgoing packets are paced below the server's plugin message rate limit and sent round
//! robin per receiver, so one long answer does not block the others.
//!
//! Pure logic: time comes in as Unix milliseconds and packets go out through
//! [`Core::take_outgoing`], so all of it runs in unit tests without a server.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, MutexGuard};

use mumcord_store::{ChatMessage, Conversation, NewChatMessage, ServerId, Store, UnixMillis};

use super::hash::{Content, content_hash, text_key, variants};
use super::images::{self, ImageRef};
use super::proof::{self, NONCE_TTL_MS, Nonce};
use super::wire::{self, HASH_LEN, Hash, MAX_MESSAGES, Packet, WireMessage};
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
/// How long a requester may fetch the images of an answer after it was sent or last used.
const GRANT_TTL_MS: i64 = 10 * 60 * 1000;
/// Most image bytes held for answers at the same time; further images stay inline.
const MAX_GRANT_BYTES: usize = 32 * 1024 * 1024;

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
    /// Whether the channel or one of its parents restricts entering; unknown channels count as
    /// locked.
    fn channel_locked(&self, channel: u32) -> bool;
    /// The access tokens (channel passwords) this session sent to the server.
    fn own_tokens(&self) -> Vec<String>;
    /// Asks the server for the own permissions in `channel`; the answer comes back through
    /// [`Core::own_permissions`].
    fn query_own_permissions(&self, channel: u32);
    /// Whether `session` announced [`images::FEATURE`] (resolves image references).
    fn resolves_images(&self, session: u32) -> bool;
    /// The "Offer my full-size images" setting (FR-1.74). Never called while [`Env::store`] is
    /// held.
    fn offer_images(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestError {
    /// The user neither is in nor follows the channel.
    NotLive,
    /// The channel is locked and the user only follows it.
    Locked,
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
    cert: Option<String>,
    /// The channel this person is physically in.
    inside: u32,
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
    channel: u32,
    /// A locked channel: the requester must stay inside it.
    locked: bool,
    /// Answered after the password proof: the own Enter permission must still hold.
    proven: bool,
    created: UnixMillis,
    messages: Vec<Served>,
}

/// A stored message prepared for one requester.
struct Served {
    message: ChatMessage,
    hash: Hash,
    /// For requesters that resolve image references: the body with its images as references,
    /// and those images.
    referenced: Option<(String, Vec<SharedImage>)>,
}

/// An image of a served message, shared with the grants made for it.
type SharedImage = (ImageRef, Arc<Vec<u8>>);

/// An image a requester may fetch by hash because it was sent a message containing it
/// (FR-1.60g); the access rules of that answer are checked again on every fetch.
struct Grant {
    channel: u32,
    locked: bool,
    proven: bool,
    used: UnixMillis,
    bytes: Arc<Vec<u8>>,
}

/// Referenced images of synced messages to fetch from the holder that sent them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageFetch {
    pub holder: u32,
    pub channel: u32,
    pub key: String,
    pub refs: Vec<ImageRef>,
}

/// A `hreq` for a locked channel waiting for the requester's password proof.
struct Challenge {
    requester: u32,
    req: u32,
    channel: u32,
    from: UnixMillis,
    to: UnixMillis,
    nonce: Nonce,
    created: UnixMillis,
}

pub(crate) struct Core {
    own: u32,
    peers: HashMap<u32, Peer>,
    next_req: u32,
    pending: VecDeque<Pending>,
    job: Option<Job>,
    serves: Vec<Serve>,
    challenges: Vec<Challenge>,
    /// The own Enter permission per channel as last answered by the server, and when.
    permissions: HashMap<u32, (UnixMillis, bool)>,
    outbox: Outbox,
    pacing: Bucket,
    events: Vec<EngineEvent>,
    /// Images requesters may fetch, by requester and image hash.
    grants: HashMap<(u32, [u8; 32]), Grant>,
    image_fetches: Vec<ImageFetch>,
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
            challenges: Vec::new(),
            permissions: HashMap::new(),
            outbox: Outbox::default(),
            pacing: Bucket::new(SEND_RATE, SEND_BURST, now),
            events: Vec::new(),
            grants: HashMap::new(),
            image_fetches: Vec::new(),
        }
    }

    /// A person's current state: `inside` is the channel they are in, `listening` the ones
    /// they follow. For the own user, every channel that starts arriving live queues a sync up
    /// to that moment (QA.49); a locked one only once the user is inside it (issue #382).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn user_seen(
        &mut self,
        env: &impl Env,
        session: u32,
        name: &str,
        cert: Option<&str>,
        inside: u32,
        listening: impl IntoIterator<Item = u32>,
        now: UnixMillis,
    ) {
        let peer = self.peers.entry(session).or_insert_with(|| Peer {
            name: String::new(),
            cert: None,
            inside,
            live: HashMap::new(),
            budget: Bucket::new(PEER_RATE, PEER_BURST, now),
        });
        name.clone_into(&mut peer.name);
        peer.cert = cert.map(str::to_owned);
        let entered = peer.inside != inside;
        peer.inside = inside;
        let channels: HashSet<u32> = std::iter::once(inside).chain(listening).collect();
        peer.live.retain(|c, _| channels.contains(c));
        let mut added: Vec<u32> = channels
            .into_iter()
            .filter(|c| !peer.live.contains_key(c))
            .collect();
        added.sort_unstable();
        for &channel in &added {
            peer.live.insert(channel, now);
        }
        // Answers for a locked channel end as soon as the requester is no longer inside it.
        self.serves
            .retain(|s| s.requester != session || !s.locked || s.channel == inside);
        self.challenges
            .retain(|c| c.requester != session || c.channel == inside);
        if session == self.own {
            // Entering a locked channel that was only followed so far reaches back to the
            // follow, like any other live channel.
            let since = self.peers[&session].live.get(&inside).copied();
            if entered
                && !added.contains(&inside)
                && env.channel_locked(inside)
                && let Some(since) = since
            {
                self.queue(inside, since);
            }
            for channel in added {
                if channel == inside || !env.channel_locked(channel) {
                    self.queue(channel, now);
                }
            }
        }
    }

    /// The server's answer to a PermissionQuery for the own user in `channel`.
    pub(crate) fn own_permissions(&mut self, channel: u32, enter: bool, now: UnixMillis) {
        self.permissions.insert(channel, (now, enter));
    }

    pub(crate) fn user_left(&mut self, session: u32) {
        self.peers.remove(&session);
        self.serves.retain(|s| s.requester != session);
        self.challenges.retain(|c| c.requester != session);
        self.grants
            .retain(|(requester, _), _| *requester != session);
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
        env: &impl Env,
        channel: u32,
        before: Option<UnixMillis>,
        _now: UnixMillis,
    ) -> Result<(), RequestError> {
        let own = self.peers.get(&self.own).ok_or(RequestError::NotLive)?;
        let since = own
            .live
            .get(&channel)
            .copied()
            .ok_or(RequestError::NotLive)?;
        if own.inside != channel && env.channel_locked(channel) {
            return Err(RequestError::Locked);
        }
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

    /// Referenced images of received messages that still have to be fetched.
    pub(crate) fn take_image_fetches(&mut self) -> Vec<ImageFetch> {
        std::mem::take(&mut self.image_fetches)
    }

    /// The bytes of an image `requester` asks for by hash, if it was sent a message containing
    /// it and may still read that message (FR-1.60g). Everything else gets `None`, so a hash
    /// alone gives no access and the reasons look the same to the requester.
    pub(crate) fn image_for(
        &mut self,
        env: &impl Env,
        requester: u32,
        hash: &[u8; 32],
        now: UnixMillis,
    ) -> Option<Arc<Vec<u8>>> {
        let grant = self.grants.get(&(requester, *hash))?;
        if now.saturating_sub(grant.used) >= GRANT_TTL_MS {
            return None;
        }
        let inside = !(grant.locked || env.channel_locked(grant.channel))
            || self
                .peers
                .get(&requester)
                .is_some_and(|p| p.inside == grant.channel);
        let entering = !grant.proven
            || self
                .permissions
                .get(&grant.channel)
                .is_some_and(|&(_, enter)| enter);
        if !inside || !entering || !env.sharing() || !env.offer_images() {
            return None;
        }
        let grant = self.grants.get_mut(&(requester, *hash))?;
        grant.used = now;
        Some(grant.bytes.clone())
    }

    /// Image bytes held for open answers and grants (counted once per holder of a copy).
    fn held_image_bytes(&self) -> usize {
        let granted: usize = self.grants.values().map(|g| g.bytes.len()).sum();
        let prepared: usize = self
            .serves
            .iter()
            .flat_map(|s| &s.messages)
            .flat_map(|m| m.referenced.iter().flat_map(|(_, images)| images))
            .map(|(_, bytes)| bytes.len())
            .sum();
        granted + prepared
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
        self.grants
            .retain(|_, g| now.saturating_sub(g.used) < GRANT_TTL_MS);
        let (expired, open): (Vec<_>, Vec<_>) = std::mem::take(&mut self.challenges)
            .into_iter()
            .partition(|c| now.saturating_sub(c.created) > NONCE_TTL_MS);
        self.challenges = open;
        for challenge in expired {
            self.outbox.push(
                vec![challenge.requester],
                &Packet::Deny { req: challenge.req },
            );
        }
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
            Packet::Proof { req, macs } => self.answer_proof(env, sender, req, &macs, now),
            Packet::Challenge {
                req,
                channel,
                nonce,
            } => self.prove(env, sender, req, channel, &nonce, now),
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

    /// A holder asks for the password proof of the own `hreq`. Proofs go only to holders inside
    /// the channel, so nobody outside it can collect them to guess the password offline.
    fn prove(
        &mut self,
        env: &impl Env,
        sender: u32,
        req: u32,
        channel: u32,
        nonce: &Nonce,
        now: UnixMillis,
    ) {
        let holder_cert = self
            .peers
            .get(&sender)
            .filter(|p| p.inside == channel)
            .and_then(|p| p.cert.clone());
        let tokens = env.own_tokens();
        let own_cert = env.own_cert();
        let job_channel = self.job.as_ref().map(|j| j.channel);
        let Some(holder) = self.holder(sender, req) else {
            return;
        };
        if !matches!(holder.state, HolderState::Listing { .. }) {
            return;
        }
        let macs = match (holder_cert, own_cert) {
            (Some(holder_cert), Some(own_cert))
                if job_channel == Some(channel) && !tokens.is_empty() =>
            {
                proof::prove(&tokens, nonce, channel, &own_cert, &holder_cert)
            }
            _ => {
                // No proof to give: do not wait for this holder.
                holder.drop_out();
                self.advance(now);
                return;
            }
        };
        self.outbox.push(vec![sender], &Packet::Proof { req, macs });
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
        let mut fetch: Vec<ImageRef> = Vec::new();
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
            let refs = images::references(&m.b);
            // A body with image references carries the hash of the holder's inline body.
            let hash = if refs.is_empty() {
                content_hash(&content)
            } else {
                match m.k.as_deref().and_then(parse_hash) {
                    Some(hash) => hash,
                    None => continue,
                }
            };
            // Only what was asked for, each at most once; and nothing already here.
            if !wanted.remove(&hash) {
                continue;
            }
            if known.contains(&hash, &content) {
                // The copy here may still lack its images: a failed fetch is retried now.
                fetch.extend(refs);
                continue;
            }
            if refs.is_empty() {
                known.hashes.extend(variants(&content));
            } else {
                known.hashes.insert(hash);
            }
            fetch.extend(refs);
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
        let mut seen = HashSet::new();
        fetch.retain(|image| seen.insert(image.full.hash));
        if !fetch.is_empty() {
            self.image_fetches.push(ImageFetch {
                holder: sender,
                channel: job.channel,
                key: job.key.clone(),
                refs: fetch,
            });
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
                        file: None,
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
        let peer = self.peers.get(&sender)?;
        let since = *peer.live.get(&channel)?;
        // Following a locked channel is not enough: only people inside it may read it.
        if peer.inside != channel && env.channel_locked(channel) {
            return None;
        }
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
        self.challenges.retain(|c| c.requester != sender);
        let range = self.answer_range(env, sender, channel, from, to, now);
        let Some((key, clamped_from, clamped_to)) =
            range.filter(|_| self.serves.len() + self.challenges.len() < MAX_SERVES)
        else {
            self.outbox.push(vec![sender], &Packet::Deny { req });
            return;
        };
        if !env.channel_locked(channel) || env.own_tokens().is_empty() {
            // Tokenless answerers cannot tell a password channel from one locked by groups:
            // being inside is all they check.
            self.serve_hashes(
                env,
                sender,
                req,
                channel,
                &key,
                clamped_from,
                clamped_to,
                false,
                now,
            );
            return;
        }
        // Holding tokens: the requester proves a password first, and the server confirms
        // that the own tokens still let this client in.
        let peer_cert = self.peers.get(&sender).and_then(|p| p.cert.clone());
        let (Some(_), Some(_), Some(nonce)) = (peer_cert, env.own_cert(), proof::nonce()) else {
            self.outbox.push(vec![sender], &Packet::Deny { req });
            return;
        };
        env.query_own_permissions(channel);
        self.challenges.push(Challenge {
            requester: sender,
            req,
            channel,
            from,
            to,
            nonce,
            created: now,
        });
        self.outbox.push(
            vec![sender],
            &Packet::Challenge {
                req,
                channel,
                nonce,
            },
        );
    }

    /// The requester's password proof for an open challenge. The nonce is used up by the first
    /// proof that names it, valid or not.
    fn answer_proof(
        &mut self,
        env: &impl Env,
        sender: u32,
        req: u32,
        macs: &[proof::Mac],
        now: UnixMillis,
    ) {
        if !self.spend(sender, now) {
            return;
        }
        let Some(at) = self
            .challenges
            .iter()
            .position(|c| c.requester == sender && c.req == req)
        else {
            return;
        };
        let challenge = self.challenges.swap_remove(at);
        let fresh = now.saturating_sub(challenge.created) <= NONCE_TTL_MS;
        let range = self.answer_range(
            env,
            sender,
            challenge.channel,
            challenge.from,
            challenge.to,
            now,
        );
        let proven = fresh
            && self.may_enter_since(challenge.channel, challenge.created)
            && match (
                self.peers.get(&sender).and_then(|p| p.cert.as_deref()),
                env.own_cert(),
            ) {
                (Some(requester), Some(own)) => proof::verify(
                    &env.own_tokens(),
                    macs,
                    &challenge.nonce,
                    challenge.channel,
                    requester,
                    &own,
                ),
                _ => false,
            };
        match range.filter(|_| proven && self.serves.len() < MAX_SERVES) {
            Some((key, from, to)) => {
                self.serve_hashes(
                    env,
                    sender,
                    req,
                    challenge.channel,
                    &key,
                    from,
                    to,
                    true,
                    now,
                );
            }
            None => {
                tracing::debug!("history sync: password proof not accepted");
                self.outbox.push(vec![sender], &Packet::Deny { req });
            }
        }
    }

    /// Whether the server confirmed the own Enter permission in `channel` at or after `since`.
    fn may_enter_since(&self, channel: u32, since: UnixMillis) -> bool {
        self.permissions
            .get(&channel)
            .is_some_and(|&(at, enter)| enter && at >= since)
    }

    /// Answers a hash request for a channel the requester may read: sends the hash list and
    /// keeps the messages for the requester's `get`.
    #[allow(clippy::too_many_arguments)]
    fn serve_hashes(
        &mut self,
        env: &impl Env,
        sender: u32,
        req: u32,
        channel: u32,
        key: &str,
        from: UnixMillis,
        to: UnixMillis,
        proven: bool,
        now: UnixMillis,
    ) {
        let rows = env.store().messages_between(
            env.server_id(),
            &Conversation::Channel(key.to_owned()),
            from,
            to,
            u32::try_from(MAX_MESSAGES).unwrap_or(u32::MAX),
        );
        // Answered with image references only while the own images may be offered (FR-1.60e).
        let referencing = env.resolves_images(sender) && env.offer_images();
        let mut budget = MAX_GRANT_BYTES.saturating_sub(self.held_image_bytes());
        let messages: Vec<Served> = match rows {
            Ok(rows) => rows
                .into_iter()
                .filter_map(|m| served(key, m, referencing, &mut budget))
                .collect(),
            Err(e) => {
                tracing::warn!(error = %e, "history sync: reading messages failed");
                self.outbox.push(vec![sender], &Packet::Deny { req });
                return;
            }
        };
        let hashes: Vec<Hash> = messages.iter().map(|m| m.hash).collect();
        tracing::debug!(count = hashes.len(), "history sync: sending a hash list");
        for packet in wire::chunk_hashes(req, &hashes) {
            self.outbox.push(vec![sender], &packet);
        }
        self.serves.push(Serve {
            requester: sender,
            req,
            channel,
            locked: env.channel_locked(channel),
            proven,
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
        // Checked again: the requester may have left a locked channel, or the own password
        // may have stopped working since the hash list went out.
        let still_inside = !(serve.locked || env.channel_locked(serve.channel))
            || self
                .peers
                .get(&sender)
                .is_some_and(|p| p.inside == serve.channel);
        let still_entering = !serve.proven
            || self
                .permissions
                .get(&serve.channel)
                .is_some_and(|&(_, enter)| enter);
        if !env.sharing() || !still_inside || !still_entering {
            self.outbox.push(vec![sender], &Packet::Deny { req });
            return;
        }
        let mut size = 0;
        let mut messages = Vec::new();
        for k in wire::selected(wanted, serve.messages.len()) {
            let served = &serve.messages[k];
            let m = &served.message;
            let (body, hash) = match &served.referenced {
                Some((body, _)) => (body.clone(), Some(hex(&served.hash))),
                None => (m.body.clone(), None),
            };
            let message = WireMessage {
                ts: m.sent_at,
                n: m.sender_name.clone(),
                h: m.sender_cert_hash.clone(),
                b: body,
                k: hash,
            };
            size += wire::encode_messages(std::slice::from_ref(&message)).len();
            if size > MAX_STREAM {
                break;
            }
            for (image, bytes) in served.referenced.iter().flat_map(|(_, images)| images) {
                self.grants.insert(
                    (sender, image.full.hash),
                    Grant {
                        channel: serve.channel,
                        locked: serve.locked,
                        proven: serve.proven,
                        used: now,
                        bytes: bytes.clone(),
                    },
                );
            }
            messages.push(message);
        }
        tracing::debug!(count = messages.len(), "history sync: sending messages");
        for packet in wire::chunk_stream(req, &wire::encode_messages(&messages)) {
            self.outbox.push(vec![sender], &packet);
        }
    }
}

/// A stored message as it goes to the requester, or `None` when its body is too large. With
/// `referencing`, inline images become references while `budget` (bytes) lasts, and the size
/// limit applies to the body as sent: the images travel through the full-image path with its
/// own limits (FR-1.60e).
fn served(
    key: &str,
    message: ChatMessage,
    referencing: bool,
    budget: &mut usize,
) -> Option<Served> {
    let hash = content_hash(&stored_content(key, &message));
    let mut referenced = None;
    if referencing && message.body.contains("data:image/") {
        let (body, found) = images::to_references(&message.body, |_, bytes| {
            let fits = bytes.len() <= *budget;
            if fits {
                *budget -= bytes.len();
            }
            fits
        });
        if !found.is_empty() {
            let images = found
                .into_iter()
                .map(|(image, bytes)| (image, Arc::new(bytes)))
                .collect::<Vec<_>>();
            referenced = Some((body, images));
        }
    }
    let size = referenced
        .as_ref()
        .map_or(message.body.len(), |(body, _)| body.len());
    if size > MAX_BODY {
        let held: usize = referenced
            .iter()
            .flat_map(|(_, images)| images)
            .map(|(_, bytes)| bytes.len())
            .sum();
        *budget += held;
        return None;
    }
    Some(Served {
        message,
        hash,
        referenced,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse_hash(text: &str) -> Option<Hash> {
    if text.len() != HASH_LEN * 2 {
        return None;
    }
    let mut hash = [0; HASH_LEN];
    for (i, b) in hash.iter_mut().enumerate() {
        *b = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(hash)
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
                let content = stored_content(key, m);
                if m.body.contains(images::ATTRIBUTE) && !images::references(&m.body).is_empty() {
                    // Its images are still missing, so its hash differs from the holder's
                    // (inline) one: only the text key matches, and fetching it again retries
                    // the images (FR-1.60f).
                    known.add_text(text_key(&content), m.sent_at);
                } else {
                    known.add(&content);
                }
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
