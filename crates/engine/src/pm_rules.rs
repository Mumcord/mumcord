//! "Private messages from: Everyone / Friends only / Nobody" with chat and message requests
//! (issue #141).
//!
//! Mumble servers deliver every private message, so the rule is applied here, on receipt: a
//! message is shown, held as a message request, or dropped. Nothing is ever sent back that
//! would reveal the rule. Other Mumcord users can instead send a hidden chat request
//! (plugin data [`DATA_ID`]) that the user accepts or declines.
//!
//! Requests, cooldowns and sent requests live in memory only (FR-3.103). Accepted
//! conversations are stored per own identity (FR-3.79).

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use mumcord_protocol::{PLUGIN_DATA_MAX, PluginData};
use mumcord_store::{AcceptedConversation, PmRule, PrivateMessageSettings, ServerId, UnixMillis};
use serde::{Deserialize, Serialize};

use crate::chat::{ChatMessageView, deliver, now_millis};
use crate::session::convert::command_error;
use crate::session::{
    Engine, EngineError, EngineEvent, EngineResult, ErrorKind, SessionCtx, lock, store_error,
};

/// Plugin data id of chat requests and their acceptance.
pub const DATA_ID: &str = "mumcord.chatreq";
/// Hello feature of clients that understand chat requests.
pub const FEATURE: &str = "chat-request";
/// Longest note of a chat request, in characters.
pub const MAX_NOTE_CHARS: usize = 200;
/// How long a declined sender's new requests are ignored, and how long a sent request stays
/// pending before it can be sent again.
const COOLDOWN: Duration = Duration::from_secs(10 * 60);
/// Bounds of what a flood of strangers can make the client hold in memory.
const MAX_REQUESTS: usize = 50;
const MAX_HELD_MESSAGES: usize = 20;

/// What happens to an incoming private message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Show,
    /// Kept as a message request.
    Hold,
    Drop,
}

/// What is known about the sender of a private message.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Sender {
    pub friend: bool,
    pub accepted: bool,
    pub same_channel: bool,
    /// The sender runs Mumcord and can send a chat request instead.
    pub mumcord: bool,
}

/// The rule applied to one sender. "People in my current channel" lifts both "Friends only"
/// and "Nobody"; accepted conversations only lift "Friends only".
pub(crate) fn verdict(settings: &PrivateMessageSettings, server: ServerId, s: Sender) -> Verdict {
    let rule = settings.rule_for(server);
    if rule == PmRule::Everyone || (settings.allow_current_channel && s.same_channel) {
        return Verdict::Show;
    }
    if rule == PmRule::Nobody {
        return Verdict::Drop;
    }
    if s.friend || (settings.allow_accepted && s.accepted) {
        return Verdict::Show;
    }
    if s.mumcord || !settings.keep_requests {
        Verdict::Drop
    } else {
        Verdict::Hold
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Wire {
    Request {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    Accepted,
}

fn encode(wire: &Wire) -> Vec<u8> {
    serde_json::to_vec(wire).unwrap_or_default()
}

fn decode(data: &[u8]) -> Option<Wire> {
    if data.len() > PLUGIN_DATA_MAX {
        return None;
    }
    let mut wire: Wire = serde_json::from_slice(data).ok()?;
    if let Wire::Request { note } = &mut wire {
        *note = note.as_deref().and_then(clean_note);
    }
    Some(wire)
}

/// A note as plain text: control characters removed, whitespace collapsed, length capped.
/// `None` when nothing is left.
pub fn clean_note(note: &str) -> Option<String> {
    let text = note
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NOTE_CHARS)
        .collect::<String>();
    (!text.is_empty()).then_some(text)
}

/// A person as requests know them: by certificate when there is one, else by session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Person {
    Cert(String),
    Session(u32),
}

impl Person {
    fn of(session: u32, cert_hash: Option<&str>) -> Self {
        cert_hash.map_or(Self::Session(session), |h| Self::Cert(h.to_owned()))
    }
}

struct Incoming {
    id: u64,
    person: Person,
    session: u32,
    name: String,
    cert_hash: Option<String>,
    chat_request: bool,
    note: Option<String>,
    messages: Vec<ChatMessageView>,
    received_at: UnixMillis,
}

struct Sent {
    person: Person,
    session: u32,
    name: String,
    cert_hash: Option<String>,
    accepted: bool,
    sent: Instant,
}

/// Requests of one connection.
#[derive(Default)]
pub(crate) struct Requests {
    next_id: u64,
    incoming: Vec<Incoming>,
    sent: Vec<Sent>,
    declined: HashMap<Person, Instant>,
    /// Accepted senders without a certificate; they cannot be stored, so this lasts as long as
    /// their session.
    accepted_sessions: HashSet<u32>,
}

pub(crate) type ServerRequests = std::sync::Mutex<HashMap<ServerId, Requests>>;

/// A held chat or message request, newest last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRequestView {
    pub id: u64,
    pub session: u32,
    pub name: String,
    pub cert_hash: Option<String>,
    /// A Mumcord chat request; otherwise held messages from a standard client.
    pub chat_request: bool,
    /// Plain text note of a chat request.
    pub note: Option<String>,
    /// Held messages (sanitized), oldest first.
    pub messages: Vec<ChatMessageView>,
    pub received_at: UnixMillis,
}

/// A chat request the user sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentChatRequestView {
    pub session: u32,
    pub name: String,
    pub cert_hash: Option<String>,
    /// `false` while pending: a decline is never told.
    pub accepted: bool,
}

/// Requests of one server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PmRequestsView {
    pub incoming: Vec<MessageRequestView>,
    pub sent: Vec<SentChatRequestView>,
}

impl Requests {
    fn view(&self) -> PmRequestsView {
        PmRequestsView {
            incoming: self
                .incoming
                .iter()
                .map(|r| MessageRequestView {
                    id: r.id,
                    session: r.session,
                    name: r.name.clone(),
                    cert_hash: r.cert_hash.clone(),
                    chat_request: r.chat_request,
                    note: r.note.clone(),
                    messages: r.messages.clone(),
                    received_at: r.received_at,
                })
                .collect(),
            sent: self
                .sent
                .iter()
                .map(|s| SentChatRequestView {
                    session: s.session,
                    name: s.name.clone(),
                    cert_hash: s.cert_hash.clone(),
                    accepted: s.accepted,
                })
                .collect(),
        }
    }

    fn take_incoming(&mut self, pred: impl Fn(&Incoming) -> bool) -> Option<Incoming> {
        let index = self.incoming.iter().position(pred)?;
        Some(self.incoming.remove(index))
    }

    fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}

fn publish(ctx: &SessionCtx, requests: &ServerRequests) {
    let view = lock(requests)
        .get(&ctx.server_id)
        .map(Requests::view)
        .unwrap_or_default();
    ctx.events.emit(EngineEvent::PmRequestsChanged(view));
}

fn settings(ctx: &SessionCtx) -> PrivateMessageSettings {
    ctx.store().settings().map_or_else(
        |e| {
            // Showing everything is the behaviour before the rule existed; never lose messages
            // because the settings could not be read.
            tracing::warn!(error = %e, "reading settings failed; showing private messages");
            PrivateMessageSettings::default()
        },
        |s| s.private_messages,
    )
}

fn own_cert_hash(ctx: &SessionCtx) -> Option<String> {
    ctx.handle
        .with_state(|s| s.own_user().and_then(|u| u.cert_hash.clone()))
}

/// The rule for a private message from `session` (hook from `chat::on_text`).
pub(crate) fn screen(
    ctx: &SessionCtx,
    peers: &crate::mumcord::ServerPeers,
    requests: &ServerRequests,
    session: u32,
) -> Verdict {
    let settings = settings(ctx);
    if settings.rule_for(ctx.server_id) == PmRule::Everyone {
        return Verdict::Show;
    }
    let (cert_hash, same_channel, own_hash) = ctx.handle.with_state(|s| {
        let own = s.own_user();
        let sender = s.users.get(&session);
        (
            sender.and_then(|u| u.cert_hash.clone()),
            own.zip(sender)
                .is_some_and(|(o, u)| o.channel_id == u.channel_id),
            own.and_then(|u| u.cert_hash.clone()),
        )
    });
    let store = ctx.store();
    let check = |result: mumcord_store::Result<bool>| {
        result.unwrap_or_else(|e| {
            tracing::warn!(error = %e, "reading the private message rule data failed");
            false
        })
    };
    let friend = cert_hash
        .as_deref()
        .is_some_and(|h| check(store.is_friend(h)));
    let accepted = match (&own_hash, &cert_hash) {
        (Some(own), Some(theirs)) => check(store.is_conversation_accepted(own, theirs)),
        _ => lock(requests)
            .get(&ctx.server_id)
            .is_some_and(|r| r.accepted_sessions.contains(&session)),
    };
    drop(store);
    let mumcord = lock(peers)
        .get(&ctx.server_id)
        .is_some_and(|p| p.features(session).is_some());
    verdict(
        &settings,
        ctx.server_id,
        Sender {
            friend,
            accepted,
            same_channel,
            mumcord,
        },
    )
}

/// Keeps a filtered message as a message request; no sound, toast or unread count.
pub(crate) fn hold(
    ctx: &SessionCtx,
    requests: &ServerRequests,
    session: u32,
    mut message: ChatMessageView,
) {
    message.id = crate::chat::transient_id();
    {
        let mut all = lock(requests);
        let server = all.entry(ctx.server_id).or_default();
        let person = Person::of(session, message.sender_cert_hash.as_deref());
        if let Some(open) = server.incoming.iter_mut().find(|r| r.person == person) {
            if open.messages.len() >= MAX_HELD_MESSAGES {
                return;
            }
            open.session = session;
            open.name.clone_from(&message.sender_name);
            open.messages.push(message);
        } else {
            if server.incoming.len() >= MAX_REQUESTS {
                return;
            }
            let id = server.new_id();
            server.incoming.push(Incoming {
                id,
                person,
                session,
                name: message.sender_name.clone(),
                cert_hash: message.sender_cert_hash.clone(),
                chat_request: false,
                note: None,
                received_at: message.sent_at,
                messages: vec![message],
            });
        }
    }
    publish(ctx, requests);
}

/// A chat request or its acceptance from another Mumcord client.
pub(crate) fn on_plugin_data(ctx: &SessionCtx, requests: &ServerRequests, data: &PluginData) {
    if data.data_id != DATA_ID {
        return;
    }
    let Some(sender) = data.sender else {
        return;
    };
    let Some(wire) = decode(&data.data) else {
        return;
    };
    let Some((name, cert_hash)) = person_in(ctx, sender) else {
        return;
    };
    let person = Person::of(sender, cert_hash.as_deref());
    let changed = match wire {
        Wire::Request { note } => {
            if settings(ctx).rule_for(ctx.server_id) == PmRule::Nobody {
                return;
            }
            let mut all = lock(requests);
            let server = all.entry(ctx.server_id).or_default();
            let cooling = server
                .declined
                .get(&person)
                .is_some_and(|at| at.elapsed() < COOLDOWN);
            if cooling
                || server.incoming.len() >= MAX_REQUESTS
                || server.incoming.iter().any(|r| r.person == person)
            {
                return;
            }
            let id = server.new_id();
            server.incoming.push(Incoming {
                id,
                person,
                session: sender,
                name,
                cert_hash,
                chat_request: true,
                note,
                messages: Vec::new(),
                received_at: now_millis(),
            });
            true
        }
        Wire::Accepted => lock(requests)
            .get_mut(&ctx.server_id)
            .and_then(|server| {
                server
                    .sent
                    .iter_mut()
                    .find(|s| s.person == person && !s.accepted)
            })
            .map(|sent| {
                sent.accepted = true;
                sent.session = sender;
            })
            .is_some(),
    };
    if changed {
        publish(ctx, requests);
    }
}

/// Session-only requests end with the session: the number may be reused by someone else.
pub(crate) fn on_user_removed(ctx: &SessionCtx, requests: &ServerRequests, session: u32) {
    let changed = {
        let mut all = lock(requests);
        let Some(server) = all.get_mut(&ctx.server_id) else {
            return;
        };
        server.accepted_sessions.remove(&session);
        let gone = Person::Session(session);
        let before = server.incoming.len() + server.sent.len();
        server.incoming.retain(|r| r.person != gone);
        server.sent.retain(|s| s.person != gone);
        before != server.incoming.len() + server.sent.len()
    };
    if changed {
        publish(ctx, requests);
    }
}

pub(crate) fn on_disconnect(requests: &ServerRequests, server_id: ServerId) {
    lock(requests).remove(&server_id);
}

/// Accepts a conversation with `session`: stored per own identity when both sides have a
/// certificate, otherwise remembered for the session. Releases a held request from them.
fn accept(
    ctx: &SessionCtx,
    requests: &ServerRequests,
    session: u32,
    name: &str,
    cert_hash: Option<&str>,
) -> EngineResult<()> {
    match (own_cert_hash(ctx), cert_hash) {
        (Some(own), Some(theirs)) => {
            let store = ctx.store();
            store
                .accept_conversation(&own, theirs, name, now_millis())
                .map_err(store_error)?;
            let list = store.accepted_conversations().map_err(store_error)?;
            drop(store);
            ctx.events
                .emit(EngineEvent::AcceptedConversationsChanged(list));
        }
        _ => {
            lock(requests)
                .entry(ctx.server_id)
                .or_default()
                .accepted_sessions
                .insert(session);
        }
    }
    let person = Person::of(session, cert_hash);
    let held = lock(requests)
        .get_mut(&ctx.server_id)
        .and_then(|server| server.take_incoming(|r| r.person == person));
    if let Some(held) = held {
        release(ctx, held);
        publish(ctx, requests);
    }
    Ok(())
}

/// Name and certificate hash of another person on the server.
fn person_in(ctx: &SessionCtx, session: u32) -> Option<(String, Option<String>)> {
    ctx.handle.with_state(|s| {
        (s.session != Some(session))
            .then(|| s.users.get(&session))
            .flatten()
            .map(|u| (u.name.clone(), u.cert_hash.clone()))
    })
}

/// Shows the held messages of an accepted request as if they had just arrived.
fn release(ctx: &SessionCtx, held: Incoming) {
    for message in held.messages {
        deliver(ctx, message, true);
    }
}

/// Writing to someone accepts the conversation ("I wrote first or replied", issue #141).
pub(crate) fn on_own_private_message(ctx: &SessionCtx, requests: &ServerRequests, session: u32) {
    let Some((name, cert_hash)) = person_in(ctx, session) else {
        return;
    };
    if let Err(e) = accept(ctx, requests, session, &name, cert_hash.as_deref()) {
        tracing::warn!(error = %e, "could not accept the conversation");
    }
}

impl Engine {
    /// Held chat and message requests and sent chat requests of `server_id`.
    pub fn pm_requests(&self, server_id: ServerId) -> PmRequestsView {
        lock(self.pm_requests_state())
            .get(&server_id)
            .map(Requests::view)
            .unwrap_or_default()
    }

    /// Accepts a held request: the sender may keep messaging, held messages are shown, and a
    /// Mumcord sender is told.
    pub fn pm_request_accept(&self, server_id: ServerId, id: u64) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let requests = self.pm_requests_state();
        let (session, name, cert_hash, chat_request) = lock(requests)
            .get(&server_id)
            .and_then(|s| s.incoming.iter().find(|r| r.id == id))
            .map(|r| {
                (
                    r.session,
                    r.name.clone(),
                    r.cert_hash.clone(),
                    r.chat_request,
                )
            })
            .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown request"))?;
        accept(&ctx, requests, session, &name, cert_hash.as_deref())?;
        // A sender who left has nobody to tell; the acceptance is stored all the same.
        let still_there = person_in(&ctx, session).is_some_and(|(_, h)| h == cert_hash);
        if chat_request && still_there {
            ctx.handle
                .send_plugin_data(&[session], DATA_ID, encode(&Wire::Accepted))
                .map_err(command_error)?;
        }
        Ok(())
    }

    /// Declines a held request silently; the sender's new chat requests are ignored for a
    /// while.
    pub fn pm_request_decline(&self, server_id: ServerId, id: u64) -> EngineResult<()> {
        let ctx = self.session_ctx_for(server_id)?;
        let requests = self.pm_requests_state();
        {
            let mut all = lock(requests);
            let server = all
                .get_mut(&server_id)
                .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown request"))?;
            let request = server
                .take_incoming(|r| r.id == id)
                .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown request"))?;
            server.declined.insert(request.person, Instant::now());
        }
        publish(&ctx, requests);
        Ok(())
    }

    /// Sends a chat request with an optional note to another Mumcord user. Their reply is
    /// accepted on this side right away, since the user wrote first.
    pub fn chat_request_send(
        &self,
        server_id: ServerId,
        session: u32,
        note: Option<&str>,
    ) -> EngineResult<()> {
        if !self.mumcord_user_has_feature(server_id, session, FEATURE) {
            return Err(EngineError::invalid(
                "this person cannot receive chat requests",
            ));
        }
        let ctx = self.session_ctx_for(server_id)?;
        let (name, cert_hash) = person_in(&ctx, session)
            .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown person"))?;
        let person = Person::of(session, cert_hash.as_deref());
        let requests = self.pm_requests_state();
        let open = lock(requests).get(&server_id).is_some_and(|s| {
            s.sent
                .iter()
                .any(|r| r.person == person && (r.accepted || r.sent.elapsed() < COOLDOWN))
        });
        if open {
            return Err(EngineError::invalid("a chat request is already open"));
        }
        let wire = Wire::Request {
            note: note.and_then(clean_note),
        };
        ctx.handle
            .send_plugin_data(&[session], DATA_ID, encode(&wire))
            .map_err(command_error)?;
        accept(&ctx, requests, session, &name, cert_hash.as_deref())?;
        {
            let mut all = lock(requests);
            let server = all.entry(server_id).or_default();
            server.sent.retain(|r| r.person != person);
            server.sent.push(Sent {
                person,
                session,
                name,
                cert_hash,
                accepted: false,
                sent: Instant::now(),
            });
        }
        publish(&ctx, requests);
        Ok(())
    }

    /// Accepted conversations of every own identity, by name.
    pub fn accepted_conversations(&self) -> EngineResult<Vec<AcceptedConversation>> {
        lock(&self.store_handle())
            .accepted_conversations()
            .map_err(store_error)
    }

    /// Revokes an accepted conversation of one own identity.
    pub fn accepted_conversation_revoke(
        &self,
        own_cert_hash: &str,
        cert_hash: &str,
    ) -> EngineResult<()> {
        let list = {
            let handle = self.store_handle();
            let store = lock(&handle);
            store
                .revoke_conversation(own_cert_hash, cert_hash)
                .map_err(store_error)?;
            store.accepted_conversations().map_err(store_error)?
        };
        self.events()
            .emit(EngineEvent::AcceptedConversationsChanged(list));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(rule: PmRule) -> PrivateMessageSettings {
        PrivateMessageSettings {
            rule,
            ..PrivateMessageSettings::default()
        }
    }

    const STRANGER: Sender = Sender {
        friend: false,
        accepted: false,
        same_channel: false,
        mumcord: false,
    };

    #[test]
    fn everyone_shows_every_message() {
        assert_eq!(
            verdict(&rules(PmRule::Everyone), 1, STRANGER),
            Verdict::Show
        );
    }

    #[test]
    fn friends_only_holds_strangers_and_drops_mumcord_senders() {
        let s = rules(PmRule::FriendsOnly);
        assert_eq!(verdict(&s, 1, STRANGER), Verdict::Hold);
        let mumcord = Sender {
            mumcord: true,
            ..STRANGER
        };
        assert_eq!(verdict(&s, 1, mumcord), Verdict::Drop);
        let friend = Sender {
            friend: true,
            ..STRANGER
        };
        assert_eq!(verdict(&s, 1, friend), Verdict::Show);
        let dropping = PrivateMessageSettings {
            keep_requests: false,
            ..s
        };
        assert_eq!(verdict(&dropping, 1, STRANGER), Verdict::Drop);
    }

    #[test]
    fn exceptions_follow_their_switches() {
        let accepted = Sender {
            accepted: true,
            ..STRANGER
        };
        let channel = Sender {
            same_channel: true,
            ..STRANGER
        };
        let friends = rules(PmRule::FriendsOnly);
        assert_eq!(verdict(&friends, 1, accepted), Verdict::Show);
        assert_eq!(verdict(&friends, 1, channel), Verdict::Show);
        let off = PrivateMessageSettings {
            allow_accepted: false,
            allow_current_channel: false,
            ..friends
        };
        assert_eq!(verdict(&off, 1, accepted), Verdict::Hold);
        assert_eq!(verdict(&off, 1, channel), Verdict::Hold);
        let nobody = rules(PmRule::Nobody);
        assert_eq!(verdict(&nobody, 1, channel), Verdict::Show);
        assert_eq!(verdict(&nobody, 1, accepted), Verdict::Drop);
        let friend = Sender {
            friend: true,
            ..STRANGER
        };
        assert_eq!(verdict(&nobody, 1, friend), Verdict::Drop);
    }

    #[test]
    fn server_override_replaces_the_global_rule() {
        let mut s = rules(PmRule::FriendsOnly);
        s.server_rules.insert(2, PmRule::Everyone);
        assert_eq!(verdict(&s, 2, STRANGER), Verdict::Show);
        assert_eq!(verdict(&s, 3, STRANGER), Verdict::Hold);
    }

    #[test]
    fn wire_round_trips_and_cleans_the_note() {
        let request = Wire::Request {
            note: Some("Hi!".into()),
        };
        assert_eq!(decode(&encode(&request)), Some(request));
        assert_eq!(decode(&encode(&Wire::Accepted)), Some(Wire::Accepted));
        let messy = br#"{"t":"request","note":"  a\u0007\n  b  "}"#;
        assert_eq!(
            decode(messy),
            Some(Wire::Request {
                note: Some("a b".into())
            })
        );
        let blank = br#"{"t":"request","note":"   "}"#;
        assert_eq!(decode(blank), Some(Wire::Request { note: None }));
        assert_eq!(decode(b"<b>hi</b>"), None);
        assert_eq!(decode(&vec![b' '; PLUGIN_DATA_MAX + 1]), None);
    }

    #[test]
    fn notes_are_capped() {
        let long = "x".repeat(MAX_NOTE_CHARS + 50);
        assert_eq!(
            clean_note(&long).map(|n| n.chars().count()),
            Some(MAX_NOTE_CHARS)
        );
    }
}
