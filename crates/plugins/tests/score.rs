//! Score vectors (spec 09, FR-9.75): inputs and expected results pinned in
//! `tests/fixtures/score-vectors.json`.

use mumcord_plugins::score::{
    Band, Effect, FactorKind, Facts, HardStop, Licence, PluginKind, Scored, is_osi_approved, score,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Vector {
    name: String,
    facts: Facts,
    expect: Expect,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Expect {
    Official,
    Refused(HardStop),
    Scored {
        value: u8,
        band: Band,
        e: u32,
        f_percent: u32,
        h: u32,
    },
}

fn vectors() -> Vec<Vector> {
    serde_json::from_str(include_str!("fixtures/score-vectors.json")).expect("vectors parse")
}

#[test]
fn vectors_match() {
    let mut failures = Vec::new();
    for v in vectors() {
        let got = score(&v.facts);
        let ok = match (&v.expect, &got) {
            (Expect::Official, Scored::Official) => true,
            (Expect::Refused(a), Scored::Refused(b)) => a == b,
            (
                Expect::Scored {
                    value,
                    band,
                    e,
                    f_percent,
                    h,
                },
                Scored::Scored(s),
            ) => {
                s.value == *value
                    && s.band == *band
                    && s.e == *e
                    && (s.f * 100.0).round() as u32 == *f_percent
                    && s.h == *h
                    && s.version == 1
            }
            _ => false,
        };
        if !ok {
            failures.push(format!("{}: got {got:?}", v.name));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn sample() -> Facts {
    Facts {
        kind: PluginKind::Unofficial,
        permissions: vec!["net.http".into()],
        origins: 0,
        package_bytes: 1024,
        wasm_modules: 1,
        source_link: true,
        licence: Licence::Spdx("MIT".into()),
        reproducible: true,
        key_age_days: Some(200),
        expected_key_matched: false,
        history: None,
        hard_stop: None,
    }
}

#[test]
fn network_floor_is_listed_in_the_breakdown() {
    let Scored::Scored(s) = score(&sample()) else {
        panic!("scored");
    };
    assert_eq!(s.value, 30);
    assert_eq!(s.band, Band::Medium);
    let floor = s
        .factors
        .iter()
        .find(|f| f.kind == FactorKind::NetworkFloor)
        .expect("floor row");
    assert_eq!(floor.effect, Effect::Points(15));
}

#[test]
fn no_history_is_said_in_the_breakdown() {
    let Scored::Scored(s) = score(&sample()) else {
        panic!("scored");
    };
    assert!(s.factors.iter().any(|f| f.kind == FactorKind::NoHistory));
}

#[test]
fn same_facts_give_the_same_score() {
    assert_eq!(score(&sample()), score(&sample()));
}

#[test]
fn duplicate_permissions_count_once() {
    let mut facts = sample();
    facts.permissions = vec!["net.http".into(); 3];
    let Scored::Scored(s) = score(&facts) else {
        panic!("scored");
    };
    assert_eq!(s.e, 20);
}

#[test]
fn spdx_list_knows_common_licences() {
    for id in [
        "MIT",
        "Apache-2.0",
        "GPL-3.0-or-later",
        "MPL-2.0",
        "BSD-3-Clause",
    ] {
        assert!(is_osi_approved(id), "{id}");
    }
    assert!(!is_osi_approved("LicenseRef-custom"));
    assert!(!is_osi_approved("CC-BY-NC-4.0"));
}
