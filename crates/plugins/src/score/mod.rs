//! The security score of unofficial plugins (spec 09, FR-9.71 to FR-9.75): a pure function of
//! the facts about a plugin. No I/O, no clock, no randomness, so the same facts always give the
//! same score. All numbers come from `score-weights.toml` (see [`weights`]).
//!
//! `Score = min(100, round(E x F) + H)`, raised to the network floor when the plugin holds a
//! network permission.

mod weights;

use std::collections::BTreeSet;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::permissions::{self, Risk};
use weights::{Weights, weights};

const SPDX_OSI: &str = include_str!("../../assets/spdx-osi.json");

/// Who published the plugin; decides whether and how it is scored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    /// Reviewed by the maintainers: shown "Official · reviewed", never scored.
    Official,
    /// From a third-party repository.
    Unofficial,
    /// Loaded in Developer mode; scored for information only, trust factor fixed at 1.00.
    Developer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Licence {
    /// An SPDX identifier; OSI-approved when it is on the bundled list.
    Spdx(String),
    Proprietary,
    NotDeclared,
}

/// A published version before the one being scored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Days between that version's publication and the version being scored.
    pub age_days: u32,
    pub permissions: Vec<String>,
}

/// Why a package is refused instead of scored (FR-9.78). Detected by the install flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardStop {
    InvalidSignature,
    HashMismatch,
    ManifestMismatch,
    Revoked,
    UnsafeOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Facts {
    pub kind: PluginKind,
    /// Every permission the manifest may ask for, optional ones included.
    pub permissions: Vec<String>,
    /// Declared external websites (`net.origins`).
    pub origins: u32,
    pub package_bytes: u64,
    pub wasm_modules: u32,
    /// An https source-code link is given.
    pub source_link: bool,
    pub licence: Licence,
    /// The repository claims a reproducible build.
    pub reproducible: bool,
    /// Days this device has known the author key; `None` counts as new.
    pub key_age_days: Option<u32>,
    /// The user entered an "Expected key" for the repository and it matched.
    pub expected_key_matched: bool,
    /// Earlier published versions, newest first; `None` when no history is available.
    pub history: Option<Vec<HistoryEntry>>,
    pub hard_stop: Option<HardStop>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactorKind {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Added to E, H or the final score.
    Points(i32),
    /// Added to the trust factor F.
    Percent(i32),
}

/// One row of the breakdown table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Factor {
    pub kind: FactorKind,
    pub effect: Effect,
    /// Stable key the UI maps to a text, e.g. `net.http` or `licence.osi`.
    pub detail_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub value: u8,
    pub band: Band,
    pub e: u32,
    pub f: f32,
    pub h: u32,
    pub factors: Vec<Factor>,
    pub version: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scored {
    /// Official plugins carry "Official · reviewed" instead of a number.
    Official,
    /// Refused, not penalised: a penalty could be bought back with trust points.
    Refused(HardStop),
    Scored(Score),
}

/// Whether an SPDX identifier is on the bundled OSI-approved list.
pub fn is_osi_approved(id: &str) -> bool {
    static LIST: OnceLock<BTreeSet<String>> = OnceLock::new();
    LIST.get_or_init(|| {
        #[derive(Deserialize)]
        struct File {
            ids: BTreeSet<String>,
        }
        serde_json::from_str::<File>(SPDX_OSI)
            .expect("spdx-osi.json is valid")
            .ids
    })
    .contains(id)
}

pub fn band_of(value: u8) -> Band {
    let w = weights();
    if value >= w.bands.high_from {
        Band::High
    } else if value >= w.bands.medium_from {
        Band::Medium
    } else {
        Band::Low
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    High,
    Web,
    Relay,
    Medium,
    Low,
}

fn class_of(w: &Weights, id: &str) -> Class {
    let listed = |list: &[String]| list.iter().any(|p| p == id);
    if listed(&w.e.high) {
        Class::High
    } else if listed(&w.e.web) {
        Class::Web
    } else if listed(&w.e.relay) {
        Class::Relay
    } else if listed(&w.e.medium) {
        Class::Medium
    } else {
        match permissions::get(id).map(|p| p.risk) {
            Some(Risk::Red) => Class::High,
            Some(Risk::Yellow) => Class::Medium,
            _ => Class::Low,
        }
    }
}

fn points(n: u32) -> Effect {
    Effect::Points(i32::try_from(n).unwrap_or(i32::MAX))
}

fn push(factors: &mut Vec<Factor>, kind: FactorKind, effect: Effect, key: &str) {
    factors.push(Factor {
        kind,
        effect,
        detail_key: key.to_owned(),
    });
}

/// Capability points E over the deduplicated permission set.
fn capability(w: &Weights, facts: &Facts, factors: &mut Vec<Factor>) -> u32 {
    let perms: BTreeSet<&str> = facts.permissions.iter().map(String::as_str).collect();
    let mut e = 0;
    let mut web_seen = 0;
    let mut low_total = 0;
    for id in perms {
        match class_of(w, id) {
            Class::High => {
                e += w.e.high_points;
                push(
                    factors,
                    FactorKind::HighPermission,
                    points(w.e.high_points),
                    id,
                );
            }
            Class::Web => {
                let p = if web_seen == 0 {
                    w.e.web_first_points
                } else {
                    w.e.web_further_points
                };
                web_seen += 1;
                e += p;
                push(factors, FactorKind::WebSurface, points(p), id);
            }
            Class::Relay => {
                e += w.e.relay_points;
                push(factors, FactorKind::Relay, points(w.e.relay_points), id);
            }
            Class::Medium => {
                e += w.e.medium_points;
                push(
                    factors,
                    FactorKind::MediumPermission,
                    points(w.e.medium_points),
                    id,
                );
            }
            Class::Low => {
                let p = w.e.low_points.min(w.e.low_cap.saturating_sub(low_total));
                low_total += p;
                e += p;
                push(factors, FactorKind::LowPermissions, points(p), id);
            }
        }
    }
    if facts.origins >= 2 {
        let p = ((facts.origins - 1) * w.e.website_points).min(w.e.website_cap);
        e += p;
        push(factors, FactorKind::Websites, points(p), "origins");
    }
    let size =
        w.e.size_bounds
            .iter()
            .position(|bound| facts.package_bytes <= *bound)
            .map_or(w.e.size_over_points, |i| w.e.size_points[i]);
    e += size;
    push(
        factors,
        FactorKind::PackageSize,
        points(size),
        "package_size",
    );
    let modules = match facts.wasm_modules {
        0 | 1 => 0,
        2 | 3 => w.e.modules_few_points,
        _ => w.e.modules_many_points,
    };
    e += modules;
    push(
        factors,
        FactorKind::WasmModules,
        points(modules),
        "wasm_modules",
    );
    e.min(w.e.cap)
}

/// Trust factor F in whole percent (100 = 1.00), clamped.
fn trust(w: &Weights, facts: &Facts, factors: &mut Vec<Factor>) -> i32 {
    if facts.kind == PluginKind::Developer {
        push(
            factors,
            FactorKind::DeveloperTrust,
            Effect::Percent(0),
            "developer",
        );
        return 100;
    }
    let mut add = |kind, pct: i32, key: &str| {
        push(factors, kind, Effect::Percent(pct), key);
        pct
    };
    let mut total = 0;
    total += if facts.source_link {
        add(
            FactorKind::SourceLink,
            w.f.source_link_given,
            "source_link.given",
        )
    } else {
        add(
            FactorKind::SourceLink,
            w.f.source_link_missing,
            "source_link.missing",
        )
    };
    total += match &facts.licence {
        Licence::Spdx(id) if is_osi_approved(id) => {
            add(FactorKind::Licence, w.f.licence_osi, "licence.osi")
        }
        Licence::Spdx(_) | Licence::Proprietary => add(
            FactorKind::Licence,
            w.f.licence_proprietary,
            "licence.proprietary",
        ),
        Licence::NotDeclared => add(
            FactorKind::Licence,
            w.f.licence_not_declared,
            "licence.not_declared",
        ),
    };
    total += if facts.reproducible {
        add(
            FactorKind::Reproducible,
            w.f.reproducible,
            "reproducible.claimed",
        )
    } else {
        add(FactorKind::Reproducible, 0, "reproducible.none")
    };
    total += match facts.key_age_days {
        Some(d) if d >= w.f.key_old_days => add(FactorKind::KeyAge, w.f.key_old, "key_age.old"),
        Some(d) if d >= w.f.key_new_days => add(FactorKind::KeyAge, 0, "key_age.known"),
        _ => add(FactorKind::KeyAge, w.f.key_new, "key_age.new"),
    };
    if facts.expected_key_matched {
        total += add(
            FactorKind::ExpectedKey,
            w.f.expected_key_matched,
            "expected_key.matched",
        );
    }
    (100 + total).clamp(w.f.min_percent, w.f.max_percent)
}

/// Recent-change points H: permissions added since the oldest version of the look-back window.
fn recent_changes(w: &Weights, facts: &Facts, factors: &mut Vec<Factor>) -> u32 {
    let window: Vec<&HistoryEntry> = facts
        .history
        .iter()
        .flatten()
        .filter(|v| v.age_days <= w.h.days)
        .take(w.h.versions.saturating_sub(1))
        .collect();
    let Some(oldest) = window.last() else {
        push(
            factors,
            FactorKind::NoHistory,
            Effect::Points(0),
            "no_history",
        );
        return 0;
    };
    let before: BTreeSet<&str> = oldest.permissions.iter().map(String::as_str).collect();
    let now: BTreeSet<&str> = facts.permissions.iter().map(String::as_str).collect();
    let mut h = 0;
    for id in now.difference(&before) {
        let p = match class_of(w, id) {
            Class::High | Class::Web => w.h.high_points,
            _ => w.h.medium_points,
        };
        h += p;
        push(factors, FactorKind::RecentChange, points(p), id);
    }
    h.min(w.h.cap)
}

/// Scores a plugin. Official plugins and hard stops produce no number.
pub fn score(facts: &Facts) -> Scored {
    if facts.kind == PluginKind::Official {
        return Scored::Official;
    }
    if let Some(stop) = facts.hard_stop {
        return Scored::Refused(stop);
    }
    let w = weights();
    let mut factors = Vec::new();
    let e = capability(w, facts, &mut factors);
    let pct = u32::try_from(trust(w, facts, &mut factors)).unwrap_or(100);
    let h = recent_changes(w, facts, &mut factors);

    // Integer rounding half up of e * pct / 100 keeps the result free of float drift.
    let mut value = ((e * pct + 50) / 100 + h).min(100);
    let holds_network = facts
        .permissions
        .iter()
        .any(|p| w.e.floor_permissions.contains(p));
    if holds_network && value < w.e.floor {
        push(
            &mut factors,
            FactorKind::NetworkFloor,
            points(w.e.floor - value),
            "network_floor",
        );
        value = w.e.floor;
    }
    let value = u8::try_from(value).unwrap_or(100);
    Scored::Scored(Score {
        value,
        band: band_of(value),
        e,
        f: f32::from(u16::try_from(pct).unwrap_or(100)) / 100.0,
        h,
        factors,
        version: w.version,
    })
}
