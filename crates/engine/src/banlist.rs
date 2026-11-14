//! The server's ban list for the Server administration window (Epic 4, issue #254).
//!
//! Nothing here goes to disk (FR-10.63): the list lives in memory per connection while the window
//! is open and is dropped when it closes or the connection ends. The protocol has no single-entry
//! change, so a save always re-reads the server's list first and sends back the fresh copy with
//! only this user's change applied (FR-10.66). If the entry being edited or removed changed or
//! vanished meanwhile, the save stops with a conflict and the UI asks before overwriting.
//!
//! Names, reasons, addresses and fingerprints are server text and personal data: untrusted, and
//! never logged (FR-10.64, FR-10.65).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use mumble_client_protocol::ban::{self, Ban};
use tokio::sync::oneshot;

use crate::ServerId;
use crate::session::convert;
use crate::session::{Engine, EngineError, EngineResult, ErrorKind, SessionCtx, lock};

/// The root channel, where the Ban right is checked.
const ROOT: u32 = 0;
/// How long a ban-list query may take before the caller is told the server did not answer
/// (spec 13 A2, FR-10.13's read failure).
pub const ANSWER_WAIT: Duration = Duration::from_secs(5);

/// One entry as the window shows it. `key` identifies the entry across a re-read, so an edit or a
/// removal still matches the server's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BanView {
    /// `address/mask|hash`, the parts the ban acts on. Empty for a change that has no entry yet.
    pub key: String,
    pub name: Option<String>,
    pub cert_hash: Option<String>,
    /// Full address; the UI masks it by default (FR-10.51).
    pub address: String,
    /// Subnet length in the family the UI shows: 32 for a single IPv4, 128 for a single IPv6.
    pub mask: u32,
    pub reason: Option<String>,
    /// Start as RFC 3339; `None` when the server sent a shape that cannot be read.
    pub start: Option<String>,
    /// Seconds, 0 = permanent.
    pub duration_secs: u32,
    pub expired: bool,
}

/// One change the user made in the window; a save applies all of them to the fresh copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BanChange {
    Add {
        name: Option<String>,
        cert_hash: Option<String>,
        address: IpAddr,
        mask: u32,
        reason: Option<String>,
        duration_secs: u32,
    },
    Edit {
        key: String,
        reason: Option<String>,
        duration_secs: u32,
    },
    Remove {
        key: String,
    },
}

/// What a save produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BanSaveOutcome {
    Saved(Vec<BanView>),
    /// The entry was changed or removed on the server while the window was open (FR-10.66).
    Conflict {
        key: String,
        fresh: Vec<BanView>,
    },
}

/// What the window holds per server: the loaded list and the open query.
#[derive(Default)]
struct Slot {
    loaded: Vec<Ban>,
    waiting: Option<oneshot::Sender<Result<Vec<Ban>, EngineError>>>,
}

/// In-memory ban lists, one per connection.
#[derive(Default)]
pub struct BanLists {
    slots: HashMap<ServerId, Slot>,
}

impl BanLists {
    fn expect(&mut self, server_id: ServerId) -> oneshot::Receiver<Result<Vec<Ban>, EngineError>> {
        let (tx, rx) = oneshot::channel();
        self.slots.entry(server_id).or_default().waiting = Some(tx);
        rx
    }

    fn answer(&mut self, server_id: ServerId, list: &[Ban]) {
        let slot = self.slots.entry(server_id).or_default();
        slot.loaded = list.to_vec();
        if let Some(tx) = slot.waiting.take() {
            let _ = tx.send(Ok(list.to_vec()));
        }
    }

    fn deny(&mut self, server_id: ServerId, error: EngineError) {
        if let Some(slot) = self.slots.get_mut(&server_id)
            && let Some(tx) = slot.waiting.take()
        {
            let _ = tx.send(Err(error));
        }
    }

    fn loaded(&self, server_id: ServerId) -> Vec<Ban> {
        self.slots
            .get(&server_id)
            .map(|slot| slot.loaded.clone())
            .unwrap_or_default()
    }

    fn forget(&mut self, server_id: ServerId) {
        self.slots.remove(&server_id);
    }
}

impl Engine {
    /// Whether the user may see the ban list: the Ban right in the root channel, as the server's
    /// own answer says (FR-10.1, FR-10.2).
    pub fn can_ban(&self, server_id: ServerId) -> EngineResult<bool> {
        let ctx = self.session_ctx_for(server_id)?;
        Ok(ctx.handle.with_state(|state| {
            state
                .permissions
                .get(&ROOT)
                .copied()
                .is_some_and(|bits| bits & mumble_client_protocol::permissions::BAN != 0)
        }))
    }

    /// Reads the list for the window and keeps it as the base of the change check (FR-10.51).
    pub async fn ban_list_open(&self, server_id: ServerId) -> EngineResult<Vec<BanView>> {
        let ctx = self.session_ctx_for(server_id)?;
        if !self.can_ban(server_id)? {
            return Err(EngineError::new(
                ErrorKind::PermissionDenied,
                "ban right in the root channel",
            ));
        }
        let list = query(&ctx, self.ban_lists_state()).await?;
        Ok(list.iter().map(|b| to_view(b, SystemTime::now())).collect())
    }

    /// Saves the window's changes. `force` is the UI's "Save mine anyway" after a conflict.
    pub async fn ban_list_save(
        &self,
        server_id: ServerId,
        changes: &[BanChange],
        force: bool,
    ) -> EngineResult<BanSaveOutcome> {
        let ctx = self.session_ctx_for(server_id)?;
        if changes.is_empty() {
            return Ok(BanSaveOutcome::Saved(self.ban_list_open(server_id).await?));
        }
        let before = lock(self.ban_lists_state()).loaded(server_id);
        let fresh = query(&ctx, self.ban_lists_state()).await?;

        if !force && let Some(key) = find_conflict(changes, &before, &fresh) {
            return Ok(BanSaveOutcome::Conflict {
                key,
                fresh: fresh
                    .iter()
                    .map(|b| to_view(b, SystemTime::now()))
                    .collect(),
            });
        }
        let out = apply(&fresh, changes);
        ctx.handle
            .send_ban_list(&out)
            .map_err(convert::command_error)?;
        let saved = query(&ctx, self.ban_lists_state()).await?;
        // Only the kind of action and its outcome (FR-10.64): no names, addresses or reasons.
        tracing::info!(
            server_id,
            added = count(changes, |c| matches!(c, BanChange::Add { .. })),
            edited = count(changes, |c| matches!(c, BanChange::Edit { .. })),
            removed = count(changes, |c| matches!(c, BanChange::Remove { .. })),
            "ban list saved: ok"
        );
        Ok(BanSaveOutcome::Saved(
            saved
                .iter()
                .map(|b| to_view(b, SystemTime::now()))
                .collect(),
        ))
    }

    /// The window closed: what it held is dropped (FR-10.63).
    pub fn ban_list_close(&self, server_id: ServerId) {
        lock(self.ban_lists_state()).forget(server_id);
    }
}

fn count(changes: &[BanChange], pred: impl Fn(&BanChange) -> bool) -> usize {
    changes.iter().filter(|c| pred(c)).count()
}

/// Asks the server and waits for its answer.
async fn query(ctx: &SessionCtx, hub: &Mutex<BanLists>) -> EngineResult<Vec<Ban>> {
    let reply = { lock(hub).expect(ctx.server_id) };
    ctx.handle
        .query_ban_list()
        .map_err(convert::command_error)?;
    match tokio::time::timeout(ANSWER_WAIT, reply).await {
        Ok(Ok(delivered)) => delivered,
        // The wait was dropped, which happens when the connection ends.
        Ok(Err(_)) => Err(EngineError::not_connected()),
        Err(_) => Err(EngineError::new(
            ErrorKind::ServerUnreachable,
            "the server did not answer the ban list query within 5 s",
        )),
    }
}

/// The key that identifies a change in the list: the parts the ban acts on, so an entry can be
/// matched again after the re-read (FR-10.66).
pub fn change_key(change: &BanChange) -> String {
    match change {
        BanChange::Add {
            address,
            mask,
            cert_hash,
            ..
        } => format!(
            "{}/{}|{}",
            address,
            if address.is_ipv4() {
                (*mask).min(32)
            } else {
                *mask
            },
            cert_hash.as_deref().unwrap_or("")
        ),
        BanChange::Edit { key, .. } | BanChange::Remove { key } => key.clone(),
    }
}

/// The first change whose entry the server holds in another form than the user wrote (FR-10.66):
/// the change is applied to the fresh copy, so only what someone else did here counts.
fn find_conflict(changes: &[BanChange], before: &[Ban], fresh: &[Ban]) -> Option<String> {
    for change in changes {
        let key = change_key(change);
        let now = fresh.iter().find(|b| b.key() == key);
        match change {
            // An entry the user adds must not already stand with other values.
            BanChange::Add {
                reason,
                duration_secs,
                ..
            } => {
                if now.is_some_and(|current| {
                    current.reason != *reason || current.duration != *duration_secs
                }) {
                    return Some(key);
                }
            }
            BanChange::Edit { key: touched, .. } | BanChange::Remove { key: touched } => {
                let was = before.iter().find(|b| b.key() == *touched);
                match (was, now) {
                    (Some(loaded), Some(current)) if touched_elsewhere(loaded, current) => {
                        return Some(key);
                    }
                    (Some(_), None) => return Some(key),
                    _ => {}
                }
            }
        }
    }
    None
}

/// Whether the server's copy of the entry is something other than what the user edited (FR-10.66).
/// Whether the server's copy of an entry differs from what the window loaded (FR-10.66): the user's
/// own change is applied on top of the fresh list, so only a change someone else made here counts.
fn touched_elsewhere(loaded: &Ban, fresh: &Ban) -> bool {
    loaded != fresh
}

/// The fresh list with only this user's changes applied (FR-10.66).
fn apply(fresh: &[Ban], changes: &[BanChange]) -> Vec<Ban> {
    let mut out = fresh.to_vec();
    for change in changes {
        match change {
            BanChange::Add {
                name,
                cert_hash,
                address,
                mask,
                reason,
                duration_secs,
            } => out.push(Ban {
                address: *address,
                mask: *mask,
                name: name.clone(),
                hash: cert_hash.clone(),
                reason: reason.clone(),
                start: Some(ban::format_start(SystemTime::now())),
                duration: *duration_secs,
            }),
            BanChange::Edit {
                key,
                reason,
                duration_secs,
            } => {
                if let Some(entry) = out.iter_mut().find(|b| &b.key() == key) {
                    entry.reason.clone_from(reason);
                    entry.duration = *duration_secs;
                }
            }
            BanChange::Remove { key } => out.retain(|b| &b.key() != key),
        }
    }
    out
}

/// What the UI shows for one entry.
pub fn to_view(ban: &Ban, now: SystemTime) -> BanView {
    BanView {
        key: ban.key(),
        // Server text is untrusted: control characters never reach the window (FR-10.67).
        name: ban.name.as_ref().map(|text| clean(text)),
        cert_hash: ban.hash.as_ref().map(|text| clean(text)),
        address: ban.address.to_string(),
        mask: ban.mask_bits(),
        reason: ban.reason.as_ref().map(|text| clean(text)),
        start: ban.start_time().map(rfc3339),
        duration_secs: ban.duration,
        expired: ban.is_expired(now),
    }
}

fn rfc3339(when: SystemTime) -> String {
    ban::format_start(when)
}

/// Drops control characters from text the server sent, so the window renders what it can show.
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(200).collect()
}

/// A server answer, from the session event pump.
pub(crate) fn on_answer(hub: &Mutex<BanLists>, server_id: ServerId, list: &[Ban]) {
    lock(hub).answer(server_id, list);
}

/// A refused query (FR-10.59).
pub(crate) fn on_denied(hub: &Mutex<BanLists>, server_id: ServerId, error: EngineError) {
    lock(hub).deny(server_id, error);
}

/// The connection ended: drop what the window held (FR-10.63).
pub(crate) fn on_disconnect(hub: &Mutex<BanLists>, server_id: ServerId) {
    lock(hub).forget(server_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(hash: &str) -> Ban {
        Ban {
            address: "203.0.113.1".parse().expect("address"),
            mask: 32,
            name: Some("Mallory".into()),
            hash: Some(hash.to_owned()),
            reason: Some("spam".into()),
            start: Some("2026-11-03T20:00:00Z".into()),
            duration: 3600,
        }
    }

    #[test]
    fn a_save_applies_only_the_user_s_change() {
        let fresh = vec![entry("a3f9"), entry("c21e")];
        let changes = vec![
            BanChange::Edit {
                key: entry("a3f9").key(),
                reason: Some("spam links".into()),
                duration_secs: 0,
            },
            BanChange::Remove {
                key: entry("c21e").key(),
            },
            BanChange::Add {
                name: Some("Freya".into()),
                cert_hash: Some("new".into()),
                address: "198.51.100.9".parse().expect("address"),
                mask: 32,
                reason: Some("flooding".into()),
                duration_secs: 86_400,
            },
        ];
        let out = apply(&fresh, &changes);
        assert_eq!(out.len(), 2, "one removed, one added");
        assert_eq!(out[0].reason, Some("spam links".into()));
        assert_eq!(out[0].duration, 0, "0 is permanent");
        assert_eq!(out[1].hash, Some("new".into()));
    }

    #[test]
    fn an_added_entry_that_already_stands_in_another_form_is_a_conflict() {
        let fresh = [entry("a3f9")];
        let change = BanChange::Add {
            name: Some("Mallory".into()),
            cert_hash: Some("a3f9".into()),
            address: "203.0.113.1".parse().expect("address"),
            mask: 32,
            reason: Some("spam links".into()),
            duration_secs: 3600,
        };
        let key = change_key(&change);
        assert_eq!(find_conflict(&[change], &[], &fresh), Some(key));

        // The same entry the user writes is nothing to ask about.
        let same = BanChange::Add {
            name: Some("Mallory".into()),
            cert_hash: Some("a3f9".into()),
            address: "203.0.113.1".parse().expect("address"),
            mask: 32,
            reason: Some("spam".into()),
            duration_secs: 3600,
        };
        assert_eq!(find_conflict(&[same], &[], &fresh), None);
    }

    #[test]
    fn a_server_change_to_the_same_entry_is_a_conflict() {
        let loaded = entry("a3f9");
        let mut other = loaded.clone();
        other.reason = Some("someone else's reason".into());
        assert!(touched_elsewhere(&loaded, &other));
        assert!(
            !touched_elsewhere(&loaded, &loaded),
            "the server did not touch it"
        );
    }

    #[test]
    fn nothing_is_kept_after_the_window_closes() {
        let hub = Mutex::new(BanLists::default());
        lock(&hub).expect(1);
        lock(&hub).answer(1, &[entry("a3f9")]);
        assert_eq!(lock(&hub).loaded(1).len(), 1);
        lock(&hub).forget(1);
        assert!(lock(&hub).loaded(1).is_empty(), "FR-10.63: memory only");
    }

    #[test]
    fn a_refusal_ends_the_wait_with_the_reason() {
        let hub = Mutex::new(BanLists::default());
        let mut reply = lock(&hub).expect(1);
        lock(&hub).deny(
            1,
            EngineError::new(ErrorKind::PermissionDenied, "no Ban right"),
        );
        let got = reply.try_recv().expect("answered");
        assert_eq!(got.err().map(|e| e.kind), Some(ErrorKind::PermissionDenied));
    }

    #[test]
    fn the_mask_rounds_trip_in_the_form_the_server_counts() {
        let stored = entry("a3f9");
        let wire = ban::to_message(std::slice::from_ref(&stored));
        assert_eq!(
            wire.bans[0].mask, 128,
            "a single IPv4 goes out as the wire length"
        );
        assert_eq!(
            ban::from_message(&wire).first().expect("entry").mask,
            32,
            "and reads back as /32"
        );
    }

    #[test]
    fn a_view_carries_the_mask_of_its_family_and_the_end() {
        // The length a view shows is the family's own, which is what a change carries (issue #253).
        let mapped = entry("a3f9");
        // A fixed clock so the test does not depend on the machine's date.
        let now = ban::parse_iso("2026-11-03T19:00:00Z").expect("fixed now");
        let shown = to_view(&mapped, now);
        assert_eq!(shown.mask, 32, "shown as an IPv4 /32");
        assert_eq!(shown.start, Some("2026-11-03T20:00:00.000Z".into()));
        assert!(!shown.expired, "ends in an hour");
        let mut old = entry("a3f9");
        old.start = Some("2020-01-01T00:00:00Z".into());
        assert!(to_view(&old, now).expired);
    }
}
