//! The index model (FR-6.55, FR-6.76, FR-6.80) and its validation.
//!
//! An index is parsed only after its signature was checked, but entries are still validated one
//! by one: a broken or forbidden entry is dropped with a note instead of hiding the whole list.

use std::collections::{BTreeMap, HashSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use super::keys::normalize_key;
use super::{FORMAT_VERSION, MAX_REPOSITORY_PLUGINS, RegistryError};
use crate::Digest;
use crate::manifest::is_valid_id;
use crate::package::{MAX_OFFICIAL_PACKAGE_BYTES, MAX_PACKAGE_BYTES, check_unofficial_id};
use crate::verify::IndexEntry;

/// Which kind of index is parsed; third-party indexes get the FR-6.76 rules.
pub enum IndexKind<'a> {
    Official,
    /// `is_official_id` answers from the official index the caller knows.
    Repository {
        is_official_id: &'a (dyn Fn(&str) -> bool + Sync),
    },
}

/// A verified, validated index.
#[derive(Debug, Clone, Serialize)]
pub struct Index {
    pub sequence: u64,
    /// Required for third-party repositories; absent in the official index.
    pub repository: Option<RepositoryInfo>,
    pub key_rotation: Option<KeyRotation>,
    pub plugins: Vec<IndexPlugin>,
    /// Entries that were dropped, with the reason (FR-6.76 "ignored with a note").
    pub ignored: Vec<Ignored>,
}

/// What a third-party repository says about itself (shown in the add dialog, FR-6.74).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryInfo {
    pub name: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub contact: Option<String>,
    /// The minisign public key (base64 line) the index is signed with.
    pub public_key: String,
}

/// FR-6.75: lets a repository move to a new key without pausing; `signature` is a minisign
/// signature by the old (pinned) key over the exact bytes of `new_key`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyRotation {
    pub new_key: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Screenshot {
    pub url: String,
    pub alt: BTreeMap<String, String>,
}

/// One plugin of an index (FR-6.80).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexPlugin {
    pub id: String,
    /// Language tag → text; `en` is required.
    pub name: BTreeMap<String, String>,
    /// Language tag → text; `en` is required.
    pub summary: BTreeMap<String, String>,
    #[serde(default)]
    pub description_md_url: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub screenshots: Vec<Screenshot>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    /// Author keys pinned for this id (FR-6.50), normalised to the base64 line.
    pub author_keys: Vec<String>,
    #[serde(default)]
    pub reviewed: bool,
    /// Where the plugin's source code lives (an `http(s)` URL); shown on its details page.
    #[serde(default)]
    pub source_url: Option<String>,
    /// SPDX licence expression, e.g. `MIT`.
    #[serde(default)]
    pub license: Option<String>,
    /// Lowest Mumcord version the plugin needs, e.g. `0.7`.
    #[serde(default)]
    pub min_mumcord: Option<String>,
    /// Day the maintainers reviewed the plugin, `YYYY-MM-DD` (official index only).
    #[serde(default)]
    pub reviewed_on: Option<String>,
    /// Day the plugin was added to the index, `YYYY-MM-DD`.
    #[serde(default)]
    pub added_on: Option<String>,
    /// Date of the latest version, `YYYY-MM-DD`.
    pub updated: String,
    pub versions: Vec<IndexVersion>,
}

/// One published version of a plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexVersion {
    pub version: String,
    /// Plugin API major version the package targets.
    pub api: u32,
    /// Package download URL; the author signature is at `url` + `.minisig`.
    pub url: String,
    pub size: u64,
    /// Lower-case hex SHA-256 of the package bytes.
    pub sha256: String,
    /// Every permission (required and optional) the reviewed version may ask for.
    pub permissions: Vec<String>,
}

/// A dropped index entry.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Ignored {
    /// The entry's id, or empty if it had none.
    pub id: String,
    pub reason: IgnoredReason,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum IgnoredReason {
    OfficialId,
    ReservedPrefix,
    Duplicate,
    Invalid(String),
}

/// Prefix of every note, followed by the id.
pub const IGNORED_PREFIX_TEXT: &str = "Ignored plugin";

impl fmt::Display for Ignored {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match &self.reason {
            IgnoredReason::OfficialId => "this id belongs to an official plugin".to_owned(),
            IgnoredReason::ReservedPrefix => {
                "this id uses a prefix reserved for Mumcord".to_owned()
            }
            IgnoredReason::Duplicate => "the id is listed twice".to_owned(),
            IgnoredReason::Invalid(detail) => format!("invalid entry ({detail})"),
        };
        write!(f, "{IGNORED_PREFIX_TEXT} \"{}\": {why}", self.id)
    }
}

#[derive(Deserialize)]
struct RawIndex {
    format: u32,
    sequence: u64,
    #[serde(default)]
    repository: Option<RepositoryInfo>,
    #[serde(default)]
    key_rotation: Option<KeyRotation>,
    plugins: Vec<serde_json::Value>,
}

/// Reads just the repository block, before the signature is checked, to learn which key the
/// repository claims (FR-6.74 TOFU, FR-6.75 key change).
pub(super) fn declared_key(bytes: &[u8]) -> Result<String, RegistryError> {
    #[derive(Deserialize)]
    struct Head {
        repository: Option<RepositoryInfo>,
    }
    let head: Head =
        serde_json::from_slice(bytes).map_err(|e| RegistryError::Format(e.to_string()))?;
    let repository = head
        .repository
        .ok_or_else(|| RegistryError::Format("the repository block is missing".into()))?;
    normalize_key(&repository.public_key)
        .ok_or_else(|| RegistryError::Format("the repository key is not a minisign key".into()))
}

impl Index {
    /// Parses and validates a signature-checked index.
    pub fn parse(bytes: &[u8], kind: &IndexKind<'_>) -> Result<Self, RegistryError> {
        let raw: RawIndex =
            serde_json::from_slice(bytes).map_err(|e| RegistryError::Format(e.to_string()))?;
        if raw.format != FORMAT_VERSION {
            return Err(RegistryError::Format(format!(
                "unsupported format {}",
                raw.format
            )));
        }
        let repository = match kind {
            IndexKind::Official => None,
            IndexKind::Repository { .. } => {
                if raw.plugins.len() > MAX_REPOSITORY_PLUGINS {
                    return Err(RegistryError::Format(format!(
                        "more than {MAX_REPOSITORY_PLUGINS} plugins"
                    )));
                }
                let mut info = raw.repository.ok_or_else(|| {
                    RegistryError::Format("the repository block is missing".into())
                })?;
                if info.name.trim().is_empty() {
                    return Err(RegistryError::Format("the repository has no name".into()));
                }
                info.public_key = normalize_key(&info.public_key).ok_or_else(|| {
                    RegistryError::Format("the repository key is not a minisign key".into())
                })?;
                Some(info)
            }
        };

        let mut seen = HashSet::new();
        let mut plugins = Vec::new();
        let mut ignored = Vec::new();
        for value in raw.plugins {
            let id = value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            match check_plugin(value, kind) {
                Ok(_) if !seen.insert(id.clone()) => ignored.push(Ignored {
                    id,
                    reason: IgnoredReason::Duplicate,
                }),
                Ok(plugin) => plugins.push(plugin),
                Err(reason) => ignored.push(Ignored { id, reason }),
            }
        }
        Ok(Self {
            sequence: raw.sequence,
            repository,
            key_rotation: raw.key_rotation,
            plugins,
            ignored,
        })
    }

    pub fn plugin(&self, id: &str) -> Option<&IndexPlugin> {
        self.plugins.iter().find(|plugin| plugin.id == id)
    }
}

fn check_plugin(
    value: serde_json::Value,
    kind: &IndexKind<'_>,
) -> Result<IndexPlugin, IgnoredReason> {
    let invalid = |detail: &str| IgnoredReason::Invalid(detail.to_owned());
    let mut plugin: IndexPlugin =
        serde_json::from_value(value).map_err(|e| IgnoredReason::Invalid(e.to_string()))?;
    if !is_valid_id(&plugin.id) {
        return Err(invalid("bad id"));
    }
    if let IndexKind::Repository { is_official_id } = kind {
        check_unofficial_id(&plugin.id, is_official_id).map_err(|e| match e {
            crate::PackageError::OfficialId(_) => IgnoredReason::OfficialId,
            _ => IgnoredReason::ReservedPrefix,
        })?;
    }
    if !plugin.name.contains_key("en") || !plugin.summary.contains_key("en") {
        return Err(invalid("missing English name or summary"));
    }
    plugin.author_keys = plugin
        .author_keys
        .iter()
        .map(|key| normalize_key(key))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| invalid("bad author key"))?;
    if plugin.author_keys.is_empty() {
        return Err(invalid("no author key"));
    }
    check_details(&plugin).map_err(IgnoredReason::Invalid)?;
    if plugin.versions.is_empty() {
        return Err(invalid("no versions"));
    }
    let max_size = match kind {
        IndexKind::Official => MAX_OFFICIAL_PACKAGE_BYTES,
        IndexKind::Repository { .. } => MAX_PACKAGE_BYTES,
    };
    let mut versions = HashSet::new();
    for version in &plugin.versions {
        if semver::Version::parse(&version.version).is_err() || !versions.insert(&version.version) {
            return Err(invalid("bad or duplicate version"));
        }
        if version.size == 0 || version.size > max_size {
            return Err(invalid("bad package size"));
        }
        if parse_sha256(&version.sha256).is_none() {
            return Err(invalid("bad sha256"));
        }
        if reqwest::Url::parse(&version.url).is_err() {
            return Err(invalid("bad package url"));
        }
    }
    Ok(plugin)
}

/// The optional details fields come from the repository, so they are length-limited and checked
/// before the UI shows them.
fn check_details(plugin: &IndexPlugin) -> Result<(), String> {
    const MAX_TEXT: usize = 100;
    const MAX_URL: usize = 300;
    let text = |name: &str, value: &Option<String>| match value {
        Some(v) if v.trim().is_empty() || v.chars().count() > MAX_TEXT => {
            Err(format!("bad {name}"))
        }
        _ => Ok(()),
    };
    let date = |name: &str, value: &Option<String>| match value {
        Some(v) if chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").is_err() => {
            Err(format!("bad {name}"))
        }
        _ => Ok(()),
    };
    if let Some(url) = &plugin.source_url {
        let ok = url.len() <= MAX_URL
            && reqwest::Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https"));
        if !ok {
            return Err("bad source url".to_owned());
        }
    }
    text("license", &plugin.license)?;
    text("minimum Mumcord version", &plugin.min_mumcord)?;
    date("review date", &plugin.reviewed_on)?;
    date("added date", &plugin.added_on)
}

fn parse_sha256(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

impl IndexPlugin {
    /// The highest published version.
    pub fn latest(&self) -> &IndexVersion {
        self.versions
            .iter()
            .max_by_key(|v| semver::Version::parse(&v.version).ok())
            .expect("validated: at least one version")
    }

    pub fn version(&self, version: &str) -> Option<&IndexVersion> {
        self.versions.iter().find(|v| v.version == version)
    }

    /// The name in `lang`, falling back to English.
    pub fn name(&self, lang: &str) -> &str {
        localized(&self.name, lang)
    }

    pub fn summary(&self, lang: &str) -> &str {
        localized(&self.summary, lang)
    }

    /// What B14's install checks need for `version`.
    pub fn entry(&self, version: &IndexVersion) -> IndexEntry {
        IndexEntry {
            id: self.id.clone(),
            version: version.version.clone(),
            digest: Digest {
                sha256: parse_sha256(&version.sha256).expect("validated sha256"),
                size: version.size,
            },
            author_keys: self.author_keys.clone(),
            permissions: version.permissions.clone(),
        }
    }
}

fn localized<'a>(texts: &'a BTreeMap<String, String>, lang: &str) -> &'a str {
    texts
        .get(lang)
        .or_else(|| texts.get("en"))
        .map(String::as_str)
        .unwrap_or_default()
}

#[cfg(test)]
pub(super) mod tests {
    use serde_json::json;

    use super::*;

    pub const AUTHOR: &str = "RWStYW7iMZi0HAbHe+TL+GQ4NH70MYE6hJMKZDGGvpbqucV6UpbYwVXz";

    pub fn plugin(id: &str) -> serde_json::Value {
        json!({
            "id": id,
            "name": {"en": "Dice", "de": "Würfel"},
            "summary": {"en": "Roll dice"},
            "screenshots": [{"url": "https://example.org/a.png", "alt": {"en": "A roll"}}],
            "author_keys": [AUTHOR],
            "reviewed": true,
            "updated": "2026-10-01",
            "versions": [
                {"version": "1.0.0", "api": 1, "url": "https://example.org/dice-1.0.0.mcplugin",
                 "size": 10, "sha256": "ab".repeat(32), "permissions": ["chat.send"]},
                {"version": "1.2.0", "api": 1, "url": "https://example.org/dice-1.2.0.mcplugin",
                 "size": 12, "sha256": "cd".repeat(32), "permissions": []}
            ]
        })
    }

    fn official(plugins: Vec<serde_json::Value>) -> Vec<u8> {
        serde_json::to_vec(&json!({"format": 1, "sequence": 7, "plugins": plugins}))
            .expect("test step")
    }

    #[test]
    fn only_the_official_index_may_list_big_packages() {
        let sized = |size: u64, wrap: fn(Vec<serde_json::Value>) -> Vec<u8>| {
            let mut value = plugin("dice");
            value["versions"][0]["size"] = json!(size);
            wrap(vec![value])
        };
        let none = |_: &str| false;
        let repo = IndexKind::Repository {
            is_official_id: &none,
        };
        let repo_index: fn(Vec<serde_json::Value>) -> Vec<u8> = self::repo;
        let mib = 1024 * 1024;
        let kept = |bytes: &[u8], kind: &IndexKind<'_>| {
            Index::parse(bytes, kind)
                .map(|index| index.plugins.len())
                .unwrap_or(0)
        };
        assert_eq!(kept(&sized(64 * mib, official), &IndexKind::Official), 1);
        assert_eq!(
            kept(&sized(64 * mib + 1, official), &IndexKind::Official),
            0
        );
        assert_eq!(kept(&sized(5 * mib, repo_index), &repo), 1);
        assert_eq!(kept(&sized(5 * mib + 1, repo_index), &repo), 0);
    }

    #[test]
    fn official_index_parses_with_website_fields() {
        let index =
            Index::parse(&official(vec![plugin("dice")]), &IndexKind::Official).expect("test step");
        assert_eq!(index.sequence, 7);
        let dice = index.plugin("dice").expect("test step");
        assert_eq!(dice.latest().version, "1.2.0");
        assert_eq!(dice.name("de"), "Würfel");
        assert_eq!(dice.name("fr"), "Dice");
        assert!(dice.reviewed);
        assert_eq!(dice.screenshots[0].alt["en"], "A roll");
        let entry = dice.entry(dice.version("1.0.0").expect("test step"));
        assert_eq!(entry.digest.sha256, [0xab; 32]);
        assert_eq!(entry.permissions, vec!["chat.send".to_owned()]);
    }

    #[test]
    fn bad_entries_are_dropped_with_a_note() {
        let mut bad_hash = plugin("bad-hash");
        bad_hash["versions"][0]["sha256"] = json!("xyz");
        let mut no_versions = plugin("empty");
        no_versions["versions"] = json!([]);
        let bytes = official(vec![
            plugin("dice"),
            plugin("dice"),
            bad_hash,
            no_versions,
            json!(1),
        ]);
        let index = Index::parse(&bytes, &IndexKind::Official).expect("test step");
        assert_eq!(index.plugins.len(), 1);
        let reasons: Vec<_> = index.ignored.iter().map(|i| i.reason.clone()).collect();
        assert_eq!(reasons[0], IgnoredReason::Duplicate);
        assert!(matches!(reasons[1], IgnoredReason::Invalid(_)));
        assert_eq!(index.ignored.len(), 4);
    }

    #[test]
    fn optional_details_are_read_and_checked() {
        let mut good = plugin("dice");
        good["source_url"] = json!("https://github.com/Mumcord/dice");
        good["license"] = json!("MIT");
        good["min_mumcord"] = json!("0.7");
        good["reviewed_on"] = json!("2026-10-20");
        good["added_on"] = json!("2026-09-01");
        let mut bad_url = plugin("a-url");
        bad_url["source_url"] = json!("javascript:alert(1)");
        let mut bad_date = plugin("a-date");
        bad_date["reviewed_on"] = json!("20 Oct");
        let index = Index::parse(
            &official(vec![good, bad_url, bad_date, plugin("plain")]),
            &IndexKind::Official,
        )
        .expect("test step");
        let dice = index.plugin("dice").expect("test step");
        assert_eq!(dice.license.as_deref(), Some("MIT"));
        assert_eq!(dice.reviewed_on.as_deref(), Some("2026-10-20"));
        assert!(
            index
                .plugin("plain")
                .expect("test step")
                .source_url
                .is_none()
        );
        assert_eq!(index.plugins.len(), 2);
        assert_eq!(index.ignored.len(), 2);
    }

    #[test]
    fn unknown_format_is_rejected() {
        let bytes = serde_json::to_vec(&json!({"format": 2, "sequence": 1, "plugins": []}))
            .expect("test step");
        assert!(matches!(
            Index::parse(&bytes, &IndexKind::Official),
            Err(RegistryError::Format(_))
        ));
    }

    fn repo(plugins: Vec<serde_json::Value>) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "format": 1, "sequence": 1,
            "repository": {"name": "Foo Repo", "homepage": "https://foo.example", "public_key": AUTHOR},
            "plugins": plugins
        }))
        .expect("test step")
    }

    #[test]
    fn repository_ignores_official_ids_and_reserved_prefixes() {
        let is_official = |id: &str| id == "poll";
        let kind = IndexKind::Repository {
            is_official_id: &is_official,
        };
        let index = Index::parse(
            &repo(vec![plugin("poll"), plugin("mumcord-x"), plugin("foo")]),
            &kind,
        )
        .expect("test step");
        assert_eq!(index.plugins.len(), 1);
        assert_eq!(index.ignored[0].reason, IgnoredReason::OfficialId);
        assert_eq!(index.ignored[1].reason, IgnoredReason::ReservedPrefix);
        assert_eq!(
            index.ignored[0].to_string(),
            "Ignored plugin \"poll\": this id belongs to an official plugin"
        );
        assert_eq!(index.repository.expect("test step").name, "Foo Repo");
    }

    #[test]
    fn repository_limits() {
        let none = |_: &str| false;
        let kind = IndexKind::Repository {
            is_official_id: &none,
        };
        let many: Vec<_> = (0..=MAX_REPOSITORY_PLUGINS)
            .map(|i| plugin(&format!("p{i:03}")))
            .collect();
        assert!(Index::parse(&repo(many), &kind).is_err());
        assert!(
            Index::parse(&official(vec![]), &kind).is_err(),
            "repository block required"
        );
    }
}
