//! Kick and ban of a person (Epic 4, issue #253).
//!
//! A kick is the standard `UserRemove` message, so the person can reconnect afterwards. A precise
//! ban needs the server's ban list: `UserRemove` with `ban = true` always takes the server's own
//! default length and both targets at once, so a chosen duration and the "Ban by" targets only work
//! through the list (FR-10.12). When the list cannot be read the dialog falls back to that message
//! (FR-10.13). The rights are the server's own answer for the root channel (FR-10.1); the server
//! has the final say.
//!
//! Nothing here goes to disk (FR-10.63): what a dialog holds lives in memory for the connection. The
//! person's address comes from `UserStats`, which standard servers send only to admins (FR-10.65).
//! Logs name the kind of action and its outcome, never names, addresses, fingerprints or reasons
//! (FR-10.64).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Duration;

use mumcord_protocol::PermissionDenied;
use mumcord_protocol::ban;
use mumcord_protocol::permissions;
use mumcord_protocol::proto;
use tokio::sync::oneshot;

use crate::ServerId;
use crate::banlist::{self, BanChange, BanView};
use crate::session::{Engine, EngineError, EngineResult, ErrorKind, SessionCtx, convert, lock};

/// The root channel, where the Kick and Ban rights are checked (FR-10.1).
const ROOT: u32 = 0;
/// How long a dialog waits for the server's answer, as the ban list read does (FR-10.13).
pub const ANSWER_WAIT: Duration = banlist::ANSWER_WAIT;

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

/// What a kick produced. `Confirmed` means the server answered with the person's removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KickOutcome {
    Confirmed,
    /// The server answered nothing within [`ANSWER_WAIT`]; the message was written to it.
    Unconfirmed,
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
    /// The dialog waiting for the server's answer to a kick or a ban.
    action: HashMap<u32, oneshot::Sender<Result<(), EngineError>>>,
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
    ) -> oneshot::Receiver<Result<(), EngineError>> {
        let (tx, rx) = oneshot::channel();
        self.slots
            .entry(server_id)
            .or_default()
            .action
            .insert(session, tx);
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

    fn complete(&mut self, server_id: ServerId, session: u32, outcome: Result<(), EngineError>) {
        if let Some(slot) = self.slots.get_mut(&server_id)
            && let Some(tx) = slot.action.remove(&session)
        {
            let _ = tx.send(outcome);
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
    ) -> EngineResult<KickOutcome> {
        let ctx = self.session_ctx_for(server_id)?;
        if !self.can_kick(server_id)? {
            return Err(denied("kick right in the root channel"));
        }
        if is_own(&ctx, session) {
            return Err(EngineError::invalid("kicking yourself is not offered"));
        }
        let outcome = send_and_wait(&ctx, self.admin_state(), session, reason, false).await;
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
            let outcome = send_and_wait(
                &ctx,
                self.admin_state(),
                session,
                request.reason.as_deref(),
                true,
            )
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
            send_and_wait(
                &ctx,
                self.admin_state(),
                session,
                request.reason.as_deref(),
                false
            )
            .await,
            Ok(KickOutcome::Confirmed)
        );
        tracing::info!(server_id, action = "ban", confirmed, "admin action: ban");
        Ok(BanOutcome::Saved {
            key,
            removed: confirmed,
        })
    }

    /// The dialog closed: what it held is dropped (FR-10.63).
    pub fn admin_close(&self, server_id: ServerId) {
        lock(self.admin_state()).forget(server_id);
    }
}

/// Writes the `UserRemove` message and waits for the server's answer: the person's removal, a
/// refusal, or nothing within [`ANSWER_WAIT`] (FR-10.59). The wait is registered first, so a fast
/// answer cannot miss it.
async fn send_and_wait(
    ctx: &SessionCtx,
    hub: &Mutex<Admin>,
    session: u32,
    reason: Option<&str>,
    ban: bool,
) -> EngineResult<KickOutcome> {
    let receiver = { lock(hub).expect_action(ctx.server_id, session) };
    ctx.handle
        .remove_user(session, reason, ban)
        .map_err(convert::command_error)?;
    match tokio::time::timeout(ANSWER_WAIT, receiver).await {
        Ok(Ok(Ok(()))) => Ok(KickOutcome::Confirmed),
        Ok(Ok(Err(error))) => Err(error),
        // The wait was dropped, which happens when the connection ends.
        Ok(Err(_)) => Err(EngineError::not_connected()),
        Err(_) => Ok(KickOutcome::Unconfirmed),
    }
}

fn report(ctx: &SessionCtx, action: &str, outcome: &EngineResult<KickOutcome>) {
    // Only the kind of action and its outcome (FR-10.64): no names, addresses or reasons.
    match outcome {
        Ok(KickOutcome::Confirmed) => {
            tracing::info!(server_id = ctx.server_id, action, "admin action: confirmed")
        }
        Ok(KickOutcome::Unconfirmed) => {
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

fn denied(message: &str) -> EngineError {
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

/// The person is gone: the open dialog's wait ends with the server's answer.
pub(crate) fn on_removed(hub: &Mutex<Admin>, server_id: ServerId, session: u32) {
    lock(hub).complete(server_id, session, Ok(()));
}

/// A refused kick or ban, which the open dialog shows (FR-10.59).
pub(crate) fn on_denied(hub: &Mutex<Admin>, server_id: ServerId, denied: &PermissionDenied) {
    let Some(session) = denied.session else {
        return;
    };
    if denied
        .permission
        .is_none_or(|bits| bits & (permissions::KICK | permissions::BAN) == 0)
    {
        return;
    }
    lock(hub).complete(server_id, session, Err(convert::denial_error(denied)));
}

/// The connection ended: drop what the dialog held (FR-10.63).
pub(crate) fn on_disconnect(hub: &Mutex<Admin>, server_id: ServerId) {
    lock(hub).forget(server_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_kick_and_ban_denials_reach_an_open_dialog() {
        let hub = Mutex::new(Admin::default());
        let mut receiver = lock(&hub).expect_action(1, 7);
        let unrelated = PermissionDenied {
            kind: mumcord_protocol::DenyKind::Permission,
            permission: Some(permissions::MOVE),
            channel_id: Some(0),
            session: Some(7),
            reason: None,
            name: None,
        };
        on_denied(&hub, 1, &unrelated);
        assert!(
            receiver.try_recv().is_err(),
            "a denial for another permission is not this dialog's"
        );

        let kick = PermissionDenied {
            permission: Some(permissions::KICK),
            ..unrelated.clone()
        };
        on_denied(&hub, 1, &kick);
        let delivered = receiver.try_recv().expect("the dialog was answered");
        assert_eq!(
            delivered.err().map(|e| e.kind),
            Some(ErrorKind::PermissionDenied)
        );
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
    fn a_removed_person_answers_the_open_dialog() {
        let hub = Mutex::new(Admin::default());
        let mut receiver = lock(&hub).expect_action(3, 11);
        on_removed(&hub, 3, 11);
        assert!(receiver.try_recv().expect("answered").is_ok());
    }
}
