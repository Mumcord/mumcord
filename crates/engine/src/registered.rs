//! The people registered on the server (Epic 4, issue #256).
//!
//! The Registered users tab reads the server's list with an entry-less `UserList`, renames one
//! account with a single entry that carries the new name, and removes a registration with an entry
//! that carries none. Unlike the ban list the server takes each entry on its own, so a change is
//! one message for one person (issue: "changes are sent per user when saving").
//!
//! What a change did cannot be read from the message: measured against the local test server
//! (`crates/protocol/tests/live_registered_users.rs`), mumble-server 1.5.735 sends its list only in
//! answer to a query and never pushes it after a change. It also neither refuses nor confirms a
//! rename onto a name another account holds — it keeps the old name and says nothing. So every
//! change here is followed by a read, and the effect is taken from what the server's list says
//! afterwards; the name a user typed is never shown as if it stood.
//!
//! Nothing here goes to disk (FR-10.63): the list lives in memory for the connection while the tab
//! is open and is dropped when the tab closes or the connection ends. Names and channel names are
//! server text and personal data: untrusted, cleaned before they are shown, and never logged
//! (FR-10.64, FR-10.67).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use mumcord_protocol::permissions;
use mumcord_protocol::{PermissionDenied, RegisteredUser, ServerState, proto};
use tokio::sync::oneshot;

use crate::ServerId;
use crate::session::convert;
use crate::session::{Engine, EngineError, EngineResult, ErrorKind, SessionCtx, lock};

/// The root channel, where the Register right is checked (FR-10.1).
const ROOT: u32 = 0;
/// How long the server may take with a list, as the ban list read does (spec 13 A2).
pub const ANSWER_WAIT: Duration = Duration::from_secs(5);

/// One account as the window shows it. `user_id` is the key: a name that looks free may not be, and
/// only the id says which account a change acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredView {
    pub user_id: u32,
    pub name: String,
    /// When the account was last used, as RFC 3339; `None` when the server does not say (FR-10.5).
    pub last_seen: Option<String>,
    /// The channel the account was last seen in, named; `None` when the server does not say, or the
    /// channel is gone.
    pub last_channel: Option<String>,
    /// Whether one of their accounts is connected right now.
    pub online: bool,
    /// The SHA-1 hash of their certificate, when a connected session reported one. Only a window
    /// with the Ban right may show it (FR-10.65), which is why it travels separately from the list.
    pub cert_hash: Option<String>,
}

/// What one change produced. The caller shows `entries` either way: the server's list is the only
/// account of what stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisteredOutcome {
    /// The server's list now carries the change.
    Saved { entries: Vec<RegisteredView> },
    /// The list still shows the account as it was: a rename onto a name another account holds,
    /// which this server neither refuses nor confirms (see the module docs).
    Kept { entries: Vec<RegisteredView> },
}

/// What the window holds per server: the loaded list and the open reads.
#[derive(Default)]
struct Slot {
    loaded: Vec<RegisteredUser>,
    /// The dialog waiting for a `UserList` answer.
    waiting: Option<oneshot::Sender<Result<Vec<RegisteredUser>, EngineError>>>,
    /// The detail panel waiting for a channel's groups, with the account it was read for.
    groups: HashMap<u32, (u32, oneshot::Sender<Vec<String>>)>,
}

/// In-memory registered user lists, one per connection.
#[derive(Default)]
pub struct Registered {
    slots: HashMap<ServerId, Slot>,
}

impl Registered {
    fn slot(&mut self, server_id: ServerId) -> &mut Slot {
        self.slots.entry(server_id).or_default()
    }

    fn expect(
        &mut self,
        server_id: ServerId,
    ) -> oneshot::Receiver<Result<Vec<RegisteredUser>, EngineError>> {
        let (tx, rx) = oneshot::channel();
        self.slot(server_id).waiting = Some(tx);
        rx
    }

    fn expect_groups(
        &mut self,
        server_id: ServerId,
        channel_id: u32,
        user_id: u32,
    ) -> oneshot::Receiver<Vec<String>> {
        let (tx, rx) = oneshot::channel();
        // A second read of the same channel replaces the first, whose waiter then drops unread.
        self.slot(server_id)
            .groups
            .insert(channel_id, (user_id, tx));
        rx
    }

    fn answer(&mut self, server_id: ServerId, list: &[RegisteredUser]) {
        let slot = self.slot(server_id);
        slot.loaded = list.to_vec();
        if let Some(tx) = slot.waiting.take() {
            let _ = tx.send(Ok(list.to_vec()));
        }
    }

    fn answer_groups(&mut self, server_id: ServerId, acl: &proto::Acl) {
        if let Some(slot) = self.slots.get_mut(&server_id)
            && let Some((user_id, tx)) = slot.groups.remove(&acl.channel_id)
        {
            let _ = tx.send(group_names(acl, user_id));
        }
    }

    /// A refused group read: the panel gets an empty answer instead of waiting for the timeout.
    /// True when a group read was waiting, so the caller knows the refusal was answered.
    fn deny_groups(&mut self, server_id: ServerId, channel_id: u32) -> bool {
        if let Some(slot) = self.slots.get_mut(&server_id)
            && let Some((_, tx)) = slot.groups.remove(&channel_id)
        {
            let _ = tx.send(Vec::new());
            return true;
        }
        false
    }

    fn deny(&mut self, server_id: ServerId, error: EngineError) -> bool {
        if let Some(slot) = self.slots.get_mut(&server_id)
            && let Some(tx) = slot.waiting.take()
        {
            let _ = tx.send(Err(error));
            return true;
        }
        false
    }

    /// Where the account was last seen, as the list this connection read says.
    fn last_channel(&self, user_id: u32) -> Option<u32> {
        self.slots
            .values()
            .find_map(|slot| slot.loaded.iter().find(|user| user.user_id == user_id))
            .and_then(|user| user.last_channel)
    }

    fn forget(&mut self, server_id: ServerId) {
        self.slots.remove(&server_id);
    }
}

impl Engine {
    /// Whether the user may see and change the list: the Register right in the root channel, as the
    /// server's own answer says (FR-10.1, FR-10.2).
    pub fn can_register(&self, server_id: ServerId) -> EngineResult<bool> {
        let ctx = self.session_ctx_for(server_id)?;
        Ok(ctx.handle.with_state(|state| {
            state
                .permissions
                .get(&ROOT)
                .copied()
                .is_some_and(|bits| bits & permissions::REGISTER != 0)
        }))
    }

    /// Reads the list for the tab (issue #256). Fails with `permissionDenied` without the Register
    /// right, and with `serverUnreachable` when the server does not answer within [`ANSWER_WAIT`].
    pub async fn registered_open(&self, server_id: ServerId) -> EngineResult<Vec<RegisteredView>> {
        let ctx = self.session_ctx_for(server_id)?;
        if !self.can_register(server_id)? {
            return Err(denied("register right in the root channel"));
        }
        let list = query(&ctx, self.registered_state()).await?;
        Ok(self.views(server_id, &list))
    }

    /// Renames one account (FR-10.52, AC-10.37) and reads the list back, which is the only way to
    /// learn whether the server took the name.
    pub async fn registered_rename(
        &self,
        server_id: ServerId,
        user_id: u32,
        name: &str,
    ) -> EngineResult<RegisteredOutcome> {
        let wanted = name.trim();
        if wanted.is_empty() {
            return Err(EngineError::invalid("a name is needed"));
        }
        if wanted.chars().count() > MAX_NAME_LEN {
            return Err(EngineError::invalid("the name is too long"));
        }
        let entries = self.change(server_id, user_id, Some(wanted)).await?;
        let taken = rename_took(&entries, user_id, wanted);
        Ok(if taken {
            RegisteredOutcome::Saved { entries }
        } else {
            RegisteredOutcome::Kept { entries }
        })
    }

    /// Removes one registration (FR-10.52, AC-10.37). The person can still connect, as a guest, and
    /// loses the name and the group memberships that name them.
    pub async fn registered_unregister(
        &self,
        server_id: ServerId,
        user_id: u32,
    ) -> EngineResult<RegisteredOutcome> {
        let entries = self.change(server_id, user_id, None).await?;
        let gone = unregister_took(&entries, user_id);
        Ok(if gone {
            RegisteredOutcome::Saved { entries }
        } else {
            RegisteredOutcome::Kept { entries }
        })
    }

    /// One change for one account, then the server's list as it stands now. The read is what tells
    /// whether the server took the change: it answers nothing else after one (measured, issue #256).
    async fn change(
        &self,
        server_id: ServerId,
        user_id: u32,
        name: Option<&str>,
    ) -> EngineResult<Vec<RegisteredView>> {
        let ctx = self.session_ctx_for(server_id)?;
        if !self.can_register(server_id)? {
            return Err(denied("register right in the root channel"));
        }
        // The wait is registered before anything is written, so a fast answer cannot miss it.
        let receiver = { lock(self.registered_state()).expect(server_id) };
        ctx.handle
            .change_registered_user(user_id, name)
            .and_then(|()| ctx.handle.query_user_list())
            .map_err(convert::command_error)?;
        let list = wait(receiver).await?;
        let views = self.views(server_id, &list);
        // Only the kind of action and its outcome (FR-10.64): no names, no ids.
        tracing::info!(
            server_id,
            renamed = name.is_some(),
            entries = views.len(),
            "registered users: change sent"
        );
        Ok(views)
    }

    /// Registers an online guest under their current name (FR-10.20, AC-10.15). This needs a client
    /// certificate, which is what the account is bound to, and only works while the person is
    /// connected (issue: "registers an online guest").
    pub fn registered_register_person(
        &self,
        server_id: ServerId,
        session: u32,
    ) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        if !self.can_register(server_id)? {
            return Err(denied("register right in the root channel"));
        }
        if ctx.handle.with_state(|state| state.session) == Some(session) {
            // Registering yourself is the SelfRegister path, which has its own place (FR-1.34).
            return Err(EngineError::invalid(
                "registering yourself is not offered here",
            ));
        }
        let known = ctx.handle.with_state(|state| {
            state
                .users
                .get(&session)
                .map(|user| (user.user_id, user.cert_hash.clone()))
        });
        let Some((user_id, cert_hash)) = known else {
            return Err(EngineError::invalid("this person is not connected"));
        };
        if user_id.is_some() {
            return Err(EngineError::invalid("this person is registered already"));
        }
        if cert_hash.is_none() {
            return Err(EngineError::invalid(
                "registering needs a client certificate",
            ));
        }
        ctx.handle
            .send_admin_user_state(proto::UserState {
                session: Some(session),
                user_id: Some(0),
                ..Default::default()
            })
            .map_err(convert::command_error)?;
        tracing::info!(server_id, "registered users: person registered");
        Ok(())
    }

    /// The groups that name one account, which the detail panel shows (issue #256). They are read
    /// where the account was last seen, or at the root when the server does not say, because that
    /// is the tree the panel is talking about. Standard servers answer an ACL query only for users
    /// with the Edit-permissions right, so a server that will not say is answered with an empty
    /// list rather than an error the user cannot act on.
    pub async fn registered_groups(
        &self,
        server_id: ServerId,
        user_id: u32,
    ) -> EngineResult<Vec<String>> {
        let ctx = self.session_ctx_for(server_id)?;
        if !self.can_register(server_id)? {
            return Err(denied("register right in the root channel"));
        }
        let channel_id = lock(self.registered_state())
            .last_channel(user_id)
            .unwrap_or(ROOT);
        let receiver = {
            let hub = self.registered_state();
            lock(hub).expect_groups(server_id, channel_id, user_id)
        };
        ctx.handle
            .query_acl(channel_id)
            .map_err(convert::command_error)?;
        Ok(match tokio::time::timeout(ANSWER_WAIT, receiver).await {
            Ok(Ok(names)) => names,
            // Nothing waiting any more: the connection ended or the panel moved on.
            Ok(Err(_)) => Vec::new(),
            Err(_) => {
                tracing::debug!(server_id, "registered users: no group answer");
                Vec::new()
            }
        })
    }

    /// The tab closed: what it held is dropped (FR-10.63).
    pub fn registered_close(&self, server_id: ServerId) {
        lock(self.registered_state()).forget(server_id);
    }

    /// The server's accounts as the window shows them: names cleaned, channel names and the online
    /// state taken from what the connection knows.
    fn views(&self, server_id: ServerId, list: &[RegisteredUser]) -> Vec<RegisteredView> {
        let (online, channels, certs) = match self.session_ctx_for(server_id) {
            Ok(ctx) => ctx.handle.with_state(state_facts),
            Err(_) => (HashSet::new(), HashMap::new(), HashMap::new()),
        };
        list.iter()
            .map(|user| RegisteredView {
                user_id: user.user_id,
                // Server text is untrusted: control characters never reach the window (FR-10.67).
                name: user.name.as_deref().map(clean).unwrap_or_default(),
                last_seen: user.last_seen.map(rfc3339),
                last_channel: user
                    .last_channel
                    .and_then(|channel| channels.get(&channel).cloned()),
                online: online.contains(&user.user_id),
                cert_hash: certs.get(&user.user_id).cloned(),
            })
            .collect()
    }
}

/// Whether the server's list now holds this account under this name. Reading the list back is the
/// only answer a rename gets: this server takes a rename onto a held name without a word (issue #256).
pub fn rename_took(entries: &[RegisteredView], user_id: u32, wanted: &str) -> bool {
    entries
        .iter()
        .any(|entry| entry.user_id == user_id && entry.name == wanted)
}

/// Whether the server's list no longer holds this account.
pub fn unregister_took(entries: &[RegisteredView], user_id: u32) -> bool {
    !entries.iter().any(|entry| entry.user_id == user_id)
}

/// The longest name a server takes, counted as the protocol counts it.
pub const MAX_NAME_LEN: usize = 100;

/// What the connection knows about the accounts it sees: who of them is online, the names of its
/// channels, and the certificate hash of each online account.
fn state_facts(state: &ServerState) -> (HashSet<u32>, HashMap<u32, String>, HashMap<u32, String>) {
    let mut online = HashSet::new();
    let mut certs = HashMap::new();
    for user in state.users.values() {
        if let Some(user_id) = user.user_id {
            online.insert(user_id);
            if let Some(hash) = &user.cert_hash {
                certs.entry(user_id).or_insert_with(|| hash.clone());
            }
        }
    }
    let channels = state
        .channels
        .iter()
        .map(|(id, channel)| (*id, clean(&channel.name)))
        .collect();
    (online, channels, certs)
}

/// Asks the server and waits for its answer.
async fn query(ctx: &SessionCtx, hub: &Mutex<Registered>) -> EngineResult<Vec<RegisteredUser>> {
    let receiver = { lock(hub).expect(ctx.server_id) };
    ctx.handle
        .query_user_list()
        .map_err(convert::command_error)?;
    wait(receiver).await
}

async fn wait(
    receiver: oneshot::Receiver<Result<Vec<RegisteredUser>, EngineError>>,
) -> EngineResult<Vec<RegisteredUser>> {
    match tokio::time::timeout(ANSWER_WAIT, receiver).await {
        Ok(Ok(delivered)) => delivered,
        // The wait was dropped, which happens when the connection ends.
        Ok(Err(_)) => Err(EngineError::not_connected()),
        Err(_) => Err(EngineError::new(
            ErrorKind::ServerUnreachable,
            "the server did not answer the registered users query within 5 s",
        )),
    }
}

/// The groups of one channel's answer that name this account, sorted and cleaned. The groups whose
/// name the server builds itself (a channel password `#…`, a certificate `$…`, an inherited rule
/// `~…`) stand for a condition, not for a group an admin made, so they are left out.
pub fn group_names(acl: &proto::Acl, user_id: u32) -> Vec<String> {
    let mut out: Vec<String> = acl
        .groups
        .iter()
        .filter(|group| {
            group
                .add
                .iter()
                .chain(group.inherited_members.iter())
                .any(|id| *id == user_id)
                && !group.remove.contains(&user_id)
        })
        .map(|group| clean(&group.name))
        .filter(|name| !name.starts_with('~') && !name.starts_with('#') && !name.starts_with('$'))
        .collect();
    out.sort();
    out.dedup();
    out
}

fn denied(message: &str) -> EngineError {
    EngineError::new(ErrorKind::PermissionDenied, message)
}

fn rfc3339(when: SystemTime) -> String {
    mumcord_protocol::ban::format_start(when)
}

/// Drops control characters from text the server sent, so the window renders what it can show.
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(200).collect()
}

/// A server answer, from the session event pump.
pub(crate) fn on_answer(hub: &Mutex<Registered>, server_id: ServerId, list: &[RegisteredUser]) {
    lock(hub).answer(server_id, list);
}

/// A channel's groups, from the same answer the ACL editor reads (issue #256).
pub(crate) fn on_acl(hub: &Mutex<Registered>, server_id: ServerId, acl: &proto::Acl) {
    lock(hub).answer_groups(server_id, acl);
}

/// A refusal the tab asked for (FR-10.59, FR-10.2): the Register right was missing for a read or
/// a change, or the Edit-permissions right for a group read, which the panel then leaves empty
/// instead of waiting. True when the open tab had a request waiting, so the refusal gets answered
/// here and the UI adds no second notice.
pub(crate) fn on_denied(
    hub: &Mutex<Registered>,
    server_id: ServerId,
    denied: &PermissionDenied,
) -> bool {
    if denied.permission == Some(permissions::REGISTER) {
        return lock(hub).deny(server_id, convert::denial_error(denied));
    }
    if denied.permission == Some(permissions::WRITE)
        && let Some(channel) = denied.channel_id
    {
        return lock(hub).deny_groups(server_id, channel);
    }
    false
}

/// The connection ended: drop what the tab held (FR-10.63).
pub(crate) fn on_disconnect(hub: &Mutex<Registered>, server_id: ServerId) {
    lock(hub).forget(server_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(user_id: u32, name: &str) -> RegisteredUser {
        RegisteredUser {
            user_id,
            name: Some(name.to_owned()),
            last_seen: None,
            last_channel: None,
        }
    }

    #[test]
    fn a_group_names_the_account_that_is_written_into_it() {
        let acl = proto::Acl {
            channel_id: 2,
            groups: vec![
                proto::acl::ChanGroup {
                    name: "officers".into(),
                    add: vec![7],
                    ..Default::default()
                },
                proto::acl::ChanGroup {
                    name: "raiders".into(),
                    inherited_members: vec![7],
                    ..Default::default()
                },
                proto::acl::ChanGroup {
                    name: "exiled".into(),
                    inherited_members: vec![7],
                    remove: vec![7],
                    ..Default::default()
                },
                proto::acl::ChanGroup {
                    name: "others".into(),
                    add: vec![9],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(group_names(&acl, 7), ["officers", "raiders"]);
        assert!(group_names(&acl, 8).is_empty());
    }

    fn view(user_id: u32, name: &str) -> RegisteredView {
        RegisteredView {
            user_id,
            name: name.to_owned(),
            last_seen: None,
            last_channel: None,
            online: false,
            cert_hash: None,
        }
    }

    #[test]
    fn a_rename_counts_as_taken_only_when_the_list_holds_the_new_name() {
        let entries = [view(7, "Tove L."), view(9, "Sigrid")];
        assert!(rename_took(&entries, 7, "Tove L."));
        assert!(
            !rename_took(&[view(7, "Tove")], 7, "Sigrid"),
            "the server kept the name it had, so the window must not claim the new one"
        );
        assert!(!rename_took(&entries, 8, "Whoever"), "an unknown account");
    }

    #[test]
    fn the_groups_are_read_where_the_account_was_last_seen() {
        let hub = Mutex::new(Registered::default());
        on_answer(&hub, 1, &[account(7, "Tove")]);
        // The slot's list is what the panel asks about, so the answer is put there first.
        lock(&hub).answer(
            1,
            &[{
                let mut entry = account(7, "Tove");
                entry.last_channel = Some(4);
                entry
            }],
        );
        assert_eq!(lock(&hub).last_channel(7), Some(4));
        assert_eq!(lock(&hub).last_channel(8), None, "an unknown account");
    }

    #[test]
    fn an_unregister_counts_as_taken_only_when_the_account_is_gone() {
        assert!(unregister_took(&[view(9, "Sigrid")], 7));
        assert!(!unregister_took(&[view(7, "Tove")], 7));
    }

    #[test]
    fn a_query_answer_lands_in_the_slot_and_ends_the_wait() {
        let hub = Mutex::new(Registered::default());
        let mut receiver = lock(&hub).expect(1);
        let list = vec![account(7, "Tove")];
        on_answer(&hub, 1, &list);
        let got = receiver.try_recv().expect("answered").expect("ok");
        assert_eq!(got, list);
    }

    #[test]
    fn a_refusal_ends_the_wait_with_the_reason() {
        let hub = Mutex::new(Registered::default());
        let mut receiver = lock(&hub).expect(1);
        lock(&hub).deny(1, denied("register right in the root channel"));
        let got = receiver.try_recv().expect("answered");
        assert_eq!(
            got.err().map(|error| error.kind),
            Some(ErrorKind::PermissionDenied)
        );
    }

    #[test]
    fn nothing_is_kept_after_the_tab_closes() {
        let hub = Mutex::new(Registered::default());
        lock(&hub).expect(2);
        on_answer(&hub, 2, &[account(7, "Tove")]);
        on_disconnect(&hub, 2);
        assert!(!lock(&hub).slots.contains_key(&2), "FR-10.63: memory only");
    }

    #[test]
    fn server_text_reaches_the_window_without_control_characters() {
        assert_eq!(clean("Tove\u{7} L."), "Tove L.");
        assert_eq!(clean(&"x".repeat(400)).len(), 200);
    }
}
