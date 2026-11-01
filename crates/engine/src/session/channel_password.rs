//! Channel passwords (FR-2.78). Mumble has no channel passwords of its own: a channel's ACL
//! grants Enter to a group named `#<token>`, and a client belongs to that group while its
//! Authenticate access-token list holds the token. Mumcord keeps the tokens per server in
//! secure storage, sends them on every connect, and asks for one when a join is refused.
//! The server never says which token opened which channel, so Mumcord remembers the channel
//! a password was entered for itself (issue #146), as non-secret details in the database.

use std::collections::HashMap;

use mumcord_protocol::{DenyKind, Handle, PermissionDenied, permissions};
use mumcord_store::{ChannelToken, ServerId, TokenChannel, UnixMillis};

use super::convert::channel_path;
use super::{Engine, EngineError, EngineEvent, EngineResult, Inner, SessionCtx, lock};
use crate::chat::now_millis;

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

/// The join the own user asked for, per server, until the server answers it. Only a refusal
/// of such a join asks for a password: the server also refuses Enter for things the user never
/// asked for, such as the statistics of someone in a locked channel.
#[derive(Default)]
pub(super) struct Attempts(HashMap<ServerId, Attempt>);

struct Attempt {
    channel_id: u32,
    /// The password the join was retried with; `None` for a join without one.
    token: Option<String>,
}

impl Attempts {
    /// The own user asked to join `channel_id`. A password retry for that channel stays.
    pub(super) fn request(&mut self, server_id: ServerId, channel_id: u32) {
        if self
            .0
            .get(&server_id)
            .is_none_or(|a| a.channel_id != channel_id)
        {
            self.0.insert(
                server_id,
                Attempt {
                    channel_id,
                    token: None,
                },
            );
        }
    }

    fn start(&mut self, server_id: ServerId, channel_id: u32, token: String) {
        self.0.insert(
            server_id,
            Attempt {
                channel_id,
                token: Some(token),
            },
        );
    }

    /// What a denial means for the dialog, plus the token to remove again when it was wrong.
    /// `None` when the denial answers no join the own user asked for. The attempt ends with
    /// the answer, so one join asks at most once.
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
        let attempt = attempt?;
        if !refused_entry(denied, own_session) {
            return Some((channel_id, ChannelPasswordState::Ended, None));
        }
        Some(match attempt.token {
            Some(token) => (channel_id, ChannelPasswordState::Wrong, Some(token)),
            None => (channel_id, ChannelPasswordState::Required, None),
        })
    }

    /// The own user arrived in `channel_id`: ends an attempt on it, handing back the token that
    /// let them in when it was a password retry.
    fn on_own_moved(&mut self, server_id: ServerId, channel_id: u32) -> Option<String> {
        let done = self
            .0
            .get(&server_id)
            .is_some_and(|a| a.channel_id == channel_id);
        if done {
            self.0.remove(&server_id).and_then(|a| a.token)
        } else {
            None
        }
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
        if let Err(e) =
            lock(&self.store).touch_token_channel(ctx.server_id, channel_id, now_millis())
        {
            tracing::warn!(error = %e, "noting the use of a channel password failed");
        }
        if let Some(token) = lock(&self.password_attempts).on_own_moved(ctx.server_id, channel_id) {
            self.attach_channel(ctx, token, channel_id);
            ctx.events.emit(EngineEvent::ChannelPassword {
                channel_id,
                state: ChannelPasswordState::Ended,
            });
        }
    }

    /// Remembers that `token` let the user into `channel_id`, so the settings can name it.
    fn attach_channel(&self, ctx: &SessionCtx, token: String, channel_id: u32) {
        let Some(path) = ctx.handle.with_state(|s| channel_path(s, channel_id)) else {
            return;
        };
        let channel = TokenChannel { channel_id, path };
        let secrets = self.secrets.clone();
        let store = self.store.clone();
        let server_id = ctx.server_id;
        let rt = self.rt.clone();
        self.rt.spawn(async move {
            let attached = rt
                .spawn_blocking(move || {
                    let tokens = secrets.access_tokens(server_id)?;
                    // Gone when the user removed it meanwhile: nothing to name then.
                    let Some(position) = tokens.iter().position(|t| *t == token) else {
                        return Ok(());
                    };
                    lock(&store).attach_token_channel(server_id, position, channel, now_millis())
                })
                .await;
            match attached {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::warn!(server_id, error = %e, "saving a channel password's channel failed");
                }
                Err(e) => {
                    tracing::warn!(server_id, error = %e, "saving a channel password's channel failed");
                }
            }
        });
    }

    pub(super) fn forget_password_attempt(&self, server_id: ServerId) {
        lock(&self.password_attempts).forget(server_id);
    }

    /// Removes a refused token from secure storage and from the session.
    fn remove_token(&self, ctx: &SessionCtx, token: String) {
        let secrets = self.secrets.clone();
        let handle = ctx.handle.clone();
        let server_id = ctx.server_id;
        let store = self.store.clone();
        let rt = self.rt.clone();
        self.rt.spawn(async move {
            let removed = rt
                .spawn_blocking(move || {
                    let mut tokens = secrets.access_tokens(server_id)?;
                    let positions: Vec<usize> = tokens
                        .iter()
                        .enumerate()
                        .filter(|(_, t)| **t == token)
                        .map(|(i, _)| i)
                        .collect();
                    tokens.retain(|t| *t != token);
                    secrets.set_access_tokens(server_id, &tokens)?;
                    lock(&store).remove_channel_tokens(server_id, &positions)?;
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
        let store = self.inner.store.clone();
        let token = password.clone();
        let tokens = self
            .inner
            .rt
            .spawn_blocking(move || {
                let mut tokens = secrets.access_tokens(server_id)?;
                if !tokens.contains(&token) {
                    lock(&store).truncate_channel_tokens(server_id, tokens.len())?;
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

    /// The stored channel passwords of a server with what Mumcord knows about each, in token
    /// order (the edit-server dialog lists them masked, FR-2.78, issue #146). The passwords
    /// themselves are never handed out. Whether a remembered channel still exists is only
    /// known while the server is connected.
    pub async fn channel_passwords(
        &self,
        server_id: ServerId,
    ) -> EngineResult<Vec<ChannelPasswordView>> {
        let secrets = self.inner.secrets.clone();
        let store = self.inner.store.clone();
        let tokens = self
            .inner
            .rt
            .spawn_blocking(move || {
                let count = secrets.access_tokens(server_id)?.len();
                lock(&store).channel_tokens(server_id, count)
            })
            .await
            .map_err(|e| EngineError::internal(e.to_string()))?
            .map_err(super::store_error)?;
        let session = self.session_ctx_for(server_id).ok();
        Ok(tokens
            .into_iter()
            .map(|token| {
                let found = |channel: &TokenChannel| {
                    session.as_ref().map(|ctx| {
                        ctx.handle.with_state(|s| {
                            channel_path(s, channel.channel_id).as_ref() == Some(&channel.path)
                        })
                    })
                };
                ChannelPasswordView::new(token, found)
            })
            .collect())
    }

    /// Gives a stored channel password the user's own name; an empty one goes back to the
    /// default (the channels it was entered for, or "Password N").
    pub async fn channel_password_rename(
        &self,
        server_id: ServerId,
        index: u32,
        label: String,
    ) -> EngineResult<()> {
        let position =
            usize::try_from(index).map_err(|_| EngineError::invalid("index out of range"))?;
        let secrets = self.inner.secrets.clone();
        let store = self.inner.store.clone();
        self.inner
            .rt
            .spawn_blocking(move || {
                if position >= secrets.access_tokens(server_id)?.len() {
                    return Ok(false);
                }
                let label = label.trim();
                lock(&store).rename_channel_token(
                    server_id,
                    position,
                    (!label.is_empty()).then_some(label),
                )?;
                Ok(true)
            })
            .await
            .map_err(|e| EngineError::internal(e.to_string()))?
            .map_err(super::store_error)?
            .then_some(())
            .ok_or_else(|| EngineError::invalid("no channel password at this index"))
    }

    /// Removes stored channel passwords by their position in the list; a connected server gets
    /// the remaining tokens at once, so the next join asks again.
    pub async fn channel_passwords_remove(
        &self,
        server_id: ServerId,
        indices: Vec<u32>,
    ) -> EngineResult<()> {
        let secrets = self.inner.secrets.clone();
        let store = self.inner.store.clone();
        let tokens = self
            .inner
            .rt
            .spawn_blocking(move || {
                let tokens = secrets.access_tokens(server_id)?;
                let kept: Vec<String> = without_indices(tokens, &indices);
                secrets.set_access_tokens(server_id, &kept)?;
                let positions: Vec<usize> = indices
                    .iter()
                    .filter_map(|i| usize::try_from(*i).ok())
                    .collect();
                lock(&store).remove_channel_tokens(server_id, &positions)?;
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

/// A stored channel password as the settings show it (issue #146).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelPasswordView {
    /// The user's own name; `None` shows the channels, or "Password N" without any.
    pub label: Option<String>,
    pub channels: Vec<ChannelPasswordChannel>,
    pub last_used_at: Option<UnixMillis>,
}

/// A channel a password was entered for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelPasswordChannel {
    pub channel_id: u32,
    /// Path below the root as the channel was named then.
    pub path: Vec<String>,
    /// Whether the channel still exists under that path; `None` while not connected.
    pub found: Option<bool>,
}

impl ChannelPasswordView {
    fn new(token: ChannelToken, found: impl Fn(&TokenChannel) -> Option<bool>) -> Self {
        Self {
            label: token.label,
            channels: token
                .channels
                .into_iter()
                .map(|channel| ChannelPasswordChannel {
                    found: found(&channel),
                    channel_id: channel.channel_id,
                    path: channel.path,
                })
                .collect(),
            last_used_at: token.last_used_at,
        }
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
        attempts.request(1, 3);
        assert_eq!(
            attempts.on_denied(1, &enter_refused(), Some(OWN)),
            Some((3, ChannelPasswordState::Required, None))
        );
        attempts.request(1, 3);
        let traverse = denied(DenyKind::Permission, permissions::TRAVERSE, None);
        assert_eq!(
            attempts.on_denied(1, &traverse, Some(OWN)),
            Some((3, ChannelPasswordState::Required, None))
        );
    }

    #[test]
    fn a_refusal_without_an_own_join_never_asks() {
        // Murmur refuses the statistics of someone in a locked channel with exactly the Enter
        // denial a refused join gets, again on every refresh.
        let mut attempts = Attempts::default();
        for _ in 0..3 {
            assert_eq!(attempts.on_denied(1, &enter_refused(), Some(OWN)), None);
        }
    }

    #[test]
    fn one_join_asks_only_once() {
        let mut attempts = Attempts::default();
        attempts.request(1, 3);
        assert_eq!(
            attempts.on_denied(1, &enter_refused(), Some(OWN)),
            Some((3, ChannelPasswordState::Required, None))
        );
        assert_eq!(attempts.on_denied(1, &enter_refused(), Some(OWN)), None);
    }

    #[test]
    fn other_denials_do_not_ask() {
        let other_person = denied(DenyKind::Permission, permissions::ENTER, Some(OWN + 1));
        let text = denied(DenyKind::Permission, permissions::TEXT_MESSAGE, Some(OWN));
        let full = denied(DenyKind::ChannelFull, 0, Some(OWN));
        for d in [other_person, text, full] {
            let mut attempts = Attempts::default();
            attempts.request(1, 3);
            assert_eq!(
                attempts.on_denied(1, &d, Some(OWN)),
                Some((3, ChannelPasswordState::Ended, None))
            );
        }
    }

    #[test]
    fn a_refused_retry_is_wrong_and_hands_back_the_token() {
        let mut attempts = Attempts::default();
        attempts.start(1, 3, "wrong".into());
        // The join that follows the retry keeps its password.
        attempts.request(1, 3);
        assert_eq!(
            attempts.on_denied(1, &enter_refused(), Some(OWN)),
            Some((3, ChannelPasswordState::Wrong, Some("wrong".into())))
        );
        // The attempt is over: only the next join asks afresh.
        assert_eq!(attempts.on_denied(1, &enter_refused(), Some(OWN)), None);
        attempts.request(1, 3);
        assert_eq!(
            attempts.on_denied(1, &enter_refused(), Some(OWN)),
            Some((3, ChannelPasswordState::Required, None))
        );
    }

    #[test]
    fn arriving_in_the_channel_ends_the_attempt() {
        let mut attempts = Attempts::default();
        attempts.start(1, 3, "secret".into());
        assert_eq!(attempts.on_own_moved(2, 3), None, "another server");
        assert_eq!(attempts.on_own_moved(1, 4), None, "another channel");
        assert_eq!(attempts.on_own_moved(1, 3).as_deref(), Some("secret"));
        assert_eq!(attempts.on_own_moved(1, 3), None, "only once");

        attempts.request(1, 3);
        assert_eq!(attempts.on_own_moved(1, 3), None, "no password");
        assert_eq!(attempts.on_denied(1, &enter_refused(), Some(OWN)), None);
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
        assert_eq!(attempts.on_denied(1, &enter_refused(), Some(OWN)), None);
        assert!(attempts.on_own_moved(1, 4).is_some());
    }

    async fn engine_with_tokens(dir: &std::path::Path, tokens: &[&str]) -> (Engine, ServerId) {
        let (engine, _rx) = Engine::new(
            dir,
            crate::SecretStore::in_memory(),
            tokio::runtime::Handle::current(),
        )
        .expect("engine");
        let server = engine
            .server_add(crate::ServerFields {
                label: "a".into(),
                address: "a.example".into(),
                port: 64738,
                username: "me".into(),
                password: crate::PasswordChange::Keep,
            })
            .await
            .expect("server")
            .id;
        let tokens: Vec<String> = tokens.iter().map(|t| (*t).to_owned()).collect();
        engine
            .inner
            .secrets
            .set_access_tokens(server, &tokens)
            .expect("tokens");
        (engine, server)
    }

    fn officers() -> TokenChannel {
        TokenChannel {
            channel_id: 5,
            path: vec!["Raid".into(), "Officers".into()],
        }
    }

    #[tokio::test]
    async fn tokens_saved_before_are_kept_and_listed_unnamed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (engine, server) = engine_with_tokens(dir.path(), &["one", "two"]).await;
        let listed = engine.channel_passwords(server).await.expect("list");
        assert_eq!(listed.len(), 2);
        assert!(
            listed
                .iter()
                .all(|p| p.label.is_none() && p.channels.is_empty() && p.last_used_at.is_none())
        );
        assert_eq!(
            engine.inner.secrets.access_tokens(server).expect("tokens"),
            ["one", "two"]
        );
    }

    #[tokio::test]
    async fn a_saved_channel_is_listed_and_unknown_while_offline() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (engine, server) = engine_with_tokens(dir.path(), &["one", "two"]).await;
        lock(&engine.inner.store)
            .attach_token_channel(server, 1, officers(), 42)
            .expect("attach");
        let listed = engine.channel_passwords(server).await.expect("list");
        assert!(listed[0].channels.is_empty());
        assert_eq!(
            listed[1].channels,
            [ChannelPasswordChannel {
                channel_id: 5,
                path: vec!["Raid".into(), "Officers".into()],
                found: None,
            }]
        );
        assert_eq!(listed[1].last_used_at, Some(42));
    }

    #[tokio::test]
    async fn renaming_and_removing_keep_the_details_with_their_token() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (engine, server) = engine_with_tokens(dir.path(), &["one", "two", "three"]).await;
        lock(&engine.inner.store)
            .attach_token_channel(server, 2, officers(), 1)
            .expect("attach");
        engine
            .channel_password_rename(server, 1, "  Raid password ".into())
            .await
            .expect("rename");
        assert!(
            engine
                .channel_password_rename(server, 3, "nothing there".into())
                .await
                .is_err()
        );
        engine
            .channel_passwords_remove(server, vec![0])
            .await
            .expect("remove");
        assert_eq!(
            engine.inner.secrets.access_tokens(server).expect("tokens"),
            ["two", "three"]
        );
        let listed = engine.channel_passwords(server).await.expect("list");
        assert_eq!(listed[0].label.as_deref(), Some("Raid password"));
        assert_eq!(listed[1].channels[0].channel_id, 5);

        engine
            .channel_password_rename(server, 0, " ".into())
            .await
            .expect("back to default");
        assert_eq!(
            engine.channel_passwords(server).await.expect("list")[0].label,
            None
        );
    }

    #[test]
    fn removing_by_index_keeps_the_rest_in_order() {
        let tokens = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        assert_eq!(without_indices(tokens, &[0, 2, 9]), ["b"]);
    }
}
