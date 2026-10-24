//! Following channels (FR-1.31): listen without joining, audio muted locally, persisted by
//! channel path. Restoring after a reconnect happens in the session's post-sync step.
//!
//! The server confirms or denies listen/profile requests only asynchronously, so commands
//! wait briefly for the own user's state to change or for a matching denial.

use std::sync::LazyLock;
use std::time::Duration;

use mumcord_protocol::{DenyKind, PermissionDenied, ServerState};
use mumcord_store::ServerId;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::TryRecvError;

use crate::chat::{forget_unread, now_millis};
use crate::session::convert::{channel_view, command_error, denial_error, join_path};
use crate::session::{
    ConversationRef, Engine, EngineError, EngineEvent, EngineResult, ErrorKind, SessionCtx,
    UnreadView, store_error,
};

/// How long a command waits for the server's answer.
pub(crate) const CONFIRM_TIMEOUT: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(50);

static DENIALS: LazyLock<broadcast::Sender<(ServerId, PermissionDenied)>> =
    LazyLock::new(|| broadcast::channel(32).0);

/// Hands a server denial to waiting commands (called from the session hook).
pub(crate) fn publish_denial(server_id: ServerId, denied: &PermissionDenied) {
    // No receiver just means no command is waiting.
    let _ = DENIALS.send((server_id, denied.clone()));
}

/// Subscribe before sending the request, so its denial cannot be missed.
pub(crate) fn subscribe_denials() -> broadcast::Receiver<(ServerId, PermissionDenied)> {
    DENIALS.subscribe()
}

/// What happens when the server neither confirms nor denies in time.
pub(crate) enum OnTimeout {
    Accept,
    Fail(&'static str),
}

/// Waits until `accepted` holds for the server state, a `relevant` denial arrives, or the
/// timeout passes.
pub(crate) async fn confirm(
    ctx: &SessionCtx,
    mut denials: broadcast::Receiver<(ServerId, PermissionDenied)>,
    accepted: impl Fn(&ServerState) -> bool,
    relevant: impl Fn(&PermissionDenied) -> bool,
    on_timeout: OnTimeout,
) -> EngineResult<()> {
    let deadline = tokio::time::Instant::now() + CONFIRM_TIMEOUT;
    loop {
        loop {
            match denials.try_recv() {
                Ok((server, denied)) if server == ctx.server_id && relevant(&denied) => {
                    return Err(denial_error(&denied));
                }
                Ok(_) | Err(TryRecvError::Lagged(_)) => {}
                Err(TryRecvError::Empty | TryRecvError::Closed) => break,
            }
        }
        if !ctx.handle.is_connected() {
            return Err(EngineError::not_connected());
        }
        if ctx.handle.with_state(&accepted) {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return match on_timeout {
                OnTimeout::Accept => Ok(()),
                OnTimeout::Fail(what) => Err(EngineError::internal(format!(
                    "the server did not confirm {what}"
                ))),
            };
        }
        tokio::time::sleep(POLL).await;
    }
}

fn is_listen_denial(denied: &PermissionDenied, channel_id: u32) -> bool {
    match denied.kind {
        DenyKind::ChannelListenerLimit | DenyKind::UserListenerLimit => true,
        DenyKind::Permission => denied.channel_id.is_none_or(|c| c == channel_id),
        _ => false,
    }
}

fn listening(state: &ServerState, channel_id: u32) -> bool {
    state
        .own_user()
        .is_some_and(|u| u.listening.contains(&channel_id))
}

impl Engine {
    /// Starts following a channel. Errors: `ServerTooOld` (< 1.4), `PermissionDenied`,
    /// `ListenerLimitReached`.
    pub async fn channel_follow(&self, channel_id: u32) -> EngineResult<()> {
        let ctx = self.session_ctx()?;
        let (supported, volume, channel) = ctx.handle.with_state(|s| {
            (
                s.server.supports_listening(),
                s.server.supports_listener_volume(),
                s.channels.get(&channel_id).map(|c| channel_view(s, c)),
            )
        });
        if !supported {
            return Err(EngineError::new(
                ErrorKind::ServerTooOld,
                "following channels needs server 1.4 or newer",
            ));
        }
        let channel =
            channel.ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown channel"))?;
        if !channel.permissions.listen {
            return Err(EngineError::new(
                ErrorKind::PermissionDenied,
                "no permission to listen to this channel",
            ));
        }
        let path = ctx
            .channel_path(channel_id)
            .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown channel"))?;
        if !ctx.handle.with_state(|s| listening(s, channel_id)) {
            let denials = subscribe_denials();
            ctx.handle.listen(&[channel_id]).map_err(command_error)?;
            confirm(
                &ctx,
                denials,
                |s| listening(s, channel_id),
                |d| is_listen_denial(d, channel_id),
                OnTimeout::Fail("following the channel"),
            )
            .await?;
            // The mixer drops listener audio anyway; 1.5+ servers also keep it silent.
            if volume && let Err(e) = ctx.handle.set_listener_volume(channel_id, 0.0) {
                tracing::warn!(error = %e, "setting the listener volume failed");
            }
        }
        ctx.store()
            .follow_channel(ctx.server_id, &join_path(&path), now_millis())
            .map_err(store_error)?;
        ctx.events.emit(EngineEvent::FollowedChannelsChanged(
            ctx.followed_channel_ids(),
        ));
        Ok(())
    }

    /// Stops following a channel.
    pub fn channel_unfollow(&self, channel_id: u32) -> EngineResult<()> {
        let ctx = self.session_ctx()?;
        let path = ctx
            .channel_path(channel_id)
            .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown channel"))?;
        let listens = ctx
            .handle
            .with_state(|s| s.server.supports_listening() && listening(s, channel_id));
        if listens {
            ctx.handle.unlisten(&[channel_id]).map_err(command_error)?;
        }
        let path = join_path(&path);
        ctx.store()
            .unfollow_channel(ctx.server_id, &path)
            .map_err(store_error)?;
        ctx.events.emit(EngineEvent::FollowedChannelsChanged(
            ctx.followed_channel_ids(),
        ));
        if forget_unread(ctx.server_id, &path) {
            ctx.events.emit(EngineEvent::UnreadChanged(UnreadView {
                conversation: ConversationRef::Channel { channel_id },
                count: 0,
            }));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denial(kind: DenyKind, channel_id: Option<u32>) -> PermissionDenied {
        PermissionDenied {
            kind,
            permission: None,
            channel_id,
            session: None,
            reason: None,
            name: None,
        }
    }

    #[test]
    fn listen_denials_are_recognised_and_mapped() {
        let limit = denial(DenyKind::ChannelListenerLimit, None);
        assert!(is_listen_denial(&limit, 2));
        assert_eq!(denial_error(&limit).kind, ErrorKind::ListenerLimitReached);
        assert!(is_listen_denial(
            &denial(DenyKind::UserListenerLimit, None),
            2
        ));
        let perm = denial(DenyKind::Permission, Some(2));
        assert!(is_listen_denial(&perm, 2));
        assert_eq!(denial_error(&perm).kind, ErrorKind::PermissionDenied);
        assert!(!is_listen_denial(&denial(DenyKind::Permission, Some(3)), 2));
        assert!(!is_listen_denial(&denial(DenyKind::TextTooLong, None), 2));
    }
}
