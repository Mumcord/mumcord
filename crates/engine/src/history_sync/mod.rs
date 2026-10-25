//! History sync between Mumcord clients (FR-1.60, FR-1.60b, QA.48–QA.50).
//!
//! Mumble servers keep no chat history, so Mumcord clients fill each other's gaps through
//! plugin data (no server changes, no private protocol). When the user joins or follows a
//! channel, or asks for earlier messages, the engine asks the other people on the server for
//! the channel's messages before the moment it started receiving the channel live, fetches
//! only what it does not have yet and stores it marked with the holder's name. See [`core`]
//! for the flow and limits, [`wire`] for the packets and [`hash`] for deduplication.
//!
//! The session task feeds user changes and plugin data in; a small driver task per
//! connection runs timeouts and sends the paced outgoing packets.

mod core;
pub mod hash;
pub mod wire;

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use mumcord_protocol::{CommandError, PluginData, User};
use mumcord_store::{ServerId, Store, UnixMillis};
use tokio::runtime::Handle as Runtime;
use tokio::task::JoinHandle;

use self::core::{Core, Env, RequestError};
use crate::chat::now_millis;
use crate::session::convert::join_path;
use crate::session::{Engine, EngineError, EngineResult, ErrorKind, SessionCtx, lock};
pub use wire::DATA_ID;

/// How often timeouts are checked and queued packets go out.
const TICK: Duration = Duration::from_millis(100);

/// The engine's history sync state; one [`Core`] per connection.
#[derive(Default)]
pub(crate) struct HistorySync {
    core: Option<Core>,
    driver: Option<JoinHandle<()>>,
}

struct CtxEnv<'a>(&'a SessionCtx);

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

    fn own_cert(&self) -> Option<String> {
        self.0
            .handle
            .with_state(|s| s.own_user().and_then(|u| u.cert_hash.clone()))
    }

    fn channel_key(&self, channel: u32) -> Option<String> {
        self.0.channel_path(channel).map(|p| join_path(&p))
    }
}

fn live_channels(user: &User) -> impl Iterator<Item = u32> + '_ {
    std::iter::once(user.channel_id).chain(user.listening.iter().copied())
}

/// Runs `f` on the connection's core and emits the events it produced.
fn with_core(state: &Mutex<HistorySync>, ctx: &SessionCtx, f: impl FnOnce(&mut Core)) {
    let events = {
        let mut state = lock(state);
        let Some(core) = state.core.as_mut() else {
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
    ctx.handle.with_state(|s| {
        for user in s.users.values() {
            core.user_seen(user.session, &user.name, live_channels(user), now);
        }
    });
    let mut guard = lock(state);
    if let Some(driver) = guard.driver.take() {
        driver.abort();
    }
    guard.core = Some(core);
    guard.driver = Some(rt.spawn(drive(state.clone(), ctx.clone())));
}

/// The connection ended.
pub(crate) fn on_disconnect(state: &Mutex<HistorySync>) {
    let mut guard = lock(state);
    guard.core = None;
    if let Some(driver) = guard.driver.take() {
        driver.abort();
    }
}

/// Someone joined, moved, followed or unfollowed (own user included).
pub(crate) fn on_user(state: &Mutex<HistorySync>, ctx: &SessionCtx, user: &User) {
    let now = now_millis();
    with_core(state, ctx, |core| {
        core.user_seen(user.session, &user.name, live_channels(user), now);
    });
}

pub(crate) fn on_user_removed(state: &Mutex<HistorySync>, session: u32) {
    if let Some(core) = lock(state).core.as_mut() {
        core.user_left(session);
    }
}

pub(crate) fn on_plugin_data(state: &Mutex<HistorySync>, ctx: &SessionCtx, data: &PluginData) {
    if data.data_id != DATA_ID {
        return;
    }
    let Some(sender) = data.sender else {
        return;
    };
    let now = now_millis();
    with_core(state, ctx, |core| {
        core.on_packet(&CtxEnv(ctx), sender, &data.data, now);
    });
}

/// Timeouts, the next queued channel and paced sending, until the connection ends.
async fn drive(state: Arc<Mutex<HistorySync>>, ctx: SessionCtx) {
    let mut tick = tokio::time::interval(TICK);
    loop {
        tick.tick().await;
        let now = now_millis();
        let mut outgoing = Vec::new();
        with_core(&state, &ctx, |core| {
            core.tick(&CtxEnv(&ctx), now);
            outgoing = core.take_outgoing(now);
        });
        for (receivers, data) in outgoing {
            match ctx.handle.send_plugin_data(&receivers, DATA_ID, data) {
                Ok(()) => {}
                Err(CommandError::NotConnected) => return,
                Err(e @ CommandError::Unsupported { .. }) => {
                    // Servers before 1.4 cannot forward plugin data: no history sync there.
                    tracing::info!(error = %e, "history sync unavailable on this server");
                    lock(&state).core = None;
                    return;
                }
            }
        }
    }
}

impl Engine {
    /// "Load earlier messages" (FR-1.60): asks other Mumcord users for the channel's messages
    /// before `before` (Unix ms; `None`: up to when the channel started arriving live). The
    /// channel must be the own or a followed one. Progress arrives as `HistorySyncStatus`,
    /// messages as `HistorySynced` events.
    pub fn history_sync_request(
        &self,
        channel_id: u32,
        before: Option<UnixMillis>,
    ) -> EngineResult<()> {
        self.session_ctx()?;
        let mut state = lock(self.history_sync_state());
        let core = state.core.as_mut().ok_or_else(|| {
            EngineError::new(
                ErrorKind::ServerTooOld,
                "history sync needs a Mumble 1.4+ server",
            )
        })?;
        core.request(channel_id, before, now_millis())
            .map_err(|e| match e {
                RequestError::NotLive => EngineError::new(
                    ErrorKind::NotFound,
                    "channel is neither joined nor followed",
                ),
            })
    }
}

#[cfg(test)]
mod tests;
