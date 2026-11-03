//! `site-data`: builds the registry website data from a `Mumcord/plugins` checkout (FR-6.80,
//! FR-9.14): `v1/index.json` (format 1, API 1.0 versions only), `v1.1/index.json` (format 2, all
//! versions), `v1/revoked.json` (the one kill switch for both), `v1/permissions.json`,
//! `v1.1/permissions.json` and `v1/media/<id>/…`. The output is unsigned; the publish workflow
//! signs both indexes and the revocation list with `mumcord-plugin sign`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mumcord_plugins::registry::{Index, IndexKind, IndexPlugin, Screenshot};
use mumcord_plugins::version::ApiMinor;

use crate::date;
use crate::registry::{self, IndexHead, ListedPlugin, ListedVersion, package_file_name};
use crate::source::{self, SourcePlugin};

const BASE_URL: &str = "https://mumcord.github.io/plugins/v1/";
const PACKAGE_URL: &str =
    "https://github.com/Mumcord/plugins/releases/download/{id}-v{version}/{file}";

#[derive(clap::Args)]
pub struct Args {
    /// Checkout of the plugins registry.
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    /// Output folder; `v1/` and `v1.1/` are created inside.
    #[arg(long)]
    pub out: PathBuf,
    /// Index and revocation list sequence (default: the current Unix time, which only grows).
    #[arg(long)]
    pub sequence: Option<u64>,
    /// Where `v1/` is published (media URLs start with it).
    #[arg(long, default_value = BASE_URL)]
    pub base_url: String,
    /// Package download URL; `{id}`, `{version}` and `{file}` are filled in.
    #[arg(long, default_value = PACKAGE_URL)]
    pub package_url: String,
}

pub fn run(args: Args) -> Result<()> {
    let sequence = args.sequence.unwrap_or_else(date::unix_seconds);
    let base = if args.base_url.ends_with('/') {
        args.base_url.clone()
    } else {
        format!("{}/", args.base_url)
    };
    let out = args.out.join("v1");
    let out_1_1 = args.out.join("v1.1");
    fs::create_dir_all(&out)?;
    fs::create_dir_all(&out_1_1)?;

    let mut plugins = Vec::new();
    for plugin in source::load_plugins(&args.repo)? {
        plugins.push(listed_plugin(&plugin, &base, &args.package_url, &out)?);
    }
    let head = IndexHead {
        sequence,
        repository: None,
        key_rotation: None,
    };
    let index = registry::index_v1_json(&head, &plugins)?;
    let parsed = check_with_client(&index, "v1/index.json")?;
    let index_1_1 = registry::index_v1_1_json(&head, &plugins)?;
    check_with_client(&index_1_1, "v1.1/index.json")?;
    fs::write(out.join("index.json"), index)?;
    fs::write(out_1_1.join("index.json"), index_1_1)?;
    let revoked = source::load_revoked(&args.repo)?;
    fs::write(
        out.join("revoked.json"),
        registry::revoked_json(sequence, &revoked)?,
    )?;
    fs::write(
        out.join("permissions.json"),
        registry::permissions_json(ApiMinor::V1_0)?,
    )?;
    fs::write(
        out_1_1.join("permissions.json"),
        registry::permissions_json(ApiMinor::V1_1)?,
    )?;
    println!(
        "{} plugins ({} in v1), sequence {sequence}, written to {}",
        plugins.len(),
        parsed.plugins.len(),
        args.out.display()
    );
    Ok(())
}

fn listed_plugin(
    source: &SourcePlugin,
    base: &str,
    package_url: &str,
    out: &Path,
) -> Result<ListedPlugin> {
    let id = &source.id;
    let plugin = &source.plugin;
    let media = |file: &str| -> Result<String> {
        let target = out.join("media").join(id).join(file);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source.dir.join(file), &target)
            .with_context(|| format!("plugins/{id}: could not copy {file}"))?;
        Ok(format!("{base}media/{id}/{file}"))
    };
    let plugin_index = IndexPlugin {
        id: id.clone(),
        name: plugin.name.clone(),
        summary: plugin.summary.clone(),
        description_md_url: plugin.description.as_deref().map(media).transpose()?,
        icon: plugin.icon.as_deref().map(media).transpose()?,
        screenshots: plugin
            .screenshots
            .iter()
            .map(|s| {
                Ok(Screenshot {
                    url: media(&s.file)?,
                    alt: s.alt.clone(),
                })
            })
            .collect::<Result<_>>()?,
        categories: plugin.categories.clone(),
        authors: plugin.authors.clone(),
        author_keys: plugin.author_keys.clone(),
        reviewed: plugin.reviewed,
        source_url: plugin.source_url.clone(),
        license: plugin.license.clone(),
        min_mumcord: plugin.min_mumcord.clone(),
        reviewed_on: plugin.reviewed_on.clone(),
        added_on: plugin.added_on.clone(),
        // Both set per index from the versions it lists.
        updated: String::new(),
        versions: Vec::new(),
    };
    let versions = source
        .versions
        .iter()
        .map(|v| ListedVersion {
            version: v.version.clone(),
            api: v.api.0,
            url: package_url
                .replace("{id}", id)
                .replace("{version}", &v.version)
                .replace("{file}", &package_file_name(id, &v.version)),
            size: v.size,
            sha256: v.sha256.clone(),
            permissions: v.permissions.clone(),
            published: v.published.clone(),
            min_mumcord: v.min_mumcord.clone(),
            origins: v.origins.clone(),
        })
        .collect();
    Ok(ListedPlugin {
        plugin: plugin_index,
        versions,
    })
}

/// Reads a generated index the way Mumcord does. The client drops bad entries with a note; a
/// published index must have none.
fn check_with_client(index: &[u8], name: &str) -> Result<Index> {
    let parsed = Index::parse(index, &IndexKind::Official)
        .with_context(|| format!("the generated {name} is invalid"))?;
    if let Some(ignored) = parsed.ignored.first() {
        bail!(
            "plugin {} would be ignored by Mumcord in {name}: {:?}",
            ignored.id,
            ignored.reason
        );
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::tests::write_plugin;
    use mumcord_plugins::registry::Choice;

    #[test]
    fn api_1_1_versions_go_only_into_the_v1_1_index() {
        let repo = tempfile::tempdir().expect("test step");
        write_plugin(repo.path(), "dice", &["1.0.0", "1.1.0"]);
        write_plugin(repo.path(), "stage", &["1.0.0"]);
        for (id, version) in [("dice", "1.1.0"), ("stage", "1.0.0")] {
            let path = source::version_path(repo.path(), id, version);
            let text = fs::read_to_string(&path).expect("test step");
            let text = text.replace(
                "api = 1",
                "api = \"1.1\"\nmin_mumcord = \"0.7\"\norigins = [\"https://api.example\"]",
            );
            fs::write(&path, text).expect("test step");
        }
        let out = tempfile::tempdir().expect("test step");
        run(Args {
            repo: repo.path().to_owned(),
            out: out.path().to_owned(),
            sequence: Some(9),
            base_url: BASE_URL.into(),
            package_url: PACKAGE_URL.into(),
        })
        .expect("test step");

        let v1 = Index::parse(
            &fs::read(out.path().join("v1/index.json")).expect("test step"),
            &IndexKind::Official,
        )
        .expect("0.6 clients accept the index");
        assert!(v1.plugin("stage").is_none(), "never had a 1.0 version");
        let dice = v1.plugin("dice").expect("keeps its 1.0 version");
        assert_eq!(dice.latest().version, "1.0.0");
        assert_eq!(dice.updated, "2026-10-01");

        let v2: serde_json::Value = serde_json::from_slice(
            &fs::read(out.path().join("v1.1/index.json")).expect("test step"),
        )
        .expect("json");
        let dice = &v2["plugins"][0];
        assert_eq!(dice["updated"], "2026-10-02");
        assert_eq!(dice["versions"][1]["api"], "1.1");
        assert_eq!(dice["versions"][1]["min_mumcord"], "0.7");
        assert_eq!(dice["versions"][1]["origins"][0], "https://api.example");
        assert_eq!(v2["plugins"][1]["id"], "stage");

        // The client reads the v1.1 index too and picks 1.1.0 only from 0.7 on (AC-9.26 unit).
        let v1_1 = Index::parse(
            &fs::read(out.path().join("v1.1/index.json")).expect("test step"),
            &IndexKind::Official,
        )
        .expect("client reads v1.1");
        let dice = v1_1.plugin("dice").expect("test step");
        let chosen = |app: &str| match dice.choose(&semver::Version::parse(app).expect("test step"))
        {
            Choice::Runnable(v) => v.version.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(chosen("0.7.0"), "1.1.0");
        assert_eq!(chosen("0.9.1"), "1.1.0");
    }

    #[test]
    fn generates_index_media_and_permissions() {
        let repo = tempfile::tempdir().expect("test step");
        write_plugin(repo.path(), "dice", &["1.0.0", "1.2.0"]);
        let out = tempfile::tempdir().expect("test step");
        run(Args {
            repo: repo.path().to_owned(),
            out: out.path().to_owned(),
            sequence: Some(7),
            base_url: "https://example.org/v1".into(),
            package_url: PACKAGE_URL.into(),
        })
        .expect("test step");

        let v1 = out.path().join("v1");
        let index = Index::parse(
            &fs::read(v1.join("index.json")).expect("test step"),
            &IndexKind::Official,
        )
        .expect("client accepts the index");
        assert_eq!(index.sequence, 7);
        let dice = index.plugin("dice").expect("listed");
        assert_eq!(dice.latest().version, "1.2.0");
        assert_eq!(dice.updated, "2026-10-02");
        assert_eq!(dice.license.as_deref(), Some("MIT"));
        assert_eq!(dice.reviewed_on.as_deref(), Some("2026-10-20"));
        assert_eq!(dice.name["de"], "Würfel");
        assert_eq!(
            dice.icon.as_deref(),
            Some("https://example.org/v1/media/dice/icon-128.png")
        );
        assert_eq!(
            dice.latest().url,
            "https://github.com/Mumcord/plugins/releases/download/dice-v1.2.0/dice-1.2.0.mcplugin"
        );
        assert!(v1.join("media/dice/shot-1.png").exists());
        assert!(v1.join("permissions.json").exists());
        assert!(out.path().join("v1.1/permissions.json").exists());
        let v2: serde_json::Value = serde_json::from_slice(
            &fs::read(out.path().join("v1.1/index.json")).expect("test step"),
        )
        .expect("json");
        assert_eq!(v2["format"], 2);
        assert_eq!(v2["sequence"], 7);
        assert_eq!(v2["plugins"][0]["versions"][1]["api"], "1.0");
        assert_eq!(
            v2["plugins"][0]["icon"],
            dice.icon.clone().expect("icon").as_str()
        );
        let revoked: serde_json::Value =
            serde_json::from_slice(&fs::read(v1.join("revoked.json")).expect("test step"))
                .expect("json");
        assert_eq!(revoked["sequence"], 7);
    }
}
