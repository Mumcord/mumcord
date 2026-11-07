//! Create, edit, link and delete channels (Epic 4, issue #250).
//!
//! Every change is one standard message: `ChannelState` without a channel id creates a channel,
//! with one it edits, moves or links it, and `ChannelRemove` deletes it. The server has the final
//! say, so each call waits for its answer: the channel's new state, its removal, or a refusal
//! (FR-10.59). The client hides what the user may not do (FR-10.2) but never checks rights itself,
//! so a forced attempt shows the server's own reason.
//!
//! Nothing here goes to disk (FR-10.63). Logs name the action and its outcome only (FR-10.64).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use mumcord_protocol::Channel;
use mumcord_protocol::{PermissionDenied, permissions, proto};
use tokio::sync::oneshot;

use crate::ServerId;
use crate::session::{
    DenialKind, Engine, EngineError, EngineResult, Right, SessionCtx, convert, lock,
};

/// How long a dialog waits for the server's answer, as the other admin dialogs do.
pub const ANSWER_WAIT: Duration = crate::admin::ANSWER_WAIT;

/// Rights a channel change can be refused for. Other refusals (joining, speaking, kick, ban)
/// belong to something else.
const CHANNEL_RIGHTS: u32 = permissions::WRITE
    | permissions::MAKE_CHANNEL
    | permissions::MAKE_TEMP_CHANNEL
    | permissions::LINK_CHANNEL;

/// A new channel (FR-10.21, FR-10.22).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelCreate {
    pub parent: u32,
    pub name: String,
    /// Clean HTML from the description editor (FR-10.26).
    pub description: Option<String>,
    pub position: i32,
    /// 0 is the server's own limit.
    pub max_users: u32,
    /// The server deletes it when the last person leaves and moves the creator into it.
    pub temporary: bool,
}

/// What an edit changes; `None` leaves a field as it is (FR-10.23, FR-10.24, FR-10.27).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChannelChange {
    pub name: Option<String>,
    pub description: Option<String>,
    /// A new parent moves the channel with all its sub-channels.
    pub parent: Option<u32>,
    pub position: Option<i32>,
    pub max_users: Option<u32>,
    pub links_add: Vec<u32>,
    pub links_remove: Vec<u32>,
}

impl ChannelChange {
    fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.description.is_none()
            && self.parent.is_none()
            && self.position.is_none()
            && self.max_users.is_none()
            && self.links_add.is_empty()
            && self.links_remove.is_empty()
    }
}

/// The server's answer to a channel change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelOutcome {
    /// The server applied it; `channel_id` is the channel (the new one after a create).
    Confirmed { channel_id: u32 },
    /// The server answered nothing within [`ANSWER_WAIT`]; the message was written to it.
    Unconfirmed,
    /// The server refused, with its reason (FR-10.59, FR-10.60).
    Refused {
        kind: DenialKind,
        permission: Option<Right>,
        reason: Option<String>,
    },
}

/// What a waiting dialog expects to see.
enum Target {
    /// A channel with this name appearing under this parent.
    Created { parent: u32, name: String },
    /// A new state of this channel.
    Updated(u32),
    /// This channel going away.
    Removed(u32),
}

struct Waiter {
    target: Target,
    answer: oneshot::Sender<ChannelOutcome>,
}

/// The channel dialogs waiting for an answer, per connection.
#[derive(Default)]
pub struct ChannelAdmin {
    waiting: HashMap<ServerId, Vec<Waiter>>,
}

impl ChannelAdmin {
    fn expect(&mut self, server_id: ServerId, target: Target) -> oneshot::Receiver<ChannelOutcome> {
        let (answer, receiver) = oneshot::channel();
        self.waiting
            .entry(server_id)
            .or_default()
            .push(Waiter { target, answer });
        receiver
    }

    /// Answers the first waiter `matches` picks.
    fn answer(
        &mut self,
        server_id: ServerId,
        matches: impl Fn(&Target) -> bool,
        outcome: ChannelOutcome,
    ) -> bool {
        let Some(waiters) = self.waiting.get_mut(&server_id) else {
            return false;
        };
        let Some(index) = waiters.iter().position(|w| matches(&w.target)) else {
            return false;
        };
        let waiter = waiters.remove(index);
        let _ = waiter.answer.send(outcome);
        true
    }
}

impl Engine {
    /// Creates a channel under `request.parent` (FR-10.21, FR-10.22).
    pub async fn channel_create(
        &self,
        server_id: ServerId,
        request: &ChannelCreate,
    ) -> EngineResult<ChannelOutcome> {
        let name = request.name.trim();
        if name.is_empty() {
            return Err(EngineError::invalid("a channel needs a name"));
        }
        let ctx = self.session_ctx_for(server_id)?;
        let state = proto::ChannelState {
            parent: Some(request.parent),
            name: Some(name.to_owned()),
            description: request.description.clone(),
            position: Some(request.position),
            max_users: Some(request.max_users),
            temporary: Some(request.temporary),
            ..Default::default()
        };
        let target = Target::Created {
            parent: request.parent,
            name: name.to_owned(),
        };
        let outcome = self
            .send_and_wait(&ctx, target, Message::State(state))
            .await;
        report(&ctx, "create channel", &outcome);
        outcome
    }

    /// Edits, moves or links a channel; only the fields set in `change` are sent (FR-10.23,
    /// FR-10.24, FR-10.27).
    pub async fn channel_edit(
        &self,
        server_id: ServerId,
        channel_id: u32,
        change: &ChannelChange,
    ) -> EngineResult<ChannelOutcome> {
        if change.is_empty() {
            return Ok(ChannelOutcome::Confirmed { channel_id });
        }
        if change.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
            return Err(EngineError::invalid("a channel needs a name"));
        }
        if change.parent == Some(channel_id) {
            return Err(EngineError::invalid("a channel cannot be its own parent"));
        }
        let ctx = self.session_ctx_for(server_id)?;
        let state = proto::ChannelState {
            channel_id: Some(channel_id),
            name: change.name.as_deref().map(|n| n.trim().to_owned()),
            description: change.description.clone(),
            parent: change.parent,
            position: change.position,
            max_users: change.max_users,
            links_add: change.links_add.clone(),
            links_remove: change.links_remove.clone(),
            ..Default::default()
        };
        let outcome = self
            .send_and_wait(&ctx, Target::Updated(channel_id), Message::State(state))
            .await;
        report(&ctx, "edit channel", &outcome);
        outcome
    }

    /// Deletes a channel with its sub-channels; the server moves the people in them to the
    /// parent (FR-10.25).
    pub async fn channel_remove(
        &self,
        server_id: ServerId,
        channel_id: u32,
    ) -> EngineResult<ChannelOutcome> {
        if channel_id == 0 {
            return Err(EngineError::invalid("the root channel cannot be deleted"));
        }
        let ctx = self.session_ctx_for(server_id)?;
        let outcome = self
            .send_and_wait(
                &ctx,
                Target::Removed(channel_id),
                Message::Remove(channel_id),
            )
            .await;
        report(&ctx, "delete channel", &outcome);
        outcome
    }

    /// Registers the wait first, so a fast answer cannot miss it, then writes the message.
    async fn send_and_wait(
        &self,
        ctx: &SessionCtx,
        target: Target,
        message: Message,
    ) -> EngineResult<ChannelOutcome> {
        let receiver = { lock(self.channel_admin_state()).expect(ctx.server_id, target) };
        let sent = match message {
            Message::State(state) => ctx.handle.send_channel_state(state),
            Message::Remove(channel_id) => ctx.handle.send_channel_remove(channel_id),
        };
        sent.map_err(convert::command_error)?;
        match tokio::time::timeout(ANSWER_WAIT, receiver).await {
            Ok(Ok(outcome)) => Ok(outcome),
            // The wait was dropped, which happens when the connection ends.
            Ok(Err(_)) => Err(EngineError::not_connected()),
            Err(_) => Ok(ChannelOutcome::Unconfirmed),
        }
    }
}

enum Message {
    State(proto::ChannelState),
    Remove(u32),
}

fn report(ctx: &SessionCtx, action: &str, outcome: &EngineResult<ChannelOutcome>) {
    // Only the kind of action and its outcome (FR-10.64): no channel names or descriptions.
    match outcome {
        Ok(ChannelOutcome::Confirmed { .. }) => {
            tracing::info!(server_id = ctx.server_id, action, "admin action: confirmed")
        }
        Ok(ChannelOutcome::Unconfirmed) => {
            tracing::info!(server_id = ctx.server_id, action, "admin action: no answer")
        }
        Ok(ChannelOutcome::Refused { kind, .. }) => {
            tracing::info!(
                server_id = ctx.server_id,
                action,
                ?kind,
                "admin action: refused"
            )
        }
        Err(error) => tracing::warn!(
            server_id = ctx.server_id,
            action,
            code = ?error.kind,
            "admin action: failed"
        ),
    }
}

/// A channel's new state answers the dialog that created or changed it.
pub(crate) fn on_channel(hub: &Mutex<ChannelAdmin>, server_id: ServerId, channel: &Channel) {
    let confirmed = ChannelOutcome::Confirmed {
        channel_id: channel.id,
    };
    lock(hub).answer(
        server_id,
        |target| match target {
            Target::Created { parent, name } => {
                channel.parent == Some(*parent) && channel.name == *name
            }
            Target::Updated(id) => *id == channel.id,
            Target::Removed(_) => false,
        },
        confirmed,
    );
}

/// A channel went away: the delete dialog waiting for it is answered.
pub(crate) fn on_removed(hub: &Mutex<ChannelAdmin>, server_id: ServerId, channel_id: u32) {
    lock(hub).answer(
        server_id,
        |target| matches!(target, Target::Removed(id) if *id == channel_id),
        ChannelOutcome::Confirmed { channel_id },
    );
}

/// A refusal of a channel change answers the oldest waiting dialog; `true` when one took it, so
/// the UI adds no second notice.
pub(crate) fn on_denied(
    hub: &Mutex<ChannelAdmin>,
    server_id: ServerId,
    denied: &PermissionDenied,
) -> bool {
    if !is_channel_refusal(denied) {
        return false;
    }
    let outcome = ChannelOutcome::Refused {
        kind: convert::denial_kind(denied),
        permission: denied.permission.and_then(convert::right_of),
        reason: denied.reason.clone(),
    };
    lock(hub).answer(server_id, |_| true, outcome)
}

/// Refusals a channel change gets: a missing channel right, or a rule about channels (name,
/// nesting, count, temporary). Joining (Enter), speaking or kicking are someone else's.
fn is_channel_refusal(denied: &PermissionDenied) -> bool {
    use mumcord_protocol::DenyKind;
    if denied.session.is_some() {
        return false;
    }
    match denied.kind {
        DenyKind::Permission => denied
            .permission
            .is_some_and(|bits| bits & CHANNEL_RIGHTS != 0),
        DenyKind::ChannelName
        | DenyKind::NestingLimit
        | DenyKind::ChannelCountLimit
        | DenyKind::TemporaryChannel
        | DenyKind::TextTooLong
        | DenyKind::Text
        | DenyKind::SuperUser => true,
        _ => false,
    }
}

/// The connection ended: every waiting dialog is told so.
pub(crate) fn on_disconnect(hub: &Mutex<ChannelAdmin>, server_id: ServerId) {
    lock(hub).waiting.remove(&server_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_protocol::DenyKind;

    fn channel(id: u32, parent: u32, name: &str) -> Channel {
        Channel {
            id,
            parent: Some(parent),
            name: name.to_owned(),
            ..Default::default()
        }
    }

    fn denial(kind: DenyKind, permission: Option<u32>) -> PermissionDenied {
        PermissionDenied {
            kind,
            permission,
            channel_id: Some(3),
            session: None,
            reason: None,
            name: None,
        }
    }

    #[test]
    fn a_new_channel_answers_the_create_with_its_id() {
        let hub = Mutex::new(ChannelAdmin::default());
        let mut rx = lock(&hub).expect(
            1,
            Target::Created {
                parent: 2,
                name: "Raid".into(),
            },
        );
        on_channel(&hub, 1, &channel(9, 3, "Raid"));
        assert!(rx.try_recv().is_err(), "another parent is not the answer");
        on_channel(&hub, 1, &channel(10, 2, "Raid"));
        assert_eq!(
            rx.try_recv().expect("answer"),
            ChannelOutcome::Confirmed { channel_id: 10 }
        );
    }

    #[test]
    fn edit_and_delete_wait_for_their_own_channel() {
        let hub = Mutex::new(ChannelAdmin::default());
        let mut edit = lock(&hub).expect(1, Target::Updated(4));
        let mut delete = lock(&hub).expect(1, Target::Removed(5));
        on_removed(&hub, 1, 4);
        on_channel(&hub, 1, &channel(5, 0, "Other"));
        assert!(edit.try_recv().is_err());
        assert!(delete.try_recv().is_err());
        on_channel(&hub, 1, &channel(4, 0, "Raid"));
        on_removed(&hub, 1, 5);
        assert!(matches!(
            edit.try_recv(),
            Ok(ChannelOutcome::Confirmed { .. })
        ));
        assert!(matches!(
            delete.try_recv(),
            Ok(ChannelOutcome::Confirmed { .. })
        ));
    }

    #[test]
    fn channel_refusals_answer_the_dialog_and_others_pass_by() {
        let hub = Mutex::new(ChannelAdmin::default());
        let mut rx = lock(&hub).expect(1, Target::Updated(3));
        assert!(!on_denied(
            &hub,
            1,
            &denial(DenyKind::Permission, Some(permissions::ENTER))
        ));
        assert!(rx.try_recv().is_err());
        assert!(on_denied(&hub, 1, &denial(DenyKind::ChannelName, None)));
        assert_eq!(
            rx.try_recv().expect("answer"),
            ChannelOutcome::Refused {
                kind: DenialKind::ChannelName,
                permission: None,
                reason: None,
            }
        );
        assert!(
            !on_denied(&hub, 1, &denial(DenyKind::ChannelName, None)),
            "nothing waits any more, so the usual notice shows"
        );
    }

    #[test]
    fn a_missing_write_right_names_the_right() {
        let hub = Mutex::new(ChannelAdmin::default());
        let mut rx = lock(&hub).expect(1, Target::Removed(3));
        assert!(on_denied(
            &hub,
            1,
            &denial(DenyKind::Permission, Some(permissions::WRITE))
        ));
        assert!(matches!(
            rx.try_recv().expect("answer"),
            ChannelOutcome::Refused {
                permission: Some(Right::Write),
                ..
            }
        ));
    }

    #[test]
    fn a_disconnect_ends_every_wait() {
        let hub = Mutex::new(ChannelAdmin::default());
        let mut rx = lock(&hub).expect(1, Target::Updated(3));
        on_disconnect(&hub, 1);
        assert!(matches!(
            rx.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
    }
}
