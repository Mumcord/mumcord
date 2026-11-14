//! Kick and ban of a person (Epic 4, issue #253) and the admin actions a slash command runs
//! (issue #378): server mute, deafen, priority speaker, registering a guest and moving a whole
//! channel at once.
//!
//! A kick is the standard `UserRemove` message, so the person can reconnect afterwards. A precise
//! ban needs the server's ban list: `UserRemove` with `ban = true` always takes the server's own
//! default length and both targets at once, so a chosen duration and the "Ban by" targets only work
//! through the list (FR-10.12). When the list cannot be read the dialog falls back to that message
//! (FR-10.13). The rights are the server's own answers (FR-10.1); the server has the final say.
//!
//! Nothing here goes to disk (FR-10.63): what a dialog holds lives in memory for the connection. The
//! person's address comes from `UserStats`, which standard servers send only to admins (FR-10.65).
//! Logs name the kind of action and its outcome, never names, addresses, fingerprints or reasons
//! (FR-10.64).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Duration;

use mumble_client_protocol::PermissionDenied;
use mumble_client_protocol::{CommandError, ban, permissions, proto};
use tokio::sync::oneshot;

use crate::ServerId;
use crate::banlist::{self, BanChange, BanView};
use crate::session::{Engine, EngineError, EngineResult, ErrorKind, SessionCtx, convert, lock};

/// The root channel, where the Kick and Ban rights are checked (FR-10.1).
const ROOT: u32 = 0;
/// How long a dialog waits for the server's answer, as the ban list read does (FR-10.13).
pub const ANSWER_WAIT: Duration = banlist::ANSWER_WAIT;
/// The flood limit of a standard server: this many moves go out at once, the rest one per second
/// (FR-10.18; spike S2 may still change these, section 13).
const MOVE_BURST: usize = 5;
/// How long a mass move waits between two moves after the burst (FR-10.18).
const MOVE_GAP: Duration = Duration::from_secs(1);
/// How long a mass move waits after its last message to see who arrived (FR-10.18).
const ARRIVAL_WAIT: Duration = Duration::from_secs(5);

/// What the kick or ban dialog needs to know about a person before it can be filled.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PersonView {
    pub session: u32,
    /// Address the server reported in `UserStats`, which it sends only to admins (FR-10.65).
    pub address: Option<IpAddr>,
    /// Seconds the person has been connected, when the server reported it.
    pub online_secs: Option<u64>,
    /// Whether the ban list can be read, which decides precise ban or the quick fallback (FR-10.13).
    pub ban_list_readable: bool,
    /// The person's certificate hash, from the server's user list; `None` for a guest.
    pub cert_hash: Option<String>,
    /// The two rights at the root channel, so the dialog offers only what the server grants
    /// (FR-10.1, FR-10.2).
    pub can_kick: bool,
    pub can_ban: bool,
}

/// What the ban dialog sends (FR-10.12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BanRequest {
    pub reason: Option<String>,
    /// Seconds; 0 is permanent. Ignored by the quick ban, which uses the server's own length.
    pub duration_secs: u32,
    pub by_cert: bool,
    pub by_ip: bool,
    /// Subnet length in the family of the address: 32 for a single IPv4, 128 for a single IPv6.
    pub mask: u32,
    /// "Remove from server now": after saving, the person is kicked with the same reason.
    pub remove_now: bool,
    /// The dialog's "Save mine anyway" after a conflict (FR-10.66).
    pub force: bool,
    /// The quick-ban path, which the dialog can only take when the list cannot be read (FR-10.13).
    pub quick: bool,
}

/// What an action on a person produced. `Confirmed` means the server answered with the change it
/// was asked for: the person's removal, or the echo of their new state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionOutcome {
    Confirmed,
    /// The server answered nothing within [`ANSWER_WAIT`]; the message was written to it.
    Unconfirmed,
}

/// What a mass move produced (FR-10.17, FR-10.18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MassMoveOutcome {
    /// All moves were sent; of `total` people, `moved` arrived in the target channel within the
    /// grace period. The rest were refused by the server or left on their own.
    Done { moved: u32, total: u32 },
    /// Another mass move is still running on this server (FR-10.18).
    AlreadyRunning,
}

/// What the open wait for one person expects from the server. The right it names is the one a
/// `PermissionDenied` answer carries, so a refusal reaches the dialog or command that asked
/// (FR-10.59) and not one that happens to wait for the same person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pending {
    /// `UserRemove` (kick, ban): answered by the person's removal.
    Removal,
    /// A change of the person's own state: answered by the echo of their `UserState`.
    State(u32),
}

/// What a ban produced (FR-10.12, FR-10.13, FR-10.66).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BanOutcome {
    /// One entry added to the server's list with the chosen length and targets.
    Saved { key: String, removed: bool },
    /// The server's own "remove with ban" message: its default length, certificate and address.
    QuickBanned,
    /// The entry the dialog wanted to add is different on the server already (FR-10.66).
    Conflict { key: String, fresh: Vec<BanView> },
}

/// The part of a `UserStats` answer the ban dialog uses.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatsAnswer {
    pub address: Option<IpAddr>,
    pub online_secs: Option<u64>,
}

/// What the open dialogs of one connection hold.
#[derive(Default)]
struct Slot {
    /// Addresses the server reported for people of this server (FR-10.65).
    address: HashMap<u32, IpAddr>,
    /// The dialog waiting for a `UserStats` answer.
    stats: HashMap<u32, oneshot::Sender<StatsAnswer>>,
    /// The dialog or command waiting for the server's answer on a person, with what it expects.
    actions: HashMap<u32, (Pending, oneshot::Sender<Result<(), EngineError>>)>,
    /// One mass move runs per server at a time (FR-10.18).
    mass_move: bool,
}

/// In-memory kick and ban state, one slot per connection.
#[derive(Default)]
pub struct Admin {
    slots: HashMap<ServerId, Slot>,
}

impl Admin {
    fn expect_stats(
        &mut self,
        server_id: ServerId,
        session: u32,
    ) -> oneshot::Receiver<StatsAnswer> {
        let (tx, rx) = oneshot::channel();
        self.slots
            .entry(server_id)
            .or_default()
            .stats
            .insert(session, tx);
        rx
    }

    fn expect_action(
        &mut self,
        server_id: ServerId,
        session: u32,
        pending: Pending,
    ) -> oneshot::Receiver<Result<(), EngineError>> {
        let (tx, rx) = oneshot::channel();
        self.slots
            .entry(server_id)
            .or_default()
            .actions
            .insert(session, (pending, tx));
        rx
    }

    fn answer_stats(&mut self, server_id: ServerId, session: u32, answer: StatsAnswer) {
        let slot = self.slots.entry(server_id).or_default();
        if let Some(address) = answer.address {
            slot.address.insert(session, address);
        }
        if let Some(tx) = slot.stats.remove(&session) {
            let _ = tx.send(answer);
        }
    }

    /// Answers the open wait for `session`, when it is waiting for this kind of answer.
    fn complete<F: Fn(Pending) -> bool>(
        &mut self,
        server_id: ServerId,
        session: u32,
        outcome: Result<(), EngineError>,
        accepts: F,
    ) {
        let Some(slot) = self.slots.get_mut(&server_id) else {
            return;
        };
        if !slot
            .actions
            .get(&session)
            .is_some_and(|(pending, _)| accepts(*pending))
        {
            return;
        }
        if let Some((_, tx)) = slot.actions.remove(&session) {
            let _ = tx.send(outcome);
        }
    }

    /// Takes the mass-move slot; false when one already runs (FR-10.18).
    fn begin_mass_move(&mut self, server_id: ServerId) -> bool {
        let slot = self.slots.entry(server_id).or_default();
        if slot.mass_move {
            return false;
        }
        slot.mass_move = true;
        true
    }

    fn end_mass_move(&mut self, server_id: ServerId) {
        if let Some(slot) = self.slots.get_mut(&server_id) {
            slot.mass_move = false;
        }
    }

    fn address(&self, server_id: ServerId, session: u32) -> Option<IpAddr> {
        self.slots.get(&server_id)?.address.get(&session).copied()
    }

    fn forget(&mut self, server_id: ServerId) {
        self.slots.remove(&server_id);
    }
}

impl Engine {
    /// The Kick right in the root channel, as the server's own answer says (FR-10.1).
    pub fn can_kick(&self, server_id: ServerId) -> EngineResult<bool> {
        root_right(self, server_id, permissions::KICK)
    }

    /// What the dialog needs about a person: their address, how long they are connected, whether
    /// the ban list can be read, and the two rights (FR-10.11, FR-10.12, FR-10.13).
    pub async fn admin_target(
        &self,
        server_id: ServerId,
        session: u32,
    ) -> EngineResult<PersonView> {
        let ctx = self.session_ctx_for(server_id)?;
        let can_kick = self.can_kick(server_id)?;
        let can_ban = self.can_ban(server_id)?;
        if !can_kick && !can_ban {
            return Err(denied("kick or ban right in the root channel"));
        }
        let answer = { lock(self.admin_state()).expect_stats(server_id, session) };
        ctx.handle
            .request_user_stats(session, false)
            .map_err(convert::command_error)?;
        let stats = match tokio::time::timeout(ANSWER_WAIT, answer).await {
            Ok(Ok(answer)) => answer,
            // The wait was dropped, which happens when the connection ends.
            Ok(Err(_)) => return Err(EngineError::not_connected()),
            Err(_) => StatsAnswer::default(),
        };
        let cert_hash = ctx.handle.with_state(|state| {
            state
                .users
                .get(&session)
                .and_then(|user| user.cert_hash.clone())
        });
        Ok(PersonView {
            session,
            address: stats.address,
            online_secs: stats.online_secs,
            ban_list_readable: can_ban && self.ban_list_open(server_id).await.is_ok(),
            cert_hash,
            can_kick,
            can_ban,
        })
    }

    /// Kicks a person with an optional reason, which they see (FR-10.11). Yourself is refused,
    /// because the dialog never offers it (issue #253).
    pub async fn kick_person(
        &self,
        server_id: ServerId,
        session: u32,
        reason: Option<&str>,
    ) -> EngineResult<ActionOutcome> {
        let ctx = self.session_ctx_for(server_id)?;
        if !self.can_kick(server_id)? {
            return Err(denied("kick right in the root channel"));
        }
        if is_own(&ctx, session) {
            return Err(EngineError::invalid("kicking yourself is not offered"));
        }
        let outcome = send_and_wait(&ctx, self.admin_state(), session, Pending::Removal, || {
            ctx.handle.remove_user(session, reason, false)
        })
        .await;
        report(&ctx, "kick", &outcome);
        outcome
    }

    /// Bans a person (FR-10.12): one entry added to the server's ban list with the safe save of
    /// FR-10.66, then the person is removed when the dialog asked for it. `quick` is the fallback
    /// for servers that do not show their list (FR-10.13).
    pub async fn ban_person(
        &self,
        server_id: ServerId,
        session: u32,
        request: &BanRequest,
    ) -> EngineResult<BanOutcome> {
        let ctx = self.session_ctx_for(server_id)?;
        if !self.can_ban(server_id)? {
            return Err(denied("ban right in the root channel"));
        }
        if is_own(&ctx, session) {
            return Err(EngineError::invalid("banning yourself is not offered"));
        }
        if !request.by_cert && !request.by_ip {
            return Err(EngineError::invalid("at least one target must stay on"));
        }

        if request.quick {
            let outcome =
                send_and_wait(&ctx, self.admin_state(), session, Pending::Removal, || {
                    ctx.handle
                        .remove_user(session, request.reason.as_deref(), true)
                })
                .await;
            report(&ctx, "quick ban", &outcome);
            return outcome.map(|_| BanOutcome::QuickBanned);
        }

        let hash = ctx.handle.with_state(|state| {
            state
                .users
                .get(&session)
                .and_then(|user| user.cert_hash.clone())
        });
        let name = ctx
            .handle
            .with_state(|state| state.users.get(&session).map(|user| user.name.clone()));
        let address = lock(self.admin_state()).address(server_id, session);
        // A ban entry always carries an address, so without one Mumcord cannot write a ban with its
        // own length and targets (issue #254, spike S1).
        let Some(address) = address else {
            return Err(EngineError::invalid(
                "the server did not report this person's address",
            ));
        };
        if request.by_cert && hash.is_none() {
            return Err(EngineError::invalid("this person has no certificate"));
        }
        let family = if address.is_ipv4() { 32 } else { 128 };
        if request.mask > family {
            return Err(EngineError::invalid(
                "the subnet mask cannot be longer than the address",
            ));
        }
        let entry = BanChange::Add {
            name,
            cert_hash: request.by_cert.then_some(hash.clone()).flatten(),
            address,
            mask: request.mask,
            reason: request.reason.clone(),
            duration_secs: request.duration_secs,
        };
        // The key is what identifies the entry across the re-read of FR-10.66.
        let key = format!("{address}/{}|{}", request.mask, hash.unwrap_or_default());
        match self
            .ban_list_save(server_id, &[entry], request.force)
            .await?
        {
            banlist::BanSaveOutcome::Conflict { key, fresh } => {
                return Ok(BanOutcome::Conflict { key, fresh });
            }
            banlist::BanSaveOutcome::Saved(_) => {}
        }
        if !request.remove_now || !self.can_kick(server_id).unwrap_or(false) {
            // Without the Kick right the entry stands on its own and the person stays: the dialog
            // says so in the toggle's reason (FR-10.2).
            return Ok(BanOutcome::Saved {
                key,
                removed: false,
            });
        }
        // `removed` is the server's own answer; without it the entry stands and the person is only
        // taken off on their next connect (issue #253).
        let confirmed = matches!(
            send_and_wait(&ctx, self.admin_state(), session, Pending::Removal, || ctx
                .handle
                .remove_user(session, request.reason.as_deref(), false),)
            .await,
            Ok(ActionOutcome::Confirmed)
        );
        tracing::info!(server_id, action = "ban", confirmed, "admin action: ban");
        Ok(BanOutcome::Saved {
            key,
            removed: confirmed,
        })
    }

    /// Server mute of another person (issue #378, FR-10.15): one `UserState` for their session,
    /// answered by the server echoing their new state.
    pub async fn person_set_server_mute(
        &self,
        server_id: ServerId,
        session: u32,
        muted: bool,
    ) -> EngineResult<ActionOutcome> {
        self.person_change_state(
            server_id,
            session,
            permissions::MUTE_DEAFEN,
            None,
            proto::UserState {
                mute: Some(muted),
                ..Default::default()
            },
            "server mute",
        )
        .await
    }

    /// Server deafen of another person (FR-10.15). Deafening always mutes too, as in Mumble
    /// (spec 10, A5); undeafening leaves the mute the admin set before.
    pub async fn person_set_server_deafen(
        &self,
        server_id: ServerId,
        session: u32,
        deaf: bool,
    ) -> EngineResult<ActionOutcome> {
        self.person_change_state(
            server_id,
            session,
            permissions::MUTE_DEAFEN,
            None,
            proto::UserState {
                mute: deaf.then_some(true),
                deaf: Some(deaf),
                ..Default::default()
            },
            "server deafen",
        )
        .await
    }

    /// Priority speaker of another person (issue #378, FR-10.15); the same right as mute and
    /// deafen.
    pub async fn person_set_priority(
        &self,
        server_id: ServerId,
        session: u32,
        priority: bool,
    ) -> EngineResult<ActionOutcome> {
        self.person_change_state(
            server_id,
            session,
            permissions::MUTE_DEAFEN,
            None,
            proto::UserState {
                priority_speaker: Some(priority),
                ..Default::default()
            },
            "priority speaker",
        )
        .await
    }

    /// Registers an online guest under their current name (issue #378, FR-10.20): `user_id = 0`
    /// is the standard "register this person" message. Only a guest can be registered, and the
    /// check is the Register right in the root channel.
    pub async fn person_register(
        &self,
        server_id: ServerId,
        session: u32,
    ) -> EngineResult<ActionOutcome> {
        let known = {
            let ctx = self.session_ctx_for(server_id)?;
            ctx.handle.with_state(|state| {
                state
                    .users
                    .get(&session)
                    .map(|user| (user.user_id.is_some(), user.cert_hash.is_some()))
            })
        };
        if let Some((registered, has_cert)) = known {
            if registered {
                return Err(EngineError::invalid("this person is registered already"));
            }
            // The account is bound to the client certificate (issue #256).
            if !has_cert {
                return Err(EngineError::invalid(
                    "registering needs a client certificate",
                ));
            }
        }
        self.person_change_state(
            server_id,
            session,
            permissions::REGISTER,
            Some(ROOT),
            proto::UserState {
                user_id: Some(0),
                ..Default::default()
            },
            "register",
        )
        .await
    }

    /// One change of another person's state (issue #378): `right` names the right the server
    /// checks, which is its answer for the person's own channel unless `channel` names another
    /// (the Register right sits in the root channel). Nothing is sent without that right
    /// (FR-10.1); the server has the final say either way.
    async fn person_change_state(
        &self,
        server_id: ServerId,
        session: u32,
        right: u32,
        channel: Option<u32>,
        mut change: proto::UserState,
        what: &str,
    ) -> EngineResult<ActionOutcome> {
        let ctx = self.session_ctx_for(server_id)?;
        let person_channel = ctx
            .handle
            .with_state(|state| state.users.get(&session).map(|user| user.channel_id));
        let Some(person_channel) = person_channel else {
            return Err(EngineError::new(ErrorKind::NotFound, "unknown person"));
        };
        if is_own(&ctx, session) {
            return Err(EngineError::invalid("this action never offers yourself"));
        }
        if !right_in(&ctx, channel.unwrap_or(person_channel), right) {
            return Err(denied(format!("{what} right in this channel")));
        }
        // The session names the person the server changes; without it a standard server would
        // apply the change to the sender (the admin) instead (issue #378).
        change.session = Some(session);
        let handle = &ctx.handle;
        let outcome = send_and_wait(
            &ctx,
            self.admin_state(),
            session,
            Pending::State(right),
            move || handle.send_admin_user_state(change),
        )
        .await;
        report(&ctx, what, &outcome);
        outcome
    }

    /// Moves everyone in the admin's own channel to `target`, including the admin when they are
    /// there (FR-10.17). The moves go out at the paced rate of FR-10.18 and, after a grace
    /// period, the count of those who really arrived is compared with the number of people that
    /// were there. At most one mass move runs per server at a time.
    pub async fn move_all(
        &self,
        server_id: ServerId,
        target: u32,
    ) -> EngineResult<MassMoveOutcome> {
        let ctx = self.session_ctx_for(server_id)?;
        let from = ctx.handle.with_state(|state| {
            let source = state
                .session
                .and_then(|own| state.users.get(&own))
                .map(|own| own.channel_id)?;
            let people = state
                .users
                .values()
                .filter(|user| user.channel_id == source)
                .map(|user| user.session)
                .collect::<Vec<u32>>();
            Some((source, people))
        });
        let Some((source, people)) = from else {
            return Err(EngineError::not_connected());
        };
        if !right_in(&ctx, source, permissions::MOVE) {
            return Err(denied("move right in this channel"));
        }
        let total = u32::try_from(people.len()).unwrap_or(u32::MAX);
        if source == target || people.is_empty() {
            return Ok(MassMoveOutcome::Done { moved: 0, total });
        }
        if !lock(self.admin_state()).begin_mass_move(server_id) {
            return Ok(MassMoveOutcome::AlreadyRunning);
        }
        for (index, session) in people.iter().enumerate() {
            if index >= MOVE_BURST {
                tokio::time::sleep(MOVE_GAP).await;
            }
            let _ = ctx.handle.move_user(*session, target);
        }
        tokio::time::sleep(ARRIVAL_WAIT).await;
        let moved = ctx.handle.with_state(|state| {
            people
                .iter()
                .filter(|session| {
                    state
                        .users
                        .get(*session)
                        .is_some_and(|user| user.channel_id == target)
                })
                .count()
        });
        lock(self.admin_state()).end_mass_move(server_id);
        // Only the counts (FR-10.64): no names, no channel names.
        tracing::info!(
            server_id,
            action = "mass move",
            moved,
            total,
            "admin action: mass move"
        );
        Ok(MassMoveOutcome::Done {
            moved: u32::try_from(moved).unwrap_or(u32::MAX),
            total,
        })
    }

    /// The dialog closed: what it held is dropped (FR-10.63).
    pub fn admin_close(&self, server_id: ServerId) {
        lock(self.admin_state()).forget(server_id);
    }
}

/// A server-side moderation action on one person (issue #257, FR-10.15, FR-10.19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModerateAction {
    /// Server mute on or off. Turning it off also lifts a server deafen, as in Mumble.
    Mute(bool),
    /// Server deafen on or off. Deafen implies mute, as in Mumble.
    Deafen(bool),
    /// Heard over everyone else in the channel.
    PrioritySpeaker(bool),
    /// Lets someone the channel's rules keep quiet speak anyway.
    Unsuppress,
    /// Clears their comment for everyone.
    ResetComment,
    /// Clears their avatar for everyone.
    ResetAvatar,
}

impl ModerateAction {
    fn message(self, session: u32) -> proto::UserState {
        let mut m = proto::UserState {
            session: Some(session),
            ..Default::default()
        };
        match self {
            Self::Mute(on) => {
                m.mute = Some(on);
                if !on {
                    m.deaf = Some(false);
                }
            }
            Self::Deafen(on) => {
                m.deaf = Some(on);
                if on {
                    m.mute = Some(true);
                }
            }
            Self::PrioritySpeaker(on) => m.priority_speaker = Some(on),
            Self::Unsuppress => m.suppress = Some(false),
            Self::ResetComment => m.comment = Some(String::new()),
            Self::ResetAvatar => m.texture = Some(Vec::new()),
        }
        m
    }

    fn name(self) -> &'static str {
        match self {
            Self::Mute(_) => "server mute",
            Self::Deafen(_) => "server deafen",
            Self::PrioritySpeaker(_) => "priority speaker",
            Self::Unsuppress => "unsuppress",
            Self::ResetComment => "reset comment",
            Self::ResetAvatar => "reset avatar",
        }
    }
}

impl Engine {
    /// Acts on a person at once (FR-10.15, FR-10.19). The server has the final say: success shows
    /// in the person's state, a refusal arrives as a `ServerDenied` notice with its reason. The
    /// menu offers each action only with the right (MuteDeafen in their channel, ResetUserContent
    /// in Root); Mumcord does not check it again, so a forced attempt shows the server's reason.
    pub fn person_moderate(
        &self,
        server_id: ServerId,
        session: u32,
        action: ModerateAction,
    ) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let known = ctx
            .handle
            .with_state(|state| state.users.contains_key(&session));
        if !known {
            return Err(EngineError::new(ErrorKind::NotFound, "unknown person"));
        }
        ctx.handle
            .send_admin_user_state(action.message(session))
            .map_err(convert::command_error)?;
        // Only the kind of action (FR-10.64): no names, comments or pictures.
        tracing::info!(server_id, action = action.name(), "admin action: sent");
        Ok(())
    }
}

/// Writes one message and waits for the server's answer: the change it confirms, a refusal, or
/// nothing within [`ANSWER_WAIT`] (FR-10.59). The wait is registered first, so a fast answer
/// cannot miss it. Also used by [`Engine::person_move`], which reports its result to the admin
/// who typed `/move` (issue #378).
pub(crate) async fn send_and_wait<S: FnOnce() -> Result<(), CommandError>>(
    ctx: &SessionCtx,
    hub: &Mutex<Admin>,
    session: u32,
    pending: Pending,
    send: S,
) -> EngineResult<ActionOutcome> {
    let receiver = { lock(hub).expect_action(ctx.server_id, session, pending) };
    send().map_err(convert::command_error)?;
    match tokio::time::timeout(ANSWER_WAIT, receiver).await {
        Ok(Ok(Ok(()))) => Ok(ActionOutcome::Confirmed),
        Ok(Ok(Err(error))) => Err(error),
        // The wait was dropped, which happens when the connection ends.
        Ok(Err(_)) => Err(EngineError::not_connected()),
        Err(_) => Ok(ActionOutcome::Unconfirmed),
    }
}

pub(crate) fn report(ctx: &SessionCtx, action: &str, outcome: &EngineResult<ActionOutcome>) {
    // Only the kind of action and its outcome (FR-10.64): no names, addresses or reasons.
    match outcome {
        Ok(ActionOutcome::Confirmed) => {
            tracing::info!(server_id = ctx.server_id, action, "admin action: confirmed")
        }
        Ok(ActionOutcome::Unconfirmed) => {
            tracing::info!(server_id = ctx.server_id, action, "admin action: no answer")
        }
        Err(error) => tracing::warn!(
            server_id = ctx.server_id,
            action,
            code = ?error.kind,
            "admin action: refused"
        ),
    }
}

fn denied(message: impl Into<String>) -> EngineError {
    EngineError::new(ErrorKind::PermissionDenied, message)
}

/// The server's own answer for the root channel decides a right (FR-10.1).
fn root_right(engine: &Engine, server_id: ServerId, bit: u32) -> EngineResult<bool> {
    let ctx = engine.session_ctx_for(server_id)?;
    Ok(ctx.handle.with_state(|state| {
        state
            .permissions
            .get(&ROOT)
            .copied()
            .is_some_and(|bits| bits & bit != 0)
    }))
}

fn is_own(ctx: &SessionCtx, session: u32) -> bool {
    ctx.handle
        .with_state(|state| state.session == Some(session))
}

/// Whether the server's own answers grant a right in a channel (FR-10.1). A channel whose answer
/// is not known yet falls back to the root channel's, because ACLs inherit downwards.
fn right_in(ctx: &SessionCtx, channel: u32, bit: u32) -> bool {
    ctx.handle.with_state(|state| {
        state
            .permissions
            .get(&channel)
            .or_else(|| state.permissions.get(&ROOT))
            .copied()
            .is_some_and(|bits| bits & bit != 0)
    })
}

/// A `UserStats` answer for a person the dialog asked about.
pub(crate) fn on_stats(hub: &Mutex<Admin>, server_id: ServerId, stats: &proto::UserStats) {
    let Some(session) = stats.session else {
        return;
    };
    let answer = StatsAnswer {
        address: stats.address.as_deref().and_then(ban::read_address),
        online_secs: stats.onlinesecs.map(u64::from),
    };
    lock(hub).answer_stats(server_id, session, answer);
}

/// The person is gone: the open kick or ban wait ends with the server's answer.
pub(crate) fn on_removed(hub: &Mutex<Admin>, server_id: ServerId, session: u32) {
    lock(hub).complete(server_id, session, Ok(()), |pending| {
        pending == Pending::Removal
    });
}

/// The server echoed a person's state: the open wait for a change of theirs ends (FR-10.15,
/// FR-10.20, FR-10.35). Like a refusal, only an echo of the field the wait asked to change
/// (`echoed_rights`, see [`mumble_client_protocol::UserChange`]) ends it, so an unrelated change of the
/// same person (they move or mute themselves) does not confirm the action (issue #640).
pub(crate) fn on_state(hub: &Mutex<Admin>, server_id: ServerId, session: u32, echoed_rights: u32) {
    lock(hub).complete(server_id, session, Ok(()), |pending| match pending {
        Pending::State(bit) => echoed_rights & bit != 0,
        Pending::Removal => false,
    });
}

/// A refused action, which the open dialog or command shows (FR-10.59). A denial only reaches a
/// wait that asked for the right the server names, so an unrelated refusal does not end it.
pub(crate) fn on_denied(hub: &Mutex<Admin>, server_id: ServerId, denied: &PermissionDenied) {
    let Some(session) = denied.session else {
        return;
    };
    let asked_for = |pending: Pending| match denied.permission {
        // A refusal names the right it is about, so it only reaches the wait that asked for it.
        Some(bits) => match pending {
            Pending::Removal => bits & (permissions::KICK | permissions::BAN) != 0,
            Pending::State(bit) => bits & bit != 0,
        },
        // Without the right in the answer any open wait for this person may be the one.
        None => true,
    };
    lock(hub).complete(
        server_id,
        session,
        Err(convert::denial_error(denied)),
        asked_for,
    );
}

/// The connection ended: drop what the dialog held (FR-10.63).
pub(crate) fn on_disconnect(hub: &Mutex<Admin>, server_id: ServerId) {
    lock(hub).forget(server_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deafen_mutes_and_unmute_lifts_deafen() {
        let deafen = ModerateAction::Deafen(true).message(4);
        assert_eq!((deafen.mute, deafen.deaf), (Some(true), Some(true)));
        let unmute = ModerateAction::Mute(false).message(4);
        assert_eq!((unmute.mute, unmute.deaf), (Some(false), Some(false)));
        let undeafen = ModerateAction::Deafen(false).message(4);
        assert_eq!((undeafen.mute, undeafen.deaf), (None, Some(false)));
    }

    #[test]
    fn resets_send_empty_values() {
        let comment = ModerateAction::ResetComment.message(4);
        assert_eq!(comment.comment.as_deref(), Some(""));
        assert_eq!(comment.session, Some(4));
        assert_eq!(
            ModerateAction::ResetAvatar.message(4).texture,
            Some(Vec::new())
        );
        assert_eq!(ModerateAction::Unsuppress.message(4).suppress, Some(false));
    }

    #[test]
    fn only_the_right_denial_reaches_the_open_wait() {
        let hub = Mutex::new(Admin::default());
        let mut mute = lock(&hub).expect_action(1, 7, Pending::State(permissions::MUTE_DEAFEN));
        let base = PermissionDenied {
            kind: mumble_client_protocol::DenyKind::Permission,
            permission: Some(permissions::KICK),
            channel_id: Some(0),
            session: Some(7),
            reason: None,
            name: None,
        };
        on_denied(&hub, 1, &base);
        assert!(
            mute.try_recv().is_err(),
            "a denial for another right is not this wait's"
        );

        // The right the wait asked for ends it, with the server's reason (FR-10.59).
        on_denied(
            &hub,
            1,
            &PermissionDenied {
                permission: Some(permissions::MUTE_DEAFEN),
                ..base
            },
        );
        let delivered = mute.try_recv().expect("the wait was answered");
        assert_eq!(
            delivered.err().map(|e| e.kind),
            Some(ErrorKind::PermissionDenied)
        );
    }

    #[test]
    fn the_echo_of_a_person_ends_only_a_state_wait() {
        let hub = Mutex::new(Admin::default());
        let mut moved = lock(&hub).expect_action(2, 5, Pending::State(permissions::MOVE));
        let mut kicked = lock(&hub).expect_action(2, 6, Pending::Removal);
        on_state(&hub, 2, 5, permissions::MOVE);
        on_state(&hub, 2, 6, permissions::MOVE);
        assert!(moved.try_recv().expect("the echo answered").is_ok());
        assert!(
            kicked.try_recv().is_err(),
            "an echo is not the removal a kick waits for"
        );
    }

    #[test]
    fn only_the_echo_of_the_asked_field_ends_the_wait() {
        let hub = Mutex::new(Admin::default());
        let mut mute = lock(&hub).expect_action(3, 7, Pending::State(permissions::MUTE_DEAFEN));
        // The person moves or mutes themselves while the server mute is in flight (issue #640).
        on_state(&hub, 3, 7, permissions::MOVE);
        on_state(&hub, 3, 7, 0);
        assert!(
            mute.try_recv().is_err(),
            "an unrelated change of the person is not the confirmation"
        );
        on_state(&hub, 3, 7, permissions::MUTE_DEAFEN);
        assert!(mute.try_recv().expect("the echo answered").is_ok());
    }

    #[test]
    fn one_mass_move_runs_per_server() {
        let hub = Mutex::new(Admin::default());
        assert!(lock(&hub).begin_mass_move(4), "the first one starts");
        assert!(
            !lock(&hub).begin_mass_move(4),
            "a second one is refused while it runs (FR-10.18)"
        );
        assert!(
            lock(&hub).begin_mass_move(5),
            "another server is unaffected"
        );
        lock(&hub).end_mass_move(4);
        assert!(
            lock(&hub).begin_mass_move(4),
            "it runs again once the first ended"
        );
        // Disconnecting drops the slot, so a reconnect is not blocked (FR-10.63).
        lock(&hub).forget(4);
        assert!(lock(&hub).begin_mass_move(4));
    }

    #[test]
    fn the_server_answer_for_a_person_ends_the_stats_wait_and_is_kept_in_memory() {
        let hub = Mutex::new(Admin::default());
        let mut receiver = lock(&hub).expect_stats(2, 9);
        let stats = proto::UserStats {
            session: Some(9),
            address: Some(vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 203, 0, 113, 1,
            ]),
            onlinesecs: Some(7200),
            ..Default::default()
        };
        on_stats(&hub, 2, &stats);
        let answer = receiver.try_recv().expect("the answer arrived");
        assert_eq!(
            answer.address.as_ref().map(|a| a.to_string()),
            Some("203.0.113.1".into())
        );
        assert_eq!(answer.online_secs, Some(7200));
        assert_eq!(
            lock(&hub).address(2, 9).map(|a| a.to_string()),
            Some("203.0.113.1".into())
        );

        // Closing the dialog drops what it held (FR-10.63).
        lock(&hub).forget(2);
        assert_eq!(lock(&hub).address(2, 9), None);
    }

    #[test]
    fn a_removed_person_answers_the_open_wait() {
        let hub = Mutex::new(Admin::default());
        let mut receiver = lock(&hub).expect_action(3, 11, Pending::Removal);
        on_removed(&hub, 3, 11);
        assert!(receiver.try_recv().expect("answered").is_ok());
    }
}
