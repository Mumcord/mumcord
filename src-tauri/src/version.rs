//! The version this build shows and reports, set at build time (see `build/version.rs`).

/// Shown in About, written to diagnostics and logs, sent to servers and in the update check's
/// User-Agent: `X.Y.Z` for a release, `dev~…` for any other build.
pub const DISPLAY: &str = env!("MUMCORD_DISPLAY_VERSION");

/// The numeric package version (the last release for dev builds), for places where other
/// clients parse the version, such as theme share codes.
pub const PACKAGE: &str = env!("CARGO_PKG_VERSION");

/// The release name servers show for this client, e.g. `Mumcord 0.8.0`. The protocol crate would
/// otherwise report its own name and version, so the app sets this before the first connection.
pub fn client_release() -> String {
    format!("Mumcord {DISPLAY}")
}

/// Release builds carry a plain version; only they compare themselves against GitHub releases.
pub fn is_release() -> bool {
    is_release_version(DISPLAY)
}

fn is_release_version(version: &str) -> bool {
    !version.starts_with("dev~")
}

#[cfg(test)]
#[path = "../build/version.rs"]
mod build_logic;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_see_mumcord_and_the_build_version() {
        assert_eq!(
            client_release(),
            format!("Mumcord {}", env!("MUMCORD_DISPLAY_VERSION"))
        );
    }

    #[test]
    fn only_plain_versions_are_releases() {
        assert!(is_release_version("0.4.3"));
        assert!(is_release_version("0.5.0-rc.1"));
        assert!(!is_release_version("dev~a1b2c3d"));
        assert!(!is_release_version("dev~20261028-dirty"));
    }
}
