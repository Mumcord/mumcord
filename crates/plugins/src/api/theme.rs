//! `theme` (FR-6.23): the mode, reduced motion and the colour token names a UI tree may use.
//! Plugins never see CSS or colour values; the renderer maps each token to the app's theme.

use std::sync::{Mutex, PoisonError};

use wasmtime::component::{HasSelf, Linker};

use crate::runtime::bindings::mumcord::plugin::theme::{Host, Mode, ThemeInfo, add_to_linker};
use crate::runtime::{HostApi, PluginState};

/// The colour tokens of the plugin API. The UI renderer accepts exactly these.
pub const TOKENS: &[&str] = &[
    "text",
    "text-muted",
    "accent",
    "danger",
    "warning",
    "success",
    "surface",
    "surface-raised",
    "border",
];

pub(crate) struct ThemeApi;

impl HostApi for ThemeApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
        add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
    }
}

impl Host for PluginState {
    fn current(&mut self) -> ThemeInfo {
        self.ctx.theme.info()
    }
}

/// The current theme as the service last set it; lives in [`crate::HostContext`].
#[derive(Debug)]
pub struct Theme {
    state: Mutex<(Mode, bool)>,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            state: Mutex::new((Mode::Light, false)),
        }
    }
}

impl Theme {
    /// Records a theme change. The service also broadcasts `theme-changed` with [`Self::info`].
    pub fn set(&self, mode: Mode, reduced_motion: bool) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = (mode, reduced_motion);
    }

    pub fn info(&self) -> ThemeInfo {
        let (mode, reduced_motion) = *self.state.lock().unwrap_or_else(PoisonError::into_inner);
        ThemeInfo {
            mode,
            tokens: TOKENS.iter().map(|token| (*token).to_owned()).collect(),
            reduced_motion,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_follows_the_last_change() {
        let theme = Theme::default();
        assert_eq!(theme.info().mode, Mode::Light);
        theme.set(Mode::Dark, true);
        let info = theme.info();
        assert_eq!(info.mode, Mode::Dark);
        assert!(info.reduced_motion);
        assert!(info.tokens.iter().any(|t| t == "accent"));
    }
}
