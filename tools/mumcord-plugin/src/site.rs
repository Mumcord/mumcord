//! `site-data`: builds the registry website data from a `Mumcord/plugins` checkout (FR-6.80):
//! `v1/index.json`, `v1/revoked.json`, `v1/permissions.json` and `v1/media/<id>/…`. The output
//! is unsigned; the publish workflow signs index and revocation list with `mumcord-plugin sign`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mumcord_plugins::registry::{Index, IndexKind, IndexPlugin, IndexVersion, Screenshot};

use crate::date;
use crate::registry::{self, package_file_name};
use crate::source::{self, SourcePlugin};

const BASE_URL: &str = "https://mumcord.github.io/plugins/v1/";
const PACKAGE_URL: &str =
    "https://github.com/Mumcord/plugins/releases/download/{id}-v{version}/{file}";

#[derive(clap::Args)]
pub struct Args {
    /// Checkout of the plugins registry.
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    /// Output folder; `v1/` is created inside.
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
    fs::create_dir_all(&out)?;

    let mut plugins = Vec::new();
    for plugin in source::load_plugins(&args.repo)? {
        plugins.push(index_plugin(&plugin, &base, &args.package_url, &out)?);
    }
    let index = registry::index_json(sequence, None, None, &plugins)?;
    // The client drops bad entries with a note; a published index must have none.
    let parsed =
        Index::parse(&index, &IndexKind::Official).context("the generated index is invalid")?;
    if let Some(ignored) = parsed.ignored.first() {
        bail!(
            "plugin {} would be ignored by Mumcord: {:?}",
            ignored.id,
            ignored.reason
        );
    }
    fs::write(out.join("index.json"), index)?;
    let revoked = source::load_revoked(&args.repo)?;
    fs::write(
        out.join("revoked.json"),
        registry::revoked_json(sequence, &revoked)?,
    )?;
    fs::write(out.join("permissions.json"), registry::permissions_json()?)?;
    println!(
        "{} plugins, sequence {sequence}, written to {}",
        plugins.len(),
        out.display()
    );
    Ok(())
}

fn index_plugin(
    source: &SourcePlugin,
    base: &str,
    package_url: &str,
    out: &Path,
) -> Result<IndexPlugin> {
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
    let mut versions: Vec<&source::VersionFile> = source.versions.iter().collect();
    versions.sort_by_cached_key(|v| semver::Version::parse(&v.version).ok());
    let latest = versions.last().context("no versions")?;
    Ok(IndexPlugin {
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
        updated: latest.published.clone(),
        versions: versions
            .iter()
            .map(|v| IndexVersion {
                version: v.version.clone(),
                api: v.api,
                url: package_url
                    .replace("{id}", id)
                    .replace("{version}", &v.version)
                    .replace("{file}", &package_file_name(id, &v.version)),
                size: v.size,
                sha256: v.sha256.clone(),
                permissions: v.permissions.clone(),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::tests::write_plugin;

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
        let revoked: serde_json::Value =
            serde_json::from_slice(&fs::read(v1.join("revoked.json")).expect("test step"))
                .expect("json");
        assert_eq!(revoked["sequence"], 7);
    }
}
