//! Text chat: routing incoming messages, sending, unread counts and the local history
//! (FR-1.28–1.30, FR-1.32, FR-1.33).
//!
//! Every message body is sanitized before it is stored or emitted. Unread counts live in
//! memory per server (followed channels are seeded from the store's read markers at sync),
//! so they also work while "Save history" is off.

mod sanitize;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mumcord_protocol::{
    DenyKind, PermissionDenied, ServerState, TextMessage, TextTarget, permissions,
};
use mumcord_store::{ChatMessage, Conversation, NewChatMessage, ServerId, Store, UnixMillis};

use crate::follow::{OnTimeout, confirm, subscribe_denials};
use crate::session::convert::{channel_view, command_error, join_path, split_path};
use crate::session::{
    ConversationRef, Engine, EngineError, EngineEvent, EngineResult, ErrorKind, SessionCtx,
    Snapshot, UnreadView, lock, store_error,
};
use crate::sounds::Sound;

pub use sanitize::sanitize_html;
pub(crate) use sanitize::sanitize_message_html;

/// Size cap for messages with images when the server sets no limit; the UI scales images to
/// the same target.
pub const IMAGE_SOFT_CAP: u32 = 128 * 1024;
/// Upper bound for one `history_load` page.
const MAX_PAGE: u32 = 500;
/// Prefix of ids of messages that were shown but not stored.
const TRANSIENT_PREFIX: &str = "tmp-";

/// A chat message as the UI sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessageView {
    /// Store id, or `tmp-<n>` for messages that were not stored; paging cursor.
    pub id: String,
    pub conversation: ConversationRef,
    pub sender_name: String,
    pub sender_cert_hash: Option<String>,
    /// Sender session while connected; `None` for history entries.
    pub sender_session: Option<u32>,
    pub own: bool,
    pub sent_at: UnixMillis,
    /// Sanitized HTML.
    pub body: String,
    /// Links whose preview card the user dismissed (QA.42).
    pub dismissed_previews: Vec<String>,
    /// Name of the person whose history this message was synced from (FR-1.60); `None` for
    /// messages received or sent here.
    pub synced_from: Option<String>,
}

/// What `history_clear` removes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryScope {
    Conversation {
        server_id: ServerId,
        conversation: ConversationRef,
    },
    Server(ServerId),
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum UnreadKey {
    Stored(Conversation),
    /// Private chat with a person without certificate (lasts only for the session).
    Session(u32),
}

static UNREAD: LazyLock<Mutex<HashMap<(ServerId, UnreadKey), u32>>> =
    LazyLock::new(Default::default);
static TRANSIENT_IDS: AtomicU64 = AtomicU64::new(1);

pub(crate) fn now_millis() -> UnixMillis {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

/// Where an incoming message belongs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Private { session: u32 },
    Channel(u32),
}

fn route(state: &ServerState, msg: &TextMessage) -> Option<Route> {
    let t = &msg.target;
    if t.channels.is_empty() && t.trees.is_empty() {
        // Private messages without a sender come from server scripts; there is no one to
        // answer, so there is no conversation to put them in.
        return msg
            .actor
            .filter(|_| !t.sessions.is_empty())
            .map(|session| Route::Private { session });
    }
    let own = state.own_user();
    let own_channel = own.map(|u| u.channel_id);
    let listens = |c: u32| own.is_some_and(|u| u.listening.contains(&c));
    let reaches_own = |c: &u32| {
        Some(*c) == own_channel
            || (t.trees.contains(c) && own_channel.is_some_and(|o| is_within(state, o, *c)))
    };
    let targets = || t.channels.iter().chain(&t.trees);
    if targets().any(reaches_own) {
        return own_channel.map(Route::Channel);
    }
    targets()
        .find(|c| listens(**c))
        .or_else(|| targets().next())
        .copied()
        .map(Route::Channel)
}

/// Unread badges exist for private chats, the own channel and followed channels (FR-1.31).
fn counts_unread(state: &ServerState, route: Route) -> bool {
    match route {
        Route::Private { .. } => true,
        Route::Channel(c) => state
            .own_user()
            .is_some_and(|u| u.channel_id == c || u.listening.contains(&c)),
    }
}

/// The message sound stays limited to private chats and followed channels: the own channel's
/// messages were silent before they counted as unread.
fn plays_message_sound(state: &ServerState, route: Route) -> bool {
    match route {
        Route::Private { .. } => true,
        Route::Channel(c) => state
            .own_user()
            .is_some_and(|u| u.channel_id != c && u.listening.contains(&c)),
    }
}

/// Which sound a new message makes (QA.41): the ping for a mention, wherever it arrives and
/// even in the chat on screen; otherwise the message sound where it applies. A mention with
/// the ping switched off falls back to the message sound.
fn sound_for(
    state: &ServerState,
    route: Route,
    mention: bool,
    ping_enabled: bool,
) -> Option<Sound> {
    if mention && ping_enabled {
        Some(Sound::Mention)
    } else if plays_message_sound(state, route) {
        Some(Sound::Message)
    } else {
        None
    }
}

/// Whether `channel` is `root` or one of its sub-channels.
fn is_within(state: &ServerState, channel: u32, root: u32) -> bool {
    let mut current = Some(channel);
    while let Some(id) = current {
        if id == root {
            return true;
        }
        current = state.channels.get(&id).and_then(|c| c.parent);
    }
    false
}

/// Rejects empty messages and messages above the server limit (FR-1.30). Lengths are in
/// UTF-16 units like the server counts them; messages with images use the image limit.
fn check_size(html: &str, max_message: u32, max_image: u32) -> EngineResult<()> {
    if html.trim().is_empty() {
        return Err(EngineError::invalid("empty message"));
    }
    let len = html.encode_utf16().count();
    let has_image = html.to_ascii_lowercase().contains("<img");
    let limit = match (has_image, max_image) {
        (true, 0) => IMAGE_SOFT_CAP,
        (true, n) => n,
        (false, _) => max_message,
    };
    if limit > 0 && len > limit as usize {
        return Err(EngineError::new(
            ErrorKind::MessageTooLarge,
            format!("message length {len} exceeds the limit of {limit}"),
        ));
    }
    Ok(())
}

/// Server limits for text (message, image message) in characters; 0 = none.
pub(crate) fn text_limits(state: &ServerState) -> (u32, u32) {
    (
        state.server.message_length.unwrap_or(0),
        state.server.image_message_length.unwrap_or(0),
    )
}

pub(crate) fn check_text_size(ctx: &SessionCtx, html: &str) -> EngineResult<()> {
    let (message, image) = ctx.handle.with_state(text_limits);
    check_size(html, message, image)
}

fn stored_conversation(
    ctx: &SessionCtx,
    conversation: &ConversationRef,
) -> EngineResult<Option<Conversation>> {
    Ok(match conversation {
        ConversationRef::Channel { channel_id } => {
            let path = ctx
                .channel_path(*channel_id)
                .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown channel"))?;
            Some(Conversation::Channel(join_path(&path)))
        }
        ConversationRef::Private { cert_hash } => Some(Conversation::Private(cert_hash.clone())),
        ConversationRef::PrivateSession { .. } => None,
    })
}

fn unread_key(ctx: &SessionCtx, conversation: &ConversationRef) -> EngineResult<UnreadKey> {
    Ok(match stored_conversation(ctx, conversation)? {
        Some(c) => UnreadKey::Stored(c),
        None => match conversation {
            ConversationRef::PrivateSession { session } => UnreadKey::Session(*session),
            _ => return Err(EngineError::internal("conversation without key")),
        },
    })
}

/// Conversation reference of an unread key on the connected server, if it still exists.
fn key_conversation(ctx: &SessionCtx, key: &UnreadKey) -> Option<ConversationRef> {
    match key {
        UnreadKey::Stored(Conversation::Channel(path)) => ctx
            .resolve_channel_path(&split_path(path))
            .map(|channel_id| ConversationRef::Channel { channel_id }),
        UnreadKey::Stored(Conversation::Private(cert_hash)) => Some(ConversationRef::Private {
            cert_hash: cert_hash.clone(),
        }),
        UnreadKey::Session(session) => Some(ConversationRef::PrivateSession { session: *session }),
    }
}

/// Stores a message (if saving is on and the conversation is storable) and returns its id.
fn store_message(
    store: &Store,
    server_id: ServerId,
    conversation: Option<Conversation>,
    message: &ChatMessageView,
) -> String {
    let stored = conversation.and_then(|conversation| {
        let new = NewChatMessage {
            conversation,
            sender_name: message.sender_name.clone(),
            sender_cert_hash: message.sender_cert_hash.clone(),
            sent_at: message.sent_at,
            body: message.body.clone(),
        };
        store.insert_message(server_id, &new).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "storing a chat message failed");
            None
        })
    });
    stored.map_or_else(transient_id, |id| id.to_string())
}

/// A fresh id for a message that was shown but not stored.
pub(crate) fn transient_id() -> String {
    format!(
        "{TRANSIENT_PREFIX}{}",
        TRANSIENT_IDS.fetch_add(1, Ordering::Relaxed)
    )
}

/// Incoming text message: route, sanitize, apply blocks (issue #140) and the private message
/// rule (issue #141), then store, emit and count unread (hook from the session).
pub(crate) fn on_text(
    ctx: &SessionCtx,
    msg: &TextMessage,
    peers: &crate::mumcord::ServerPeers,
    requests: &crate::pm_rules::ServerRequests,
    block_list: &crate::blocking::BlockList,
) {
    let routed = ctx.handle.with_state(|s| {
        let route = route(s, msg)?;
        let sender = msg.actor.and_then(|a| s.users.get(&a));
        Some((
            route,
            sender.map(|u| u.name.clone()).unwrap_or_default(),
            sender.and_then(|u| u.cert_hash.clone()),
            is_own_sender(s, msg.actor),
        ))
    });
    let Some((route, sender_name, sender_cert_hash, own)) = routed else {
        return;
    };
    let conversation = match route {
        Route::Channel(channel_id) => ConversationRef::Channel { channel_id },
        Route::Private { session } => match &sender_cert_hash {
            Some(cert_hash) => ConversationRef::Private {
                cert_hash: cert_hash.clone(),
            },
            None => ConversationRef::PrivateSession { session },
        },
    };
    let message = ChatMessageView {
        id: String::new(),
        conversation,
        sender_name,
        sender_cert_hash,
        sender_session: msg.actor,
        own,
        sent_at: now_millis(),
        body: sanitize_message_html(&msg.message),
        dismissed_previews: Vec::new(),
        synced_from: None,
    };
    let blocked = msg
        .actor
        .is_some_and(|a| crate::blocking::is_blocked(ctx, block_list, a));
    let private = matches!(route, Route::Private { .. });
    let verdict = crate::blocking::text_verdict(private, blocked);
    if verdict == crate::blocking::TextVerdict::Drop {
        return;
    }
    if let Route::Private { session } = route {
        match crate::pm_rules::screen(ctx, peers, requests, session) {
            crate::pm_rules::Verdict::Show => {}
            crate::pm_rules::Verdict::Hold => {
                crate::pm_rules::hold(ctx, requests, session, message);
                return;
            }
            crate::pm_rules::Verdict::Drop => return,
        }
    }
    // Own messages never count as unread, even when a server or another session of the user
    // sends them back.
    let counts_unread = !own
        && verdict == crate::blocking::TextVerdict::Normal
        && ctx.handle.with_state(|s| counts_unread(s, route));
    deliver(ctx, message, counts_unread);
}

/// Whether `actor` is the user's own session or another session with the user's certificate:
/// those messages are the user's own and never notify (issue #215).
fn is_own_sender(state: &ServerState, actor: Option<u32>) -> bool {
    let Some(actor) = actor else {
        return false;
    };
    if state.session == Some(actor) {
        return true;
    }
    let own_hash = state.own_user().and_then(|u| u.cert_hash.as_deref());
    own_hash.is_some() && state.users.get(&actor).and_then(|u| u.cert_hash.as_deref()) == own_hash
}

/// Stores, emits and counts an incoming message; also used for held message requests once
/// they are accepted (issue #141).
pub(crate) fn deliver(ctx: &SessionCtx, mut message: ChatMessageView, counts_unread: bool) {
    let conversation = message.conversation.clone();
    let Ok(key) = unread_key(ctx, &conversation) else {
        return;
    };
    let stored = match &key {
        UnreadKey::Stored(c) => Some(c.clone()),
        UnreadKey::Session(_) => None,
    };
    message.id = store_message(&ctx.store(), ctx.server_id, stored, &message);
    ctx.events
        .emit(EngineEvent::MessageReceived(Box::new(message)));
    if counts_unread {
        let count = {
            let mut unread = lock(&UNREAD);
            let count = unread.entry((ctx.server_id, key)).or_insert(0);
            *count = count.saturating_add(1);
            *count
        };
        ctx.events.emit(EngineEvent::UnreadChanged(UnreadView {
            conversation,
            count,
        }));
    }
}

/// Fills `snapshot.unread` after a (re)sync. Followed channels start from the store's read
/// marker the first time; session-bound private chats end with the old session.
pub(crate) fn on_synced(ctx: &SessionCtx, snapshot: &mut Snapshot) {
    let server = ctx.server_id;
    let followed = ctx.store().followed_channels(server).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "reading followed channels failed");
        Vec::new()
    });
    let mut unread = lock(&UNREAD);
    unread.retain(|(s, key), _| *s != server || !matches!(key, UnreadKey::Session(_)));
    for f in followed {
        let Some(channel_id) = ctx.resolve_channel_path(&split_path(&f.channel_path)) else {
            continue;
        };
        if !snapshot.followed_channel_ids.contains(&channel_id) {
            continue;
        }
        let conversation = Conversation::Channel(f.channel_path.clone());
        let count = *unread
            .entry((server, UnreadKey::Stored(conversation.clone())))
            .or_insert_with(|| {
                ctx.store()
                    .count_messages_since(server, &conversation, f.last_read_at)
                    .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
                    .unwrap_or_else(|e| {
                        tracing::warn!(error = %e, "counting unread messages failed");
                        0
                    })
            });
        if count > 0 {
            snapshot.unread.push(UnreadView {
                conversation: ConversationRef::Channel { channel_id },
                count,
            });
        }
    }
    for ((s, key), count) in unread.iter() {
        // Counts of the own channel (not followed) live only in memory.
        if let (true, UnreadKey::Stored(Conversation::Channel(path))) = (*s == server, key)
            && *count > 0
            && let Some(channel_id) = ctx.resolve_channel_path(&split_path(path))
            && !snapshot
                .unread
                .iter()
                .any(|u| u.conversation == ConversationRef::Channel { channel_id })
        {
            snapshot.unread.push(UnreadView {
                conversation: ConversationRef::Channel { channel_id },
                count: *count,
            });
        }
        if let (true, UnreadKey::Stored(Conversation::Private(cert_hash))) = (*s == server, key)
            && *count > 0
        {
            snapshot.unread.push(UnreadView {
                conversation: ConversationRef::Private {
                    cert_hash: cert_hash.clone(),
                },
                count: *count,
            });
        }
    }
}

/// Drops the unread count of a followed channel path (on unfollow).
pub(crate) fn forget_unread(server: ServerId, channel_path: &str) -> bool {
    lock(&UNREAD)
        .remove(&(
            server,
            UnreadKey::Stored(Conversation::Channel(channel_path.to_owned())),
        ))
        .is_some_and(|n| n > 0)
}

pub(crate) fn history_view(
    m: ChatMessage,
    conversation: &ConversationRef,
    own_hash: Option<&str>,
    dismissed_previews: Vec<String>,
    synced_from: Option<String>,
) -> ChatMessageView {
    let own = own_hash.is_some() && m.sender_cert_hash.as_deref() == own_hash;
    ChatMessageView {
        id: m.id.to_string(),
        conversation: conversation.clone(),
        sender_name: m.sender_name,
        sender_cert_hash: m.sender_cert_hash,
        sender_session: None,
        own,
        sent_at: m.sent_at,
        body: m.body,
        dismissed_previews,
        synced_from,
    }
}

impl Engine {
    /// Plays the sound of a new incoming message (FR-1.27, QA.41). The UI calls this per message
    /// because only it knows the matching rules and the names the user went by: `mention` is
    /// its verdict. The sound toggles and deafness are applied by the sound hub.
    pub fn message_sound(
        &self,
        server_id: ServerId,
        conversation: &ConversationRef,
        mention: bool,
    ) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let route = match conversation {
            ConversationRef::Channel { channel_id } => Route::Channel(*channel_id),
            _ => Route::Private { session: 0 },
        };
        let ping_enabled = self.settings()?.sounds.mention;
        let sound = ctx
            .handle
            .with_state(|s| sound_for(s, route, mention, ping_enabled));
        if let Some(sound) = sound {
            ctx.events.sounds().play(sound);
        }
        Ok(())
    }

    /// Sends Mumble HTML to a channel on `server_id` (FR-1.28, FR-1.30) and returns the stored
    /// own message. Waits briefly for a refusal (FR-2.47); nothing is stored then.
    pub async fn send_channel_message(
        &self,
        server_id: ServerId,
        channel_id: u32,
        html: &str,
    ) -> EngineResult<ChatMessageView> {
        let ctx = self.session_ctx_for(server_id)?;
        let allowed = ctx.handle.with_state(|s| {
            s.channels
                .get(&channel_id)
                .map(|c| channel_view(s, c).permissions.text_message)
        });
        match allowed {
            None => return Err(EngineError::new(ErrorKind::NotFound, "unknown channel")),
            Some(false) => {
                return Err(EngineError::new(
                    ErrorKind::PermissionDenied,
                    "no permission to write in this channel",
                ));
            }
            Some(true) => {}
        }
        check_text_size(&ctx, html)?;
        let denials = subscribe_denials();
        ctx.handle
            .send_text(&TextTarget::channel(channel_id), html)
            .map_err(command_error)?;
        await_refusal(&ctx, denials, Some(channel_id)).await?;
        Ok(own_message(
            &ctx,
            ConversationRef::Channel { channel_id },
            html,
        ))
    }

    /// Sends Mumble HTML to one person on `server_id` and returns the stored own message.
    /// Waits briefly for a refusal (FR-2.47); nothing is stored then.
    pub async fn send_private_message(
        &self,
        server_id: ServerId,
        session: u32,
        html: &str,
    ) -> EngineResult<ChatMessageView> {
        let ctx = self.session_ctx_for(server_id)?;
        let recipient = ctx
            .handle
            .with_state(|s| s.users.get(&session).map(|u| u.cert_hash.clone()))
            .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown person"))?;
        check_text_size(&ctx, html)?;
        let denials = subscribe_denials();
        ctx.handle
            .send_text(&TextTarget::user(session), html)
            .map_err(command_error)?;
        await_refusal(&ctx, denials, None).await?;
        crate::pm_rules::on_own_private_message(&ctx, self.pm_requests_state(), session);
        let conversation = match recipient {
            Some(cert_hash) => ConversationRef::Private { cert_hash },
            None => ConversationRef::PrivateSession { session },
        };
        Ok(own_message(&ctx, conversation, html))
    }

    /// Resets the unread count of a conversation (and the stored read marker of a followed
    /// channel).
    pub fn conversation_mark_read(
        &self,
        server_id: ServerId,
        conversation: ConversationRef,
    ) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let key = unread_key(&ctx, &conversation)?;
        if let UnreadKey::Stored(Conversation::Channel(path)) = &key {
            ctx.store()
                .mark_channel_read(ctx.server_id, path, now_millis())
                .map_err(store_error)?;
        }
        lock(&UNREAD).remove(&(ctx.server_id, key));
        ctx.events.emit(EngineEvent::UnreadChanged(UnreadView {
            conversation,
            count: 0,
        }));
        Ok(())
    }

    /// Stored messages of a conversation on the connected server `server_id`, oldest first;
    /// `before` pages backwards from a message id.
    pub fn history_load(
        &self,
        server_id: ServerId,
        conversation: ConversationRef,
        before: Option<&str>,
        limit: u32,
    ) -> EngineResult<Vec<ChatMessageView>> {
        let ctx = self.session_ctx_for(server_id)?;
        let Some(stored) = stored_conversation(&ctx, &conversation)? else {
            return Ok(Vec::new());
        };
        let limit = limit.clamp(1, MAX_PAGE);
        let rows = {
            let store = ctx.store();
            match before {
                None => store.messages(ctx.server_id, &stored, limit),
                Some(id) if id.starts_with(TRANSIENT_PREFIX) => return Ok(Vec::new()),
                Some(id) => {
                    let id = id
                        .parse::<i64>()
                        .map_err(|_| EngineError::invalid("unknown message id"))?;
                    store.messages_before(ctx.server_id, &stored, id, limit)
                }
            }
            .map_err(store_error)?
        };
        let own_hash = ctx
            .handle
            .with_state(|s| s.own_user().and_then(|u| u.cert_hash.clone()));
        let ids: Vec<i64> = rows.iter().map(|m| m.id).collect();
        let mut dismissed = ctx.store().dismissed_previews(&ids).unwrap_or_else(|e| {
            // The cards then show again; better than failing to load the history.
            tracing::warn!(error = %e, "reading dismissed previews failed");
            Default::default()
        });
        let mut origins = ctx.store().synced_origins(&ids).unwrap_or_else(|e| {
            // Synced messages then show without their origin; better than no history.
            tracing::warn!(error = %e, "reading synced message origins failed");
            Default::default()
        });
        Ok(rows
            .into_iter()
            .map(|m| {
                let urls = dismissed.remove(&m.id).unwrap_or_default();
                let origin = origins.remove(&m.id);
                history_view(m, &conversation, own_hash.as_deref(), urls, origin)
            })
            .collect())
    }

    /// Remembers that the preview card of `url` under the message was dismissed (QA.42).
    /// Messages that were not stored (`tmp-` ids) have nothing to remember it with.
    pub fn preview_dismiss(&self, message_id: &str, url: &str) -> EngineResult<()> {
        if message_id.starts_with(TRANSIENT_PREFIX) {
            return Ok(());
        }
        let id = message_id
            .parse::<i64>()
            .map_err(|_| EngineError::invalid("unknown message id"))?;
        lock(&self.store_handle())
            .dismiss_preview(id, url)
            .map_err(store_error)
    }

    /// The saved GIF favorites, most recently used first (FR-1.52).
    pub fn gif_favorites(&self) -> EngineResult<Vec<mumcord_store::GifFavorite>> {
        lock(&self.store_handle())
            .gif_favorites()
            .map_err(store_error)
    }

    /// Saves an image link as a favorite; only http(s) links are accepted.
    pub fn gif_favorite_add(&self, url: &str) -> EngineResult<()> {
        let url = url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(EngineError::invalid("favorites need an http(s) link"));
        }
        lock(&self.store_handle())
            .add_gif_favorite(url, now_millis())
            .map_err(store_error)
    }

    pub fn gif_favorite_remove(&self, url: &str) -> EngineResult<()> {
        lock(&self.store_handle())
            .remove_gif_favorite(url)
            .map_err(store_error)
    }

    /// Moves a favorite to the front after it was sent.
    pub fn gif_favorite_use(&self, url: &str) -> EngineResult<()> {
        lock(&self.store_handle())
            .use_gif_favorite(url, now_millis())
            .map_err(store_error)
    }

    /// When the user last cleared the history of a channel (issue #119), for channel info
    /// (issue #170); `None` if never, and for private chats.
    pub fn history_cleared_at(
        &self,
        server_id: ServerId,
        conversation: &ConversationRef,
    ) -> EngineResult<Option<UnixMillis>> {
        let key = unread_key(&self.session_ctx_for(server_id)?, conversation)?;
        let UnreadKey::Stored(Conversation::Channel(path)) = key else {
            return Ok(None);
        };
        lock(&self.store_handle())
            .history_cleared_at(server_id, &path)
            .map_err(store_error)
    }

    /// Clears local history (FR-1.33); cleared conversations are no longer unread.
    pub fn history_clear(&self, scope: HistoryScope) -> EngineResult<()> {
        let store = self.store_handle();
        let cleared: Vec<(ServerId, UnreadKey)> = match &scope {
            HistoryScope::Conversation {
                server_id,
                conversation,
            } => {
                let key = match conversation {
                    ConversationRef::Private { cert_hash } => {
                        UnreadKey::Stored(Conversation::Private(cert_hash.clone()))
                    }
                    _ => unread_key(&self.session_ctx_for(*server_id)?, conversation)?,
                };
                if let UnreadKey::Stored(c) = &key {
                    lock(&store)
                        .clear_conversation_history(*server_id, c, now_millis())
                        .map_err(store_error)?;
                }
                vec![(*server_id, key)]
            }
            HistoryScope::Server(server_id) => {
                lock(&store)
                    .clear_server_history(*server_id, now_millis())
                    .map_err(store_error)?;
                lock(&UNREAD)
                    .keys()
                    .filter(|(s, _)| s == server_id)
                    .cloned()
                    .collect()
            }
            HistoryScope::All => {
                lock(&store)
                    .clear_all_history(now_millis())
                    .map_err(store_error)?;
                lock(&UNREAD).keys().cloned().collect()
            }
        };
        let had: Vec<(ServerId, UnreadKey)> = {
            let mut unread = lock(&UNREAD);
            cleared
                .into_iter()
                .filter(|entry| unread.remove(entry).is_some_and(|n| n > 0))
                .collect()
        };
        // Outside the unread lock: a sync takes the core lock first, then that one.
        for (server_id, key) in had {
            if let Ok(ctx) = self.session_ctx_for(server_id)
                && let Some(conversation) = key_conversation(&ctx, &key)
            {
                ctx.events.emit(EngineEvent::UnreadChanged(UnreadView {
                    conversation,
                    count: 0,
                }));
            }
        }
        Ok(())
    }
}

/// Mumble never acknowledges a text message, only refuses it. A refusal arrives within a round
/// trip, so a message counts as delivered once none came for this long.
const REFUSAL_GRACE: Duration = Duration::from_millis(1200);

fn is_text_refusal(denied: &PermissionDenied, channel_id: Option<u32>) -> bool {
    match denied.kind {
        DenyKind::TextTooLong => true,
        DenyKind::Permission => {
            denied
                .permission
                .is_none_or(|p| p & permissions::TEXT_MESSAGE != 0)
                && channel_id.is_none_or(|c| denied.channel_id.is_none_or(|d| d == c))
        }
        _ => false,
    }
}

async fn await_refusal(
    ctx: &SessionCtx,
    denials: tokio::sync::broadcast::Receiver<(ServerId, PermissionDenied)>,
    channel_id: Option<u32>,
) -> EngineResult<()> {
    confirm(
        ctx,
        denials,
        |_| false,
        |d| is_text_refusal(d, channel_id),
        OnTimeout::Accept(REFUSAL_GRACE),
    )
    .await
}

/// Stores and returns an own sent message.
fn own_message(ctx: &SessionCtx, conversation: ConversationRef, html: &str) -> ChatMessageView {
    let (sender_name, sender_cert_hash, sender_session) = ctx.handle.with_state(|s| {
        let own = s.own_user();
        (
            own.map(|u| u.name.clone()).unwrap_or_default(),
            own.and_then(|u| u.cert_hash.clone()),
            s.session,
        )
    });
    let mut message = ChatMessageView {
        id: String::new(),
        conversation,
        sender_name,
        sender_cert_hash,
        sender_session,
        own: true,
        sent_at: now_millis(),
        body: sanitize_message_html(html),
        dismissed_previews: Vec::new(),
        synced_from: None,
    };
    let stored = match &message.conversation {
        ConversationRef::Channel { channel_id } => ctx
            .channel_path(*channel_id)
            .map(|p| Conversation::Channel(join_path(&p))),
        ConversationRef::Private { cert_hash } => Some(Conversation::Private(cert_hash.clone())),
        ConversationRef::PrivateSession { .. } => None,
    };
    message.id = store_message(&ctx.store(), ctx.server_id, stored, &message);
    message
}

#[cfg(test)]
mod tests {
    use mumcord_protocol::{Channel, User};

    use super::*;

    /// Root(0) > Lobby(1), Raid(2) > Team(3); own session 10 in Lobby listening to Raid.
    fn state() -> ServerState {
        let mut s = ServerState {
            session: Some(10),
            ..Default::default()
        };
        for (id, parent, name) in [
            (0, None, "Root"),
            (1, Some(0), "Lobby"),
            (2, Some(0), "Raid"),
            (3, Some(2), "Team"),
        ] {
            s.channels.insert(
                id,
                Channel {
                    id,
                    parent,
                    name: name.into(),
                    ..Default::default()
                },
            );
        }
        s.users.insert(
            10,
            User {
                session: 10,
                channel_id: 1,
                listening: [2].into(),
                ..Default::default()
            },
        );
        s
    }

    fn text(actor: Option<u32>, target: TextTarget) -> TextMessage {
        TextMessage {
            actor,
            target,
            message: "hi".into(),
        }
    }

    #[test]
    fn routes_private_channel_followed_and_tree_messages() {
        let s = state();
        assert_eq!(
            route(&s, &text(Some(5), TextTarget::user(10))),
            Some(Route::Private { session: 5 })
        );
        assert_eq!(route(&s, &text(None, TextTarget::user(10))), None);
        assert_eq!(
            route(&s, &text(Some(5), TextTarget::channel(1))),
            Some(Route::Channel(1))
        );
        assert_eq!(
            route(&s, &text(Some(5), TextTarget::channel(2))),
            Some(Route::Channel(2))
        );
        let both = TextTarget {
            channels: vec![2, 1],
            ..Default::default()
        };
        assert_eq!(route(&s, &text(Some(5), both)), Some(Route::Channel(1)));
        let tree = TextTarget {
            trees: vec![0],
            ..Default::default()
        };
        assert_eq!(route(&s, &text(Some(5), tree)), Some(Route::Channel(1)));
        let other_tree = TextTarget {
            trees: vec![3],
            ..Default::default()
        };
        assert_eq!(
            route(&s, &text(Some(5), other_tree)),
            Some(Route::Channel(3))
        );
    }

    #[test]
    fn messages_from_own_sessions_are_own() {
        let mut s = state();
        s.session = Some(10);
        s.users.insert(
            10,
            User {
                session: 10,
                cert_hash: Some("me".into()),
                ..Default::default()
            },
        );
        s.users.insert(
            11,
            User {
                session: 11,
                cert_hash: Some("me".into()),
                ..Default::default()
            },
        );
        s.users.insert(
            5,
            User {
                session: 5,
                cert_hash: Some("other".into()),
                ..Default::default()
            },
        );
        assert!(is_own_sender(&s, Some(10)), "own session");
        assert!(
            is_own_sender(&s, Some(11)),
            "second session, same certificate"
        );
        assert!(!is_own_sender(&s, Some(5)));
        assert!(!is_own_sender(&s, None));
    }

    #[test]
    fn unread_counts_private_own_and_followed_channels() {
        let s = state();
        assert!(counts_unread(&s, Route::Private { session: 5 }));
        assert!(counts_unread(&s, Route::Channel(2)));
        assert!(counts_unread(&s, Route::Channel(1)), "own channel");
        assert!(!counts_unread(&s, Route::Channel(3)), "not followed");
        assert!(!plays_message_sound(&s, Route::Channel(1)), "own channel");
        assert!(plays_message_sound(&s, Route::Channel(2)));
    }

    #[test]
    fn mention_ping_replaces_message_sound() {
        let s = state();
        let private = Route::Private { session: 5 };
        assert_eq!(sound_for(&s, private, false, true), Some(Sound::Message));
        assert_eq!(sound_for(&s, private, true, true), Some(Sound::Mention));
        assert_eq!(
            sound_for(&s, Route::Channel(1), true, true),
            Some(Sound::Mention),
            "ping even in the own channel"
        );
        assert_eq!(sound_for(&s, Route::Channel(1), false, true), None);
        assert_eq!(
            sound_for(&s, private, true, false),
            Some(Sound::Message),
            "ping switched off"
        );
        assert_eq!(sound_for(&s, Route::Channel(1), true, false), None);
    }

    #[test]
    fn size_limits_follow_server_settings() {
        assert!(check_size("hello", 5, 0).is_ok());
        let err = check_size("hello!", 5, 0).expect_err("too long");
        assert_eq!(err.kind, ErrorKind::MessageTooLarge);
        assert!(check_size(&"x".repeat(100_000), 0, 0).is_ok());
        // Images use the image limit; 0 falls back to the 128 KiB soft cap.
        let img = format!("<img src=\"data:image/png;base64,{}\">", "A".repeat(200));
        assert!(check_size(&img, 10, 1000).is_ok());
        assert!(check_size(&img, 10, 100).is_err());
        let big = format!(
            "<IMG src=\"data:image/png;base64,{}\">",
            "A".repeat(IMAGE_SOFT_CAP as usize)
        );
        assert_eq!(
            check_size(&big, 0, 0).expect_err("cap").kind,
            ErrorKind::MessageTooLarge
        );
        // UTF-16 units, like the server counts.
        assert!(check_size("äöü", 3, 0).is_ok());
        assert!(check_size("😀😀", 3, 0).is_err());
        assert_eq!(
            check_size("  ", 0, 0).expect_err("empty").kind,
            ErrorKind::InvalidInput
        );
    }

    #[test]
    fn history_marks_own_messages_by_certificate() {
        let conv = ConversationRef::Channel { channel_id: 1 };
        let row = |hash: Option<&str>| ChatMessage {
            id: 7,
            conversation: Conversation::Channel("Lobby".into()),
            sender_name: "A".into(),
            sender_cert_hash: hash.map(str::to_owned),
            sent_at: 1,
            body: "b".into(),
        };
        assert!(history_view(row(Some("aa")), &conv, Some("aa"), Vec::new(), None).own);
        assert!(!history_view(row(Some("bb")), &conv, Some("aa"), Vec::new(), None).own);
        assert!(!history_view(row(None), &conv, None, Vec::new(), None).own);
        assert_eq!(
            history_view(row(None), &conv, None, Vec::new(), None).id,
            "7"
        );
    }

    fn denial(
        kind: DenyKind,
        permission: Option<u32>,
        channel_id: Option<u32>,
    ) -> PermissionDenied {
        PermissionDenied {
            kind,
            permission,
            channel_id,
            session: None,
            reason: None,
            name: None,
        }
    }

    #[test]
    fn only_text_denials_refuse_a_message() {
        let write = denial(
            DenyKind::Permission,
            Some(permissions::TEXT_MESSAGE),
            Some(3),
        );
        assert!(is_text_refusal(&write, Some(3)));
        assert!(
            !is_text_refusal(&write, Some(4)),
            "another channel's denial"
        );
        assert!(
            is_text_refusal(&write, None),
            "a private message names the target's channel"
        );
        let enter = denial(DenyKind::Permission, Some(permissions::ENTER), Some(3));
        assert!(!is_text_refusal(&enter, Some(3)), "an unrelated permission");
        let long = denial(DenyKind::TextTooLong, None, None);
        assert!(is_text_refusal(&long, Some(3)));
        assert!(!is_text_refusal(
            &denial(DenyKind::ChannelFull, None, None),
            Some(3)
        ));
    }
}
