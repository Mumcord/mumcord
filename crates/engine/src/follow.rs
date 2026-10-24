//! Following channels (FR-1.31): listen without joining, audio muted locally, persisted by
//! channel path. Restoring after a reconnect happens in the session's post-sync step.
//!
//! The server confirms or denies listen/profile requests only asynchronously, so commands
//! wait briefly for the own user's state to change or for a matching denial.

use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Duration;

use mumcord_protocol::{DenyKind, PermissionDenied, ServerState};
use mumcord_store::ServerId;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::TryRecvError;

use crate::chat::{forget_unread, now_millis};
use crate::session::convert::{
    channel_path, channel_view, command_error, denial_error, join_path, split_path,
};
use crate::session::{
    ConversationRef, Engine, EngineError, EngineEvent, EngineResult, ErrorKind, SessionCtx,
    UnreadView, store_error,
};

/// How long a command waits for the server's answer.
pub(crate) const CONFIRM_TIMEOUT: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(50);

static DENIALS: LazyLock<broadcast::Sender<(ServerId, PermissionDenied)>> =
    // Roomy: "Follow all" on a big server gets one listener-limit denial per refused channel.
    LazyLock::new(|| broadcast::channel(1024).0);

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

/// Outcome of [`Engine::channel_follow_all`]. Channels that were already followed are not
/// counted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FollowAllSummary {
    /// Channels newly followed.
    pub followed: u32,
    /// Skipped because the server's listener limit was reached.
    pub limit_reached: u32,
    /// Skipped because the user may not listen to them.
    pub not_allowed: u32,
    /// Skipped because the server neither confirmed nor refused in time.
    pub no_answer: u32,
}

/// What the server answered to one batched listen request.
#[derive(Debug, Default)]
struct BatchAnswers {
    /// Channels refused with a per-channel permission denial.
    denied: HashSet<u32>,
    /// Permission denials without a channel id.
    unattributed: u32,
    /// Listener-limit denials; the server names no channel in these.
    limit: u32,
}

impl BatchAnswers {
    fn record(&mut self, denied: &PermissionDenied, requested: &[u32]) {
        match (denied.kind, denied.channel_id) {
            (DenyKind::ChannelListenerLimit | DenyKind::UserListenerLimit, _) => self.limit += 1,
            (DenyKind::Permission, Some(c)) if requested.contains(&c) => {
                self.denied.insert(c);
            }
            (DenyKind::Permission, None) => self.unattributed += 1,
            _ => {}
        }
    }

    /// Every requested channel is accounted for (listening or refused).
    fn settled(&self, requested: usize, listening: usize) -> bool {
        listening + self.denied.len() + (self.limit + self.unattributed) as usize >= requested
    }

    /// Splits the requested channels into followed and skipped. Limit denials carry no channel
    /// id, so any refused channel without its own denial counts as limit reached once the
    /// server reported the limit.
    fn tally(&self, requested: &[u32], listening: impl Fn(u32) -> bool) -> FollowAllSummary {
        let mut summary = FollowAllSummary::default();
        for &channel in requested {
            if listening(channel) {
                summary.followed += 1;
            } else if self.denied.contains(&channel) {
                summary.not_allowed += 1;
            } else if self.limit > 0 {
                summary.limit_reached += 1;
            } else if self.unattributed > 0 {
                summary.not_allowed += 1;
            } else {
                summary.no_answer += 1;
            }
        }
        summary
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
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

impl Engine {
    /// Follows every channel the user may listen to, except their own voice channel (QA.13).
    /// One batched listen request; the server refuses what exceeds its listener limit.
    /// Errors: `ServerTooOld` (< 1.4).
    pub async fn channel_follow_all(&self) -> EngineResult<FollowAllSummary> {
        let ctx = self.session_ctx()?;
        let followed = ctx.followed_channel_ids();
        let (supported, volume, candidates, not_allowed) = ctx.handle.with_state(|s| {
            let own = s.own_user().map(|u| u.channel_id);
            let mut candidates = Vec::new();
            let mut not_allowed = 0;
            for channel in s.channels.values() {
                if Some(channel.id) == own || followed.contains(&channel.id) {
                    continue;
                }
                if channel_view(s, channel).permissions.listen {
                    if let Some(path) = channel_path(s, channel.id) {
                        candidates.push((channel.id, join_path(&path)));
                    }
                } else {
                    not_allowed += 1;
                }
            }
            (
                s.server.supports_listening(),
                s.server.supports_listener_volume(),
                candidates,
                not_allowed,
            )
        });
        if !supported {
            return Err(EngineError::new(
                ErrorKind::ServerTooOld,
                "following channels needs server 1.4 or newer",
            ));
        }
        let requested: Vec<u32> = ctx.handle.with_state(|s| {
            candidates
                .iter()
                .map(|(id, _)| *id)
                .filter(|id| !listening(s, *id))
                .collect()
        });
        let mut answers = BatchAnswers::default();
        if !requested.is_empty() {
            let mut denials = subscribe_denials();
            ctx.handle.listen(&requested).map_err(command_error)?;
            let listening_count =
                |s: &ServerState| requested.iter().filter(|id| listening(s, **id)).count();
            // The deadline moves with every answer, so big batches are not cut short.
            let mut progress = 0;
            let mut deadline = tokio::time::Instant::now() + CONFIRM_TIMEOUT;
            loop {
                loop {
                    match denials.try_recv() {
                        Ok((server, denied)) if server == ctx.server_id => {
                            answers.record(&denied, &requested);
                        }
                        Ok(_) | Err(TryRecvError::Lagged(_)) => {}
                        Err(TryRecvError::Empty | TryRecvError::Closed) => break,
                    }
                }
                if !ctx.handle.is_connected() {
                    return Err(EngineError::not_connected());
                }
                let granted = ctx.handle.with_state(listening_count);
                if answers.settled(requested.len(), granted) {
                    break;
                }
                let now = tokio::time::Instant::now();
                let answered = granted + answers.denied.len() + answers.limit as usize;
                if answered > progress {
                    progress = answered;
                    deadline = now + CONFIRM_TIMEOUT;
                } else if now >= deadline {
                    break;
                }
                tokio::time::sleep(POLL).await;
            }
        }
        let (granted, mut summary) = ctx.handle.with_state(|s| {
            let granted: Vec<u32> = requested
                .iter()
                .copied()
                .filter(|id| listening(s, *id))
                .collect();
            (granted, answers.tally(&requested, |id| listening(s, id)))
        });
        if volume {
            for &channel_id in &granted {
                // The mixer drops listener audio anyway; 1.5+ servers also keep it silent.
                if let Err(e) = ctx.handle.set_listener_volume(channel_id, 0.0) {
                    tracing::warn!(error = %e, "setting the listener volume failed");
                }
            }
        }
        // Channels already listened to (but not yet stored) count as followed too.
        let paths: Vec<String> = ctx.handle.with_state(|s| {
            candidates
                .iter()
                .filter(|(id, _)| listening(s, *id))
                .map(|(_, path)| path.clone())
                .collect()
        });
        summary.followed += count(paths.len()).saturating_sub(count(granted.len()));
        summary.not_allowed += not_allowed;
        if !paths.is_empty() {
            ctx.store()
                .follow_channels(ctx.server_id, &paths, now_millis())
                .map_err(store_error)?;
            ctx.events.emit(EngineEvent::FollowedChannelsChanged(
                ctx.followed_channel_ids(),
            ));
        }
        Ok(summary)
    }

    /// Stops following every channel of the connected server (QA.13).
    pub fn channel_unfollow_all(&self) -> EngineResult<()> {
        let ctx = self.session_ctx()?;
        let paths = ctx
            .store()
            .followed_channels(ctx.server_id)
            .map_err(store_error)?;
        let followed = ctx.followed_channel_ids();
        let listened: Vec<u32> = ctx.handle.with_state(|s| {
            followed
                .iter()
                .copied()
                .filter(|id| listening(s, *id))
                .collect()
        });
        if !listened.is_empty() {
            ctx.handle.unlisten(&listened).map_err(command_error)?;
        }
        ctx.store()
            .unfollow_all_channels(ctx.server_id)
            .map_err(store_error)?;
        ctx.events
            .emit(EngineEvent::FollowedChannelsChanged(Vec::new()));
        for stored in paths {
            if forget_unread(ctx.server_id, &stored.channel_path)
                && let Some(channel_id) =
                    ctx.resolve_channel_path(&split_path(&stored.channel_path))
            {
                ctx.events.emit(EngineEvent::UnreadChanged(UnreadView {
                    conversation: ConversationRef::Channel { channel_id },
                    count: 0,
                }));
            }
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

    #[test]
    fn follow_all_stops_at_the_listener_limit() {
        let requested = [1, 2, 3, 4, 5];
        let mut answers = BatchAnswers::default();
        answers.record(&denial(DenyKind::Permission, Some(2)), &requested);
        assert!(!answers.settled(requested.len(), 2));
        answers.record(&denial(DenyKind::UserListenerLimit, None), &requested);
        answers.record(&denial(DenyKind::ChannelListenerLimit, None), &requested);
        // Unrelated denials neither settle the batch nor count.
        answers.record(&denial(DenyKind::TextTooLong, None), &requested);
        answers.record(&denial(DenyKind::Permission, Some(9)), &requested);
        assert!(answers.settled(requested.len(), 2));
        let summary = answers.tally(&requested, |id| id == 1 || id == 3);
        assert_eq!(
            summary,
            FollowAllSummary {
                followed: 2,
                limit_reached: 2,
                not_allowed: 1,
                no_answer: 0,
            }
        );
    }

    #[test]
    fn follow_all_without_answers_reports_no_answer() {
        let answers = BatchAnswers::default();
        assert!(answers.settled(0, 0));
        assert!(!answers.settled(2, 1));
        let summary = answers.tally(&[1, 2], |id| id == 1);
        assert_eq!(summary.followed, 1);
        assert_eq!(summary.no_answer, 1);
    }
}
