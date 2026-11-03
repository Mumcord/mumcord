//! The score weights, read from the bundled `score-weights.toml`. The numbers live only there.

use std::sync::OnceLock;

use serde::Deserialize;

const SOURCE: &str = include_str!("../../score-weights.toml");

#[derive(Debug, Deserialize)]
pub struct Weights {
    pub version: u32,
    pub bands: Bands,
    pub e: E,
    pub f: F,
    pub h: H,
}

#[derive(Debug, Deserialize)]
pub struct Bands {
    pub medium_from: u8,
    pub high_from: u8,
}

#[derive(Debug, Deserialize)]
pub struct E {
    pub cap: u32,
    pub floor: u32,
    pub floor_permissions: Vec<String>,
    pub high: Vec<String>,
    pub high_points: u32,
    pub web: Vec<String>,
    pub web_first_points: u32,
    pub web_further_points: u32,
    pub relay: Vec<String>,
    pub relay_points: u32,
    pub medium: Vec<String>,
    pub medium_points: u32,
    pub low_points: u32,
    pub low_cap: u32,
    pub website_points: u32,
    pub website_cap: u32,
    pub size_bounds: Vec<u64>,
    pub size_points: Vec<u32>,
    pub size_over_points: u32,
    pub modules_few_points: u32,
    pub modules_many_points: u32,
}

#[derive(Debug, Deserialize)]
pub struct F {
    pub min_percent: i32,
    pub max_percent: i32,
    pub source_link_given: i32,
    pub source_link_missing: i32,
    pub licence_osi: i32,
    pub licence_proprietary: i32,
    pub licence_not_declared: i32,
    pub reproducible: i32,
    pub key_old_days: u32,
    pub key_old: i32,
    pub key_new_days: u32,
    pub key_new: i32,
    pub expected_key_matched: i32,
}

#[derive(Debug, Deserialize)]
pub struct H {
    pub cap: u32,
    pub versions: usize,
    pub days: u32,
    pub medium_points: u32,
    pub high_points: u32,
}

/// The bundled weights. The file ships inside the binary and is covered by tests, so a parse
/// failure is a build defect, not a runtime condition.
pub fn weights() -> &'static Weights {
    static WEIGHTS: OnceLock<Weights> = OnceLock::new();
    WEIGHTS.get_or_init(|| toml::from_str(SOURCE).expect("score-weights.toml is valid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_weights_parse_and_are_consistent() {
        let w = weights();
        assert_eq!(w.version, 1);
        assert_eq!(w.e.size_bounds.len(), w.e.size_points.len());
        assert!(w.f.min_percent <= 100 && w.f.max_percent >= 100);
        assert!(w.bands.medium_from < w.bands.high_from);
    }
}
