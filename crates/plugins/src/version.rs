//! Version rules (#269, FR-9.5 … FR-9.13): when a plugin may load on this host, which API minor
//! it is linked against, and when an update is offered.
//!
//! The host API is the WIT package `mumcord:plugin@MAJOR.MINOR.0` in `wit/<version>/`. A minor
//! only adds (functions, events, components, permissions); a major breaks. A plugin asks for a
//! caret range such as `^1.1`; its lower bound is the minor it is built against, and the host
//! links it against exactly that minor (FR-9.8). Versions compare by major.minor only (FR-9.13).

use std::fmt;

use semver::{Comparator, Version, VersionReq};
use wasmtime::component::Component;

use crate::manifest::Manifest;

/// One minor of the plugin API, e.g. 1.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApiMinor {
    pub major: u64,
    pub minor: u64,
}

impl ApiMinor {
    pub const V1_0: Self = Self { major: 1, minor: 0 };
    pub const V1_1: Self = Self { major: 1, minor: 1 };

    /// The WIT package version, e.g. `1.1.0`; plugins see it as `start-info.api-version`.
    pub fn wit_version(self) -> Version {
        Version::new(self.major, self.minor, 0)
    }
}

impl std::str::FromStr for ApiMinor {
    type Err = ();

    /// `"1.1"`; the index and manifests write API minors as major.minor (FR-9.15).
    fn from_str(text: &str) -> Result<Self, ()> {
        let (major, minor) = text.split_once('.').ok_or(())?;
        Ok(Self {
            major: number(major).ok_or(())?,
            minor: number(minor).ok_or(())?,
        })
    }
}

impl fmt::Display for ApiMinor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Every API minor this build links, oldest first (FR-9.5). Later 1.x builds keep all of them.
pub const SUPPORTED: &[ApiMinor] = &[ApiMinor::V1_0, ApiMinor::V1_1];

/// API minors that still work but will go away with the next major (FR-9.11). None yet.
const DEPRECATED: &[ApiMinor] = &[];

/// The newest host API this build implements.
pub const API_VERSION: Version = Version::new(1, 1, 0);

/// Whether plugins built for `api` should be warned about in Developer mode (FR-9.11).
pub fn deprecated(api: ApiMinor) -> bool {
    DEPRECATED.contains(&api)
}

/// The API minor a manifest is built against: the lower bound of its `api` range (FR-9.6).
pub fn api_of(manifest: &Manifest) -> ApiMinor {
    let (major, minor) = manifest.api_minor();
    ApiMinor { major, minor }
}

/// The API minor a component is built against: the newest `mumcord:plugin` package version
/// among its imports. `None` when it imports no versioned `mumcord:plugin` interface.
pub fn imports_api(component: &Component) -> Option<ApiMinor> {
    component
        .component_type()
        .imports(component.engine())
        .filter_map(|(name, _)| name.strip_prefix("mumcord:plugin/"))
        .filter_map(|name| name.split_once('@'))
        .filter_map(|(_, version)| Version::parse(version).ok())
        .map(|v| ApiMinor {
            major: v.major,
            minor: v.minor,
        })
        .max()
}

/// A Mumcord release as major.minor (FR-9.13): `min_mumcord` `"0.7.3"` is release 0.7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Release {
    pub major: u64,
    pub minor: u64,
}

impl Release {
    /// Reads `"0.7"` or `"0.7.3"`; the patch level is checked but ignored.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let major = number(parts.next()?)?;
        let minor = number(parts.next()?)?;
        match (parts.next(), parts.next()) {
            (None, _) => {}
            (Some(patch), None) => {
                number(patch)?;
            }
            (Some(_), Some(_)) => return None,
        }
        Some(Self { major, minor })
    }

    /// The release of a running app; pre-release tags and patch levels do not count.
    pub fn of(app: &Version) -> Self {
        Self {
            major: app.major,
            minor: app.minor,
        }
    }
}

impl fmt::Display for Release {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.0", self.major, self.minor)
    }
}

/// A plain decimal number without sign or leading `+`.
fn number(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// The first Mumcord release that links an API minor (FR-9.15): 1.0 → 0.6, 1.1 → 0.7.
/// `None` for an API this build does not know.
pub fn first_release(api: ApiMinor) -> Option<Release> {
    match api {
        ApiMinor::V1_0 => Some(Release { major: 0, minor: 6 }),
        ApiMinor::V1_1 => Some(Release { major: 0, minor: 7 }),
        _ => None,
    }
}

/// FR-9.16 / FR-9.17: shown when no version of a plugin runs on this Mumcord; `release` is the
/// lowest release that would run one.
pub fn needs_mumcord_text(release: Release) -> String {
    format!("Needs Mumcord {release} or newer")
}

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

/// Checks a manifest against this host's API minors and the running app version.
pub fn check_compatibility(
    manifest: &Manifest,
    app_version: &Version,
) -> Result<(), Incompatibility> {
    check_api(api_of(manifest))?;
    match &manifest.mumcord {
        Some(range) if !matches_release(range, app_version) => Err(
            if lowest_bound(range).is_some_and(|low| low > release(app_version)) {
                Incompatibility::NeedsNewerMumcord
            } else {
                Incompatibility::TooOld
            },
        ),
        _ => Ok(()),
    }
}

/// FR-9.17: the user-facing reason a package cannot be installed on this Mumcord, `None` if it
/// can run. Names the lowest release that would run it when that is known.
pub fn refusal(manifest: &Manifest, app_version: &Version) -> Option<String> {
    match check_compatibility(manifest, app_version) {
        Ok(()) => None,
        Err(Incompatibility::TooOld) => Some(Incompatibility::TooOld.to_string()),
        Err(Incompatibility::NeedsNewerMumcord) => {
            let from_api = first_release(api_of(manifest));
            let from_range = manifest
                .mumcord
                .as_ref()
                .and_then(lowest_bound)
                .map(|low| Release {
                    major: low.major,
                    minor: low.minor,
                });
            Some(match from_api {
                Some(api) => needs_mumcord_text(from_range.map_or(api, |range| api.max(range))),
                None => Incompatibility::NeedsNewerMumcord.to_string(),
            })
        }
    }
}

/// Whether this host links the API minor a plugin is built against.
pub fn check_api(api: ApiMinor) -> Result<(), Incompatibility> {
    let newest = SUPPORTED[SUPPORTED.len() - 1];
    if SUPPORTED.contains(&api) {
        Ok(())
    } else if api > newest {
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

/// The app version as major.minor (FR-9.13): pre-release tags and patch levels do not count, so
/// `0.7.0-dev` satisfies `>=0.7.0` and `min_mumcord` `0.7.3` is satisfied by 0.7.0.
fn release(app: &Version) -> Version {
    Version::new(app.major, app.minor, 0)
}

/// The range with its patch levels dropped (FR-9.13).
fn major_minor(range: &VersionReq) -> VersionReq {
    VersionReq {
        comparators: range
            .comparators
            .iter()
            .map(|c| Comparator {
                patch: None,
                pre: semver::Prerelease::EMPTY,
                ..c.clone()
            })
            .collect(),
    }
}

fn matches_release(range: &VersionReq, app: &Version) -> bool {
    major_minor(range).matches(&release(app))
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
        .map(|c| Version::new(c.major, c.minor.unwrap_or(0), 0))
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

    fn api(major: u64, minor: u64) -> ApiMinor {
        ApiMinor { major, minor }
    }

    #[test]
    fn links_every_supported_minor_and_refuses_others() {
        assert_eq!(check_api(ApiMinor::V1_0), Ok(()));
        assert_eq!(check_api(ApiMinor::V1_1), Ok(()));
        assert_eq!(
            check_api(api(1, 2)),
            Err(Incompatibility::NeedsNewerMumcord)
        );
        assert_eq!(
            check_api(api(2, 0)),
            Err(Incompatibility::NeedsNewerMumcord)
        );
        assert_eq!(check_api(api(0, 9)), Err(Incompatibility::TooOld));
        assert!(!deprecated(ApiMinor::V1_0));
    }

    #[test]
    fn wit_packages_match_the_supported_minors() {
        let worlds = [
            include_str!("../wit/1.0.0/world.wit"),
            include_str!("../wit/1.1.0/world.wit"),
        ];
        for (api, world) in SUPPORTED.iter().zip(worlds) {
            assert!(world.contains(&format!("package mumcord:plugin@{};", api.wit_version())));
        }
        assert_eq!(SUPPORTED.last().map(|a| a.wit_version()), Some(API_VERSION));
    }

    fn manifest_with(api: &str, mumcord: Option<&str>) -> Manifest {
        let mumcord = mumcord.map_or(String::new(), |m| format!(r#""mumcord":"{m}","#));
        let json = format!(
            r#"{{"manifest":1,"id":"test-plugin","name":"T","version":"1.0.0","api":"{api}",{mumcord}
            "authors":["A"],"description":"d","categories":["tools"],"license":"MIT",
            "permissions":{{"required":["ui.panel"]}}}}"#
        );
        Manifest::parse(json.as_bytes()).expect("valid manifest")
    }

    #[test]
    fn refusal_names_the_release_that_would_run_the_package() {
        let app = v("0.7.0");
        assert_eq!(refusal(&manifest_with("^1.1", None), &app), None);
        assert_eq!(
            refusal(&manifest_with("^1.0", Some(">=0.8.0")), &app).as_deref(),
            Some("Needs Mumcord 0.8.0 or newer")
        );
        assert_eq!(
            refusal(&manifest_with("^1.2", None), &app).as_deref(),
            Some("Needs a newer Mumcord")
        );
    }

    #[test]
    fn app_version_range_accepts_dev_builds() {
        assert!(matches_release(&req(">=0.7.0"), &v("0.7.0-dev")));
        assert!(!matches_release(&req(">=0.7.0"), &v("0.6.9")));
    }

    #[test]
    fn app_version_compares_major_minor_only() {
        assert!(matches_release(&req(">=0.7.3"), &v("0.7.0")));
        assert!(matches_release(&req("^0.7.3"), &v("0.7.1")));
        assert!(!matches_release(&req(">=0.8.1"), &v("0.7.9")));
        assert_eq!(lowest_bound(&req(">=0.7.3")), Some(v("0.7.0")));
    }

    #[test]
    fn api_minors_and_releases_parse_major_minor() {
        assert_eq!("1.1".parse(), Ok(ApiMinor::V1_1));
        assert_eq!("1.0".parse(), Ok(ApiMinor::V1_0));
        for bad in ["1", "1.", "1.x", "+1.0", "1.1.0", ""] {
            assert!(bad.parse::<ApiMinor>().is_err(), "{bad}");
        }
        let r = |major, minor| Some(Release { major, minor });
        assert_eq!(Release::parse("0.7"), r(0, 7));
        assert_eq!(Release::parse("0.7.3"), r(0, 7));
        for bad in ["0", "0.7.3.1", "0.7-dev", "v0.7", "0..7"] {
            assert_eq!(Release::parse(bad), None, "{bad}");
        }
        // FR-9.13: 0.7.0 satisfies min_mumcord 0.7.3.
        assert!(Release::of(&v("0.7.0-dev")) >= Release::parse("0.7.3").expect("test step"));
        assert_eq!(
            needs_mumcord_text(Release { major: 0, minor: 7 }),
            "Needs Mumcord 0.7.0 or newer"
        );
        assert_eq!(first_release(ApiMinor::V1_1), r(0, 7));
        assert_eq!(first_release(api(2, 0)), None);
    }

    #[test]
    fn updates_only_go_up() {
        assert!(is_update(&v("1.2.0"), &v("1.2.1")));
        assert!(is_update(&v("1.2.0-beta.1"), &v("1.2.0")));
        assert!(!is_update(&v("1.2.0"), &v("1.2.0")));
        assert!(!is_update(&v("1.2.0"), &v("1.1.9")));
    }
}
