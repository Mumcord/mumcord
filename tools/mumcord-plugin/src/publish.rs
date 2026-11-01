//! `publish-pr`: adds a built and signed package to a checkout of the plugins registry
//! (`plugins/<id>/plugin.toml` for a new plugin, `plugins/<id>/versions/<ver>.toml`). With
//! `--open` it also commits on a new branch, pushes and opens the pull request with `gh`;
//! without it, it prints those commands.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use mumcord_plugins::manifest::Source;
use mumcord_plugins::package::Package;

use crate::date;
use crate::registry::{PackageInfo, api_major};
use crate::sign;
use crate::source::{self, PLUGIN_FILE, PluginFile, SourceRef, VersionFile};

#[derive(clap::Args)]
pub struct Args {
    /// The built package; its signature `<package>.minisig` must sit next to it.
    pub package: PathBuf,
    /// Local checkout of the plugins registry (`Mumcord/plugins` or a fork).
    #[arg(long)]
    pub plugins_repo: PathBuf,
    /// Author public key (base64 line or `.pub` file) the package is signed with.
    #[arg(long)]
    pub author_key: String,
    /// Git checkout of the plugin's source, for the reproducible rebuild (default: current folder).
    #[arg(long, default_value = ".")]
    pub source_dir: PathBuf,
    /// Commit, push and open the pull request (needs `git` and `gh`).
    #[arg(long)]
    pub open: bool,
}

pub fn run(args: Args) -> Result<()> {
    let bytes = fs::read(&args.package)
        .with_context(|| format!("could not read {}", args.package.display()))?;
    // The official registry applies the official rules (README.md and LICENSE, FR-6.10).
    let package =
        Package::read(&bytes, Source::Official).context("the package would be refused")?;
    let signature_path = sign::signature_path(&args.package);
    let signature = fs::read_to_string(&signature_path).with_context(|| {
        format!(
            "sign the package first: {} is missing",
            signature_path.display()
        )
    })?;
    let author_key = sign::read_public(&args.author_key)?;
    sign::verify(&author_key, &bytes, &signature).context("author signature check failed")?;
    let source = source_ref(&args.source_dir)?;

    let info = PackageInfo::read(bytes)?;
    let manifest = &info.manifest;
    let (id, version) = (manifest.id.clone(), manifest.version.to_string());
    let version_path = source::version_path(&args.plugins_repo, &id, &version);
    if version_path.exists() {
        bail!(
            "{} already exists; publish a new version number",
            version_path.display()
        );
    }
    let plugin_path = source::plugin_dir(&args.plugins_repo, &id).join(PLUGIN_FILE);
    let mut changed = vec![version_path.clone()];
    if plugin_path.exists() {
        let mut plugin: PluginFile = source::read_toml(&plugin_path)?;
        if !plugin.author_keys.contains(&author_key) {
            plugin.author_keys.push(author_key.clone());
            fs::write(&plugin_path, toml::to_string(&plugin)?)?;
            changed.push(plugin_path.clone());
            println!("note: a new author key needs maintainer review");
        }
    } else {
        let plugin = new_plugin_file(&package, author_key);
        fs::create_dir_all(source::plugin_dir(&args.plugins_repo, &id))?;
        fs::write(&plugin_path, toml::to_string(&plugin)?)?;
        changed.push(plugin_path.clone());
    }
    let file = VersionFile {
        version: version.clone(),
        api: api_major(&manifest.api)?,
        size: info.bytes.len() as u64,
        sha256: info.sha256.clone(),
        permissions: info.permissions(),
        published: date::today(),
        signature,
        source,
    };
    fs::create_dir_all(version_path.parent().context("no parent")?)?;
    fs::write(&version_path, toml::to_string(&file)?)?;
    for path in &changed {
        println!("wrote {}", path.display());
    }

    let branch = format!("publish/{id}-{version}");
    let message = format!("feat({id}): publish {version}");
    let relative: Vec<String> = changed
        .iter()
        .map(|p| {
            p.strip_prefix(&args.plugins_repo)
                .unwrap_or(p)
                .display()
                .to_string()
        })
        .collect();
    let steps: Vec<Vec<String>> = vec![
        vec!["git".into(), "switch".into(), "-c".into(), branch.clone()],
        [vec!["git".into(), "add".into(), "--".into()], relative].concat(),
        vec!["git".into(), "commit".into(), "-m".into(), message],
        vec![
            "git".into(),
            "push".into(),
            "-u".into(),
            "origin".into(),
            branch,
        ],
        vec!["gh".into(), "pr".into(), "create".into(), "--fill".into()],
    ];
    if args.open {
        for step in &steps {
            run_in(&args.plugins_repo, step)?;
        }
    } else {
        println!("next, in {}:", args.plugins_repo.display());
        for step in &steps {
            println!("  {}", step.join(" "));
        }
    }
    Ok(())
}

/// Name and summary per language come from the package's `i18n/<lang>.json` (FR-6.80).
fn new_plugin_file(package: &Package, author_key: String) -> PluginFile {
    let manifest = &package.manifest;
    let mut name = BTreeMap::from([("en".to_owned(), manifest.name.clone())]);
    let mut summary = BTreeMap::from([("en".to_owned(), manifest.description.clone())]);
    for file in package.file_names() {
        let Some(lang) = file
            .strip_prefix("i18n/")
            .and_then(|f| f.strip_suffix(".json"))
        else {
            continue;
        };
        let Some(table) = package
            .file(file)
            .and_then(|bytes| serde_json::from_slice::<BTreeMap<String, String>>(bytes).ok())
        else {
            continue;
        };
        if let Some(text) = table.get("manifest.name") {
            name.insert(lang.to_owned(), text.clone());
        }
        if let Some(text) = table.get("manifest.description") {
            summary.insert(lang.to_owned(), text.clone());
        }
    }
    PluginFile {
        name,
        summary,
        categories: manifest.categories.clone(),
        authors: manifest.authors.clone(),
        author_keys: vec![author_key],
        reviewed: false,
        description: None,
        icon: None,
        screenshots: Vec::new(),
    }
}

fn source_ref(dir: &Path) -> Result<SourceRef> {
    let git = |args: &[&str]| -> Result<String> {
        let output = Command::new("git").arg("-C").arg(dir).args(args).output()?;
        if !output.status.success() {
            bail!(
                "{} is not a git checkout with an origin remote (needed for the reproducible rebuild)",
                dir.display()
            );
        }
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    };
    if !git(&["status", "--porcelain"])?.is_empty() {
        println!(
            "warning: {} has uncommitted changes; the rebuild uses the committed source",
            dir.display()
        );
    }
    Ok(SourceRef {
        repository: git(&["remote", "get-url", "origin"])?,
        commit: git(&["rev-parse", "HEAD"])?,
        path: git(&["rev-parse", "--show-prefix"])?
            .trim_end_matches('/')
            .to_owned(),
    })
}

fn run_in(dir: &Path, step: &[String]) -> Result<()> {
    let (program, args) = step.split_first().context("empty command")?;
    let status = Command::new(program)
        .current_dir(dir)
        .args(args)
        .status()
        .with_context(|| format!("could not run {program}"))?;
    if !status.success() {
        bail!("`{}` failed", step.join(" "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::tests::package;
    use crate::sign::Signer;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.org"])
            .args(args)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn writes_registry_files_for_a_signed_package() {
        let work = tempfile::tempdir().expect("test step");
        let source_dir = work.path().join("src");
        fs::create_dir_all(&source_dir).expect("test step");
        git(&source_dir, &["init", "-q"]);
        git(
            &source_dir,
            &["remote", "add", "origin", "https://example.org/dice.git"],
        );
        git(
            &source_dir,
            &["commit", "-q", "--allow-empty", "-m", "init"],
        );

        let signer = Signer::generate().expect("test step");
        let package_path = work.path().join("dice-1.0.0.mcplugin");
        let bytes = package("dice", "1.0.0", &["chat.send"]);
        fs::write(&package_path, &bytes).expect("test step");
        fs::write(
            sign::signature_path(&package_path),
            signer.sign(&bytes).expect("test step"),
        )
        .expect("test step");
        let repo = work.path().join("registry");
        let args = |key: String| Args {
            package: package_path.clone(),
            plugins_repo: repo.clone(),
            author_key: key,
            source_dir: source_dir.clone(),
            open: false,
        };

        let stranger = Signer::generate().expect("test step").public_line();
        assert!(
            run(args(stranger)).is_err(),
            "signature must match the author key"
        );
        run(args(signer.public_line())).expect("test step");
        assert!(
            run(args(signer.public_line())).is_err(),
            "a version is never overwritten"
        );

        let plugins = source::load_plugins(&repo).expect("readable by site-data");
        let version = &plugins[0].versions[0];
        assert_eq!(
            version.sha256,
            mumcord_plugins::package::Digest::of(&bytes).sha256_hex()
        );
        assert_eq!(version.source.repository, "https://example.org/dice.git");
        assert_eq!(version.source.commit.len(), 40);
        assert_eq!(plugins[0].plugin.author_keys, [signer.public_line()]);
        assert!(!plugins[0].plugin.reviewed);
    }
}
