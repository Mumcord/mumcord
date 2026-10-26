//! Channel passwords (FR-2.78). Mumble has no channel passwords of its own: a channel's ACL
//! grants Enter to a group named `#<token>`, and a client belongs to that group while its
//! Authenticate access-token list holds the token. Mumcord keeps the tokens per server in
//! secure storage, sends them on every connect, and asks for one when a join is refused.

use std::collections::HashMap;

use mumcord_protocol::{DenyKind, Handle, PermissionDenied, permissions};
use mumcord_store::ServerId;

use super::{Engine, EngineError, EngineEvent, EngineResult, Inner, SessionCtx, lock};

/// What the channel-password dialog shows (FR-2.78).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPasswordState {
    /// A join was refused for a missing Enter permission: ask for a password.
    Required,
    /// The server refused the join again with the typed password; it was removed again.
    Wrong,
    /// The attempt is over without a password problem: the user is in the channel, or the
    /// join was refused for another reason (reported as `ServerDenied`).
    Ended,
}

/// The join retried with a new token, per server, until the server answers it.
#[derive(Default)]
pub(super) struct Attempts(HashMap<ServerId, Attempt>);

struct Attempt {
    channel_id: u32,
    token: String,
}

impl Attempts {
    fn start(&mut self, server_id: ServerId, channel_id: u32, token: String) {
        self.0.insert(server_id, Attempt { channel_id, token });
    }

    /// What a denial means for the dialog, plus the token to remove again when it was wrong.
    /// `None` when the denial is no refused join of the own user.
    fn on_denied(
        &mut self,
        server_id: ServerId,
        denied: &PermissionDenied,
        own_session: Option<u32>,
    ) -> Option<(u32, ChannelPasswordState, Option<String>)> {
        let channel_id = denied.channel_id?;
        let attempt = self
            .0
            .remove_entry(&server_id)
            .and_then(|(server, attempt)| {
                if attempt.channel_id == channel_id {
                    Some(attempt)
                } else {
                    self.0.insert(server, attempt);
                    None
                }
            });
        if !refused_entry(denied, own_session) {
            return attempt.map(|_| (channel_id, ChannelPasswordState::Ended, None));
        }
        Some(match attempt {
            Some(attempt) => (channel_id, ChannelPasswordState::Wrong, Some(attempt.token)),
            None => (channel_id, ChannelPasswordState::Required, None),
        })
    }

    /// The own user arrived in `channel_id`: true if that ends an attempt.
    fn on_own_moved(&mut self, server_id: ServerId, channel_id: u32) -> bool {
        let done = self
            .0
            .get(&server_id)
            .is_some_and(|a| a.channel_id == channel_id);
        if done {
            self.0.remove(&server_id);
        }
        done
    }

    fn forget(&mut self, server_id: ServerId) {
        self.0.remove(&server_id);
    }
}

/// The server refused the own user entering a channel. Moving someone else is refused with the
/// other person's session, so that never asks the user for a password.
fn refused_entry(denied: &PermissionDenied, own_session: Option<u32>) -> bool {
    denied.kind == DenyKind::Permission
        && denied
            .permission
            .is_some_and(|p| p & (permissions::ENTER | permissions::TRAVERSE) != 0)
        && denied.session.is_none_or(|s| Some(s) == own_session)
}

impl Inner {
    pub(super) fn on_channel_denied(&self, ctx: &SessionCtx, denied: &PermissionDenied) {
        let own = ctx.handle.with_state(|s| s.session);
        let Some((channel_id, state, wrong)) =
            lock(&self.password_attempts).on_denied(ctx.server_id, denied, own)
        else {
            return;
        };
        if let Some(token) = wrong {
            self.remove_token(ctx, token);
        }
        ctx.events
            .emit(EngineEvent::ChannelPassword { channel_id, state });
    }

    pub(super) fn on_own_channel(&self, ctx: &SessionCtx, channel_id: u32) {
        if lock(&self.password_attempts).on_own_moved(ctx.server_id, channel_id) {
            ctx.events.emit(EngineEvent::ChannelPassword {
                channel_id,
                state: ChannelPasswordState::Ended,
            });
        }
    }

    pub(super) fn forget_password_attempt(&self, server_id: ServerId) {
        lock(&self.password_attempts).forget(server_id);
    }

    /// Removes a refused token from secure storage and from the session.
    fn remove_token(&self, ctx: &SessionCtx, token: String) {
        let secrets = self.secrets.clone();
        let handle = ctx.handle.clone();
        let server_id = ctx.server_id;
        let rt = self.rt.clone();
        self.rt.spawn(async move {
            let removed = rt
                .spawn_blocking(move || {
                    let mut tokens = secrets.access_tokens(server_id)?;
                    tokens.retain(|t| *t != token);
                    secrets.set_access_tokens(server_id, &tokens)?;
                    Ok::<_, mumcord_store::Error>(tokens)
                })
                .await;
            match removed {
                Ok(Ok(tokens)) => send_tokens(&handle, tokens),
                Ok(Err(e)) => {
                    tracing::warn!(server_id, error = %e, "removing a wrong channel password failed");
                }
                Err(e) => {
                    tracing::warn!(server_id, error = %e, "removing a wrong channel password failed");
                }
            }
        });
    }
}

/// Sends the token list and asks for the permissions again: servers before 1.4 do not resend
/// which channels the user may enter, and every server forgets the answers it gave before.
fn send_tokens(handle: &Handle, tokens: Vec<String>) {
    if let Err(e) = handle.set_access_tokens(tokens) {
        tracing::warn!(error = %e, "sending the channel passwords failed");
        return;
    }
    for id in handle.with_state(|s| s.channels.keys().copied().collect::<Vec<_>>()) {
        let _ = handle.query_permissions(id);
    }
}

impl Engine {
    /// Retries a refused join with a channel password (FR-2.78): stores it as an access token
    /// of the server, sends the server all its tokens, and joins again. The outcome arrives as
    /// [`EngineEvent::ChannelPassword`] (`Wrong` or `Ended`).
    pub async fn channel_join_with_password(
        &self,
        server_id: ServerId,
        channel_id: u32,
        password: String,
    ) -> EngineResult<()> {
        if password.is_empty() {
            return Err(EngineError::invalid("password must not be empty"));
        }
        let ctx = self.session_ctx_for(server_id)?;
        let secrets = self.inner.secrets.clone();
        let token = password.clone();
        let tokens = self
            .inner
            .rt
            .spawn_blocking(move || {
                let mut tokens = secrets.access_tokens(server_id)?;
                if !tokens.contains(&token) {
                    tokens.push(token);
                    secrets.set_access_tokens(server_id, &tokens)?;
                }
                Ok(tokens)
            })
            .await
            .map_err(|e| EngineError::internal(e.to_string()))?
            .map_err(super::store_error)?;
        lock(&self.inner.password_attempts).start(server_id, channel_id, password);
        send_tokens(&ctx.handle, tokens);
        self.join_channel(server_id, channel_id).inspect_err(|_| {
            self.inner.forget_password_attempt(server_id);
        })
    }

    /// How many channel passwords are stored for a server (the edit-server dialog lists them
    /// masked, FR-2.78).
    pub async fn channel_password_count(&self, server_id: ServerId) -> EngineResult<u32> {
        let secrets = self.inner.secrets.clone();
        let tokens = self
            .inner
            .rt
            .spawn_blocking(move || secrets.access_tokens(server_id))
            .await
            .map_err(|e| EngineError::internal(e.to_string()))?
            .map_err(super::store_error)?;
        Ok(u32::try_from(tokens.len()).unwrap_or(u32::MAX))
    }

    /// Removes stored channel passwords by their position in the list; a connected server gets
    /// the remaining tokens at once, so the next join asks again.
    pub async fn channel_passwords_remove(
        &self,
        server_id: ServerId,
        indices: Vec<u32>,
    ) -> EngineResult<()> {
        let secrets = self.inner.secrets.clone();
        let tokens = self
            .inner
            .rt
            .spawn_blocking(move || {
                let tokens = secrets.access_tokens(server_id)?;
                let kept: Vec<String> = without_indices(tokens, &indices);
                secrets.set_access_tokens(server_id, &kept)?;
                Ok(kept)
            })
            .await
            .map_err(|e| EngineError::internal(e.to_string()))?
            .map_err(super::store_error)?;
        if let Ok(ctx) = self.session_ctx_for(server_id) {
            send_tokens(&ctx.handle, tokens);
        }
        Ok(())
    }
}

fn without_indices(tokens: Vec<String>, indices: &[u32]) -> Vec<String> {
    tokens
        .into_iter()
        .enumerate()
        .filter(|(i, _)| {
            !indices
                .iter()
                .any(|r| usize::try_from(*r).is_ok_and(|r| r == *i))
        })
        .map(|(_, t)| t)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: u32 = 7;

    fn denied(kind: DenyKind, permission: u32, session: Option<u32>) -> PermissionDenied {
        PermissionDenied {
            kind,
            permission: Some(permission),
            channel_id: Some(3),
            session,
            reason: None,
            name: None,
        }
    }

    fn enter_refused() -> PermissionDenied {
        denied(DenyKind::Permission, permissions::ENTER, Some(OWN))
    }

    #[test]
    fn a_refused_join_asks_for_a_password() {
        let mut attempts = Attempts::default();
        assert_eq!(
            attempts.on_denied(1, &enter_refused(), Some(OWN)),
            Some((3, ChannelPasswordState::Required, None))
        );
        let traverse = denied(DenyKind::Permission, permissions::TRAVERSE, None);
        assert_eq!(
            attempts.on_denied(1, &traverse, Some(OWN)),
            Some((3, ChannelPasswordState::Required, None))
        );
    }

    #[test]
    fn other_denials_do_not_ask() {
        let mut attempts = Attempts::default();
        let other_person = denied(DenyKind::Permission, permissions::ENTER, Some(OWN + 1));
        let text = denied(DenyKind::Permission, permissions::TEXT_MESSAGE, Some(OWN));
        let full = denied(DenyKind::ChannelFull, 0, Some(OWN));
        for d in [other_person, text, full] {
            assert_eq!(attempts.on_denied(1, &d, Some(OWN)), None);
        }
    }

    #[test]
    fn a_refused_retry_is_wrong_and_hands_back_the_token() {
        let mut attempts = Attempts::default();
        attempts.start(1, 3, "wrong".into());
        assert_eq!(
            attempts.on_denied(1, &enter_refused(), Some(OWN)),
            Some((3, ChannelPasswordState::Wrong, Some("wrong".into())))
        );
        // The attempt is over: the next refusal asks afresh.
        assert_eq!(
            attempts.on_denied(1, &enter_refused(), Some(OWN)),
            Some((3, ChannelPasswordState::Required, None))
        );
    }

    #[test]
    fn arriving_in_the_channel_ends_the_attempt() {
        let mut attempts = Attempts::default();
        attempts.start(1, 3, "secret".into());
        assert!(!attempts.on_own_moved(2, 3), "another server");
        assert!(!attempts.on_own_moved(1, 4), "another channel");
        assert!(attempts.on_own_moved(1, 3));
        assert!(!attempts.on_own_moved(1, 3), "only once");
    }

    #[test]
    fn another_refusal_during_an_attempt_ends_it_without_asking() {
        let mut attempts = Attempts::default();
        attempts.start(1, 3, "secret".into());
        let full = denied(DenyKind::ChannelFull, 0, Some(OWN));
        assert_eq!(
            attempts.on_denied(1, &full, Some(OWN)),
            Some((3, ChannelPasswordState::Ended, None))
        );
    }

    #[test]
    fn an_attempt_on_another_channel_stays() {
        let mut attempts = Attempts::default();
        attempts.start(1, 4, "secret".into());
        assert_eq!(
            attempts.on_denied(1, &enter_refused(), Some(OWN)),
            Some((3, ChannelPasswordState::Required, None))
        );
        assert!(attempts.on_own_moved(1, 4));
    }

    #[test]
    fn removing_by_index_keeps_the_rest_in_order() {
        let tokens = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        assert_eq!(without_indices(tokens, &[0, 2, 9]), ["b"]);
    }
}
