//! `theme` (FR-6.23): the mode, reduced motion and the colour token names a UI tree may use.
//! Plugins never see CSS or colour values; the renderer maps each token to the app's theme.
//!
//! Web pages of API 1.2 plugins do get values: [`PageTheme`] carries the resolved `--mc-`
//! tokens and mode attributes (FR-13.9 … FR-13.14) that the custom protocol embeds into each
//! served page before its first paint.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use serde::Serialize;

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

/// `data-mc-mode` of a page (FR-13.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageMode {
    Dark,
    Light,
    /// The operating system's forced colours are active in Mumcord's window.
    HighContrast,
}

/// `data-mc-scheme` and `color-scheme` of a page: the base variant, also under high contrast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PageScheme {
    Dark,
    Light,
}

/// The longest token name and value accepted; Mumcord's own values are far shorter.
const MAX_TOKEN_NAME: usize = 64;
const MAX_TOKEN_VALUE: usize = 256;

/// The theme of plugin pages as the UI resolved it (FR-13.9, FR-13.11, FR-13.12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageTheme {
    pub mode: PageMode,
    pub scheme: PageScheme,
    pub reduced_motion: bool,
    /// `--mc-…` name → CSS value; only tokens that pass [`is_valid_token`].
    pub tokens: BTreeMap<String, String>,
}

impl PageTheme {
    /// A page theme with every token that fails [`is_valid_token`] left out (FR-13.14).
    pub fn new(
        mode: PageMode,
        scheme: PageScheme,
        reduced_motion: bool,
        tokens: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        let tokens = tokens
            .into_iter()
            .filter(|(name, value)| {
                let valid = is_valid_token(name, value);
                if !valid {
                    tracing::warn!("a theme token for plugin pages was dropped");
                }
                valid
            })
            .collect();
        Self {
            mode,
            scheme,
            reduced_motion,
            tokens,
        }
    }

    /// The JSON data block the custom protocol puts in front of the bootstrap (FR-13.13). It is
    /// never executed; the bootstrap reads it, applies the values and removes it. Characters that
    /// could end the block or confuse an HTML or JS parser are escaped (FR-13.14).
    pub fn data_block(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_else(|error| {
            tracing::error!(%error, "the theme of plugin pages could not be serialized");
            "null".to_owned()
        });
        let mut escaped = String::with_capacity(json.len() + 64);
        for c in json.chars() {
            match c {
                '<' => escaped.push_str("\\u003c"),
                '>' => escaped.push_str("\\u003e"),
                '&' => escaped.push_str("\\u0026"),
                '\u{2028}' => escaped.push_str("\\u2028"),
                '\u{2029}' => escaped.push_str("\\u2029"),
                other => escaped.push(other),
            }
        }
        format!(r#"<script type="application/json">{escaped}</script>"#)
    }
}

/// Whether `name: value` may go into a page (FR-13.14): a `--mc-` name of lowercase letters,
/// digits and dashes, and a value of the characters Mumcord's own tokens use (hex and functional
/// colours, lengths, durations, quoted font names). Nothing that could load a resource
/// (`url(`), escape (`\`) or end a declaration or block (`;`, `{`, `}`, `<`) passes.
pub fn is_valid_token(name: &str, value: &str) -> bool {
    let valid_name = name.len() <= MAX_TOKEN_NAME
        && name.strip_prefix("--mc-").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        });
    let valid_value = !value.trim().is_empty()
        && value.len() <= MAX_TOKEN_VALUE
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" #%().,-_\"'/+".contains(&b))
        && !value.to_ascii_lowercase().contains("url(");
    valid_name && valid_value
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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

    fn token(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    #[test]
    fn accepts_mumcords_own_values() {
        for (name, value) in [
            ("--mc-color-text", "#f1edff"),
            ("--mc-color-backdrop", "rgb(7 5 20 / 72%)"),
            (
                "--mc-font-family",
                r#""Instrument Sans Variable", "Segoe UI", system-ui, sans-serif"#,
            ),
            ("--mc-ease-out", "cubic-bezier(0.22, 1, 0.36, 1)"),
            ("--mc-duration-fast", "0ms"),
            (
                "--mc-shadow-raised",
                "0 1rem 2.5rem -0.75rem rgb(0 0 0 / 60%)",
            ),
            (
                "--mc-color-hover",
                "color-mix(in srgb, #fff 6%, transparent)",
            ),
            (
                "--mc-shadow-primary-glow",
                "0 0.5rem 1.5rem -0.25rem color-mix(in srgb, #7c5cff 55%, transparent)",
            ),
        ] {
            assert!(is_valid_token(name, value), "{name}: {value}");
        }
    }

    #[test]
    fn drops_anything_else() {
        for (name, value) in [
            ("--color-text", "#fff"),
            ("--mc-", "#fff"),
            ("--mc-Color", "#fff"),
            ("--mc-color-text", ""),
            ("--mc-color-text", "  "),
            ("--mc-color-text", "red;background:red"),
            ("--mc-color-text", "</script><script>alert(1)</script>"),
            ("--mc-color-text", "url(https://example.com/x)"),
            ("--mc-color-text", "URL(x)"),
            ("--mc-color-text", "\\75rl(x)"),
            ("--mc-color-text", "a\u{2028}b"),
            ("--mc-color-text", "}"),
        ] {
            assert!(!is_valid_token(name, value), "{name}: {value}");
        }
        assert!(!is_valid_token(&format!("--mc-{}", "a".repeat(64)), "#fff"));
        assert!(!is_valid_token("--mc-x", &"a".repeat(257)));
    }

    #[test]
    fn invalid_tokens_are_left_out() {
        let theme = PageTheme::new(
            PageMode::Dark,
            PageScheme::Dark,
            false,
            [
                token("--mc-color-text", "#fff"),
                token("--mc-color-accent", "x;y"),
            ],
        );
        assert_eq!(theme.tokens.len(), 1);
        assert_eq!(theme.tokens["--mc-color-text"], "#fff");
    }

    #[test]
    fn the_data_block_is_json_that_cannot_close_itself() {
        // Valid tokens never carry these characters; the escaping still holds if one slipped by.
        let mut theme = PageTheme::new(PageMode::HighContrast, PageScheme::Light, true, []);
        theme.tokens.insert(
            "--mc-x".to_owned(),
            "</script><b>&\u{2028}\u{2029}".to_owned(),
        );
        let block = theme.data_block();
        let json = block
            .strip_prefix(r#"<script type="application/json">"#)
            .and_then(|rest| rest.strip_suffix("</script>"))
            .unwrap();
        for forbidden in ['<', '>', '&', '\u{2028}', '\u{2029}'] {
            assert!(!json.contains(forbidden), "{forbidden:?}");
        }
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(parsed["mode"], "high-contrast");
        assert_eq!(parsed["scheme"], "light");
        assert_eq!(parsed["reducedMotion"], true);
        assert_eq!(parsed["tokens"]["--mc-x"], "</script><b>&\u{2028}\u{2029}");
    }
}
