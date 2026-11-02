//! The `.mcplugin` package (FR-6.10): a zip with `manifest.json`, `plugin.wasm`,
//! `i18n/<lang>.json` (English required), optional `assets/` and, for official plugins with
//! `ui.webview`, `web/`. Packages come from strangers, so every limit is checked while reading,
//! before anything is written to disk; SVG assets are sanitised on the way (FR-6.42).
//!
//! Integrity: [`Digest`] is the sha256 and size the registry index publishes for each package.
//! Signature checks (FR-6.50) plug in through [`PackageVerifier`], which runs before the package
//! is even opened.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use sha2::{Digest as _, Sha256};
use zip::CompressionMethod;

use crate::i18n::MAX_I18N_BYTES;
use crate::manifest::{
    Manifest, ManifestError, RESERVED_ID_PREFIXES, Source, is_language_tag, relative_path,
};

pub const MANIFEST_FILE: &str = "manifest.json";
pub const WASM_FILE: &str = "plugin.wasm";
pub const ENGLISH_FILE: &str = "i18n/en.json";
/// Official plugins must ship these (FR-6.10).
pub const OFFICIAL_FILES: &[&str] = &["README.md", "LICENSE"];
pub const MAX_PACKAGE_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_UNPACKED_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_FILES: usize = 200;
pub const MAX_ASSET_BYTES: u64 = 512 * 1024;
/// Official plugins may bundle a whole web app with its data and images (FR-6.10, addendum
/// 2026-11-01); third-party, repository and developer plugins keep the limits above.
pub const MAX_OFFICIAL_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_OFFICIAL_UNPACKED_BYTES: u64 = 96 * 1024 * 1024;
pub const MAX_OFFICIAL_FILES: usize = 10_000;
/// Largest single file under `web/` of an official plugin. Other plugins are only held to the
/// unpacked total.
pub const MAX_OFFICIAL_WEB_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// The packed size limit for packages from `source`.
pub fn max_package_bytes(source: Source) -> u64 {
    if source == Source::Official {
        MAX_OFFICIAL_PACKAGE_BYTES
    } else {
        MAX_PACKAGE_BYTES
    }
}

fn max_unpacked_bytes(source: Source) -> u64 {
    if source == Source::Official {
        MAX_OFFICIAL_UNPACKED_BYTES
    } else {
        MAX_UNPACKED_BYTES
    }
}

const ASSET_EXTENSIONS: &[&str] = &[".png", ".webp", ".svg"];
/// HTML, CSS, JS and images for the sealed web view (FR-6.46).
const WEB_EXTENSIONS: &[&str] = &[
    ".html", ".css", ".js", ".json", ".wasm", ".woff2", ".png", ".webp", ".svg",
];
const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

/// sha256 and size of a package file, as listed in the registry index.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Digest {
    pub sha256: [u8; 32],
    pub size: u64,
}

impl Digest {
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            sha256: Sha256::digest(bytes).into(),
            size: bytes.len() as u64,
        }
    }

    /// Lowercase hex, the form used in the index.
    pub fn sha256_hex(&self) -> String {
        self.sha256.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sha256:{} ({} bytes)", self.sha256_hex(), self.size)
    }
}

/// Decides whether package bytes may be installed. The registry flow checks the index digest
/// ([`ExpectedDigest`]); signing (#276) adds author and repository signatures as further
/// verifiers.
pub trait PackageVerifier {
    fn verify(&self, package: &[u8], digest: &Digest) -> Result<(), VerifyError>;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct VerifyError(pub String);

/// The digest a trusted source (the signed index) promised for this package.
pub struct ExpectedDigest(pub Digest);

impl PackageVerifier for ExpectedDigest {
    fn verify(&self, _package: &[u8], digest: &Digest) -> Result<(), VerifyError> {
        if *digest == self.0 {
            Ok(())
        } else {
            Err(VerifyError(format!(
                "the download does not match the index: expected {:?}, got {digest:?}",
                self.0
            )))
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("The package is larger than {0} MiB")]
    TooLarge(u64),
    #[error("The package is not a valid zip file: {0}")]
    Zip(String),
    #[error("The package has more than {0} files")]
    TooManyFiles(usize),
    #[error("The package unpacks to more than {0} MiB")]
    UnpackedTooLarge(u64),
    #[error("The web file \"{0}\" is larger than 16 MiB")]
    WebFileTooLarge(String),
    #[error("\"{0}\" is not allowed in a package (only plain files with safe relative paths)")]
    BadEntry(String),
    #[error("\"{0}\" appears twice in the package (names may not differ only in case)")]
    DuplicateEntry(String),
    #[error("\"{0}\" uses encryption or a compression other than stored or deflate")]
    UnsupportedEntry(String),
    #[error("\"{0}\" does not belong in a plugin package")]
    UnexpectedFile(String),
    #[error("The asset \"{0}\" is not a valid PNG, WebP or SVG image of at most 512 KiB")]
    BadAsset(String),
    #[error("The translation file \"{0}\" is larger than 64 KiB")]
    BadTranslation(String),
    #[error("The package has no {0}")]
    MissingFile(String),
    #[error("Web content needs the permission \"ui.webview\"")]
    WebWithoutWebview,
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("The package could not be verified: {0}")]
    Verification(#[from] VerifyError),
    #[error(
        "The plugin id \"{0}\" belongs to an official plugin and cannot be installed from this source"
    )]
    OfficialId(String),
    #[error("The plugin id \"{0}\" uses a prefix reserved for Mumcord")]
    ReservedId(String),
    #[error("Could not write the plugin files: {0}")]
    Io(#[from] std::io::Error),
}

/// A package read into memory and fully checked.
#[derive(Debug)]
pub struct Package {
    pub manifest: Manifest,
    pub digest: Digest,
    files: BTreeMap<String, Vec<u8>>,
}

impl Package {
    /// Reads and checks a package from `source`. Nothing is written to disk.
    pub fn read(bytes: &[u8], source: Source) -> Result<Self, PackageError> {
        let max_package = max_package_bytes(source);
        if bytes.len() as u64 > max_package {
            return Err(PackageError::TooLarge(max_package / 1024 / 1024));
        }
        let max_unpacked = max_unpacked_bytes(source);
        let digest = Digest::of(bytes);
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| PackageError::Zip(e.to_string()))?;
        let max_files = if source == Source::Official {
            MAX_OFFICIAL_FILES
        } else {
            MAX_FILES
        };
        if archive.len() > max_files {
            return Err(PackageError::TooManyFiles(max_files));
        }

        let mut files = BTreeMap::new();
        // Case-insensitive file systems (Windows, macOS) would merge names that differ only in case.
        let mut folded = BTreeSet::new();
        let mut unpacked = 0u64;
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|e| PackageError::Zip(e.to_string()))?;
            let name = entry
                .name()
                .map_err(|e| PackageError::Zip(e.to_string()))?
                .into_owned();
            if entry.is_dir() {
                relative_path(name.trim_end_matches('/'))
                    .map_err(|_| PackageError::BadEntry(name.clone()))?;
                continue;
            }
            if entry.is_symlink() || !entry.is_file() || relative_path(&name).is_err() {
                return Err(PackageError::BadEntry(name));
            }
            if entry.encrypted()
                || !matches!(
                    entry.compression(),
                    CompressionMethod::Stored | CompressionMethod::Deflated
                )
            {
                return Err(PackageError::UnsupportedEntry(name));
            }
            if !folded.insert(name.to_lowercase()) {
                return Err(PackageError::DuplicateEntry(name));
            }
            let kind = FileKind::of(&name)?;
            // The size in the zip header is not trusted: read at most what is still allowed.
            let budget = max_unpacked - unpacked;
            let mut data = Vec::new();
            (&mut entry)
                .take(budget + 1)
                .read_to_end(&mut data)
                .map_err(|e| PackageError::Zip(e.to_string()))?;
            unpacked += data.len() as u64;
            if unpacked > max_unpacked {
                return Err(PackageError::UnpackedTooLarge(max_unpacked / 1024 / 1024));
            }
            let data = match kind {
                FileKind::Asset if data.len() as u64 > MAX_ASSET_BYTES => {
                    return Err(PackageError::BadAsset(name));
                }
                FileKind::Web
                    if source == Source::Official
                        && data.len() as u64 > MAX_OFFICIAL_WEB_FILE_BYTES =>
                {
                    return Err(PackageError::WebFileTooLarge(name));
                }
                FileKind::Translation if data.len() as u64 > MAX_I18N_BYTES => {
                    return Err(PackageError::BadTranslation(name));
                }
                FileKind::Asset if name.ends_with(".svg") => {
                    sanitize_svg(&data).ok_or_else(|| PackageError::BadAsset(name.clone()))?
                }
                _ => data,
            };
            files.insert(name, data);
        }

        let manifest = Manifest::parse(
            files
                .get(MANIFEST_FILE)
                .ok_or_else(|| PackageError::MissingFile(MANIFEST_FILE.into()))?,
        )?;
        manifest.validate(source)?;
        let mut required = vec![WASM_FILE, ENGLISH_FILE];
        if source == Source::Official {
            required.extend(OFFICIAL_FILES);
        }
        for path in required.into_iter().chain(manifest.icon.as_deref()) {
            if !files.contains_key(path) {
                return Err(PackageError::MissingFile(path.to_owned()));
            }
        }
        if files.keys().any(|name| name.starts_with("web/"))
            && !manifest.has_permission("ui.webview")
        {
            return Err(PackageError::WebWithoutWebview);
        }
        Ok(Self {
            manifest,
            digest,
            files,
        })
    }

    /// Verifies, reads and checks a package in one step.
    pub fn read_verified(
        bytes: &[u8],
        source: Source,
        verifier: &dyn PackageVerifier,
    ) -> Result<Self, PackageError> {
        verifier.verify(bytes, &Digest::of(bytes))?;
        Self::read(bytes, source)
    }

    pub fn file_names(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    /// A file as it will be installed (SVG assets already sanitised).
    pub fn file(&self, name: &str) -> Option<&[u8]> {
        self.files.get(name).map(Vec::as_slice)
    }

    /// Writes the package to `<plugins_dir>/<id>/<version>/` (FR-6.15): unpacked into a temporary
    /// folder next to it and renamed into place only once complete, so a failed install leaves
    /// nothing half-written and other installed versions stay untouched. Returns the version
    /// directory, which [`crate::PluginHost::load`] reads.
    pub fn install(&self, plugins_dir: &Path) -> Result<PathBuf, PackageError> {
        let plugin_dir = plugins_dir.join(&self.manifest.id);
        std::fs::create_dir_all(&plugin_dir)?;
        let version = self.manifest.version.to_string();
        // Removed again on any error.
        let staging = tempfile::Builder::new()
            .prefix(&format!(".{version}.installing-"))
            .tempdir_in(&plugin_dir)?;
        for (name, data) in &self.files {
            let path = staging.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, data)?;
        }
        let target = plugin_dir.join(&version);
        // Reinstalling the same version: move the old copy aside first so the new one appears
        // in a single rename.
        let replaced = if target.exists() {
            let old = tempfile::Builder::new()
                .prefix(&format!(".{version}.replaced-"))
                .tempdir_in(&plugin_dir)?;
            let old_path = old.path().join("old");
            std::fs::rename(&target, &old_path)?;
            Some(old)
        } else {
            None
        };
        std::fs::rename(staging.keep(), &target)?;
        drop(replaced);
        Ok(target)
    }
}

/// What a package entry is, by path. Anything not listed in FR-6.10 is refused.
enum FileKind {
    Fixed,
    Translation,
    Asset,
    Web,
}

impl FileKind {
    fn of(name: &str) -> Result<Self, PackageError> {
        let has_ext = |exts: &[&str]| exts.iter().any(|ext| name.ends_with(ext));
        if name == MANIFEST_FILE || name == WASM_FILE || OFFICIAL_FILES.contains(&name) {
            Ok(Self::Fixed)
        } else if let Some(file) = name.strip_prefix("i18n/") {
            file.strip_suffix(".json")
                .filter(|tag| is_language_tag(tag))
                .map(|_| Self::Translation)
                .ok_or_else(|| PackageError::UnexpectedFile(name.to_owned()))
        } else if name.starts_with("assets/") {
            if has_ext(ASSET_EXTENSIONS) {
                Ok(Self::Asset)
            } else {
                Err(PackageError::BadAsset(name.to_owned()))
            }
        } else if name.starts_with("web/") && has_ext(WEB_EXTENSIONS) {
            Ok(Self::Web)
        } else {
            Err(PackageError::UnexpectedFile(name.to_owned()))
        }
    }
}

/// SVG elements and attributes that draw shapes, gradients and text. Nothing that runs script,
/// loads other resources (`href`, `use`, `image`, `foreignObject`) or carries CSS.
const SVG_TAGS: &[&str] = &[
    "svg",
    "g",
    "defs",
    "title",
    "desc",
    "path",
    "circle",
    "ellipse",
    "rect",
    "line",
    "polyline",
    "polygon",
    "text",
    "tspan",
    "linearGradient",
    "radialGradient",
    "stop",
    "clipPath",
    "mask",
];

const SVG_ATTRIBUTES: &[&str] = &[
    "xmlns",
    "id",
    "viewBox",
    "preserveAspectRatio",
    "width",
    "height",
    "x",
    "y",
    "x1",
    "y1",
    "x2",
    "y2",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "fx",
    "fy",
    "dx",
    "dy",
    "d",
    "points",
    "pathLength",
    "transform",
    "opacity",
    "fill",
    "fill-opacity",
    "fill-rule",
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-opacity",
    "offset",
    "stop-color",
    "stop-opacity",
    "gradientUnits",
    "gradientTransform",
    "spreadMethod",
    "clip-path",
    "clip-rule",
    "clipPathUnits",
    "mask",
    "maskUnits",
    "maskContentUnits",
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "text-anchor",
    "dominant-baseline",
    "letter-spacing",
    "visibility",
    "display",
];

static SVG_CLEANER: LazyLock<ammonia::Builder<'static>> = LazyLock::new(|| {
    let mut builder = ammonia::Builder::empty();
    builder
        .add_tags(SVG_TAGS)
        .add_generic_attributes(SVG_ATTRIBUTES)
        .url_schemes(std::collections::HashSet::new());
    builder
});

/// Keeps only the allow-listed SVG elements and attributes. `None` if nothing usable is left
/// (not UTF-8, or no `<svg>` root).
fn sanitize_svg(data: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(data).ok()?;
    let clean = SVG_CLEANER.clean(text).to_string();
    let clean = clean.trim();
    let rest = clean.strip_prefix("<svg")?;
    if !rest.starts_with([' ', '>']) || !clean.ends_with("</svg>") {
        return None;
    }
    // An `<img>` only renders SVG that declares its namespace.
    let root = &rest[..rest.find('>')?];
    Some(if root.contains("xmlns=") {
        clean.as_bytes().to_vec()
    } else {
        format!("<svg xmlns=\"{SVG_NAMESPACE}\"{rest}").into_bytes()
    })
}

/// A sideloaded package or one from a third-party repository may not take an official plugin's
/// id or a reserved prefix, so it can never pose as (or shadow) an official plugin.
pub fn check_unofficial_id(
    id: &str,
    is_official_id: impl Fn(&str) -> bool,
) -> Result<(), PackageError> {
    if is_official_id(id) {
        Err(PackageError::OfficialId(id.to_owned()))
    } else if RESERVED_ID_PREFIXES
        .iter()
        .any(|prefix| id.starts_with(prefix))
    {
        Err(PackageError::ReservedId(id.to_owned()))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(svg: &str) -> String {
        String::from_utf8(sanitize_svg(svg.as_bytes()).expect("usable svg")).expect("utf-8")
    }

    #[test]
    fn keeps_drawing_and_namespace() {
        let svg = r##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><linearGradient id="g"><stop offset="0" stop-color="#fff"/></linearGradient><path d="M0 0h16v16z" fill="url(#g)"/></svg>"##;
        let out = clean(svg);
        assert!(
            out.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""),
            "{out}"
        );
        assert!(out.contains("viewBox=\"0 0 16 16\""), "{out}");
        assert!(out.contains("<linearGradient id=\"g\">"), "{out}");
        assert!(out.contains("<path d=\"M0 0h16v16z\""), "{out}");
    }

    #[test]
    fn strips_scripts_handlers_links_and_styles() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"><script>alert(2)</script><style>*{background:url(https://x.test)}</style><a href="https://x.test"><circle r="2"/></a><image href="https://x.test/i.png"/><use href="https://x.test/s.svg#a"/><foreignObject><div>hi</div></foreignObject><rect style="fill:red" width="3"/></svg>"#;
        let out = clean(svg);
        for bad in [
            "onload",
            "script",
            "alert",
            "style",
            "href",
            "x.test",
            "image",
            "use",
            "foreignObject",
            "<div",
        ] {
            assert!(!out.contains(bad), "{bad} left in {out}");
        }
        assert!(out.contains("<circle r=\"2\">"), "{out}");
        assert!(out.contains("<rect width=\"3\">"), "{out}");
    }

    #[test]
    fn adds_a_missing_namespace_and_refuses_non_svg() {
        assert!(
            clean("<svg><g></g></svg>").starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\">")
        );
        assert!(sanitize_svg(b"<html><body>no</body></html>").is_none());
        assert!(sanitize_svg(b"\xff\xfe").is_none());
        assert!(sanitize_svg(b"<script>x</script>").is_none());
    }
}
