//! The kill switch list (FR-6.70): parsing, the bundled copy and matching.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::keys::normalize_key;
use super::{FORMAT_VERSION, RegistryError};

/// Shipped with each release so a revocation works even before the first fetch.
const BUNDLED: &[u8] = include_bytes!("../../assets/revoked.json");

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Shows the reason on the plugin row.
    Warn,
    /// Stops and disables matching plugins; their data stays.
    Disable,
}

/// One entry of `revoked.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Revocation {
    pub id: String,
    /// Revoked versions. With neither `versions` nor `sha256`, every version is revoked.
    #[serde(default)]
    pub versions: Vec<String>,
    /// Revoked packages by lower-case hex SHA-256.
    #[serde(default)]
    pub sha256: Vec<String>,
    /// If set, only packages signed by this author key match.
    #[serde(default)]
    pub author_key: Option<String>,
    pub severity: Severity,
    /// Language tag → reason; `en` is required, `de` and `es` expected.
    pub reason: BTreeMap<String, String>,
    /// Date the revocation was published, `YYYY-MM-DD`.
    pub since: String,
}

impl Revocation {
    /// The reason in `lang`, falling back to English.
    pub fn reason(&self, lang: &str) -> &str {
        self.reason
            .get(lang)
            .or_else(|| self.reason.get("en"))
            .map(String::as_str)
            .unwrap_or_default()
    }

    fn matches(&self, id: &str, version: &str, sha256: &str, author_key: Option<&str>) -> bool {
        if self.id != id {
            return false;
        }
        if let Some(revoked_key) = &self.author_key {
            let signer = author_key.and_then(normalize_key);
            if signer.is_none() || signer != normalize_key(revoked_key) {
                return false;
            }
        }
        let every_version = self.versions.is_empty() && self.sha256.is_empty();
        every_version
            || self.versions.iter().any(|v| v == version)
            || self.sha256.iter().any(|h| h.eq_ignore_ascii_case(sha256))
    }
}

#[derive(Deserialize)]
struct RawList {
    format: u32,
    sequence: u64,
    revoked: Vec<Revocation>,
}

/// A parsed revocation list. Official revocations apply to every source (FR-6.70, FR-6.76).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevocationList {
    pub sequence: u64,
    pub entries: Vec<Revocation>,
}

impl RevocationList {
    /// Parses a signature-checked `revoked.json`.
    pub fn parse(bytes: &[u8]) -> Result<Self, RegistryError> {
        let raw: RawList =
            serde_json::from_slice(bytes).map_err(|e| RegistryError::Format(e.to_string()))?;
        if raw.format != FORMAT_VERSION {
            return Err(RegistryError::Format(format!(
                "unsupported format {}",
                raw.format
            )));
        }
        if raw.revoked.iter().any(|r| !r.reason.contains_key("en")) {
            return Err(RegistryError::Format("a reason has no English text".into()));
        }
        Ok(Self {
            sequence: raw.sequence,
            entries: raw.revoked,
        })
    }

    /// The copy compiled into this release.
    pub fn bundled() -> Self {
        Self::parse(BUNDLED).expect("bundled revoked.json is valid")
    }

    /// Adds the entries of `other`; the higher sequence is kept.
    pub fn merge(&mut self, other: RevocationList) {
        self.sequence = self.sequence.max(other.sequence);
        for entry in other.entries {
            if !self.entries.contains(&entry) {
                self.entries.push(entry);
            }
        }
    }

    /// The strongest revocation matching an installed package, `disable` before `warn`.
    /// `sha256` is the package's lower-case hex digest, `author_key` the key that signed it.
    pub fn find(
        &self,
        id: &str,
        version: &str,
        sha256: &str,
        author_key: Option<&str>,
    ) -> Option<&Revocation> {
        self.entries
            .iter()
            .filter(|r| r.matches(id, version, sha256, author_key))
            .max_by_key(|r| r.severity)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const KEY: &str = "RWStYW7iMZi0HAbHe+TL+GQ4NH70MYE6hJMKZDGGvpbqucV6UpbYwVXz";

    fn list(entries: serde_json::Value) -> RevocationList {
        RevocationList::parse(
            &serde_json::to_vec(&json!({"format": 1, "sequence": 3, "revoked": entries}))
                .expect("test step"),
        )
        .expect("test step")
    }

    #[test]
    fn bundled_copy_parses() {
        RevocationList::bundled();
    }

    #[test]
    fn matching_rules() {
        let list = list(json!([
            {"id": "echolings", "severity": "warn", "reason": {"en": "Old"}, "since": "2026-10-01"},
            {"id": "echolings", "versions": ["1.0.0"], "severity": "disable",
             "reason": {"en": "Broken", "de": "Kaputt"}, "since": "2026-10-02"},
            {"id": "dice", "sha256": ["AB".repeat(32)], "author_key": KEY, "severity": "disable",
             "reason": {"en": "Bad build"}, "since": "2026-10-03"}
        ]));
        let hit = list
            .find("echolings", "1.0.0", "00", None)
            .expect("test step");
        assert_eq!(hit.severity, Severity::Disable);
        assert_eq!(hit.reason("de"), "Kaputt");
        assert_eq!(hit.reason("es"), "Broken");
        assert_eq!(
            list.find("echolings", "2.0.0", "00", None)
                .expect("test step")
                .severity,
            Severity::Warn
        );
        let sha = "ab".repeat(32);
        assert!(list.find("dice", "1.0.0", &sha, Some(KEY)).is_some());
        assert!(
            list.find("dice", "1.0.0", &sha, None).is_none(),
            "author key must match"
        );
        assert!(list.find("dice", "1.0.0", "cd", Some(KEY)).is_none());
        assert!(list.find("poll", "1.0.0", &sha, None).is_none());
    }

    #[test]
    fn merge_keeps_union_and_highest_sequence() {
        let mut bundled = RevocationList::bundled();
        bundled.merge(list(json!([
            {"id": "poll", "severity": "disable", "reason": {"en": "x"}, "since": "2026-10-01"}
        ])));
        assert_eq!(bundled.sequence, 3);
        assert!(bundled.find("poll", "1.0.0", "", None).is_some());
    }

    #[test]
    fn reason_needs_english() {
        let bytes = serde_json::to_vec(&json!({"format": 1, "sequence": 1, "revoked": [
            {"id": "poll", "severity": "warn", "reason": {"de": "x"}, "since": "2026-10-01"}
        ]}))
        .expect("test step");
        assert!(RevocationList::parse(&bytes).is_err());
    }
}
