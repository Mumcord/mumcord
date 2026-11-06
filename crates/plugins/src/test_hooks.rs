//! Test hooks for the E2E suite, read from environment variables.
//!
//! They exist only in debug builds. In release builds every hook returns `None`, so a user who
//! sets one of the variables cannot weaken the origin checks or the certificate trust.

use std::net::IpAddr;
use std::path::PathBuf;

const CA_VAR: &str = "MUMCORD_TEST_CA";
const PUBLIC_ADDR_VAR: &str = "MUMCORD_TEST_PUBLIC_ADDR";
const RESOLVE_VAR: &str = "MUMCORD_TEST_RESOLVE";
const KEY_AGE_VAR: &str = "MUMCORD_TEST_KEY_AGE";
const PLUGIN_SAVE_VAR: &str = "MUMCORD_TEST_PLUGIN_SAVE";
const PLUGIN_PICK_VAR: &str = "MUMCORD_TEST_PLUGIN_PICK";

/// What a native dialog of a plugin host call answers in a test run instead of opening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogAnswer {
    /// The user cancelled.
    Cancel,
    /// The user chose this path.
    Path(PathBuf),
}

fn parse_dialog(value: String) -> DialogAnswer {
    if value == "cancel" {
        DialogAnswer::Cancel
    } else {
        DialogAnswer::Path(PathBuf::from(value))
    }
}

/// Reads a hook variable; always `None` in release builds.
fn var(name: &str) -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var(name).ok().filter(|v| !v.is_empty())
    } else {
        None
    }
}

/// Splits `<left>=<right>`; both sides must be non-empty.
fn split_pair(value: &str) -> Option<(&str, &str)> {
    let (left, right) = value.rsplit_once('=')?;
    (!left.is_empty() && !right.is_empty()).then_some((left, right))
}

fn parse_resolve(value: &str, name: &str) -> Option<IpAddr> {
    let (host, ip) = split_pair(value)?;
    host.eq_ignore_ascii_case(name).then(|| ip.parse().ok())?
}

fn parse_key_age(value: &str, key: &str) -> Option<u32> {
    let (k, days) = split_pair(value)?;
    (k == key).then(|| days.parse().ok())?
}

/// Path of the fixture CA that Rust-side TLS should additionally trust.
pub fn test_ca() -> Option<PathBuf> {
    var(CA_VAR).map(PathBuf::from)
}

/// Address of the fixture server that the resolved-address check treats as public.
pub fn public_addr() -> Option<IpAddr> {
    var(PUBLIC_ADDR_VAR)?.parse().ok()
}

/// Address that `net.http` resolves `name` to instead of asking DNS.
pub fn resolve(name: &str) -> Option<IpAddr> {
    parse_resolve(&var(RESOLVE_VAR)?, name)
}

/// Age in days that replaces the first-seen age of the registry key `key`.
pub fn key_age(key: &str) -> Option<u32> {
    parse_key_age(&var(KEY_AGE_VAR)?, key)
}

/// `files.save` (FR-9.103) answers with this instead of the save dialog: a path to write to,
/// or `cancel`.
pub fn plugin_save_dialog() -> Option<DialogAnswer> {
    var(PLUGIN_SAVE_VAR).map(parse_dialog)
}

/// `files.import-picture` (FR-9.105) answers with this instead of the picker: the path of the
/// picked file, or `cancel`.
pub fn plugin_pick_dialog() -> Option<DialogAnswer> {
    var(PLUGIN_PICK_VAR).map(parse_dialog)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_answers_are_cancel_or_a_path() {
        assert_eq!(parse_dialog("cancel".into()), DialogAnswer::Cancel);
        assert_eq!(
            parse_dialog("/tmp/a.png".into()),
            DialogAnswer::Path(PathBuf::from("/tmp/a.png"))
        );
    }

    #[test]
    fn resolve_matches_only_the_named_host() {
        assert_eq!(
            parse_resolve("plugins.test=127.0.0.1", "PLUGINS.test"),
            "127.0.0.1".parse().ok()
        );
        assert_eq!(parse_resolve("plugins.test=127.0.0.1", "other.test"), None);
        assert_eq!(parse_resolve("plugins.test=nonsense", "plugins.test"), None);
        assert_eq!(parse_resolve("=127.0.0.1", ""), None);
    }

    #[test]
    fn key_age_matches_only_the_named_key() {
        assert_eq!(parse_key_age("RWQabc=30", "RWQabc"), Some(30));
        assert_eq!(parse_key_age("RWQabc=30", "RWQxyz"), None);
        assert_eq!(parse_key_age("RWQabc=soon", "RWQabc"), None);
    }

    #[test]
    fn hooks_are_inert_without_variables() {
        assert_eq!(var("MUMCORD_TEST_NOT_SET_ANYWHERE"), None);
    }

    /// Every hook, `MUMCORD_TEST_CA` included, reads its variable through `var`: a release build
    /// ignores a variable that is set (this test asserts that when run with `--release`).
    #[test]
    fn hooks_read_variables_only_in_debug_builds() {
        const PROBE: &str = "MUMCORD_TEST_HOOK_GATE_PROBE";
        // SAFETY: no other test reads or writes this variable.
        unsafe { std::env::set_var(PROBE, "set") };
        assert_eq!(var(PROBE).is_some(), cfg!(debug_assertions));
        unsafe { std::env::remove_var(PROBE) };
    }
}
