//! `webview` (FR-6.46, FR-6.47): not served yet. Every call is refused with `not-permitted`, so plugins
//! that import it still load and can fall back to their tree UI.

use wasmtime::component::{HasSelf, Linker};

use crate::runtime::bindings::mumcord::plugin::webview::{Host, Place, add_to_linker};
use crate::runtime::wit::types::Error;
use crate::runtime::{HostApi, PluginState};

pub(crate) struct WebviewApi;

impl HostApi for WebviewApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
        add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
    }
}

impl Host for PluginState {
    fn show(&mut self, _place: Place, _entry: String) -> Result<(), Error> {
        Err(Error::NotPermitted)
    }

    fn hide(&mut self) {}

    fn post(&mut self, _message: String) -> Result<(), Error> {
        Err(Error::NotPermitted)
    }
}
