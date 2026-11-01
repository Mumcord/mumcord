//! Version rules (#269): when a plugin may load on this host, and when an update is offered.
//!
//! The host API is the WIT package `mumcord:plugin@MAJOR.MINOR.PATCH` in `wit/`. A minor version
//! only adds (functions, events, components, permissions); a major version breaks. A plugin asks
//! for a caret range such as `^1.3`, which loads on API 1.3 or any newer 1.x.

use semver::{Version, VersionReq};

use crate::manifest::Manifest;

/// The host API this build implements. Must match the version in `wit/world.wit`.
pub const API_VERSION: Version = Version::new(1, 0, 0);

/// Why a plugin cannot run on this Mumcord.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Incompatibility {
    /// The plugin needs a newer API or app version. Shown as "Needs a newer Mumcord"; such
    /// plugins are not offered in the browse view.
    #[error("Needs a newer Mumcord")]
    NeedsNewerMumcord,
    /// The plugin targets an API major this host no longer supports.
    #[error("Made for an older Mumcord")]
    TooOld,
}

/// Checks a manifest against this host's API version and the running app version.
pub fn check_compatibility(
    manifest: &Manifest,
    app_version: &Version,
) -> Result<(), Incompatibility> {
    check_api(&manifest.api, &API_VERSION)?;
    match &manifest.mumcord {
        Some(range) if !matches_release(range, app_version) => Err(
            if lowest_bound(range).is_some_and(|low| low > *app_version) {
                Incompatibility::NeedsNewerMumcord
            } else {
                Incompatibility::TooOld
            },
        ),
        _ => Ok(()),
    }
}

/// `api` against the host API. Manifest validation already guarantees a single caret comparator.
pub fn check_api(api: &VersionReq, host: &Version) -> Result<(), Incompatibility> {
    if api.matches(host) {
        Ok(())
    } else if lowest_bound(api).is_some_and(|low| low > *host) {
        Err(Incompatibility::NeedsNewerMumcord)
    } else {
        Err(Incompatibility::TooOld)
    }
}

/// An update is offered only for a strictly higher version; going back needs an explicit
/// reinstall.
pub fn is_update(installed: &Version, candidate: &Version) -> bool {
    candidate > installed
}

/// Development builds of the app carry a pre-release tag (e.g. `0.7.0-dev`); a range like
/// `>=0.7.0` should still accept them, so ranges are checked against the release version.
fn matches_release(range: &VersionReq, app: &Version) -> bool {
    range.matches(&Version::new(app.major, app.minor, app.patch))
}

fn lowest_bound(range: &VersionReq) -> Option<Version> {
    use semver::Op;
    range
        .comparators
        .iter()
        .filter(|c| {
            matches!(
                c.op,
                Op::Caret | Op::Tilde | Op::Exact | Op::GreaterEq | Op::Greater | Op::Wildcard
            )
        })
        .map(|c| Version::new(c.major, c.minor.unwrap_or(0), c.patch.unwrap_or(0)))
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(s: &str) -> VersionReq {
        VersionReq::parse(s).expect("valid range")
    }

    fn v(s: &str) -> Version {
        Version::parse(s).expect("valid version")
    }

    #[test]
    fn caret_ranges_follow_the_minor_is_additive_rule() {
        assert_eq!(check_api(&req("^1.0"), &v("1.2.0")), Ok(()));
        assert_eq!(check_api(&req("^1.2"), &v("1.2.0")), Ok(()));
        assert_eq!(
            check_api(&req("^1.5"), &v("1.2.0")),
            Err(Incompatibility::NeedsNewerMumcord)
        );
        assert_eq!(
            check_api(&req("^2.0"), &v("1.9.0")),
            Err(Incompatibility::NeedsNewerMumcord)
        );
        assert_eq!(
            check_api(&req("^1.0"), &v("2.0.0")),
            Err(Incompatibility::TooOld)
        );
        assert_eq!(
            check_api(&req("^1.2.3"), &v("1.2.2")),
            Err(Incompatibility::NeedsNewerMumcord)
        );
    }

    #[test]
    fn wit_package_matches_the_api_version() {
        let world = include_str!("../wit/world.wit");
        assert!(world.contains(&format!("package mumcord:plugin@{API_VERSION};")));
    }

    #[test]
    fn app_version_range_accepts_dev_builds() {
        assert!(matches_release(&req(">=0.7.0"), &v("0.7.0-dev")));
        assert!(!matches_release(&req(">=0.7.0"), &v("0.6.9")));
    }

    #[test]
    fn updates_only_go_up() {
        assert!(is_update(&v("1.2.0"), &v("1.2.1")));
        assert!(is_update(&v("1.2.0-beta.1"), &v("1.2.0")));
        assert!(!is_update(&v("1.2.0"), &v("1.2.0")));
        assert!(!is_update(&v("1.2.0"), &v("1.1.9")));
    }
}
