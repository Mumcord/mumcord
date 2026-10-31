//! Blocking users (issue #140, FR-3.97–FR-3.99).
//!
//! Everything happens on this side: the server and the blocked person are never told. Their
//! private messages and chat requests are dropped, their channel messages arrive without
//! unread count (the UI collapses or hides them), their voice is muted in the mixer without
//! touching the user's own "Mute for me", and their Mumcord plugin data is ignored.
//!
//! Blocks are kept per own identity (FR-3.79). The list is held in memory so the checks on
//! every incoming message and plugin packet do not query the database.

use std::sync::Mutex;

use mumcord_protocol::{PluginData, ServerState, User};
use mumcord_store::{BlockedUser, ServerId, Store};

use crate::chat::now_millis;
use crate::session::{
    Engine, EngineError, EngineEvent, EngineResult, ErrorKind, SessionCtx, lock, store_error,
};

/// The blocks of every own identity, as stored.
pub(crate) type BlockList = Mutex<Vec<BlockedUser>>;

/// The stored blocks; an unreadable list blocks nobody rather than stopping the engine.
pub(crate) fn load(store: &Store) -> Vec<BlockedUser> {
    store.blocked_users().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "reading blocked users failed");
        Vec::new()
    })
}

/// Whether any block in `list` covers a person seen on `server` while connected as `own`.
/// Without an own certificate no block applies: blocks belong to an identity.
pub(crate) fn blocks(
    list: &[BlockedUser],
    own: Option<&str>,
    server: ServerId,
    cert_hash: Option<&str>,
    name: &str,
) -> bool {
    own.is_some_and(|own| list.iter().any(|b| b.covers(own, server, cert_hash, name)))
}

fn own_cert_hash(state: &ServerState) -> Option<&str> {
    state.own_user().and_then(|u| u.cert_hash.as_deref())
}

/// Whether `user` on the server of `state` is blocked.
pub(crate) fn user_blocked(
    list: &[BlockedUser],
    state: &ServerState,
    server: ServerId,
    user: &User,
) -> bool {
    state.session != Some(user.session)
        && blocks(
            list,
            own_cert_hash(state),
            server,
            user.cert_hash.as_deref(),
            &user.name,
        )
}

/// Whether the person with `session` on the server of `ctx` is blocked.
pub(crate) fn is_blocked(ctx: &SessionCtx, list: &BlockList, session: u32) -> bool {
    let list = lock(list);
    if list.is_empty() {
        return false;
    }
    ctx.handle.with_state(|s| {
        s.users
            .get(&session)
            .is_some_and(|u| user_blocked(&list, s, ctx.server_id, u))
    })
}

/// Plugin data from a blocked person is ignored as a whole (FR-3.98).
pub(crate) fn sent_by_blocked(ctx: &SessionCtx, list: &BlockList, data: &PluginData) -> bool {
    data.sender.is_some_and(|s| is_blocked(ctx, list, s))
}

/// What happens to an incoming text message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextVerdict {
    /// The usual handling.
    Normal,
    /// Shown without unread count; the UI collapses or hides it and stays silent.
    Quiet,
    Drop,
}

pub(crate) fn text_verdict(private: bool, blocked: bool) -> TextVerdict {
    match (blocked, private) {
        (false, _) => TextVerdict::Normal,
        (true, true) => TextVerdict::Drop,
        (true, false) => TextVerdict::Quiet,
    }
}

/// The local mute the mixer gets: the user's own "Mute for me" or a block. A block never
/// changes the stored "Mute for me".
pub(crate) fn voice_muted(local_mute: bool, blocked: bool) -> bool {
    local_mute || blocked
}

/// Sessions of `users` whose block state differs between `before` and `after`, with the new
/// state; only these need a mixer update.
pub(crate) fn changed_sessions(
    before: &[BlockedUser],
    after: &[BlockedUser],
    state: &ServerState,
    server: ServerId,
) -> Vec<(u32, bool)> {
    state
        .users
        .values()
        .filter_map(|u| {
            let now = user_blocked(after, state, server, u);
            (user_blocked(before, state, server, u) != now).then_some((u.session, now))
        })
        .collect()
}

impl Engine {
    /// Blocked users of every own identity, by name.
    pub fn blocked_users(&self) -> EngineResult<Vec<BlockedUser>> {
        lock(&self.store_handle())
            .blocked_users()
            .map_err(store_error)
    }

    /// Blocks the person with `session` on `server_id`: by certificate when they have one,
    /// else by name on this server.
    pub fn block_session(&self, server_id: ServerId, session: u32) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let (own, cert_hash, name) = ctx
            .handle
            .with_state(|s| {
                let own = own_cert_hash(s)?.to_owned();
                (s.session != Some(session))
                    .then(|| s.users.get(&session))
                    .flatten()
                    .map(|u| (own, u.cert_hash.clone(), u.name.clone()))
            })
            .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown person"))?;
        self.change_blocks(
            |store, label| match &cert_hash {
                Some(hash) => store.block_cert(&own, hash, &name, server_id, label, now_millis()),
                None => store.block_name(&own, &name, server_id, label, now_millis()),
            },
            server_id,
        )
    }

    /// Blocks a person by certificate who need not be online, e.g. from the private messages
    /// overview. The own identity is the one `server_id` connects with.
    pub fn block_cert(&self, server_id: ServerId, cert_hash: &str, name: &str) -> EngineResult<()> {
        if cert_hash.is_empty() {
            return Err(EngineError::invalid("a certificate hash is needed"));
        }
        let own = self.server_identity_hash(server_id)?;
        self.change_blocks(
            |store, label| store.block_cert(&own, cert_hash, name, server_id, label, now_millis()),
            server_id,
        )
    }

    /// Removes a block; an unknown id is a no-op.
    pub fn unblock(&self, id: i64) -> EngineResult<()> {
        let before = lock(self.blocked_state()).clone();
        let list = {
            let handle = self.store_handle();
            let store = lock(&handle);
            store.unblock(id).map_err(store_error)?;
            store.blocked_users().map_err(store_error)?
        };
        self.blocks_changed(&before, list);
        Ok(())
    }

    /// Certificate hash of the identity `server_id` connects with.
    fn server_identity_hash(&self, server_id: ServerId) -> EngineResult<String> {
        let handle = self.store_handle();
        let store = lock(&handle);
        let server = store
            .server(server_id)
            .map_err(store_error)?
            .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown server"))?;
        let identity = match server.identity_id {
            Some(id) => store.identity(id),
            None => store.default_identity(),
        }
        .map_err(store_error)?
        .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "no identity for this server"))?;
        Ok(identity.cert_hash)
    }

    fn change_blocks(
        &self,
        change: impl FnOnce(&Store, &str) -> mumcord_store::Result<()>,
        server_id: ServerId,
    ) -> EngineResult<()> {
        let before = lock(self.blocked_state()).clone();
        let list = {
            let handle = self.store_handle();
            let store = lock(&handle);
            let label = store
                .server(server_id)
                .map_err(store_error)?
                .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown server"))?
                .label;
            change(&store, &label).map_err(store_error)?;
            store.blocked_users().map_err(store_error)?
        };
        self.blocks_changed(&before, list);
        Ok(())
    }

    /// Applies a new block list: open requests of blocked people go away, voice is muted or
    /// unmuted, and the UI gets the list.
    fn blocks_changed(&self, before: &[BlockedUser], list: Vec<BlockedUser>) {
        lock(self.blocked_state()).clone_from(&list);
        for ctx in self.session_ctxs() {
            let own = ctx
                .handle
                .with_state(|s| own_cert_hash(s).map(str::to_owned));
            let server = ctx.server_id;
            crate::pm_rules::drop_blocked(&ctx, self.pm_requests_state(), |cert_hash, name| {
                blocks(&list, own.as_deref(), server, cert_hash, name)
            });
        }
        self.apply_block_mutes(before, &list);
        self.events().emit(EngineEvent::BlockedUsersChanged(list));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(own: &str, cert_hash: Option<&str>, name: &str, server: ServerId) -> BlockedUser {
        BlockedUser {
            id: 1,
            own_cert_hash: own.into(),
            cert_hash: cert_hash.map(Into::into),
            name: name.into(),
            server_id: Some(server),
            server_label: "Home".into(),
            blocked_at: 0,
        }
    }

    fn user(session: u32, name: &str, cert_hash: Option<&str>) -> User {
        User {
            session,
            name: name.into(),
            cert_hash: cert_hash.map(Into::into),
            ..User::default()
        }
    }

    fn state(users: &[User]) -> ServerState {
        let mut state = ServerState {
            session: Some(1),
            ..ServerState::default()
        };
        for u in users {
            state.users.insert(u.session, u.clone());
        }
        state
    }

    #[test]
    fn blocks_need_the_own_identity() {
        let list = [block("me", Some("bob"), "Bob", 1)];
        assert!(blocks(&list, Some("me"), 2, Some("bob"), "Renamed"));
        assert!(!blocks(&list, Some("alt"), 1, Some("bob"), "Bob"));
        assert!(!blocks(&list, None, 1, Some("bob"), "Bob"));
    }

    #[test]
    fn private_messages_are_dropped_and_channel_messages_kept_quiet() {
        assert_eq!(text_verdict(true, true), TextVerdict::Drop);
        assert_eq!(text_verdict(false, true), TextVerdict::Quiet);
        assert_eq!(text_verdict(true, false), TextVerdict::Normal);
        assert_eq!(text_verdict(false, false), TextVerdict::Normal);
    }

    #[test]
    fn voice_is_muted_without_touching_mute_for_me() {
        assert!(voice_muted(false, true));
        assert!(voice_muted(true, false));
        assert!(!voice_muted(false, false));
    }

    #[test]
    fn the_own_user_and_others_are_told_apart() {
        let users = [
            user(1, "Me", Some("me")),
            user(2, "Bob", Some("bob")),
            user(3, "Guest", None),
            user(4, "Carol", Some("carol")),
        ];
        let state = state(&users);
        let list = [
            block("me", Some("bob"), "Bob", 7),
            block("me", None, "guest", 7),
        ];
        assert!(user_blocked(&list, &state, 7, &users[1]));
        assert!(user_blocked(&list, &state, 7, &users[2]));
        assert!(!user_blocked(&list, &state, 8, &users[2]));
        assert!(!user_blocked(&list, &state, 7, &users[3]));
        // Even a block on the own certificate never silences the user themselves.
        let own = [block("me", Some("me"), "Me", 7)];
        assert!(!user_blocked(&own, &state, 7, &users[0]));
    }

    #[test]
    fn only_changed_people_get_a_mixer_update() {
        let users = [
            user(1, "Me", Some("me")),
            user(2, "Bob", Some("bob")),
            user(3, "Carol", Some("carol")),
        ];
        let state = state(&users);
        let bob = block("me", Some("bob"), "Bob", 7);
        let carol = block("me", Some("carol"), "Carol", 7);
        let before = [bob.clone()];
        let after = [carol];
        let mut changed = changed_sessions(&before, &after, &state, 7);
        changed.sort_unstable();
        assert_eq!(changed, vec![(2, false), (3, true)]);
        assert!(changed_sessions(&before, &before, &state, 7).is_empty());
    }
}
