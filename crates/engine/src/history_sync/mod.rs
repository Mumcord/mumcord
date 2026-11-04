//! History sync between Mumcord clients (FR-1.60, FR-1.60b, QA.48–QA.50).
//!
//! Mumble servers keep no chat history, so Mumcord clients fill each other's gaps through
//! plugin data (no server changes, no private protocol). When the user joins or follows a
//! channel, or asks for earlier messages, the engine asks the other people on the server for
//! the channel's messages before the moment it started receiving the channel live, fetches
//! only what it does not have yet and stores it marked with the holder's name. See [`core`]
//! for the flow and limits, [`wire`] for the packets, [`hash`] for deduplication and
//! [`proof`] for the password proof of locked channels.
//!
//! The session task feeds user changes and plugin data in; a small driver task per
//! connection runs timeouts and sends the paced outgoing packets.

mod core;
mod fetch;
pub mod hash;
pub mod images;
pub mod proof;
pub mod wire;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use mumcord_protocol::{CommandError, PluginData, ServerState, User, permissions};
use mumcord_store::{ServerId, Store, UnixMillis};
use tokio::runtime::Handle as Runtime;
use tokio::task::JoinHandle;

use self::core::{Core, Env, RequestError};
use crate::chat::now_millis;
use crate::mumcord::ServerPeers;
use crate::session::convert::join_path;
use crate::session::{Engine, EngineError, EngineResult, ErrorKind, SessionCtx, lock};
pub(crate) use fetch::HistoryImages;
pub use wire::DATA_ID;

/// How often timeouts are checked and queued packets go out.
const TICK: Duration = Duration::from_millis(100);

/// The engine's history sync state: one [`Connection`] per connected server, so every server
/// syncs with its own Mumcord users (FR-2.4).
pub(crate) type HistorySync = HashMap<ServerId, Connection>;

/// History sync on one connection.
pub(crate) struct Connection {
    /// `None` once the server turned out to be too old for plugin data.
    core: Option<Core>,
    driver: JoinHandle<()>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

/// The connection, and the Mumcord hellos where a decision depends on the peer's features.
struct CtxEnv<'a>(&'a SessionCtx, Option<&'a ServerPeers>);

impl Env for CtxEnv<'_> {
    fn server_id(&self) -> ServerId {
        self.0.server_id
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        self.0.store()
    }

    fn sharing(&self) -> bool {
        self.0.store().settings().map_or_else(
            |e| {
                tracing::warn!(error = %e, "reading settings failed; not sharing history");
                false
            },
            |s| s.history_sharing,
        )
    }

    fn history_max_age_days(&self) -> Option<u32> {
        self.0.store().settings().map_or_else(
            |e| {
                tracing::warn!(error = %e, "reading settings failed; syncing the full week");
                None
            },
            |s| s.history_max_age_days,
        )
    }

    fn own_cert(&self) -> Option<String> {
        self.0
            .handle
            .with_state(|s| s.own_user().and_then(|u| u.cert_hash.clone()))
    }

    fn channel_key(&self, channel: u32) -> Option<String> {
        self.0.channel_path(channel).map(|p| join_path(&p))
    }

    fn channel_locked(&self, channel: u32) -> bool {
        self.0.handle.with_state(|s| channel_locked(s, channel))
    }

    fn own_tokens(&self) -> Vec<String> {
        self.0.handle.access_tokens()
    }

    fn query_own_permissions(&self, channel: u32) {
        if let Err(e) = self.0.handle.query_permissions(channel) {
            tracing::warn!(error = %e, "history sync: asking for the own permissions failed");
        }
    }

    fn resolves_images(&self, session: u32) -> bool {
        self.1.is_some_and(|peers| {
            lock(peers)
                .get(&self.0.server_id)
                .and_then(|p| p.features(session))
                .is_some_and(|f| f.iter().any(|x| x == images::FEATURE))
        })
    }

    fn offer_images(&self) -> bool {
        self.0.store().settings().map_or_else(
            |e| {
                tracing::warn!(error = %e, "reading settings failed; not offering images");
                false
            },
            |s| s.offer_full_images,
        )
    }
}

/// Whether `channel` or one of its parents restricts entering (issue #382). The server flags
/// only channels whose own ACL restricts Enter, so a sub-channel inheriting a deny from its
/// parent is found through the parent. Unknown channels count as locked.
fn channel_locked(state: &ServerState, channel: u32) -> bool {
    let mut next = Some(channel);
    // The depth bound guards against a malformed (cyclic) tree from the server.
    for _ in 0..=state.channels.len() {
        let Some(id) = next else {
            return false;
        };
        let Some(c) = state.channels.get(&id) else {
            return true;
        };
        if c.is_enter_restricted {
            return true;
        }
        next = c.parent;
    }
    true
}

fn see(core: &mut Core, env: &impl Env, user: &User, now: i64) {
    core.user_seen(
        env,
        user.session,
        &user.name,
        user.cert_hash.as_deref(),
        user.channel_id,
        user.listening.iter().copied(),
        now,
    );
}

/// Runs `f` on the connection's core and emits the events it produced.
fn with_core(state: &Mutex<HistorySync>, ctx: &SessionCtx, f: impl FnOnce(&mut Core)) {
    let events = {
        let mut state = lock(state);
        let Some(core) = state.get_mut(&ctx.server_id).and_then(|c| c.core.as_mut()) else {
            return;
        };
        f(core);
        core.take_events()
    };
    for event in events {
        ctx.events.emit(event);
    }
}

/// After every (re)sync: a fresh core that knows everyone; queues the own channels.
pub(crate) fn on_synced(state: &Arc<Mutex<HistorySync>>, ctx: &SessionCtx, rt: &Runtime) {
    let now = now_millis();
    let Some(own) = ctx.handle.with_state(|s| s.session) else {
        return;
    };
    let mut core = Core::new(own, now);
    // Collected first: the environment reads the state again.
    let users: Vec<User> = ctx
        .handle
        .with_state(|s| s.users.values().cloned().collect());
    for user in &users {
        see(&mut core, &CtxEnv(ctx, None), user, now);
    }
    let connection = Connection {
        core: Some(core),
        driver: rt.spawn(drive(state.clone(), ctx.clone())),
    };
    // Replacing the connection of an earlier sync stops its driver.
    lock(state).insert(ctx.server_id, connection);
}

/// The connection to `server_id` ended.
pub(crate) fn on_disconnect(state: &Mutex<HistorySync>, server_id: ServerId) {
    lock(state).remove(&server_id);
}

/// Someone joined, moved, followed or unfollowed (own user included).
pub(crate) fn on_user(state: &Mutex<HistorySync>, ctx: &SessionCtx, user: &User) {
    let now = now_millis();
    with_core(state, ctx, |core| see(core, &CtxEnv(ctx, None), user, now));
}

/// The server answered a PermissionQuery of the own user for `channel_id`.
pub(crate) fn on_permissions(state: &Mutex<HistorySync>, ctx: &SessionCtx, channel_id: u32) {
    let Some(bits) = ctx
        .handle
        .with_state(|s| s.permissions.get(&channel_id).copied())
    else {
        return;
    };
    let now = now_millis();
    with_core(state, ctx, |core| {
        core.own_permissions(channel_id, bits & permissions::ENTER != 0, now);
    });
}

pub(crate) fn on_user_removed(state: &Mutex<HistorySync>, server_id: ServerId, session: u32) {
    if let Some(core) = lock(state)
        .get_mut(&server_id)
        .and_then(|c| c.core.as_mut())
    {
        core.user_left(session);
    }
}

pub(crate) fn on_plugin_data(
    state: &Mutex<HistorySync>,
    images: &HistoryImages,
    peers: &ServerPeers,
    ctx: &SessionCtx,
    data: &PluginData,
) {
    if data.data_id != DATA_ID {
        return;
    }
    let Some(sender) = data.sender else {
        return;
    };
    let now = now_millis();
    let mut fetches = Vec::new();
    with_core(state, ctx, |core| {
        core.on_packet(&CtxEnv(ctx, Some(peers)), sender, &data.data, now);
        fetches = core.take_image_fetches();
    });
    images.fetch(ctx.server_id, fetches);
}

/// The bytes of an image that `requester` asks for by hash through the full-image path, if a
/// history answer sent it a message containing that image and it may still read it
/// (FR-1.60g).
pub(crate) fn image_for(
    state: &Mutex<HistorySync>,
    ctx: &SessionCtx,
    requester: u32,
    hash: &[u8; 32],
) -> Option<Vec<u8>> {
    let mut state = lock(state);
    let core = state.get_mut(&ctx.server_id)?.core.as_mut()?;
    core.image_for(&CtxEnv(ctx, None), requester, hash, now_millis())
        .map(|bytes| bytes.as_ref().clone())
}

/// Timeouts, the next queued channel and paced sending, until the connection ends.
async fn drive(state: Arc<Mutex<HistorySync>>, ctx: SessionCtx) {
    let mut tick = tokio::time::interval(TICK);
    loop {
        tick.tick().await;
        let now = now_millis();
        let mut outgoing = Vec::new();
        with_core(&state, &ctx, |core| {
            core.tick(&CtxEnv(&ctx, None), now);
            outgoing = core.take_outgoing(now);
        });
        for (receivers, data) in outgoing {
            match ctx.handle.send_plugin_data(&receivers, DATA_ID, data) {
                Ok(()) => {}
                Err(CommandError::NotConnected) => return,
                Err(e @ CommandError::Unsupported { .. }) => {
                    // Servers before 1.4 cannot forward plugin data: no history sync there.
                    tracing::info!(error = %e, "history sync unavailable on this server");
                    if let Some(c) = lock(&state).get_mut(&ctx.server_id) {
                        c.core = None;
                    }
                    return;
                }
            }
        }
    }
}

impl Engine {
    /// "Load earlier messages" (FR-1.60): asks other Mumcord users on `server_id` for the
    /// channel's messages before `before` (Unix ms; `None`: up to when the channel started
    /// arriving live). The channel must be the own or a followed one; a locked channel only
    /// the own one (issue #382). Progress arrives as `HistorySyncStatus`, messages as
    /// `HistorySynced` events.
    pub fn history_sync_request(
        &self,
        server_id: ServerId,
        channel_id: u32,
        before: Option<UnixMillis>,
    ) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let mut state = lock(self.history_sync_state());
        let core = state
            .get_mut(&server_id)
            .and_then(|c| c.core.as_mut())
            .ok_or_else(|| {
                EngineError::new(
                    ErrorKind::ServerTooOld,
                    "history sync needs a Mumble 1.4+ server",
                )
            })?;
        core.request(&CtxEnv(&ctx, None), channel_id, before, now_millis())
            .map_err(|e| match e {
                RequestError::NotLive => EngineError::new(
                    ErrorKind::NotFound,
                    "channel is neither joined nor followed",
                ),
                RequestError::Locked => EngineError::new(
                    ErrorKind::PermissionDenied,
                    "the history of a locked channel is shared only with people inside it",
                ),
            })
    }
}

#[cfg(test)]
mod tests;
