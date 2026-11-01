//! `chat` (FR-6.44, FR-6.45): text lines posted for the user and hiding them behind cards. The
//! permission is checked here; the gesture window and supersede matching are the
//! [`PluginIo`](super::io::PluginIo)'s job (the service's `GestureGate` and `SupersedeTracker`).

use wasmtime::component::{HasSelf, Linker};

use crate::runtime::bindings::mumcord::plugin::chat::{Host, add_to_linker};
use crate::runtime::wit::types::Error;
use crate::runtime::{HostApi, PluginState};

pub const SEND_PERMISSION: &str = "chat.send";
pub const SUPERSEDE_PERMISSION: &str = "chat.supersedes";

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
}
