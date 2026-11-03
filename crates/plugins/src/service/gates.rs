//! The safety gates in front of installs and updates (FR-9.78, FR-9.80 – FR-9.83).
//!
//! A **hard stop** (bad signature, hash or manifest mismatch, revoked, an origin breaking the
//! rules) refuses the package before it is scored, so trust points can never buy it back. A
//! package that passes gets an [`InstallGate`]: one confirmation box that merges every warning,
//! and the "Don't allow high-risk plugins" safety setting. Updates install by themselves only
//! when nothing about their risk grows ([`auto_update_hold`]); otherwise they wait in the
//! Updates tab with an [`UpdateHold`] and the [`UpdateReview`] the user decides on.
//!
//! Everything here is plain data for the UI; the service decides when to compute it.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::{InstallScore, ServiceError};
use crate::manifest::{ManifestError, Source};
use crate::origins;
use crate::package::PackageError;
use crate::score::{Band, Effect, FactorKind, HardStop, Score, Scored};
use crate::verify::VerifyFailure;

/// Permissions that make an unofficial plugin bring its own code and talk to websites, which
/// the install prompt has the user confirm (FR-9.80).
pub const OWN_CODE_AND_WEBSITES: [&str; 2] = ["ui.webview", "net.origins"];

/// An update may raise the score by less than this many points and still install by itself
/// (FR-9.82).
pub const MAX_AUTO_SCORE_RISE: u8 = 10;

/// A package that cannot be installed: no score is shown, only the reason (FR-9.78).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    pub stop: HardStop,
    /// User-facing, e.g. "Cannot be installed: it wants to use a website that is not
    /// encrypted."
    pub reason: String,
}

/// The hard stop behind a failed check, if the failure is one (FR-9.78). Other failures (a
/// broken download, a full disk) are ordinary errors.
pub fn refusal_of(error: &ServiceError) -> Option<Refusal> {
    let refusal = |stop, reason: String| Some(Refusal { stop, reason });
    match error {
        ServiceError::Verify(VerifyFailure::Signature) => {
            refusal(HardStop::InvalidSignature, error.to_string())
        }
        ServiceError::Verify(VerifyFailure::Mismatch) => {
            refusal(HardStop::HashMismatch, error.to_string())
        }
        ServiceError::Verify(VerifyFailure::Permissions) => {
            refusal(HardStop::ManifestMismatch, error.to_string())
        }
        ServiceError::Revoked(_) => refusal(HardStop::Revoked, error.to_string()),
        ServiceError::Verify(VerifyFailure::Package(PackageError::Manifest(manifest)))
        | ServiceError::Package(PackageError::Manifest(manifest))
        | ServiceError::Manifest(manifest) => match manifest {
            ManifestError::Origin(stop) => refusal(
                HardStop::UnsafeOrigin,
                format!("Cannot be installed: {stop}."),
            ),
            _ => None,
        },
        _ => None,
    }
}

/// What the person must confirm before Install is enabled (FR-9.80).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfirmKind {
    /// "I understand the risk and I trust the author of <plugin>" / "Install anyway".
    HighRisk,
    /// "I understand" / "Install".
    Understand,
}

/// Why the confirmation box shows; several merge into one box with one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GateReason {
    HighRisk,
    /// An unofficial plugin asks for `ui.webview` or `net.origins`.
    OwnCodeAndWebsites,
    /// The first install from this third-party repository (FR-6.61).
    RepositoryWarning,
}

/// The gate of the install prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallGate {
    /// `None`: Install is enabled right away.
    pub confirm: Option<ConfirmKind>,
    /// "Don't allow high-risk plugins" is on and the plugin is High: Install stays disabled
    /// with "Blocked by your safety setting" (FR-9.81).
    pub blocked_by_safety: bool,
    pub reasons: Vec<GateReason>,
}

/// What the install gate looks at.
#[derive(Debug, Clone, Copy)]
pub struct GateInput<'a> {
    pub source: Source,
    pub scored: &'a Scored,
    /// The plugin asks (required or optional) for one of [`OWN_CODE_AND_WEBSITES`].
    pub own_code_and_websites: bool,
    /// No plugin from this repository is installed yet.
    pub first_from_repository: bool,
    /// The "Don't allow high-risk plugins" setting.
    pub block_high_risk: bool,
}

pub fn install_gate(input: GateInput<'_>) -> InstallGate {
    // Official and developer plugins are never gated (FR-9.81); only repositories are.
    let unofficial = matches!(input.source, Source::Repo { .. });
    let high = unofficial && band(input.scored) == Some(Band::High);
    let mut reasons = Vec::new();
    if high {
        reasons.push(GateReason::HighRisk);
    }
    if unofficial && input.own_code_and_websites {
        reasons.push(GateReason::OwnCodeAndWebsites);
    }
    if unofficial && input.first_from_repository {
        reasons.push(GateReason::RepositoryWarning);
    }
    let confirm = match (high, reasons.is_empty()) {
        (true, _) => Some(ConfirmKind::HighRisk),
        (false, false) => Some(ConfirmKind::Understand),
        (false, true) => None,
    };
    InstallGate {
        confirm,
        blocked_by_safety: high && input.block_high_risk,
        reasons,
    }
}

/// What the install prompt gets once the package is downloaded and checked.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InstallCheck {
    /// "Can't be installed": no score, no Install button.
    Refused { refusal: Refusal },
    Ready {
        score: InstallScore,
        gate: InstallGate,
    },
}

/// Why an update waits for the user although it may otherwise install by itself (FR-9.81,
/// FR-9.82). New permissions and a new author key keep using [`super::Approval`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpdateHold {
    /// "This update wants to talk to <count> new websites. <plugin> stays on <version> until you
    /// decide."
    NewWebsites { count: u32 },
    /// "Not installed automatically: the risk went up from <from> to <to>. Review it first."
    RiskUp { from: Band, to: Band },
    /// The score rose by 10 points or more within the same band.
    ScoreUp { from: u8, to: u8 },
    /// The update would make the plugin High while "Don't allow high-risk plugins" is on.
    BlockedBySafety,
    /// No score to compare (the installed version is no longer listed): reviewed by the user.
    ScoreUnknown,
}

/// Whether an update may install by itself, judged on its new websites and its score against
/// the installed version's. `None`: it may.
pub fn auto_update_hold(
    new_origins: usize,
    old: Option<&Scored>,
    new: Option<&Scored>,
    block_high_risk: bool,
) -> Option<UpdateHold> {
    if block_high_risk && becomes_high(old, new) {
        return Some(UpdateHold::BlockedBySafety);
    }
    if new_origins > 0 {
        return Some(UpdateHold::NewWebsites {
            count: u32::try_from(new_origins).unwrap_or(u32::MAX),
        });
    }
    match (old, new) {
        // Official plugins are reviewed instead of scored.
        (_, Some(Scored::Official)) => None,
        (Some(Scored::Scored(old)), Some(Scored::Scored(new))) => {
            if rank(new.band) > rank(old.band) {
                Some(UpdateHold::RiskUp {
                    from: old.band,
                    to: new.band,
                })
            } else if new.value >= old.value.saturating_add(MAX_AUTO_SCORE_RISE) {
                Some(UpdateHold::ScoreUp {
                    from: old.value,
                    to: new.value,
                })
            } else {
                None
            }
        }
        _ => Some(UpdateHold::ScoreUnknown),
    }
}

/// The update turns a plugin that is not High into a High one (FR-9.81, FR-9.83). An unknown
/// old score counts as not High, so the user confirms rather than misses the change.
pub fn becomes_high(old: Option<&Scored>, new: Option<&Scored>) -> bool {
    new.and_then(band) == Some(Band::High) && old.and_then(band) != Some(Band::High)
}

fn band(scored: &Scored) -> Option<Band> {
    match scored {
        Scored::Scored(Score { band, .. }) => Some(*band),
        Scored::Official | Scored::Refused(_) => None,
    }
}

fn rank(band: Band) -> u8 {
    match band {
        Band::Low => 0,
        Band::Medium => 1,
        Band::High => 2,
    }
}

/// One website of an update, for the red warning (FR-9.83).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewOrigin {
    /// ASCII form; plain, untrusted text.
    pub origin: String,
    /// Marked "New".
    pub new: bool,
    /// The purpose key the manifest declares (e.g. `origins.api`), plain untrusted text. Only
    /// known for a website the installed version already declares: the registry index lists
    /// origins without purposes and the new package is not downloaded before the user decides,
    /// so a new website has `None`.
    pub purpose: Option<String>,
}

/// The points one new permission adds to the score ("+4 points" next to it in the review).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionEffect {
    pub permission: String,
    pub points: i32,
}

/// The "recent change" part of the new version's score: new permissions within the look-back
/// window add points on top, outside the trust percentage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentChange {
    /// Points added (H, capped).
    pub points: u32,
    /// The trust percentage applied to the capability points (F, e.g. 90).
    pub trust_percent: u32,
    /// The permissions that count as a recent change.
    pub permissions: Vec<String>,
}

/// The score before and after an update ("before/after" in the Updates tab).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskChange {
    pub from_value: u8,
    pub from_band: Band,
    pub to_value: u8,
    pub to_band: Band,
}

/// What the Updates tab shows to decide on an update (FR-9.83).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReview {
    /// Every website of the new version; empty when it declares none.
    pub origins: Vec<ReviewOrigin>,
    /// "One of them is not on the plugin's own domain."
    pub other_domain: bool,
    /// `None` for official plugins or when a score is unknown.
    pub risk: Option<RiskChange>,
    /// The points each permission that is new in this version adds; empty when the score is
    /// unknown or no permission is new.
    pub permission_effects: Vec<PermissionEffect>,
    /// New permissions within the recent versions; `None` when there are none.
    pub recent_change: Option<RecentChange>,
    /// The update makes the plugin High: the tick "I understand the risk went up and I still
    /// trust the author" and "Update anyway" are needed.
    pub needs_high_risk_confirm: bool,
    /// The user already gave that tick for this version.
    pub high_risk_confirmed: bool,
}

/// Builds the review of an update from the installed and the new version's websites and
/// scores.
pub fn update_review(
    old_origins: &[String],
    new_origins: &[String],
    old_purposes: &BTreeMap<String, String>,
    old: Option<&Scored>,
    new: Option<&Scored>,
    high_risk_confirmed: bool,
) -> UpdateReview {
    let origins: Vec<ReviewOrigin> = new_origins
        .iter()
        .map(|origin| ReviewOrigin {
            origin: origin.clone(),
            new: !old_origins.contains(origin),
            purpose: old_purposes.get(origin).cloned(),
        })
        .collect();
    let own: BTreeSet<Option<String>> = old_origins.iter().map(|o| domain_of(o)).collect();
    // Without any website before, there is no "own domain" to compare with.
    let other_domain = !own.is_empty()
        && origins
            .iter()
            .filter(|o| o.new)
            .any(|o| match domain_of(&o.origin) {
                Some(domain) => !own.contains(&Some(domain)),
                None => true,
            });
    let risk = match (old, new) {
        (Some(Scored::Scored(old)), Some(Scored::Scored(new))) => Some(RiskChange {
            from_value: old.value,
            from_band: old.band,
            to_value: new.value,
            to_band: new.band,
        }),
        _ => None,
    };
    let needs_high_risk_confirm = becomes_high(old, new);
    UpdateReview {
        origins,
        other_domain,
        risk,
        permission_effects: permission_effects(old, new),
        recent_change: recent_change(new),
        needs_high_risk_confirm,
        high_risk_confirmed: needs_high_risk_confirm && high_risk_confirmed,
    }
}

fn is_permission_factor(kind: FactorKind) -> bool {
    matches!(
        kind,
        FactorKind::HighPermission
            | FactorKind::WebSurface
            | FactorKind::Relay
            | FactorKind::MediumPermission
            | FactorKind::LowPermissions
    )
}

/// The capability points of every permission the new score has and the old one did not. With an
/// unknown old score nothing can be called new.
fn permission_effects(old: Option<&Scored>, new: Option<&Scored>) -> Vec<PermissionEffect> {
    let (Some(Scored::Scored(old)), Some(Scored::Scored(new))) = (old, new) else {
        return Vec::new();
    };
    let had: BTreeSet<&str> = old
        .factors
        .iter()
        .filter(|f| is_permission_factor(f.kind))
        .map(|f| f.detail_key.as_str())
        .collect();
    new.factors
        .iter()
        .filter(|f| is_permission_factor(f.kind) && !had.contains(f.detail_key.as_str()))
        .filter_map(|f| match f.effect {
            Effect::Points(points) => Some(PermissionEffect {
                permission: f.detail_key.clone(),
                points,
            }),
            Effect::Percent(_) => None,
        })
        .collect()
}

fn recent_change(new: Option<&Scored>) -> Option<RecentChange> {
    let Some(Scored::Scored(new)) = new else {
        return None;
    };
    if new.h == 0 {
        return None;
    }
    Some(RecentChange {
        points: new.h,
        trust_percent: (new.f * 100.0).round() as u32,
        permissions: new
            .factors
            .iter()
            .filter(|f| f.kind == FactorKind::RecentChange)
            .map(|f| f.detail_key.clone())
            .collect(),
    })
}

/// The registrable domain of an origin (`https://api.example.co.uk:8443` → `example.co.uk`),
/// via the public suffix list. `None` when the origin or its host cannot be read.
fn domain_of(origin: &str) -> Option<String> {
    let host = origins::validate(origin, false).ok()?.host;
    psl::domain_str(&host).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::origins::HardStop as OriginStop;
    use crate::score::band_of;

    const REPO: Source = Source::Repo { repository_id: 1 };

    fn scored(value: u8) -> Scored {
        Scored::Scored(Score {
            value,
            band: band_of(value),
            e: 0,
            f: 1.0,
            h: 0,
            factors: Vec::new(),
            version: 1,
        })
    }

    fn with_band(value: u8, band: Band) -> Scored {
        match scored(value) {
            Scored::Scored(score) => Scored::Scored(Score { band, ..score }),
            other => other,
        }
    }

    fn input(scored: &Scored) -> GateInput<'_> {
        GateInput {
            source: REPO,
            scored,
            own_code_and_websites: false,
            first_from_repository: false,
            block_high_risk: false,
        }
    }

    // --- Hard stops ---

    #[test]
    fn every_hard_stop_is_refused_with_its_reason() {
        let cases = [
            (
                ServiceError::Verify(VerifyFailure::Signature),
                HardStop::InvalidSignature,
            ),
            (
                ServiceError::Verify(VerifyFailure::Mismatch),
                HardStop::HashMismatch,
            ),
            (
                ServiceError::Verify(VerifyFailure::Permissions),
                HardStop::ManifestMismatch,
            ),
            (
                ServiceError::Revoked("Leaks data".into()),
                HardStop::Revoked,
            ),
        ];
        for (error, stop) in cases {
            let refusal = refusal_of(&error).expect("hard stop");
            assert_eq!(refusal.stop, stop);
            assert_eq!(refusal.reason, error.to_string());
        }
    }

    #[test]
    fn an_unsafe_origin_is_refused_with_the_origin_reason() {
        let error = ServiceError::Verify(VerifyFailure::Package(PackageError::Manifest(
            ManifestError::Origin(OriginStop::NotEncrypted),
        )));
        assert_eq!(
            refusal_of(&error),
            Some(Refusal {
                stop: HardStop::UnsafeOrigin,
                reason: "Cannot be installed: it wants to use a website that is not encrypted."
                    .into(),
            })
        );
        for stop in [
            OriginStop::UserInfo,
            OriginStop::PathQueryFragment,
            OriginStop::Wildcard,
            OriginStop::IpAddress,
            OriginStop::LocalName,
            OriginStop::Malformed,
            OriginStop::TooMany,
        ] {
            let error = ServiceError::Manifest(ManifestError::Origin(stop));
            assert_eq!(
                refusal_of(&error).map(|r| r.stop),
                Some(HardStop::UnsafeOrigin)
            );
        }
    }

    #[test]
    fn ordinary_failures_are_not_hard_stops() {
        assert_eq!(refusal_of(&ServiceError::AlreadyInstalled), None);
        assert_eq!(refusal_of(&ServiceError::Incompatible("x".into())), None);
        assert_eq!(
            refusal_of(&ServiceError::Verify(VerifyFailure::Rollback)),
            None
        );
    }

    // --- Install gate ---

    #[test]
    fn a_low_plugin_without_warnings_needs_no_tick() {
        let gate = install_gate(input(&scored(10)));
        assert_eq!(gate.confirm, None);
        assert!(!gate.blocked_by_safety);
        assert!(gate.reasons.is_empty());
    }

    #[test]
    fn a_high_plugin_needs_the_high_risk_tick() {
        let high = with_band(80, Band::High);
        let gate = install_gate(input(&high));
        assert_eq!(gate.confirm, Some(ConfirmKind::HighRisk));
        assert_eq!(gate.reasons, vec![GateReason::HighRisk]);
    }

    #[test]
    fn own_code_and_the_repository_warning_need_i_understand() {
        let low = scored(10);
        let gate = install_gate(GateInput {
            own_code_and_websites: true,
            ..input(&low)
        });
        assert_eq!(gate.confirm, Some(ConfirmKind::Understand));
        assert_eq!(gate.reasons, vec![GateReason::OwnCodeAndWebsites]);

        let gate = install_gate(GateInput {
            first_from_repository: true,
            ..input(&low)
        });
        assert_eq!(gate.confirm, Some(ConfirmKind::Understand));
        assert_eq!(gate.reasons, vec![GateReason::RepositoryWarning]);
    }

    #[test]
    fn all_warnings_merge_into_one_high_risk_box() {
        let high = with_band(80, Band::High);
        let gate = install_gate(GateInput {
            own_code_and_websites: true,
            first_from_repository: true,
            ..input(&high)
        });
        assert_eq!(gate.confirm, Some(ConfirmKind::HighRisk));
        assert_eq!(
            gate.reasons,
            vec![
                GateReason::HighRisk,
                GateReason::OwnCodeAndWebsites,
                GateReason::RepositoryWarning
            ]
        );
    }

    #[test]
    fn the_safety_setting_blocks_only_high_plugins() {
        let high = with_band(80, Band::High);
        let gate = install_gate(GateInput {
            block_high_risk: true,
            ..input(&high)
        });
        assert!(gate.blocked_by_safety);

        let medium = with_band(40, Band::Medium);
        let gate = install_gate(GateInput {
            block_high_risk: true,
            ..input(&medium)
        });
        assert!(!gate.blocked_by_safety);
    }

    #[test]
    fn official_and_developer_plugins_are_never_gated() {
        let high = with_band(80, Band::High);
        for (source, scored) in [(Source::Official, &Scored::Official), (Source::Dev, &high)] {
            let gate = install_gate(GateInput {
                source,
                scored,
                own_code_and_websites: true,
                first_from_repository: true,
                block_high_risk: true,
            });
            assert_eq!(gate.confirm, None);
            assert!(!gate.blocked_by_safety);
            assert!(gate.reasons.is_empty());
        }
    }

    // --- Automatic updates ---

    #[test]
    fn a_same_risk_update_installs_by_itself() {
        let (old, new) = (with_band(20, Band::Low), with_band(29, Band::Low));
        assert_eq!(auto_update_hold(0, Some(&old), Some(&new), false), None);
        let lower = with_band(5, Band::Low);
        assert_eq!(auto_update_hold(0, Some(&old), Some(&lower), true), None);
        assert_eq!(
            auto_update_hold(0, Some(&Scored::Official), Some(&Scored::Official), true),
            None
        );
    }

    #[test]
    fn new_websites_hold_the_update() {
        let score = with_band(20, Band::Low);
        assert_eq!(
            auto_update_hold(2, Some(&score), Some(&score), false),
            Some(UpdateHold::NewWebsites { count: 2 })
        );
    }

    #[test]
    fn a_rise_of_ten_points_holds_the_update() {
        let (old, new) = (with_band(20, Band::Low), with_band(30, Band::Low));
        assert_eq!(
            auto_update_hold(0, Some(&old), Some(&new), false),
            Some(UpdateHold::ScoreUp { from: 20, to: 30 })
        );
    }

    #[test]
    fn a_worse_band_holds_the_update() {
        let (old, new) = (with_band(39, Band::Medium), with_band(41, Band::High));
        assert_eq!(
            auto_update_hold(0, Some(&old), Some(&new), false),
            Some(UpdateHold::RiskUp {
                from: Band::Medium,
                to: Band::High
            })
        );
    }

    #[test]
    fn the_safety_setting_holds_an_update_that_makes_the_plugin_high() {
        let (old, new) = (with_band(39, Band::Medium), with_band(41, Band::High));
        assert_eq!(
            auto_update_hold(0, Some(&old), Some(&new), true),
            Some(UpdateHold::BlockedBySafety)
        );
        // Already High before: the setting never touches installed plugins.
        let (old, new) = (with_band(60, Band::High), with_band(61, Band::High));
        assert_eq!(auto_update_hold(0, Some(&old), Some(&new), true), None);
    }

    #[test]
    fn an_unknown_score_holds_the_update() {
        let new = scored(10);
        assert_eq!(
            auto_update_hold(0, None, Some(&new), false),
            Some(UpdateHold::ScoreUnknown)
        );
    }

    // --- Update review ---

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn new_websites_are_marked_and_another_domain_is_noticed() {
        let old = strings(&["https://api.example.com"]);
        let same = strings(&["https://api.example.com", "wss://live.example.com:8443"]);
        let review = update_review(&old, &same, &BTreeMap::new(), None, None, false);
        assert_eq!(
            review.origins,
            vec![
                ReviewOrigin {
                    origin: "https://api.example.com".into(),
                    new: false,
                    purpose: None
                },
                ReviewOrigin {
                    origin: "wss://live.example.com:8443".into(),
                    new: true,
                    purpose: None
                },
            ]
        );
        assert!(!review.other_domain);

        let other = strings(&["https://api.example.com", "https://cdn.other.co.uk"]);
        assert!(update_review(&old, &other, &BTreeMap::new(), None, None, false).other_domain);
    }

    #[test]
    fn the_registrable_domain_respects_public_suffixes() {
        let old = strings(&["https://a.example.co.uk"]);
        let sibling = strings(&["https://b.example.co.uk"]);
        assert!(!update_review(&old, &sibling, &BTreeMap::new(), None, None, false).other_domain);
        let neighbour = strings(&["https://b.another.co.uk"]);
        assert!(update_review(&old, &neighbour, &BTreeMap::new(), None, None, false).other_domain);
    }

    #[test]
    fn a_plugin_without_websites_has_no_own_domain() {
        let new = strings(&["https://api.example.com"]);
        let review = update_review(&[], &new, &BTreeMap::new(), None, None, false);
        assert!(review.origins[0].new);
        assert!(!review.other_domain);
    }

    #[test]
    fn an_update_to_high_needs_and_records_the_tick() {
        let (old, new) = (with_band(39, Band::Medium), with_band(41, Band::High));
        let review = update_review(&[], &[], &BTreeMap::new(), Some(&old), Some(&new), false);
        assert!(review.needs_high_risk_confirm);
        assert!(!review.high_risk_confirmed);
        assert_eq!(
            review.risk,
            Some(RiskChange {
                from_value: 39,
                from_band: Band::Medium,
                to_value: 41,
                to_band: Band::High,
            })
        );
        assert!(
            update_review(&[], &[], &BTreeMap::new(), Some(&old), Some(&new), true)
                .high_risk_confirmed
        );

        let review = update_review(&[], &[], &BTreeMap::new(), Some(&new), Some(&new), true);
        assert!(!review.needs_high_risk_confirm);
        assert!(!review.high_risk_confirmed);
    }

    fn factor(kind: FactorKind, points: i32, key: &str) -> crate::score::Factor {
        crate::score::Factor {
            kind,
            effect: Effect::Points(points),
            detail_key: key.to_owned(),
        }
    }

    #[test]
    fn each_new_permission_shows_its_points_and_recent_changes_are_exposed() {
        let old = Scored::Scored(Score {
            factors: vec![factor(FactorKind::MediumPermission, 6, "chat.read")],
            ..match scored(10) {
                Scored::Scored(s) => s,
                _ => unreachable!(),
            }
        });
        let new = Scored::Scored(Score {
            h: 18,
            f: 0.9,
            factors: vec![
                factor(FactorKind::MediumPermission, 6, "chat.read"),
                factor(FactorKind::WebSurface, 4, "ui.webview"),
                factor(FactorKind::Relay, 8, "net.relay"),
                factor(FactorKind::Websites, 3, "origins"),
                factor(FactorKind::RecentChange, 9, "net.relay"),
                factor(FactorKind::RecentChange, 9, "ui.webview"),
            ],
            ..match scored(60) {
                Scored::Scored(s) => s,
                _ => unreachable!(),
            }
        });
        let review = update_review(&[], &[], &BTreeMap::new(), Some(&old), Some(&new), false);
        let effects: Vec<(&str, i32)> = review
            .permission_effects
            .iter()
            .map(|e| (e.permission.as_str(), e.points))
            .collect();
        assert_eq!(effects, [("ui.webview", 4), ("net.relay", 8)]);
        assert_eq!(
            review.recent_change,
            Some(RecentChange {
                points: 18,
                trust_percent: 90,
                permissions: strings(&["net.relay", "ui.webview"]),
            })
        );

        let quiet = update_review(&[], &[], &BTreeMap::new(), Some(&old), Some(&old), false);
        assert!(quiet.permission_effects.is_empty() && quiet.recent_change.is_none());
        let unknown = update_review(&[], &[], &BTreeMap::new(), None, Some(&new), false);
        assert!(unknown.permission_effects.is_empty());
    }

    #[test]
    fn known_origins_carry_their_declared_purpose() {
        let old = strings(&["https://api.example.com"]);
        let new = strings(&["https://api.example.com", "wss://live.example.com"]);
        let purposes = BTreeMap::from([(
            "https://api.example.com".to_owned(),
            "origins.api".to_owned(),
        )]);
        let review = update_review(&old, &new, &purposes, None, None, false);
        assert_eq!(review.origins[0].purpose.as_deref(), Some("origins.api"));
        assert_eq!(review.origins[1].purpose, None);
    }
}
