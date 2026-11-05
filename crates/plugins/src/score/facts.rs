//! Gathers the [`Facts`] the score is computed from (FR-9.72): the signed index, the verified
//! package and its manifest, and what this device remembers (key first-seen, FR-9.74; version
//! history, FR-9.73 H). Everything here is a pure function of its inputs except [`Local::load`],
//! which reads the store once so the score itself stays free of I/O.

use std::collections::BTreeMap;

use mumcord_store::{Store, UnixMillis, VersionHistoryEntry};
use semver::Version;

use super::{Facts, HistoryEntry, Licence, PluginKind};
use crate::manifest::Source;
use crate::package::Package;
use crate::registry::{IndexPlugin, IndexVersion};
use crate::test_hooks;

const DAY_MILLIS: i64 = 24 * 60 * 60 * 1000;

/// What this device remembers about a plugin and its author keys.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Local {
    pub now: UnixMillis,
    /// Author key → when this device first saw it in a refreshed index.
    pub key_first_seen: BTreeMap<String, UnixMillis>,
    /// Versions of the plugin as first seen (index refreshes record every listed version).
    pub versions: Vec<VersionHistoryEntry>,
    /// Repositories whose "Expected key" matched when they were added (FR-6.74).
    pub expected_key_repositories: Vec<i64>,
}

impl Local {
    /// Reads the first-seen times of `keys` and the stored version history of `plugin_id`.
    pub fn load(
        store: &Store,
        plugin_id: &str,
        keys: &[&str],
        now: UnixMillis,
    ) -> Result<Self, mumcord_store::Error> {
        let mut key_first_seen = BTreeMap::new();
        for key in keys {
            if let Some(at) = store.repo_key_first_seen(key)? {
                key_first_seen.insert((*key).to_owned(), at);
            }
        }
        Ok(Self {
            now,
            key_first_seen,
            versions: store.plugin_version_history(plugin_id)?,
            expected_key_repositories: store
                .plugin_repositories()?
                .into_iter()
                .filter(|repository| repository.expected_key_matched)
                .map(|repository| repository.id)
                .collect(),
        })
    }

    fn expected_key_matched(&self, source: &Source) -> bool {
        matches!(source, Source::Repo { repository_id }
            if self.expected_key_repositories.contains(repository_id))
    }

    /// Whole days this device has known `key`; `None` when it never saw it. The `key_age`
    /// test hook replaces the stored age so E2E runs can stage old keys.
    pub fn key_age_days(&self, key: &str) -> Option<u32> {
        if let Some(days) = test_hooks::key_age(key) {
            return Some(days);
        }
        let first = *self.key_first_seen.get(key)?;
        Some(days_between(first, self.now))
    }

    /// Earlier versions of the plugin than `scored`, newest first, from the index version list
    /// and the stored history; `None` when there are none ("no history available").
    ///
    /// The index carries no publication dates, so a version's age is the gap between the days
    /// this device first saw it and the scored version (or now, when the scored version was
    /// never listed). A listed version without a stored row counts as seen with the scored one.
    pub fn history(&self, scored: &Version, listed: &[IndexVersion]) -> Option<Vec<HistoryEntry>> {
        let seen: BTreeMap<&str, &VersionHistoryEntry> = self
            .versions
            .iter()
            .map(|entry| (entry.version.as_str(), entry))
            .collect();
        let scored_at = seen
            .get(scored.to_string().as_str())
            .map_or(self.now, |entry| entry.seen_at);

        let mut priors: BTreeMap<Version, (Vec<String>, UnixMillis)> = BTreeMap::new();
        for entry in &self.versions {
            if let Some(version) = earlier(&entry.version, scored) {
                let permissions = serde_json::from_str(&entry.permissions).unwrap_or_else(|e| {
                    tracing::warn!(%e, "ignoring unreadable stored plugin permissions");
                    Vec::new()
                });
                priors.insert(version, (permissions, entry.seen_at));
            }
        }
        // The signed index wins over what was stored when both know a version.
        for listed in listed {
            if let Some(version) = earlier(&listed.version, scored) {
                let at = seen
                    .get(listed.version.as_str())
                    .map_or(scored_at, |entry| entry.seen_at);
                priors.insert(version, (listed.permissions.clone(), at));
            }
        }
        if priors.is_empty() {
            return None;
        }
        Some(
            priors
                .into_values()
                .rev()
                .map(|(permissions, at)| HistoryEntry {
                    age_days: days_between(at, scored_at),
                    permissions,
                })
                .collect(),
        )
    }
}

fn earlier(version: &str, scored: &Version) -> Option<Version> {
    Version::parse(version).ok().filter(|v| v < scored)
}

fn days_between(from: UnixMillis, to: UnixMillis) -> u32 {
    u32::try_from((to - from).max(0) / DAY_MILLIS).unwrap_or(u32::MAX)
}

pub fn kind_of(source: &Source) -> PluginKind {
    match source {
        Source::Official => PluginKind::Official,
        Source::Repo { .. } => PluginKind::Unofficial,
        Source::Dev => PluginKind::Developer,
    }
}

/// Any declared SPDX expression is kept; the score checks it against the OSI list.
pub fn licence_of(license: Option<&str>) -> Licence {
    match license.map(str::trim).filter(|l| !l.is_empty()) {
        Some(id) => Licence::Spdx(id.to_owned()),
        None => Licence::NotDeclared,
    }
}

fn source_link(plugin: Option<&IndexPlugin>) -> bool {
    plugin
        .and_then(|p| p.source_url.as_deref())
        .is_some_and(|url| url.starts_with("https://"))
}

/// Browse: scores from index data only; missing format-2 fields add nothing (FR-9.76). The
/// youngest pinned author key counts, since any of them may sign the package.
pub fn from_listing(
    source: &Source,
    plugin: &IndexPlugin,
    version: &IndexVersion,
    local: &Local,
) -> Facts {
    let key_age_days = plugin
        .author_keys
        .iter()
        .map(|key| local.key_age_days(key))
        .min()
        .flatten();
    Facts {
        kind: kind_of(source),
        permissions: version.permissions.clone(),
        origins: count(version.origins.len()),
        package_bytes: version.size,
        wasm_modules: version.modules.unwrap_or(1),
        source_link: source_link(Some(plugin)),
        licence: licence_of(plugin.license.as_deref()),
        reproducible: version.reproducible.unwrap_or(false),
        key_age_days,
        expected_key_matched: local.expected_key_matched(source),
        history: Version::parse(&version.version)
            .ok()
            .and_then(|v| local.history(&v, &plugin.versions)),
        hard_stop: None,
    }
}

/// The verified package as the install prompt sees it. The listing, when there is one, adds
/// what only the index knows: the source link, the reproducible-build claim and the history.
pub fn from_package(
    source: &Source,
    package: &Package,
    package_bytes: u64,
    author_key: &str,
    listing: Option<(&IndexPlugin, Option<&IndexVersion>)>,
    local: &Local,
) -> Facts {
    let manifest = &package.manifest;
    let plugin = listing.map(|(plugin, _)| plugin);
    let listed_version = listing.and_then(|(_, version)| version);
    Facts {
        kind: kind_of(source),
        permissions: manifest.all_permissions().map(str::to_owned).collect(),
        origins: count(manifest.net.as_ref().map_or(0, |net| net.origins.len())),
        package_bytes,
        wasm_modules: count(
            package
                .file_names()
                .filter(|name| name.ends_with(".wasm"))
                .count(),
        )
        .max(1),
        source_link: source_link(plugin),
        licence: licence_of(Some(&manifest.license)),
        reproducible: listed_version.and_then(|v| v.reproducible).unwrap_or(false),
        key_age_days: local.key_age_days(author_key),
        expected_key_matched: local.expected_key_matched(source),
        history: local.history(
            &manifest.version,
            plugin.map_or(&[][..], |p| p.versions.as_slice()),
        ),
        hard_stop: None,
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use serde_json::json;
    use zip::write::SimpleFileOptions;

    use super::*;

    const KEY: &str = "RWStYW7iMZi0HAbHe+TL+GQ4NH70MYE6hJMKZDGGvpbqucV6UpbYwVXz";
    const OTHER: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const REPO: Source = Source::Repo { repository_id: 1 };
    const NOW: UnixMillis = 400 * DAY_MILLIS;

    fn version(v: &str, permissions: &[&str]) -> serde_json::Value {
        json!({"version": v, "api": 1, "url": format!("https://example.org/p-{v}.mcplugin"),
               "size": 3 * 1024 * 1024, "sha256": "ab".repeat(32), "permissions": permissions})
    }

    fn plugin(extra: serde_json::Value, versions: Vec<serde_json::Value>) -> IndexPlugin {
        let mut value = json!({
            "id": "p", "name": {"en": "P"}, "summary": {"en": "P"},
            "author_keys": [KEY, OTHER], "reviewed": false, "updated": "2026-10-01",
            "versions": versions,
        });
        if let (Some(target), serde_json::Value::Object(extra)) = (value.as_object_mut(), extra) {
            target.extend(extra);
        }
        serde_json::from_value(value).expect("test plugin")
    }

    fn row(version: &str, permissions: &str, day: i64) -> VersionHistoryEntry {
        VersionHistoryEntry {
            plugin_id: "p".into(),
            version: version.into(),
            permissions: permissions.into(),
            origins: "[]".into(),
            seen_at: day * DAY_MILLIS,
        }
    }

    fn local(keys: &[(&str, i64)], versions: Vec<VersionHistoryEntry>) -> Local {
        Local {
            now: NOW,
            key_first_seen: keys
                .iter()
                .map(|(k, day)| ((*k).to_owned(), day * DAY_MILLIS))
                .collect(),
            versions,
            expected_key_repositories: Vec::new(),
        }
    }

    #[test]
    fn a_listing_without_format_2_fields_adds_nothing() {
        let p = plugin(json!({}), vec![version("1.0.0", &["ui.panel"])]);
        let facts = from_listing(&REPO, &p, &p.versions[0], &Local::default());
        assert_eq!(facts.kind, PluginKind::Unofficial);
        assert_eq!(facts.permissions, ["ui.panel"]);
        assert_eq!(facts.origins, 0);
        assert_eq!(facts.wasm_modules, 1);
        assert!(!facts.reproducible && !facts.source_link && !facts.expected_key_matched);
        assert_eq!(facts.licence, Licence::NotDeclared);
        assert_eq!(facts.key_age_days, None);
        assert_eq!(facts.history, None);
        assert_eq!(facts.package_bytes, 3 * 1024 * 1024);
    }

    #[test]
    fn a_listing_reads_every_index_field() {
        let mut v = version("1.0.0", &["net.origins"]);
        v["api"] = json!("1.1");
        v["origins"] = json!(["https://a.example", "https://b.example"]);
        v["modules"] = json!(3);
        v["reproducible"] = json!(true);
        let p = plugin(
            json!({"source_url": "https://example.org/src", "license": "MIT"}),
            vec![v],
        );
        let facts = from_listing(&REPO, &p, &p.versions[0], &Local::default());
        assert_eq!(facts.origins, 2);
        assert_eq!(facts.wasm_modules, 3);
        assert!(facts.reproducible && facts.source_link);
        assert_eq!(facts.licence, Licence::Spdx("MIT".into()));
    }

    #[test]
    fn plain_http_is_no_source_link() {
        let p = plugin(
            json!({"source_url": "http://example.org/src"}),
            vec![version("1.0.0", &[])],
        );
        assert!(!from_listing(&REPO, &p, &p.versions[0], &Local::default()).source_link);
    }

    #[test]
    fn the_listing_counts_the_youngest_pinned_key() {
        let p = plugin(json!({}), vec![version("1.0.0", &[])]);
        let both = local(&[(KEY, 0), (OTHER, 380)], vec![]);
        assert_eq!(
            from_listing(&REPO, &p, &p.versions[0], &both).key_age_days,
            Some(20)
        );
        let one_unknown = local(&[(KEY, 0)], vec![]);
        assert_eq!(
            from_listing(&REPO, &p, &p.versions[0], &one_unknown).key_age_days,
            None
        );
    }

    #[test]
    fn the_source_decides_the_kind() {
        assert_eq!(kind_of(&Source::Official), PluginKind::Official);
        assert_eq!(kind_of(&REPO), PluginKind::Unofficial);
        assert_eq!(kind_of(&Source::Dev), PluginKind::Developer);
    }

    #[test]
    fn history_merges_index_and_store_newest_first_with_first_seen_ages() {
        let listed: Vec<IndexVersion> = [
            version("1.0.0", &["ui.panel"]),
            version("1.1.0", &["ui.panel", "chat.send"]),
            version("2.0.0", &["net.origins"]),
        ]
        .into_iter()
        .map(|v| serde_json::from_value(v).expect("version"))
        .collect();
        let stored = local(
            &[],
            vec![
                row("0.9.0", r#"["notify"]"#, 100),
                row("1.0.0", r#"["stale"]"#, 200),
                row("1.1.0", "[]", 290),
                row("2.0.0", "[]", 300),
            ],
        );
        let history = stored
            .history(&Version::new(2, 0, 0), &listed)
            .expect("history");
        let got: Vec<(u32, Vec<String>)> = history
            .into_iter()
            .map(|h| (h.age_days, h.permissions))
            .collect();
        assert_eq!(
            got,
            vec![
                (10, vec!["ui.panel".into(), "chat.send".into()]),
                (100, vec!["ui.panel".into()]),
                (200, vec!["notify".into()]),
            ]
        );
    }

    #[test]
    fn a_version_never_listed_is_aged_against_now() {
        let stored = local(&[], vec![row("1.0.0", "[]", 390)]);
        let history = stored
            .history(&Version::new(1, 1, 0), &[])
            .expect("history");
        assert_eq!(history[0].age_days, 10);
        assert_eq!(stored.history(&Version::new(1, 0, 0), &[]), None);
    }

    fn package(manifest_extra: &str, extra_wasm: bool) -> Package {
        let manifest = format!(
            r#"{{"manifest":1,"id":"scored","name":"Test","version":"2.0.0","api":"^1.0","authors":["Tests"],
               "description":"A test plugin","categories":["tools"],"license":"MPL-2.0"{manifest_extra}}}"#
        );
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let mut files = vec![
            ("manifest.json", manifest.into_bytes()),
            ("plugin.wasm", b"\0asm".to_vec()),
            ("i18n/en.json", b"{}".to_vec()),
        ];
        if extra_wasm {
            files.push(("web/helper.wasm", b"\0asm".to_vec()));
        }
        for (name, data) in files {
            writer.start_file(name, options).expect("start file");
            writer.write_all(&data).expect("write");
        }
        let bytes = writer.finish().expect("finish").into_inner();
        Package::read(&bytes, REPO).expect("package")
    }

    #[test]
    fn a_package_scores_its_manifest_and_borrows_index_only_facts() {
        let package = package(
            r#","permissions":{"required":["ui.panel","ui.webview"],"optional":["chat.send"]}"#,
            true,
        );
        let mut v = version("2.0.0", &[]);
        v["reproducible"] = json!(true);
        let p = plugin(
            json!({"source_url": "https://example.org/src", "license": "MIT"}),
            vec![version("1.0.0", &["ui.panel"]), v],
        );
        let local = local(&[(KEY, 0)], vec![row("1.0.0", "[]", 300)]);
        let facts = from_package(
            &REPO,
            &package,
            42,
            KEY,
            Some((&p, p.version("2.0.0"))),
            &local,
        );
        assert_eq!(facts.permissions, ["ui.panel", "ui.webview", "chat.send"]);
        assert_eq!(facts.package_bytes, 42);
        assert_eq!(facts.wasm_modules, 2);
        assert_eq!(facts.licence, Licence::Spdx("MPL-2.0".into()));
        assert!(facts.source_link && facts.reproducible);
        assert_eq!(facts.key_age_days, Some(400));
        let history = facts.history.expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].age_days, 100);
    }

    #[test]
    fn an_unlisted_package_has_no_index_facts() {
        let facts = from_package(
            &Source::Dev,
            &package("", false),
            42,
            KEY,
            None,
            &Local::default(),
        );
        assert_eq!(facts.kind, PluginKind::Developer);
        assert_eq!(facts.wasm_modules, 1);
        assert!(!facts.source_link && !facts.reproducible);
        assert_eq!(facts.history, None);
        assert_eq!(facts.key_age_days, None);
    }

    #[test]
    fn local_state_loads_from_the_store() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(dir.path()).expect("store");
        store.note_repo_key_seen(KEY, 10 * DAY_MILLIS).expect("key");
        store
            .record_plugin_version(&row("1.0.0", "[]", 10))
            .expect("version");
        let loaded = Local::load(&store, "p", &[KEY, OTHER], NOW).expect("load");
        assert_eq!(loaded.key_age_days(KEY), Some(390));
        assert_eq!(loaded.key_age_days(OTHER), None);
        assert_eq!(loaded.versions.len(), 1);
    }

    #[test]
    fn expected_key_match_is_read_per_repository() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(dir.path()).expect("store");
        let new = |n: u8| mumcord_store::NewPluginRepository {
            url: format!("https://r{n}.example.org/"),
            name: format!("Repo {n}"),
            public_key: "key".into(),
        };
        let keyed = store.add_plugin_repository(&new(1), 5).expect("add");
        let plain = store.add_plugin_repository(&new(2), 5).expect("add");
        store
            .set_plugin_repository_expected_key_matched(keyed, true)
            .expect("set");
        let local = Local::load(&store, "p", &[], NOW).expect("load");
        let p = plugin(json!({}), vec![version("1.0.0", &[])]);
        let facts = |id| {
            from_listing(
                &Source::Repo { repository_id: id },
                &p,
                &p.versions[0],
                &local,
            )
        };
        assert!(facts(keyed).expected_key_matched);
        assert!(!facts(plain).expected_key_matched);
        assert!(!from_listing(&Source::Dev, &p, &p.versions[0], &local).expected_key_matched);
    }
}
