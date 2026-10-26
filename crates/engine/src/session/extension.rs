//! Extension point for chat, follow, history and profile (task B14).
//!
//! The session task calls these hooks; B14 adds its module calls here instead of touching
//! the connection lifecycle. Use [`SessionCtx`] for the protocol handle, the store and event
//! output, and `Engine::session_ctx_for` from commands.

use mumcord_protocol::Event;

use super::{SessionCtx, Snapshot};
use crate::{chat, follow};

/// Every protocol event of the connected session, after the engine handled it.
pub(crate) fn on_event(ctx: &SessionCtx, event: &Event) {
    match event {
        Event::TextMessage(message) => chat::on_text(ctx, message),
        Event::PermissionDenied(denied) => follow::publish_denial(ctx.server_id, denied),
        _ => {}
    }
}

/// After every (re)sync, before `ServerSynced` is emitted; may fill e.g. `snapshot.unread`.
/// The session's core lock is held here: only use `ctx` (handle, store, events).
pub(crate) fn on_synced(ctx: &SessionCtx, snapshot: &mut Snapshot) {
    chat::on_synced(ctx, snapshot);
}
