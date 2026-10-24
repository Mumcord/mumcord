//! Extension point for chat, follow, history and profile (task B14).
//!
//! The session task calls these hooks; B14 adds its module calls here instead of touching
//! the connection lifecycle. Use [`SessionCtx`] for the protocol handle, the store and event
//! output, and `Engine::session_ctx` from commands.

use mumcord_protocol::Event;

use super::{SessionCtx, Snapshot};

/// Every protocol event of the connected session, after the engine handled it.
pub(crate) fn on_event(_ctx: &SessionCtx, _event: &Event) {}

/// After every (re)sync, before `ServerSynced` is emitted; may fill e.g. `snapshot.unread`.
pub(crate) fn on_synced(_ctx: &SessionCtx, _snapshot: &mut Snapshot) {}
