//! The `.mcplugin` package (#269): a zip with `manifest.json`, the component named in `entry`,
//! `i18n/en.json` and optional assets. Packages come from strangers, so every limit is checked
//! while reading, before anything is written to disk.
//!
//! Integrity: [`Digest`] is the sha256 and size the registry index publishes for each package.
//! Signature checks (#276) plug in through [`PackageVerifier`], which runs before the package is
//! even opened.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};
use zip::CompressionMethod;

use crate::manifest::{Manifest, ManifestError, relative_path};

pub const MANIFEST_FILE: &str = "manifest.json";
pub const MAX_PACKAGE_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_UNPACKED_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_FILES: usize = 200;
pub const MAX_ASSET_BYTES: u64 = 512 * 1024;
/// Ids starting with these are kept for plugins published by the Mumcord team.
pub const RESERVED_ID_PREFIXES: &[&str] = &["mumcord-", "core-"];

const ASSET_EXTENSIONS: &[&str] = &[".png", ".webp", ".svg"];

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
    #[error("the package is larger than {} MiB", MAX_PACKAGE_BYTES / 1024 / 1024)]
    TooLarge,
    #[error("the package is not a valid zip file: {0}")]
    Zip(String),
    #[error("the package has more than {MAX_FILES} files")]
    TooManyFiles,
    #[error("the package unpacks to more than {} MiB", MAX_UNPACKED_BYTES / 1024 / 1024)]
    UnpackedTooLarge,
    #[error("{0:?} is not allowed in a package (only plain files with safe relative paths)")]
    BadEntry(String),
    #[error("{0:?} appears twice in the package")]
    DuplicateEntry(String),
    #[error("{0:?} uses encryption or a compression other than stored or deflate")]
    UnsupportedEntry(String),
    #[error("the asset {0:?} is not a PNG, WebP or SVG image of at most 512 KiB")]
    BadAsset(String),
    #[error("the package has no {0}")]
    MissingFile(String),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("the package could not be verified: {0}")]
    Verification(#[from] VerifyError),
    #[error(
        "the plugin id {0:?} belongs to an official plugin and cannot be installed from this source"
    )]
    OfficialId(String),
    #[error("the plugin id {0:?} uses a prefix reserved for Mumcord")]
    ReservedId(String),
    #[error("could not write the plugin files: {0}")]
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
    /// Reads and checks a package. Nothing is written to disk.
    pub fn read(bytes: &[u8]) -> Result<Self, PackageError> {
        if bytes.len() as u64 > MAX_PACKAGE_BYTES {
            return Err(PackageError::TooLarge);
        }
        let digest = Digest::of(bytes);
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| PackageError::Zip(e.to_string()))?;
        if archive.len() > MAX_FILES {
            return Err(PackageError::TooManyFiles);
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
            // The size in the zip header is not trusted: read at most what is still allowed.
            let budget = MAX_UNPACKED_BYTES - unpacked;
            let mut data = Vec::new();
            (&mut entry)
                .take(budget + 1)
                .read_to_end(&mut data)
                .map_err(|e| PackageError::Zip(e.to_string()))?;
            unpacked += data.len() as u64;
            if unpacked > MAX_UNPACKED_BYTES {
                return Err(PackageError::UnpackedTooLarge);
            }
            if name.starts_with("assets/")
                && (data.len() as u64 > MAX_ASSET_BYTES
                    || !ASSET_EXTENSIONS.iter().any(|ext| name.ends_with(ext)))
            {
                return Err(PackageError::BadAsset(name));
            }
            files.insert(name, data);
        }

        let manifest = Manifest::parse(
            files
                .get(MANIFEST_FILE)
                .ok_or_else(|| PackageError::MissingFile(MANIFEST_FILE.into()))?,
        )?;
        let required = [manifest.entry.clone(), "i18n/en.json".to_owned()];
        let referenced = manifest.icons.values().chain(&manifest.screenshots);
        for path in required.iter().chain(referenced) {
            if !files.contains_key(path) {
                return Err(PackageError::MissingFile(path.clone()));
            }
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
        verifier: &dyn PackageVerifier,
    ) -> Result<Self, PackageError> {
        verifier.verify(bytes, &Digest::of(bytes))?;
        Self::read(bytes)
    }

    pub fn file_names(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    /// Writes the package to `<plugins_dir>/<id>/`, replacing an older install only once the new
    /// files are complete. Returns the plugin directory, which [`crate::PluginHost::load`] reads.
    pub fn install(&self, plugins_dir: &Path) -> Result<PathBuf, PackageError> {
        std::fs::create_dir_all(plugins_dir)?;
        // Removed again on any error, so a failed install leaves nothing half-written behind.
        let staging = tempfile::Builder::new()
            .prefix(&format!(".{}.installing-", self.manifest.id))
            .tempdir_in(plugins_dir)?;
        for (name, data) in &self.files {
            let path = staging.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, data)?;
        }
        let target = plugins_dir.join(&self.manifest.id);
        if target.exists() {
            std::fs::remove_dir_all(&target)?;
        }
        std::fs::rename(staging.keep(), &target)?;
        Ok(target)
    }
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
