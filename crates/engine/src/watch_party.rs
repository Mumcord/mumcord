//! YouTube watch party in the user's channel (FR-2.39 – FR-2.41, Beta).
//!
//! Every participant plays the video in their own embedded player; only playback commands
//! travel, as plugin data with the id [`DATA_ID`], to the Mumcord users in the same channel that
//! announced [`FEATURE`]. Official clients never get them, so they see nothing of the party.
//!
//! Starting a party invites everyone in the channel: their clients learn the party but join it
//! only when the user clicks "Join". Everyone who joined may play, pause and seek; the sender
//! of a command is the session the server stamped on the data, never a field of the payload.
//! Someone who enters the channel later gets the current state from one participant only
//! (FR-2.75): the host while they are in the party, otherwise the participant with the lowest
//! session (the one longest on the server). Every client computes the same answerer, so no
//! coordination is needed, and the answerer sends each newcomer the state once per stay, and
//! once more a few seconds later in case the server dropped the first one; the newcomer keeps
//! the first state it gets. Leaving the channel (or the server) leaves the party; the party ends
//! when nobody is left in it.
//!
//! Payloads come from other clients and are untrusted: they are size-capped, versioned, the
//! video id must look like a YouTube id and member lists are capped and checked against the
//! people actually in the channel.
//!
//! A running party has a shared playlist (FR-2.69, see [`playlist`]). When the current video
//! ends, or someone skips, the client goes on to the first entry and tells the others which entry
//! it left; the others follow only if they still play that one, so several clients noticing the
//! end at once advance the party just once.
//!
//! Version 2 added the playlist: new messages (`pl`, `next`) and optional fields in `state`.
//! Version 3 adds the position report (`pos`, issue #163): every player says where it is and
//! whether it is buffering, so the party can show each person as in sync, buffering or behind.
//! Version 1 clients drop messages they cannot read and ignore unknown fields.

mod playlist;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use mumcord_protocol::{Handle, PLUGIN_DATA_MAX, PluginData};
use serde::{Deserialize, Serialize};

use playlist::{Flush, Op, Playlist};
pub use playlist::{MAX_ENTRIES as PLAYLIST_MAX, PlaylistEntry};

use crate::ServerId;
use crate::mumcord::ServerPeers;
use crate::session::{Engine, EngineError, EngineEvent, EngineResult, SessionCtx, lock};

/// Plugin data id of every watch-party message; other ids are ignored.
pub const DATA_ID: &str = "mumcord.watch";
/// Hello feature of clients that take part in watch parties.
pub const FEATURE: &str = "watch-party";
/// The message version this client sends; higher versions are still read.
const VERSION: u32 = 3;
/// More people than this in one party are not tracked; it bounds the state message too.
const MAX_PARTICIPANTS: usize = 32;
/// When a newcomer gets the state a second time. The server silently drops plugin data over its
/// rate limit, and a join sets off a burst from everyone in the channel, so the first may be lost.
const REANNOUNCE: Duration = Duration::from_secs(3);

/// A playback command; positions are milliseconds into the video.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum WatchCommand {
    Play { position_ms: u32 },
    Pause { position_ms: u32 },
    Seek { position_ms: u32 },
}

impl WatchCommand {
    fn position_ms(self) -> u32 {
        match self {
            Self::Play { position_ms }
            | Self::Pause { position_ms }
            | Self::Seek { position_ms } => position_ms,
        }
    }
}

/// Where one participant's player was when it last reported (issue #163).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParticipantPosition {
    pub session: u32,
    pub position_ms: u32,
    /// How old the report was when the view was made, in milliseconds.
    pub age_ms: u32,
    /// The player's own state, not the party's.
    pub playing: bool,
    pub buffering: bool,
}

/// Who did what last, for the "Anna paused" line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchAction {
    pub session: u32,
    pub command: WatchCommand,
}

/// The party in the user's channel as the UI shows it. `position_ms` is the position when the
/// view was made; while `playing`, the player runs on from there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchPartyView {
    pub channel_id: u32,
    pub video_id: String,
    pub playing: bool,
    pub position_ms: u32,
    /// Sessions that joined, sorted.
    pub participants: Vec<u32>,
    pub last_action: Option<WatchAction>,
    /// Whether the user joined; otherwise the party is only an invitation.
    pub joined: bool,
    /// Entry id of the video playing; 0 for the one the party started with.
    pub current_entry: u32,
    /// What plays next, in order.
    pub playlist: Vec<PlaylistEntry>,
    /// Who skipped to the current video, until the next playback command.
    pub skipped_by: Option<u32>,
    /// Where each participant last reported being, sorted by session.
    pub positions: Vec<ParticipantPosition>,
    /// Who started the party (the crown in the watching list, issue #163).
    pub host: u32,
}

/// What travels over plugin data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Wire {
    v: u32,
    #[serde(flatten)]
    msg: Msg,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
enum Msg {
    /// The sender started a party for this video, paused at the start.
    Start {
        video: String,
    },
    Join,
    Leave,
    Cmd {
        cmd: WatchCommand,
    },
    /// The full party for someone who came into the channel later.
    State {
        video: String,
        playing: bool,
        pos: u32,
        starter: u32,
        members: Vec<u32>,
        #[serde(default)]
        last: Option<LastAction>,
        /// Entry id of the video (version 2).
        #[serde(default)]
        cur: u32,
        /// Who skipped to it (version 2).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skip: Option<u32>,
    },
    /// Playlist edits, bundled (version 2).
    Pl {
        ops: Vec<Op>,
    },
    /// The sender went on from entry `from` to entry `id` playing `video` (version 2).
    Next {
        from: u32,
        id: u32,
        video: String,
        #[serde(default)]
        skip: bool,
    },
    /// Where the sender's player is (version 3, issue #163).
    Pos {
        pos: u32,
        play: bool,
        buf: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct LastAction {
    by: u32,
    cmd: WatchCommand,
}

fn encode_ops(ops: &[Op]) -> Vec<u8> {
    encode(Msg::Pl { ops: ops.to_vec() })
}

fn encode(msg: Msg) -> Vec<u8> {
    serde_json::to_vec(&Wire { v: VERSION, msg }).unwrap_or_default()
}

fn decode(data: &[u8]) -> Option<Msg> {
    if data.len() > PLUGIN_DATA_MAX {
        return None;
    }
    let wire: Wire = serde_json::from_slice(data).ok()?;
    (wire.v >= 1).then_some(wire.msg)
}

/// Whether `id` looks like a YouTube video id; nothing else is ever put into a player URL.
pub fn valid_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Parties per connected server; only the one in the user's own channel is kept.
pub(crate) type ServerParties = Mutex<HashMap<ServerId, Party>>;

#[derive(Debug, Clone)]
pub(crate) struct Party {
    channel_id: u32,
    video_id: String,
    /// Who started it; of two parties started at once, the lower starter wins everywhere.
    starter: u32,
    playing: bool,
    /// Position at `at`.
    position_ms: u32,
    at: Instant,
    participants: BTreeSet<u32>,
    last_action: Option<WatchAction>,
    joined: bool,
    current: u32,
    playlist: Playlist,
    skipped_by: Option<u32>,
    /// The current video ended with nothing queued; the next video added starts at once.
    ended: bool,
    /// Newcomers this client already sent the state to, with when to send it once more;
    /// cleared when they leave the channel.
    announced: BTreeMap<u32, Option<Instant>>,
    /// What each participant last said about its own player (issue #163).
    reports: BTreeMap<u32, Report>,
}

/// What one player last said about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Report {
    position_ms: u32,
    at: Instant,
    playing: bool,
    buffering: bool,
}

impl Party {
    fn new(channel_id: u32, video_id: String, starter: u32) -> Self {
        Self {
            channel_id,
            video_id,
            starter,
            playing: false,
            position_ms: 0,
            at: Instant::now(),
            participants: BTreeSet::from([starter]),
            last_action: None,
            joined: false,
            current: 0,
            playlist: Playlist::default(),
            skipped_by: None,
            ended: false,
            announced: BTreeMap::new(),
            reports: BTreeMap::new(),
        }
    }

    fn position_now(&self) -> u32 {
        if !self.playing {
            return self.position_ms;
        }
        let elapsed = u32::try_from(self.at.elapsed().as_millis()).unwrap_or(u32::MAX);
        self.position_ms.saturating_add(elapsed)
    }

    fn apply(&mut self, session: u32, command: WatchCommand) {
        match command {
            WatchCommand::Play { .. } => self.playing = true,
            WatchCommand::Pause { .. } => self.playing = false,
            WatchCommand::Seek { .. } => {}
        }
        self.position_ms = command.position_ms();
        self.at = Instant::now();
        self.last_action = Some(WatchAction { session, command });
        self.skipped_by = None;
        self.ended = false;
        // The command itself says where that player is.
        self.record(session, command.position_ms(), self.playing, false);
    }

    /// Remembers where `session` said its player is.
    fn record(&mut self, session: u32, position_ms: u32, playing: bool, buffering: bool) {
        if self.reports.len() >= MAX_PARTICIPANTS && !self.reports.contains_key(&session) {
            return;
        }
        self.reports.insert(
            session,
            Report {
                position_ms,
                at: Instant::now(),
                playing,
                buffering,
            },
        );
    }

    /// Goes on to entry `id`, playing from the start; `skipped_by` names who skipped.
    fn advance(&mut self, id: u32, video: String, skipped_by: Option<u32>) {
        self.current = id;
        self.video_id = video;
        self.playing = true;
        self.position_ms = 0;
        self.at = Instant::now();
        self.last_action = None;
        self.skipped_by = skipped_by;
        self.ended = false;
    }

    /// Goes on to the first playlist entry; the message tells the others. `None` when the
    /// playlist is empty.
    fn go_on(&mut self, skipped_by: Option<u32>) -> Option<Msg> {
        let from = self.current;
        let (id, video) = self.playlist.pop_front()?;
        self.advance(id, video.clone(), skipped_by);
        Some(Msg::Next {
            from,
            id,
            video,
            skip: skipped_by.is_some(),
        })
    }

    /// The current video ended with nothing queued: the party rests paused at its end instead of
    /// running on, so no player replays it and latecomers see it ended.
    fn finish(&mut self) {
        self.position_ms = self.position_now();
        self.playing = false;
        self.at = Instant::now();
        self.ended = true;
    }

    /// Starts the video just added when the party had ended (FR-2.69).
    fn resume(&mut self) -> Option<Msg> {
        if self.ended { self.go_on(None) } else { None }
    }

    fn add(&mut self, session: u32) {
        if self.participants.len() < MAX_PARTICIPANTS {
            self.participants.insert(session);
        }
    }

    /// Who tells `newcomer` about the party: the host while they are in it, otherwise the
    /// participant with the lowest session.
    fn answerer(&self, newcomer: u32) -> Option<u32> {
        if self.starter != newcomer && self.participants.contains(&self.starter) {
            return Some(self.starter);
        }
        self.participants.iter().copied().find(|s| *s != newcomer)
    }

    fn answers(&self, own: u32, newcomer: u32) -> bool {
        self.joined
            && !self.participants.contains(&newcomer)
            && self.answerer(newcomer) == Some(own)
    }

    /// Whether this client (`own`) announces the party to `newcomer` now; true at most once per
    /// stay of the newcomer in the channel.
    fn take_announce(&mut self, own: u32, newcomer: u32, now: Instant) -> bool {
        if !self.answers(own, newcomer) || self.announced.contains_key(&newcomer) {
            return false;
        }
        self.announced.insert(newcomer, Some(now + REANNOUNCE));
        true
    }

    /// The newcomers due for their second announcement at `now`, if this client still answers
    /// them; each comes up once.
    fn take_reannounce(&mut self, own: u32, now: Instant) -> Vec<u32> {
        let due: Vec<u32> = self
            .announced
            .iter_mut()
            .filter(|(_, at)| at.is_some_and(|at| at <= now))
            .map(|(newcomer, at)| {
                *at = None;
                *newcomer
            })
            .collect();
        due.into_iter().filter(|n| self.answers(own, *n)).collect()
    }

    fn view(&self) -> WatchPartyView {
        WatchPartyView {
            channel_id: self.channel_id,
            video_id: self.video_id.clone(),
            playing: self.playing,
            position_ms: self.position_now(),
            participants: self.participants.iter().copied().collect(),
            last_action: self.last_action,
            joined: self.joined,
            current_entry: self.current,
            playlist: self.playlist.view(),
            skipped_by: self.skipped_by,
            positions: self.positions(),
            host: self.starter,
        }
    }

    /// Each participant's last report, sorted by session, with its age when the view is made.
    fn positions(&self) -> Vec<ParticipantPosition> {
        let now = Instant::now();
        self.reports
            .iter()
            .map(|(session, report)| ParticipantPosition {
                session: *session,
                position_ms: report.position_ms,
                age_ms: u32::try_from(now.saturating_duration_since(report.at).as_millis())
                    .unwrap_or(u32::MAX),
                playing: report.playing,
                buffering: report.buffering,
            })
            .collect()
    }

    fn state_msg(&self) -> Msg {
        Msg::State {
            video: self.video_id.clone(),
            playing: self.playing,
            pos: self.position_now(),
            starter: self.starter,
            members: self.participants.iter().copied().collect(),
            last: self.last_action.map(|a| LastAction {
                by: a.session,
                cmd: a.command,
            }),
            cur: self.current,
            skip: self.skipped_by,
        }
    }
}

/// The own session and channel, with the other people in that channel.
struct Here {
    own: u32,
    channel_id: u32,
    others: Vec<u32>,
}

fn here(handle: &Handle) -> Option<Here> {
    handle.with_state(|s| {
        let own = s.session?;
        let channel_id = s.users.get(&own)?.channel_id;
        let others = s
            .users
            .values()
            .filter(|u| u.session != own && u.channel_id == channel_id)
            .map(|u| u.session)
            .collect();
        Some(Here {
            own,
            channel_id,
            others,
        })
    })
}

/// The names of `sessions` that are on the server.
fn names(handle: &Handle, sessions: impl Iterator<Item = u32>) -> HashMap<u32, String> {
    handle.with_state(|s| {
        sessions
            .filter_map(|session| Some((session, s.users.get(&session)?.name.clone())))
            .collect()
    })
}

/// The people in `sessions` that announced watch-party support on this server.
fn capable(peers: &ServerPeers, server_id: ServerId, sessions: &[u32]) -> Vec<u32> {
    let servers = lock(peers);
    let Some(known) = servers.get(&server_id) else {
        return Vec::new();
    };
    sessions
        .iter()
        .copied()
        .filter(|s| {
            known
                .features(*s)
                .is_some_and(|f| f.iter().any(|x| x == FEATURE))
        })
        .collect()
}

fn send(handle: &Handle, receivers: &[u32], msg: Msg) {
    if receivers.is_empty() {
        return;
    }
    if let Err(e) = handle.send_plugin_data(receivers, DATA_ID, encode(msg)) {
        tracing::debug!(error = %e, "watch party message not sent");
    }
}

fn emit(ctx: &SessionCtx, party: Option<&Party>) {
    ctx.events
        .emit(EngineEvent::WatchPartyChanged(party.map(Party::view)));
}

/// Drops the party of `server_id` (reconnect, disconnect); true when there was one.
pub(crate) fn reset(state: &ServerParties, server_id: ServerId) -> bool {
    lock(state).remove(&server_id).is_some()
}

/// A watch-party message arrived (session task).
pub(crate) fn on_plugin_data(state: &ServerParties, ctx: &SessionCtx, data: &PluginData) {
    if data.data_id != DATA_ID {
        return;
    }
    let Some(sender) = data.sender else {
        return;
    };
    let Some(here) = here(&ctx.handle) else {
        return;
    };
    // Only people in the own channel take part; anything else is stale or forged.
    if sender == here.own || !here.others.contains(&sender) {
        return;
    }
    let Some(msg) = decode(&data.data) else {
        tracing::debug!("ignoring a malformed watch party message");
        return;
    };
    // Resolved before the lock: who added the entries of a playlist edit.
    let adders = match &msg {
        Msg::Pl { ops } => names(
            &ctx.handle,
            ops.iter().map(|op| match op {
                Op::Add { by: Some(by), .. } => *by,
                _ => sender,
            }),
        ),
        _ => HashMap::new(),
    };
    let mut servers = lock(state);
    let current = servers.get(&ctx.server_id);
    let changed = match msg {
        Msg::Start { video } => {
            if !valid_video_id(&video) || current.is_some_and(|p| p.starter < sender) {
                return;
            }
            if let Some(old) = current
                && old.joined
            {
                tracing::info!("watch party replaced by one started at the same time");
            }
            let party = Party::new(here.channel_id, video, sender);
            servers.insert(ctx.server_id, party);
            true
        }
        Msg::State {
            video,
            playing,
            pos,
            starter,
            members,
            last,
            cur,
            skip,
        } => {
            // A state only fills in a party this client does not know yet; a second one is the
            // answerer repeating its announcement.
            if !valid_video_id(&video) || current.is_some() {
                tracing::debug!(sender, "watch party state ignored");
                return;
            }
            tracing::debug!(sender, "watch party state received");
            let mut party = Party::new(here.channel_id, video, starter);
            party.participants.clear();
            for m in members.into_iter().filter(|m| here.others.contains(m)) {
                party.add(m);
            }
            party.add(sender);
            party.playing = playing;
            party.position_ms = pos;
            party.last_action = last
                .filter(|l| l.by == here.own || here.others.contains(&l.by))
                .map(|l| WatchAction {
                    session: l.by,
                    command: l.cmd,
                });
            party.current = cur;
            party.skipped_by = skip.filter(|s| *s == here.own || here.others.contains(s));
            servers.insert(ctx.server_id, party);
            true
        }
        Msg::Join => match servers.get_mut(&ctx.server_id) {
            Some(party) => {
                party.add(sender);
                true
            }
            None => false,
        },
        Msg::Leave => remove_participant(&mut servers, ctx.server_id, sender),
        Msg::Cmd { cmd } => match servers.get_mut(&ctx.server_id) {
            Some(party) if party.participants.contains(&sender) => {
                party.apply(sender, cmd);
                true
            }
            _ => false,
        },
        Msg::Pos { pos, play, buf } => match servers.get_mut(&ctx.server_id) {
            Some(party) if party.participants.contains(&sender) => {
                party.record(sender, pos, play, buf);
                true
            }
            _ => false,
        },
        Msg::Pl { ops } => match servers.get_mut(&ctx.server_id) {
            Some(party) if party.participants.contains(&sender) => {
                party
                    .playlist
                    .apply(sender, ops, |s| adders.get(&s).cloned())
            }
            _ => false,
        },
        Msg::Next {
            from,
            id,
            video,
            skip,
        } => match servers.get_mut(&ctx.server_id) {
            // Only the first client to leave the current video moves the party on.
            Some(party)
                if party.participants.contains(&sender)
                    && party.current == from
                    && id != 0
                    && id != from
                    && valid_video_id(&video) =>
            {
                party.playlist.consumed(id);
                party.advance(id, video, skip.then_some(sender));
                true
            }
            _ => false,
        },
    };
    if changed {
        emit(ctx, servers.get(&ctx.server_id));
    }
}

/// Takes `session` out of the party; the party ends when nobody is left. True when it changed.
fn remove_participant(
    servers: &mut HashMap<ServerId, Party>,
    server_id: ServerId,
    session: u32,
) -> bool {
    let Some(party) = servers.get_mut(&server_id) else {
        return false;
    };
    if !party.participants.remove(&session) {
        return false;
    }
    party.reports.remove(&session);
    if party.participants.is_empty() {
        servers.remove(&server_id);
    }
    true
}

/// Someone left the server.
pub(crate) fn on_user_removed(state: &ServerParties, ctx: &SessionCtx, session: u32) {
    let mut servers = lock(state);
    if let Some(party) = servers.get_mut(&ctx.server_id) {
        party.announced.remove(&session);
    }
    if remove_participant(&mut servers, ctx.server_id, session) {
        emit(ctx, servers.get(&ctx.server_id));
    }
}

/// Someone (or the user) joined the server or moved. The user changing channel leaves the
/// party; another person leaving the channel leaves it too; a Mumcord user coming into the
/// channel gets the current state.
pub(crate) fn on_user_moved(
    state: &ServerParties,
    peers: &ServerPeers,
    ctx: &SessionCtx,
    session: u32,
) {
    let Some(here) = here(&ctx.handle) else {
        return;
    };
    let mut servers = lock(state);
    let Some(party) = servers.get_mut(&ctx.server_id) else {
        return;
    };
    if session == here.own {
        // The others see the move themselves and take the user out; no message needed.
        if party.channel_id != here.channel_id {
            servers.remove(&ctx.server_id);
            emit(ctx, None);
        }
        return;
    }
    if !here.others.contains(&session) {
        party.announced.remove(&session);
        if remove_participant(&mut servers, ctx.server_id, session) {
            emit(ctx, servers.get(&ctx.server_id));
        }
        return;
    }
    drop(servers);
    offer_state(state, peers, ctx, here.own, session);
}

/// Another client was recognised as a Mumcord user: if they are in the channel, they may have
/// missed the party (their hello came after their move).
pub(crate) fn on_peer_known(
    state: &ServerParties,
    peers: &ServerPeers,
    ctx: &SessionCtx,
    session: u32,
) {
    let Some(here) = here(&ctx.handle) else {
        return;
    };
    if here.others.contains(&session) {
        offer_state(state, peers, ctx, here.own, session);
    }
}

/// Sends the party to a Mumcord newcomer in the channel when this client is the one to answer.
fn offer_state(
    state: &ServerParties,
    peers: &ServerPeers,
    ctx: &SessionCtx,
    own: u32,
    session: u32,
) {
    // Checked before the lock: the hello may still be missing; `on_peer_known` retries then.
    if capable(peers, ctx.server_id, &[session]).is_empty() {
        return;
    }
    let mut servers = lock(state);
    let Some(party) = servers.get_mut(&ctx.server_id) else {
        return;
    };
    if !party.take_announce(own, session, Instant::now()) {
        return;
    }
    let party = party.clone();
    drop(servers);
    send_state(ctx, &party, session);
}

/// Sends the newcomers due for it the party once more (session tick).
pub(crate) fn on_tick(state: &ServerParties, ctx: &SessionCtx) {
    let Some(here) = here(&ctx.handle) else {
        return;
    };
    let mut servers = lock(state);
    let Some(party) = servers.get_mut(&ctx.server_id) else {
        return;
    };
    let due = party.take_reannounce(here.own, Instant::now());
    if due.is_empty() {
        return;
    }
    let party = party.clone();
    drop(servers);
    for session in due.into_iter().filter(|s| here.others.contains(s)) {
        send_state(ctx, &party, session);
    }
}

fn send_state(ctx: &SessionCtx, party: &Party, session: u32) {
    tracing::debug!(session, "watch party state sent");
    send(&ctx.handle, &[session], party.state_msg());
    // The list does not fit into the state; it follows as adds, a few messages at most.
    let mut ops = party.playlist.snapshot();
    while !ops.is_empty() {
        let rest = playlist::split_fitting(&mut ops, &encode_ops);
        let chunk = std::mem::replace(&mut ops, rest);
        send(&ctx.handle, &[session], Msg::Pl { ops: chunk });
    }
}

impl Engine {
    /// Starts a party for the YouTube video `video_id` in the user's channel on `server_id`
    /// and invites the Mumcord users there (FR-2.39). The user is in it right away.
    pub fn watch_party_start(
        &self,
        server_id: ServerId,
        video_id: &str,
    ) -> EngineResult<WatchPartyView> {
        if !valid_video_id(video_id) {
            return Err(EngineError::invalid("not a YouTube video id"));
        }
        let ctx = self.session_ctx_for(server_id)?;
        let here = here(&ctx.handle).ok_or_else(EngineError::not_connected)?;
        let view = {
            let mut servers = lock(self.watch_party_state());
            if servers
                .get(&server_id)
                .is_some_and(|p| p.channel_id == here.channel_id)
            {
                return Err(EngineError::invalid(
                    "a watch party is already running in this channel",
                ));
            }
            let mut party = Party::new(here.channel_id, video_id.to_owned(), here.own);
            party.joined = true;
            let view = party.view();
            servers.insert(server_id, party);
            view
        };
        let receivers = capable(self.mumcord_state(), server_id, &here.others);
        send(
            &ctx.handle,
            &receivers,
            Msg::Start {
                video: video_id.to_owned(),
            },
        );
        ctx.events
            .emit(EngineEvent::WatchPartyChanged(Some(view.clone())));
        Ok(view)
    }

    /// Joins the party the user was invited to; the player starts at the current position.
    pub fn watch_party_join(&self, server_id: ServerId) -> EngineResult<WatchPartyView> {
        let ctx = self.session_ctx_for(server_id)?;
        let here = here(&ctx.handle).ok_or_else(EngineError::not_connected)?;
        let view = {
            let mut servers = lock(self.watch_party_state());
            let party = servers
                .get_mut(&server_id)
                .filter(|p| p.channel_id == here.channel_id)
                .ok_or_else(no_party)?;
            party.joined = true;
            party.participants.insert(here.own);
            party.view()
        };
        let receivers = capable(self.mumcord_state(), server_id, &here.others);
        send(&ctx.handle, &receivers, Msg::Join);
        ctx.events
            .emit(EngineEvent::WatchPartyChanged(Some(view.clone())));
        Ok(view)
    }

    /// Plays, pauses or seeks for everyone in the party (FR-2.40). Invited people who did not
    /// join get it too, so they join at the right position.
    pub fn watch_party_command(
        &self,
        server_id: ServerId,
        command: WatchCommand,
    ) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let here = here(&ctx.handle).ok_or_else(EngineError::not_connected)?;
        let view = {
            let mut servers = lock(self.watch_party_state());
            let party = servers
                .get_mut(&server_id)
                .filter(|p| p.joined)
                .ok_or_else(no_party)?;
            party.apply(here.own, command);
            party.view()
        };
        let receivers = capable(self.mumcord_state(), server_id, &here.others);
        send(&ctx.handle, &receivers, Msg::Cmd { cmd: command });
        ctx.events.emit(EngineEvent::WatchPartyChanged(Some(view)));
        Ok(())
    }

    /// Where this client's player is (issue #163): remembered for the sync list and told to the
    /// other participants. The UI sends it when its player begins buffering and otherwise at most
    /// every ten seconds, so the party stays under the server's plugin data rate limit.
    pub fn watch_party_report(
        &self,
        server_id: ServerId,
        position_ms: u32,
        playing: bool,
        buffering: bool,
    ) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let here = here(&ctx.handle).ok_or_else(EngineError::not_connected)?;
        let view = {
            let mut servers = lock(self.watch_party_state());
            let party = servers
                .get_mut(&server_id)
                .filter(|p| p.joined)
                .ok_or_else(no_party)?;
            party.record(here.own, position_ms, playing, buffering);
            party.view()
        };
        let receivers = capable(self.mumcord_state(), server_id, &here.others);
        send(
            &ctx.handle,
            &receivers,
            Msg::Pos {
                pos: position_ms,
                play: playing,
                buf: buffering,
            },
        );
        ctx.events.emit(EngineEvent::WatchPartyChanged(Some(view)));
        Ok(())
    }

    /// Leaves the party. It stays an invitation while others are still in it.
    pub fn watch_party_leave(&self, server_id: ServerId) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let here = here(&ctx.handle).ok_or_else(EngineError::not_connected)?;
        let view = {
            let mut servers = lock(self.watch_party_state());
            let Some(party) = servers.get_mut(&server_id).filter(|p| p.joined) else {
                return Ok(());
            };
            party.joined = false;
            remove_participant(&mut servers, server_id, here.own);
            servers.get(&server_id).map(Party::view)
        };
        let receivers = capable(self.mumcord_state(), server_id, &here.others);
        send(&ctx.handle, &receivers, Msg::Leave);
        ctx.events.emit(EngineEvent::WatchPartyChanged(view));
        Ok(())
    }

    /// Adds a YouTube video to the end of the party's playlist (FR-2.69); returns its entry id.
    pub fn watch_party_add(&self, server_id: ServerId, video_id: &str) -> EngineResult<u32> {
        if !valid_video_id(video_id) {
            return Err(EngineError::invalid("not a YouTube video id"));
        }
        let ctx = self.session_ctx_for(server_id)?;
        let here = here(&ctx.handle).ok_or_else(EngineError::not_connected)?;
        let name = names(&ctx.handle, std::iter::once(here.own)).remove(&here.own);
        let (id, next) = self.playlist_edit(&ctx, |party| {
            let id = party
                .playlist
                .add(here.own, name, video_id)
                .ok_or_else(|| EngineError::invalid("the playlist is full"))?;
            Ok((id, party.resume()))
        })?;
        if let Some(msg) = next {
            let receivers = capable(self.mumcord_state(), server_id, &here.others);
            send(&ctx.handle, &receivers, msg);
        }
        Ok(id)
    }

    /// Removes an entry from the party's playlist.
    pub fn watch_party_remove(&self, server_id: ServerId, entry: u32) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        self.playlist_edit(&ctx, |party| {
            if party.playlist.remove(entry) {
                Ok(())
            } else {
                Err(no_entry())
            }
        })
    }

    /// Moves an entry of the party's playlist to `index` (0 is next).
    pub fn watch_party_move(
        &self,
        server_id: ServerId,
        entry: u32,
        index: usize,
    ) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let here = here(&ctx.handle).ok_or_else(EngineError::not_connected)?;
        self.playlist_edit(&ctx, |party| {
            if party.playlist.move_to(here.own, entry, index) {
                Ok(())
            } else {
                Err(no_entry())
            }
        })
    }

    /// Goes on from entry `from` to the first playlist entry for everyone: when that video ended,
    /// or at once with `skip` ("Anna skipped"). Nothing happens when the party already left
    /// `from`; at the end of a video with an empty playlist the party rests paused at its end.
    pub fn watch_party_next(&self, server_id: ServerId, from: u32, skip: bool) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let here = here(&ctx.handle).ok_or_else(EngineError::not_connected)?;
        let (msg, view) = {
            let mut servers = lock(self.watch_party_state());
            let party = servers
                .get_mut(&server_id)
                .filter(|p| p.joined)
                .ok_or_else(no_party)?;
            if party.current != from {
                return Ok(());
            }
            let msg = party.go_on(skip.then_some(here.own));
            if msg.is_none() {
                if skip {
                    return Err(EngineError::invalid("the playlist is empty"));
                }
                if party.ended {
                    return Ok(());
                }
                party.finish();
            }
            (msg, party.view())
        };
        if let Some(msg) = msg {
            let receivers = capable(self.mumcord_state(), server_id, &here.others);
            send(&ctx.handle, &receivers, msg);
        }
        ctx.events.emit(EngineEvent::WatchPartyChanged(Some(view)));
        Ok(())
    }

    /// Runs a playlist edit on the joined party; the edit leaves with the next bundle.
    fn playlist_edit<T>(
        &self,
        ctx: &SessionCtx,
        edit: impl FnOnce(&mut Party) -> EngineResult<T>,
    ) -> EngineResult<T> {
        let (out, view, start) = {
            let mut servers = lock(self.watch_party_state());
            let party = servers
                .get_mut(&ctx.server_id)
                .filter(|p| p.joined)
                .ok_or_else(no_party)?;
            let out = edit(party)?;
            let start = !party.playlist.flushing;
            party.playlist.flushing = true;
            (out, party.view(), start)
        };
        ctx.events.emit(EngineEvent::WatchPartyChanged(Some(view)));
        if start {
            self.spawn_playlist_flush(ctx.server_id);
        }
        Ok(out)
    }

    /// Sends the waiting playlist edits, at most one message per second, until none are left.
    fn spawn_playlist_flush(&self, server_id: ServerId) {
        let engine = self.clone();
        self.runtime().spawn(async move {
            loop {
                let step = {
                    let mut servers = lock(engine.watch_party_state());
                    let Some(party) = servers.get_mut(&server_id) else {
                        return;
                    };
                    let step = party.playlist.flush(Instant::now(), encode_ops);
                    if matches!(step, Flush::Idle | Flush::Send { more: false, .. }) {
                        party.playlist.flushing = false;
                    }
                    step
                };
                match step {
                    Flush::Idle => return,
                    Flush::Wait(wait) => tokio::time::sleep(wait).await,
                    Flush::Send { ops, more } => {
                        engine.send_to_channel(server_id, Msg::Pl { ops });
                        if !more {
                            return;
                        }
                    }
                }
            }
        });
    }

    /// Sends `msg` to the Mumcord users in the user's channel.
    fn send_to_channel(&self, server_id: ServerId, msg: Msg) {
        let Ok(ctx) = self.session_ctx_for(server_id) else {
            return;
        };
        let Some(here) = here(&ctx.handle) else {
            return;
        };
        let receivers = capable(self.mumcord_state(), server_id, &here.others);
        send(&ctx.handle, &receivers, msg);
    }

    /// The party in the user's channel on `server_id`, if any.
    pub fn watch_party(&self, server_id: ServerId) -> Option<WatchPartyView> {
        lock(self.watch_party_state())
            .get(&server_id)
            .map(Party::view)
    }
}

fn no_party() -> EngineError {
    EngineError::new(
        crate::session::ErrorKind::NotFound,
        "no watch party in this channel",
    )
}

fn no_entry() -> EngineError {
    EngineError::new(
        crate::session::ErrorKind::NotFound,
        "no such playlist entry",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_and_fit() {
        let msgs = [
            Msg::Start {
                video: "dQw4w9WgXcQ".into(),
            },
            Msg::Join,
            Msg::Leave,
            Msg::Cmd {
                cmd: WatchCommand::Seek {
                    position_ms: 61_000,
                },
            },
            Msg::State {
                video: "dQw4w9WgXcQ".into(),
                playing: true,
                pos: u32::MAX,
                starter: 3,
                members: (0..u32::try_from(MAX_PARTICIPANTS).expect("small"))
                    .map(|i| u32::MAX - i)
                    .collect(),
                last: Some(LastAction {
                    by: u32::MAX,
                    cmd: WatchCommand::Pause {
                        position_ms: u32::MAX,
                    },
                }),
                cur: u32::MAX,
                skip: Some(u32::MAX),
            },
            Msg::Next {
                from: u32::MAX,
                id: u32::MAX,
                video: "dQw4w9WgXcQ".into(),
                skip: true,
            },
            Msg::Pos {
                pos: u32::MAX,
                play: true,
                buf: true,
            },
            Msg::Pl {
                ops: vec![
                    Op::Rm { id: u32::MAX },
                    Op::Mv {
                        id: u32::MAX,
                        k: -0.123_456_789_012_345_6,
                        c: u32::MAX,
                    },
                ],
            },
        ];
        for msg in msgs {
            let data = encode(msg.clone());
            assert!(data.len() <= PLUGIN_DATA_MAX, "{} bytes", data.len());
            assert_eq!(decode(&data), Some(msg));
        }
    }

    #[test]
    fn wire_format_is_stable() {
        let data = encode(Msg::Cmd {
            cmd: WatchCommand::Pause { position_ms: 1500 },
        });
        assert_eq!(
            std::str::from_utf8(&data).expect("utf-8"),
            r#"{"v":3,"t":"cmd","cmd":{"kind":"pause","position_ms":1500}}"#
        );
        let data = encode(Msg::Pl {
            ops: vec![Op::Add {
                id: 7,
                v: "dQw4w9WgXcQ".into(),
                k: 1.5,
                c: 3,
                by: None,
            }],
        });
        assert_eq!(
            std::str::from_utf8(&data).expect("utf-8"),
            r#"{"v":3,"t":"pl","ops":[{"o":"add","id":7,"v":"dQw4w9WgXcQ","k":1.5,"c":3}]}"#
        );
    }

    #[test]
    fn reports_keep_the_sync_state_of_every_player() {
        let mut party = Party::new(1, "dQw4w9WgXcQ".into(), 5);
        party.record(5, 4000, true, false);
        party.record(7, 1200, true, true);
        let view = party.view();
        assert_eq!(
            view.positions,
            [
                ParticipantPosition {
                    session: 5,
                    position_ms: 4000,
                    age_ms: 0,
                    playing: true,
                    buffering: false,
                },
                ParticipantPosition {
                    session: 7,
                    position_ms: 1200,
                    age_ms: 0,
                    playing: true,
                    buffering: true,
                },
            ],
            "sorted by session, so every client lists them the same way"
        );
        // A command is a report too, and a player that left is gone from the list.
        party.apply(7, WatchCommand::Pause { position_ms: 1300 });
        assert_eq!(party.view().positions.len(), 2);
        assert_eq!(party.view().positions[1].buffering, false);
        party.add(9);
        assert_eq!(
            party.view().positions.len(),
            2,
            "only players that reported show up"
        );
    }

    #[test]
    fn version_1_states_still_read() {
        let state = decode(
            br#"{"v":1,"t":"state","video":"dQw4w9WgXcQ","playing":true,"pos":5,"starter":2,"members":[2]}"#,
        );
        let Some(Msg::State { cur, skip, .. }) = state else {
            panic!("not a state: {state:?}");
        };
        assert_eq!((cur, skip), (0, None));
        assert_eq!(
            decode(br#"{"v":2,"t":"pl","ops":[{"o":"wipe"}]}"#),
            None,
            "an unknown edit drops the message"
        );
    }

    #[test]
    fn advancing_starts_the_next_video_from_the_top() {
        let mut party = Party::new(1, "dQw4w9WgXcQ".into(), 5);
        party.apply(5, WatchCommand::Pause { position_ms: 9000 });
        party.advance(42, "a-b_c-d_e-f".into(), Some(6));
        let view = party.view();
        assert_eq!(
            (view.current_entry, view.video_id.as_str(), view.playing),
            (42, "a-b_c-d_e-f", true)
        );
        assert_eq!((view.last_action, view.skipped_by), (None, Some(6)));
        party.apply(5, WatchCommand::Pause { position_ms: 10 });
        assert_eq!(
            party.view().skipped_by,
            None,
            "a later command replaces the line"
        );
    }

    #[test]
    fn newer_versions_are_read_and_broken_payloads_rejected() {
        assert_eq!(
            decode(br#"{"v":4,"t":"join","extra":true}"#),
            Some(Msg::Join)
        );
        assert_eq!(decode(br#"{"v":0,"t":"join"}"#), None);
        assert_eq!(decode(br#"{"v":1,"t":"explode"}"#), None);
        assert_eq!(decode(br#"{"v":1,"t":"cmd","cmd":{"kind":"stop"}}"#), None);
        assert_eq!(decode(b"nope"), None);
        let mut big = br#"{"v":1,"t":"join","pad":""#.to_vec();
        big.resize(PLUGIN_DATA_MAX + 1, b'x');
        assert_eq!(decode(&big), None);
    }

    #[test]
    fn only_youtube_ids_are_valid() {
        assert!(valid_video_id("dQw4w9WgXcQ"));
        assert!(valid_video_id("a-b_c-d_e-f"));
        assert!(!valid_video_id("dQw4w9WgXc"));
        assert!(!valid_video_id("dQw4w9WgXcQQ"));
        assert!(!valid_video_id("dQw4w9W/XcQ"));
        assert!(!valid_video_id("javascript:"));
        assert!(!valid_video_id("dQw4w9WgXc\u{e9}"));
    }

    #[test]
    fn commands_move_the_position_and_remember_who() {
        let mut party = Party::new(1, "dQw4w9WgXcQ".into(), 5);
        party.apply(7, WatchCommand::Pause { position_ms: 9000 });
        let view = party.view();
        assert!(!view.playing);
        assert_eq!(view.position_ms, 9000, "a paused party does not run on");
        assert_eq!(
            view.last_action,
            Some(WatchAction {
                session: 7,
                command: WatchCommand::Pause { position_ms: 9000 }
            })
        );
        party.apply(5, WatchCommand::Seek { position_ms: 100 });
        assert!(!party.playing, "seek keeps the paused state");
        party.apply(5, WatchCommand::Play { position_ms: 100 });
        party.at -= std::time::Duration::from_secs(2);
        assert!(
            party.view().position_ms >= 2100,
            "a latecomer starts further on"
        );
    }

    #[test]
    fn the_party_ends_with_its_last_participant() {
        let mut servers = HashMap::from([(1, Party::new(1, "dQw4w9WgXcQ".into(), 5))]);
        servers.get_mut(&1).expect("party").add(6);
        assert!(remove_participant(&mut servers, 1, 5));
        assert!(!remove_participant(&mut servers, 1, 5));
        assert!(servers.contains_key(&1));
        assert!(remove_participant(&mut servers, 1, 6));
        assert!(!servers.contains_key(&1));
    }

    #[test]
    fn a_new_party_starts_paused_at_zero_and_later_videos_play() {
        let mut party = Party::new(1, "dQw4w9WgXcQ".into(), 5);
        let view = party.view();
        assert!(!view.playing, "others can join before anyone presses Play");
        assert_eq!(view.position_ms, 0);
        party.advance(1, "9bZkp7q19f0".into(), None);
        assert!(party.view().playing, "the next video plays right away");
    }

    #[test]
    fn an_ended_video_with_nothing_queued_rests_paused_at_its_end() {
        let mut party = Party::new(1, "dQw4w9WgXcQ".into(), 5);
        party.apply(5, WatchCommand::Play { position_ms: 0 });
        party.at -= std::time::Duration::from_secs(30);
        assert!(party.go_on(None).is_none(), "nothing queued");
        party.finish();
        party.at -= std::time::Duration::from_secs(10);
        let view = party.view();
        assert!(!view.playing, "no replay");
        assert!(
            (30_000..31_000).contains(&view.position_ms),
            "stays at the end"
        );
        let Msg::State { playing, pos, .. } = party.state_msg() else {
            panic!("state");
        };
        assert!(!playing, "a latecomer sees it paused");
        assert_eq!(pos, view.position_ms, "at the end");
    }

    #[test]
    fn a_video_added_after_the_end_starts_at_once() {
        let mut party = Party::new(1, "dQw4w9WgXcQ".into(), 5);
        party.apply(5, WatchCommand::Play { position_ms: 0 });
        party.playlist.add(6, None, "9bZkp7q19f0").expect("added");
        assert!(party.resume().is_none(), "a running video is not cut off");
        assert_eq!(party.current, 0);
        let mut party = Party::new(1, "dQw4w9WgXcQ".into(), 5);
        party.finish();
        let id = party.playlist.add(6, None, "9bZkp7q19f0").expect("added");
        let Some(Msg::Next {
            from,
            id: next,
            video,
            skip,
        }) = party.resume()
        else {
            panic!("goes on");
        };
        assert_eq!(
            (from, next, video.as_str(), skip),
            (0, id, "9bZkp7q19f0", false)
        );
        let view = party.view();
        assert_eq!(
            (view.current_entry, view.video_id.as_str()),
            (id, "9bZkp7q19f0")
        );
        assert!(view.playing, "plays for everyone");
        assert!(view.position_ms < 1000, "from the start");
        assert!(view.playlist.is_empty(), "it left the list");
        assert!(party.resume().is_none(), "only once");
    }

    #[test]
    fn participants_are_capped() {
        let mut party = Party::new(1, "dQw4w9WgXcQ".into(), 0);
        for s in 1..100 {
            party.add(s);
        }
        assert_eq!(party.participants.len(), MAX_PARTICIPANTS);
    }

    #[test]
    fn the_host_answers_newcomers_while_in_the_party() {
        let mut party = Party::new(1, "dQw4w9WgXcQ".into(), 7);
        party.add(3);
        party.add(5);
        assert_eq!(
            party.answerer(9),
            Some(7),
            "the host, not the lowest session"
        );
        assert_eq!(
            party.answerer(1),
            Some(7),
            "a lower newcomer changes nothing"
        );
    }

    #[test]
    fn without_the_host_the_lowest_session_answers() {
        let mut party = Party::new(1, "dQw4w9WgXcQ".into(), 7);
        party.add(5);
        party.add(3);
        party.participants.remove(&7);
        assert_eq!(party.answerer(9), Some(3));
        assert_eq!(party.answerer(3), Some(5), "never the newcomer itself");
        party.participants.clear();
        assert_eq!(party.answerer(9), None);
    }

    #[test]
    fn a_newcomer_is_announced_once_per_stay_by_the_answerer_only() {
        let now = Instant::now();
        let mut host = Party::new(1, "dQw4w9WgXcQ".into(), 7);
        host.add(3);
        host.joined = true;
        let mut other = host.clone();
        assert!(!other.take_announce(3, 9, now), "only the host answers");
        assert!(host.take_announce(7, 9, now));
        assert!(!host.take_announce(7, 9, now), "once per newcomer");
        assert!(
            !host.take_announce(7, 3, now),
            "participants are not announced to"
        );
        host.announced.remove(&9);
        assert!(
            host.take_announce(7, 9, now),
            "again after leaving and coming back"
        );

        let mut invited = Party::new(1, "dQw4w9WgXcQ".into(), 7);
        invited.participants.clear();
        invited.add(3);
        assert!(
            !invited.take_announce(3, 9, now),
            "a client not in the party never answers"
        );
    }

    #[test]
    fn a_newcomer_is_announced_once_more_after_a_while() {
        let now = Instant::now();
        let mut host = Party::new(1, "dQw4w9WgXcQ".into(), 7);
        host.joined = true;
        assert!(host.take_announce(7, 9, now));
        assert!(host.take_announce(7, 8, now + Duration::from_secs(1)));
        assert!(host.take_reannounce(7, now).is_empty(), "not yet");
        assert_eq!(host.take_reannounce(7, now + REANNOUNCE), [9]);
        assert!(
            host.take_reannounce(7, now + REANNOUNCE * 2).contains(&8),
            "each newcomer on its own time"
        );
        assert!(
            host.take_reannounce(7, now + REANNOUNCE * 3).is_empty(),
            "only once"
        );
        assert!(!host.take_announce(7, 9, now), "still once per stay");

        host.announced.remove(&9);
        assert!(host.take_announce(7, 9, now));
        host.add(9);
        assert!(
            host.take_reannounce(7, now + REANNOUNCE).is_empty(),
            "not to someone who joined meanwhile"
        );
    }
}
