//! `chat` (FR-6.44, FR-6.45): text lines posted for the user and hiding them behind cards. The
//! permission is checked here; the gesture window, supersede matching and message claims are the
//! [`PluginIo`](super::io::PluginIo)'s job (the service's `GestureGate`, `SupersedeTracker` and
//! `MessageClaims`).

use wasmtime::component::{HasSelf, Linker};

use crate::runtime::bindings::mumcord::plugin::chat::{Host, add_to_linker};
use crate::runtime::wit::types::Error;
use crate::runtime::{HostApi, PluginState};

pub const SEND_PERMISSION: &str = "chat.send";
pub const SUPERSEDE_PERMISSION: &str = "chat.supersedes";
pub const CARD_PERMISSION: &str = "ui.chat-card";
/// Longest chat card id `chat.replace` accepts.
const MAX_CARD_ID: usize = 64;

pub(crate) struct ChatApi;

impl HostApi for ChatApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
        add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
    }
}

impl Host for PluginState {
    fn send(&mut self, text: String) -> Result<(), Error> {
        self.ctx.grants.check(&self.id, SEND_PERMISSION)?;
        self.io()?.chat_send(&self.id, text)
    }

    fn line_hash(&mut self, text: String) -> u32 {
        match self.io() {
            Ok(io) => io.line_hash(&self.id, &text),
            // Without an io nothing can be posted, so no line will ever match.
            Err(_) => crate::chat::hash_line(&text),
        }
    }

    fn supersede(&mut self, card_id: String, sender: u32, hash: u32) -> Result<(), Error> {
        self.ctx.grants.check(&self.id, SUPERSEDE_PERMISSION)?;
        self.io()?.supersede(&self.id, card_id, sender, hash)
    }

    fn current_message(&mut self) -> Option<u64> {
        self.chat.token
    }

    fn is_replay(&mut self) -> bool {
        self.chat.replay
    }

    fn replace(&mut self, message: u64, card_id: String) -> Result<(), Error> {
        self.ctx.grants.check(&self.id, CARD_PERMISSION)?;
        if !valid_card_id(&card_id) {
            return Err(Error::Invalid);
        }
        self.io()?.replace_message(&self.id, message, Some(card_id))
    }

    fn hide(&mut self, message: u64) -> Result<(), Error> {
        self.io()?.replace_message(&self.id, message, None)
    }
}

/// A chat card id as the UI keys it: 1–64 of `A–Z a–z 0–9 - _ . :`.
fn valid_card_id(id: &str) -> bool {
    (1..=MAX_CARD_ID).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

#[cfg(test)]
mod tests {
    use super::valid_card_id;

    #[test]
    fn card_ids_are_short_and_plain() {
        assert!(valid_card_id("echo-echo-12"));
        assert!(valid_card_id("fit:3"));
        assert!(!valid_card_id(""));
        assert!(!valid_card_id("a b"));
        assert!(!valid_card_id(&"x".repeat(65)));
    }
}
