//! Test-server settings: process environment first, then `testenv/.env` (git-ignored).

use std::path::PathBuf;

/// Default host of the local test servers.
pub const HOST: &str = "127.0.0.1";
/// Host ports of the `testenv/` services.
pub const PORT_V15: u16 = 64738;
pub const PORT_V14: u16 = 64739;
pub const PORT_V13: u16 = 64740;
pub const PORT_LIMIT: u16 = 64741;

/// Reads `key` from the environment, falling back to `testenv/.env`. Empty values count as
/// unset.
pub fn testenv_var(key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(key)
        && !value.is_empty()
    {
        return Some(value);
    }
    let text = std::fs::read_to_string(testenv_dir().join(".env")).ok()?;
    parse_env(&text, key)
}

/// `MUMCORD_TESTENV_DIR`, else the repository's `testenv/` (this crate lives in `crates/bot`).
fn testenv_dir() -> PathBuf {
    std::env::var_os("MUMCORD_TESTENV_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testenv"))
}

fn parse_env(text: &str, key: &str) -> Option<String> {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim().trim_matches('"').to_owned())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::parse_env;

    #[test]
    fn parses_env_lines() {
        let text = "# comment\nSUPERUSER_PASSWORD=secret\nSERVER_PASSWORD=\nQUOTED=\"x y\"\n";
        assert_eq!(
            parse_env(text, "SUPERUSER_PASSWORD").as_deref(),
            Some("secret")
        );
        assert_eq!(parse_env(text, "SERVER_PASSWORD"), None);
        assert_eq!(parse_env(text, "QUOTED").as_deref(), Some("x y"));
        assert_eq!(parse_env(text, "MISSING"), None);
    }
}
