//! Plugin safety for the UI (spec 09): the security score, the install gate, the update
//! review and the websites a plugin may reach. The types mirror the plugin crate's serde-only
//! types so the generated bindings describe them; the UI never computes a score or a gate.

use mumcord_plugins::score::{Band, Effect, Factor, FactorKind, HardStop, Score, Scored};
use mumcord_plugins::service::{
    ConfirmKind, GateReason, InstallCheck, InstallGate, InstallScore, Refusal, ReviewOrigin,
    RiskChange, UpdateHold, UpdateReview,
};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginBand {
    Low,
    Medium,
    High,
}

impl From<Band> for PluginBand {
    fn from(band: Band) -> Self {
        match band {
            Band::Low => Self::Low,
            Band::Medium => Self::Medium,
            Band::High => Self::High,
        }
    }
}

/// Why a package is refused instead of scored (FR-9.78).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginHardStop {
    InvalidSignature,
    HashMismatch,
    ManifestMismatch,
    Revoked,
    UnsafeOrigin,
}

impl From<HardStop> for PluginHardStop {
    fn from(stop: HardStop) -> Self {
        match stop {
            HardStop::InvalidSignature => Self::InvalidSignature,
            HardStop::HashMismatch => Self::HashMismatch,
            HardStop::ManifestMismatch => Self::ManifestMismatch,
            HardStop::Revoked => Self::Revoked,
            HardStop::UnsafeOrigin => Self::UnsafeOrigin,
        }
    }
}

/// One line of the score breakdown table.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginFactorKind {
    HighPermission,
    WebSurface,
    Relay,
    MediumPermission,
    LowPermissions,
    Websites,
    PackageSize,
    WasmModules,
    SourceLink,
    Licence,
    Reproducible,
    KeyAge,
    ExpectedKey,
    DeveloperTrust,
    RecentChange,
    NoHistory,
    NetworkFloor,
}

impl From<FactorKind> for PluginFactorKind {
    fn from(kind: FactorKind) -> Self {
        match kind {
            FactorKind::HighPermission => Self::HighPermission,
            FactorKind::WebSurface => Self::WebSurface,
            FactorKind::Relay => Self::Relay,
            FactorKind::MediumPermission => Self::MediumPermission,
            FactorKind::LowPermissions => Self::LowPermissions,
            FactorKind::Websites => Self::Websites,
            FactorKind::PackageSize => Self::PackageSize,
            FactorKind::WasmModules => Self::WasmModules,
            FactorKind::SourceLink => Self::SourceLink,
            FactorKind::Licence => Self::Licence,
            FactorKind::Reproducible => Self::Reproducible,
            FactorKind::KeyAge => Self::KeyAge,
            FactorKind::ExpectedKey => Self::ExpectedKey,
            FactorKind::DeveloperTrust => Self::DeveloperTrust,
            FactorKind::RecentChange => Self::RecentChange,
            FactorKind::NoHistory => Self::NoHistory,
            FactorKind::NetworkFloor => Self::NetworkFloor,
        }
    }
}

/// What a factor does to the score: whole points or a percentage.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PluginEffect {
    Points { value: i32 },
    Percent { value: i32 },
}

impl From<Effect> for PluginEffect {
    fn from(effect: Effect) -> Self {
        match effect {
            Effect::Points(value) => Self::Points { value },
            Effect::Percent(value) => Self::Percent { value },
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginScoreFactor {
    pub kind: PluginFactorKind,
    pub effect: PluginEffect,
    /// Translation key of the factor's detail text.
    pub detail_key: String,
}

impl From<Factor> for PluginScoreFactor {
    fn from(factor: Factor) -> Self {
        Self {
            kind: factor.kind.into(),
            effect: factor.effect.into(),
            detail_key: factor.detail_key,
        }
    }
}

/// A computed security score: 0–100, its band and the breakdown behind it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginScore {
    pub value: u8,
    pub band: PluginBand,
    pub e: u32,
    pub f: f32,
    pub h: u32,
    pub factors: Vec<PluginScoreFactor>,
    /// Version of the weights the score was computed with.
    pub version: u32,
}

impl From<Score> for PluginScore {
    fn from(score: Score) -> Self {
        Self {
            value: score.value,
            band: score.band.into(),
            e: score.e,
            f: score.f,
            h: score.h,
            factors: score.factors.into_iter().map(Into::into).collect(),
            version: score.version,
        }
    }
}

/// The score of a plugin version, or why there is none.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PluginScored {
    /// Official plugins show "Official · reviewed" instead of a number.
    Official,
    /// Cannot be installed; no number.
    Refused {
        stop: PluginHardStop,
    },
    Scored {
        score: PluginScore,
    },
}

impl From<Scored> for PluginScored {
    fn from(scored: Scored) -> Self {
        match scored {
            Scored::Official => Self::Official,
            Scored::Refused(stop) => Self::Refused { stop: stop.into() },
            Scored::Scored(score) => Self::Scored {
                score: score.into(),
            },
        }
    }
}

/// Where a score is read from.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginScoreSource {
    /// The signed index entry, as Browse shows it.
    Listing,
    /// The package the install prompt downloaded and verified.
    Package,
}

/// The package score of the install prompt next to what the listing promised.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallScore {
    pub score: PluginScored,
    /// `None` when the version is not listed (e.g. Developer mode).
    pub listing: Option<PluginScored>,
    /// "This plugin scores higher than its listing said."
    pub higher_than_listing: bool,
}

impl From<InstallScore> for PluginInstallScore {
    fn from(score: InstallScore) -> Self {
        Self {
            score: score.score.into(),
            listing: score.listing.map(Into::into),
            higher_than_listing: score.higher_than_listing,
        }
    }
}

/// What the person has to tick before Install is enabled.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginConfirmKind {
    /// "I understand the risk and I trust the author" / "Install anyway".
    HighRisk,
    /// "I understand" / "Install".
    Understand,
}

impl From<ConfirmKind> for PluginConfirmKind {
    fn from(kind: ConfirmKind) -> Self {
        match kind {
            ConfirmKind::HighRisk => Self::HighRisk,
            ConfirmKind::Understand => Self::Understand,
        }
    }
}

/// Why the one confirmation box shows.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum PluginGateReason {
    HighRisk,
    OwnCodeAndWebsites,
    RepositoryWarning,
}

impl From<GateReason> for PluginGateReason {
    fn from(reason: GateReason) -> Self {
        match reason {
            GateReason::HighRisk => Self::HighRisk,
            GateReason::OwnCodeAndWebsites => Self::OwnCodeAndWebsites,
            GateReason::RepositoryWarning => Self::RepositoryWarning,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallGate {
    /// `None`: Install is enabled right away.
    pub confirm: Option<PluginConfirmKind>,
    /// "Don't allow high-risk plugins" is on and the plugin is High: Install stays disabled
    /// with "Blocked by your safety setting".
    pub blocked_by_safety: bool,
    pub reasons: Vec<PluginGateReason>,
}

impl From<InstallGate> for PluginInstallGate {
    fn from(gate: InstallGate) -> Self {
        Self {
            confirm: gate.confirm.map(Into::into),
            blocked_by_safety: gate.blocked_by_safety,
            reasons: gate.reasons.into_iter().map(Into::into).collect(),
        }
    }
}

/// What the install prompt shows once the package is downloaded and checked.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PluginInstallCheck {
    /// "Can't be installed": no score, no Install button.
    Refused {
        stop: PluginHardStop,
        /// User-facing text.
        reason: String,
    },
    Ready {
        score: PluginInstallScore,
        gate: PluginInstallGate,
    },
}

impl From<InstallCheck> for PluginInstallCheck {
    fn from(check: InstallCheck) -> Self {
        match check {
            InstallCheck::Refused {
                refusal: Refusal { stop, reason },
            } => Self::Refused {
                stop: stop.into(),
                reason,
            },
            InstallCheck::Ready { score, gate } => Self::Ready {
                score: score.into(),
                gate: gate.into(),
            },
        }
    }
}

/// Why an update waits for the user although it may otherwise install by itself.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PluginUpdateHold {
    NewWebsites { count: u32 },
    RiskUp { from: PluginBand, to: PluginBand },
    ScoreUp { from: u8, to: u8 },
    BlockedBySafety,
    ScoreUnknown,
}

impl From<UpdateHold> for PluginUpdateHold {
    fn from(hold: UpdateHold) -> Self {
        match hold {
            UpdateHold::NewWebsites { count } => Self::NewWebsites { count },
            UpdateHold::RiskUp { from, to } => Self::RiskUp {
                from: from.into(),
                to: to.into(),
            },
            UpdateHold::ScoreUp { from, to } => Self::ScoreUp { from, to },
            UpdateHold::BlockedBySafety => Self::BlockedBySafety,
            UpdateHold::ScoreUnknown => Self::ScoreUnknown,
        }
    }
}

/// One website of an update; plain, untrusted text.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginReviewOrigin {
    pub origin: String,
    /// Marked "New".
    pub new: bool,
    /// Translation key of the purpose the installed version declares for this website
    /// (resolved in the plugin's own texts). `None` for a new website: the registry index lists
    /// no purposes and the new package is not downloaded before the user decides.
    pub purpose: Option<String>,
}

impl From<ReviewOrigin> for PluginReviewOrigin {
    fn from(origin: ReviewOrigin) -> Self {
        Self {
            origin: origin.origin,
            new: origin.new,
            purpose: origin.purpose,
        }
    }
}

/// The points one permission that is new in an update adds to the score.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginPermissionEffect {
    pub permission: String,
    pub points: i32,
}

/// The "recent change" part of the score: new permissions within the last versions.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginRecentChange {
    /// Points added on top (outside the trust percentage).
    pub points: u32,
    /// The trust percentage applied to the permission points, e.g. 90.
    pub trust_percent: u32,
    /// The permissions that count as a recent change.
    pub permissions: Vec<String>,
}

/// The score before and after an update.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginRiskChange {
    pub from_value: u8,
    pub from_band: PluginBand,
    pub to_value: u8,
    pub to_band: PluginBand,
}

impl From<RiskChange> for PluginRiskChange {
    fn from(change: RiskChange) -> Self {
        Self {
            from_value: change.from_value,
            from_band: change.from_band.into(),
            to_value: change.to_value,
            to_band: change.to_band.into(),
        }
    }
}

/// What the Updates tab shows to decide on an update.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdateReview {
    pub origins: Vec<PluginReviewOrigin>,
    /// "One of them is not on the plugin's own domain."
    pub other_domain: bool,
    /// `None` for official plugins or when a score is unknown.
    pub risk: Option<PluginRiskChange>,
    /// Points each new permission adds; empty when the score is unknown.
    pub permission_effects: Vec<PluginPermissionEffect>,
    /// `None` when no permission is new in the last versions.
    pub recent_change: Option<PluginRecentChange>,
    /// The update makes the plugin High: the tick and "Update anyway" are needed
    /// (`plugin_confirm_update_high_risk`).
    pub needs_high_risk_confirm: bool,
    pub high_risk_confirmed: bool,
}

impl From<UpdateReview> for PluginUpdateReview {
    fn from(review: UpdateReview) -> Self {
        Self {
            origins: review.origins.into_iter().map(Into::into).collect(),
            other_domain: review.other_domain,
            risk: review.risk.map(Into::into),
            permission_effects: review
                .permission_effects
                .into_iter()
                .map(|e| PluginPermissionEffect {
                    permission: e.permission,
                    points: e.points,
                })
                .collect(),
            recent_change: review.recent_change.map(|r| PluginRecentChange {
                points: r.points,
                trust_percent: r.trust_percent,
                permissions: r.permissions,
            }),
            needs_high_risk_confirm: review.needs_high_risk_confirm,
            high_risk_confirmed: review.high_risk_confirmed,
        }
    }
}

/// One website an installed plugin declares.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginWebsite {
    /// ASCII form; plain, untrusted text.
    pub origin: String,
    /// Translation key of the plugin's reason (resolved in the plugin's own texts).
    pub purpose: String,
}

/// One day of a plugin's network use (kept for 7 days).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginNetworkDay {
    /// Local calendar day, `YYYY-MM-DD`.
    pub day: String,
    pub requests: u32,
    pub blocked: u32,
    /// Unix milliseconds of the last request.
    pub last_request_at: Option<f64>,
    /// Host of the last request; plain, untrusted text.
    pub last_host: Option<String>,
}

/// The websites an installed plugin may reach and what it did with them (FR-9.30).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginWebsites {
    pub origins: Vec<PluginWebsite>,
    /// Newest day first.
    pub days: Vec<PluginNetworkDay>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_plugins::service::{PermissionEffect, RecentChange};

    #[test]
    fn a_refusal_keeps_its_hard_stop_and_text() {
        let check = PluginInstallCheck::from(InstallCheck::Refused {
            refusal: Refusal {
                stop: HardStop::UnsafeOrigin,
                reason: "Cannot be installed".into(),
            },
        });
        assert_eq!(
            check,
            PluginInstallCheck::Refused {
                stop: PluginHardStop::UnsafeOrigin,
                reason: "Cannot be installed".into(),
            }
        );
    }

    #[test]
    fn a_gate_maps_every_field() {
        let gate = PluginInstallGate::from(InstallGate {
            confirm: Some(ConfirmKind::HighRisk),
            blocked_by_safety: true,
            reasons: vec![GateReason::HighRisk, GateReason::RepositoryWarning],
        });
        assert_eq!(gate.confirm, Some(PluginConfirmKind::HighRisk));
        assert!(gate.blocked_by_safety);
        assert_eq!(
            gate.reasons,
            [
                PluginGateReason::HighRisk,
                PluginGateReason::RepositoryWarning
            ]
        );
    }

    #[test]
    fn a_hold_and_a_review_map_across() {
        assert_eq!(
            PluginUpdateHold::from(UpdateHold::RiskUp {
                from: Band::Low,
                to: Band::High
            }),
            PluginUpdateHold::RiskUp {
                from: PluginBand::Low,
                to: PluginBand::High
            }
        );
        let review = PluginUpdateReview::from(UpdateReview {
            origins: vec![ReviewOrigin {
                origin: "https://api.example".into(),
                new: true,
                purpose: Some("origins.api".into()),
            }],
            permission_effects: vec![PermissionEffect {
                permission: "net.relay".into(),
                points: 8,
            }],
            recent_change: Some(RecentChange {
                points: 18,
                trust_percent: 90,
                permissions: vec!["net.relay".into()],
            }),
            other_domain: true,
            risk: Some(RiskChange {
                from_value: 10,
                from_band: Band::Low,
                to_value: 70,
                to_band: Band::High,
            }),
            needs_high_risk_confirm: true,
            high_risk_confirmed: false,
        });
        assert!(review.origins[0].new);
        assert_eq!(review.origins[0].purpose.as_deref(), Some("origins.api"));
        assert_eq!(review.permission_effects[0].points, 8);
        assert_eq!(review.recent_change.map(|r| r.trust_percent), Some(90));
        assert_eq!(review.risk.map(|r| r.to_band), Some(PluginBand::High));
        assert!(review.needs_high_risk_confirm);
    }

    #[test]
    fn a_scored_plugin_keeps_its_breakdown() {
        let scored = PluginScored::from(Scored::Scored(Score {
            value: 42,
            band: Band::Medium,
            e: 1,
            f: 1.5,
            h: 2,
            factors: vec![Factor {
                kind: FactorKind::Websites,
                effect: Effect::Points(5),
                detail_key: "score.websites".into(),
            }],
            version: 1,
        }));
        let PluginScored::Scored { score } = scored else {
            panic!("expected a score");
        };
        assert_eq!(score.value, 42);
        assert_eq!(score.factors[0].effect, PluginEffect::Points { value: 5 });
        assert_eq!(PluginScored::from(Scored::Official), PluginScored::Official);
    }
}
