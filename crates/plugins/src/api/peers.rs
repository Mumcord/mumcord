//! `peers` (FR-6.17, FR-6.33 … FR-6.36): plugin messages to other users in the channel. The
//! permission is checked here; envelopes, rate limits and recipients are the
//! [`PluginIo`](super::io::PluginIo)'s job (the service's `Messenger`).

use wasmtime::component::{HasSelf, Linker};

use crate::runtime::bindings::mumcord::plugin::peers::{Host, Target, add_to_linker};
use crate::runtime::wit::types::{Error, Peer};
use crate::runtime::{HostApi, PluginState};

pub const PERMISSION: &str = "peers.messages";
/// `presence` is a snapshot of the same users the presence events describe.
pub const PRESENCE_PERMISSION: &str = "events.presence";

pub(crate) struct PeersApi;

impl HostApi for PeersApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
        add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
    }
}

impl Host for PluginState {
    fn send(&mut self, to: Target, kind: u8, payload: Vec<u8>) -> Result<(), Error> {
        self.ctx.grants.check(&self.id, PERMISSION)?;
        self.io()?.peer_send(&self.id, to, kind, payload)
    }

    fn start_game_burst(&mut self) -> Result<(), Error> {
        self.ctx.grants.check(&self.id, PERMISSION)?;
        self.io()?.start_game_burst(&self.id)
    }

    fn presence(&mut self) -> Result<Vec<Peer>, Error> {
        self.ctx.grants.check(&self.id, PRESENCE_PERMISSION)?;
        self.io()?.presence(&self.id)
    }
}
