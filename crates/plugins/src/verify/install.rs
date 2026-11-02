//! Install checks (FR-6.50) and official status (FR-6.51).

use super::VerifyFailure;
use super::keys::parse_key;
use crate::{Digest, Package, Source};

/// The author signature is published next to the package as `<id>-<ver>.mcplugin.minisig`: a
/// detached minisign signature over the package bytes.
pub const AUTHOR_SIGNATURE_SUFFIX: &str = ".minisig";

/// What a signed index promises about one plugin version. The registry client fills it from
/// whatever index format it parses; only the repository signature makes it trustworthy.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub id: String,
    pub version: String,
    pub digest: Digest,
    /// Author keys pinned for this plugin id (minisign public keys, base64).
    pub author_keys: Vec<String>,
    /// Every permission (required and optional) the reviewed version may ask for.
    pub permissions: Vec<String>,
}

/// A package that passed all checks.
pub struct Verified {
    pub package: Package,
    /// The pinned author key that signed it; stored with the plugin for update checks (FR-6.67).
    pub author_key: String,
    /// FR-6.51: only packages from the official index, after every check passed.
    pub official: bool,
}

/// Runs the install checks in the order FR-6.50 fixes and stops at the first failure: size,
/// SHA-256 against the index, author signature, then manifest vs. index entry (id, version,
/// permissions). `source` is where the index came from; its signature was checked when it was
/// fetched. The package is only opened once the bytes are known to be the signed ones.
pub fn verify_package(
    bytes: &[u8],
    entry: &IndexEntry,
    author_signature: &str,
    source: Source,
) -> Result<Verified, VerifyFailure> {
    let digest = Digest::of(bytes);
    if digest != entry.digest {
        return Err(VerifyFailure::Mismatch);
    }

    let signature = minisign_verify::Signature::decode(author_signature)
        .map_err(|_| VerifyFailure::Signature)?;
    let author_key = entry
        .author_keys
        .iter()
        .find(|key| parse_key(key).is_some_and(|key| key.verify(bytes, &signature, false).is_ok()))
        .ok_or(VerifyFailure::Signature)?
        .clone();

    let package = Package::read(bytes, source)?;
    let manifest = &package.manifest;
    if manifest.id != entry.id || manifest.version.to_string() != entry.version {
        return Err(VerifyFailure::Mismatch);
    }
    if manifest
        .all_permissions()
        .any(|p| !entry.permissions.iter().any(|listed| listed == p))
    {
        return Err(VerifyFailure::Permissions);
    }

    Ok(Verified {
        package,
        author_key,
        official: source == Source::Official,
    })
}
