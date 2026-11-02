//! Chat cards that take the place of the message they belong to (FR-6.45).
//!
//! While handling a [`ChatMatch`](crate::events::Event::ChatMatch) or
//! [`ChatTrigger`](crate::events::Event::ChatTrigger) event, a plugin may replace that message
//! with one of its chat cards ([`replace_message`]) or hide it ([`hide_message`]), e.g. a
//! `!poll 2` vote it counted. The host keeps the original text one click away and shows the
//! plugin's icon, so the user always sees who replaced what. Only messages the plugin was handed
//! can be replaced; outside such an event both calls do nothing.
//!
//! When the user opens a channel, the host replays its stored messages so old messages get their
//! cards too: [`is_replay`] is then true. A replay is render-only: show or replace cards, but do
//! not count votes, post chat lines or send peer messages again. A replay also hands the plugin
//! its own earlier lines ("\[Name\] …", posted with `chat::send`) as a `ChatMatch` whose
//! `pattern_index` is [`OWN_LINE`].
//!
//! Call [`replace_message`] before showing the card with `ui::show(ui::Slot::ChatCard(id), …)`:
//! the card then appears in the message's place right away instead of below the newest message.

use crate::chat;
use crate::types::Error;

/// `pattern_index` of a replayed `ChatMatch` that is one of the plugin's own earlier lines.
pub const OWN_LINE: u8 = u8::MAX;

/// Whether the event being handled was replayed from stored chat history.
pub fn is_replay() -> bool {
    chat::is_replay()
}

/// Shows chat card `card_id` in place of the message behind the current event. Returns
/// `Ok(false)` when there is no such message (another event, or the message was not stored).
/// Needs `ui.chat-card`.
pub fn replace_message(card_id: &str) -> Result<bool, Error> {
    match chat::current_message() {
        Some(message) => chat::replace(message, card_id).map(|()| true),
        None => Ok(false),
    }
}

/// Hides the message behind the current event behind a small "Hidden by <Plugin>" line.
/// Returns `Ok(false)` when there is no such message.
pub fn hide_message() -> Result<bool, Error> {
    match chat::current_message() {
        Some(message) => chat::hide(message).map(|()| true),
        None => Ok(false),
    }
}
