//! `random.bytes` (FR-6.24): secure random bytes straight from the OS generator.

use rand::TryRng;
use wasmtime::component::{HasSelf, Linker};

use crate::runtime::bindings::mumcord::plugin::random::{Host, add_to_linker};
use crate::runtime::wit::types::Error;
use crate::runtime::{HostApi, PluginState};

pub const PERMISSION: &str = "random.bytes";
/// Bytes per call.
pub const MAX_BYTES: u32 = 256;

pub(crate) struct RandomApi;

impl HostApi for RandomApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
        add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
    }
}

impl Host for PluginState {
    fn bytes(&mut self, len: u32) -> Result<Vec<u8>, Error> {
        self.ctx.grants.check(&self.id, PERMISSION)?;
        bytes(len)
    }
}

fn bytes(len: u32) -> Result<Vec<u8>, Error> {
    if len > MAX_BYTES {
        return Err(Error::TooLarge);
    }
    let mut buf = vec![0; len as usize];
    rand::rngs::SysRng
        .try_fill_bytes(&mut buf)
        .map_err(|error| {
            // Never fall back to a weaker generator.
            tracing::error!(%error, "the OS random generator failed");
            Error::NotPermitted
        })?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn up_to_256_bytes() {
        assert_eq!(bytes(0).expect("test value").len(), 0);
        let a = bytes(MAX_BYTES).expect("test value");
        assert_eq!(a.len(), 256);
        assert_ne!(a, bytes(MAX_BYTES).expect("test value"));
        assert_eq!(bytes(MAX_BYTES + 1), Err(Error::TooLarge));
    }
}
