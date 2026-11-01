//! Repository keys (FR-6.49, FR-6.53) and rollback protection (FR-6.55).

use minisign_verify::{PublicKey, Signature};

use super::VerifyFailure;

/// The official registry keys (minisign, ed25519). Only the public halves live here; the active
/// secret key is held by the protected `publish` environment of `Mumcord/plugins`, the backup
/// one offline by the owner. Id 1CB49831E26E61AD.
const OFFICIAL_ACTIVE_KEY: &str = "RWStYW7iMZi0HAbHe+TL+GQ4NH70MYE6hJMKZDGGvpbqucV6UpbYwVXz";
/// Id 7989D354F5AE80C5. Lets the owner rotate keys without a client that trusts nothing.
const OFFICIAL_BACKUP_KEY: &str = "RWTFgK71VNOJedXEUd989UDCiNA8RTKUKCVlNw/o6mtwIJ/IHYJSnf2J";

/// Extra repository key for test registries; read only by development builds.
pub const TEST_KEY_ENV: &str = "MUMCORD_PLUGIN_REGISTRY_KEY";

/// The keys a signed registry file may be signed with; any one of them is enough.
pub struct RegistryKeys(Vec<PublicKey>);

impl RegistryKeys {
    /// The official keys. Development builds also trust a key from [`TEST_KEY_ENV`] so tests can
    /// sign their own fixture registry; release builds never read it.
    pub fn official() -> Self {
        #[cfg(debug_assertions)]
        let test_key = std::env::var(TEST_KEY_ENV).ok();
        #[cfg(not(debug_assertions))]
        let test_key: Option<String> = None;
        Self::official_with(test_key.as_deref())
    }

    fn official_with(test_key: Option<&str>) -> Self {
        let mut keys: Vec<PublicKey> = [OFFICIAL_ACTIVE_KEY, OFFICIAL_BACKUP_KEY]
            .into_iter()
            .map(|key| PublicKey::from_base64(key).expect("compiled-in key is valid"))
            .collect();
        if let Some(text) = test_key {
            match parse_key(text) {
                Some(key) => keys.push(key),
                None => tracing::warn!("{TEST_KEY_ENV} is not a minisign public key; ignored"),
            }
        }
        Self(keys)
    }

    /// The key pinned for a third-party repository (FR-6.58). `None` if it is not a minisign
    /// public key.
    pub fn pinned(key: &str) -> Option<Self> {
        parse_key(key).map(|key| Self(vec![key]))
    }

    /// Checks a detached minisign signature (the `.minisig` file) over `data`, e.g. `index.json`
    /// or `revoked.json`.
    pub fn verify(&self, data: &[u8], signature: &str) -> Result<(), VerifyFailure> {
        let signature = Signature::decode(signature).map_err(|_| VerifyFailure::Signature)?;
        if self
            .0
            .iter()
            .any(|key| key.verify(data, &signature, false).is_ok())
        {
            Ok(())
        } else {
            Err(VerifyFailure::Signature)
        }
    }
}

/// Accepts the bare base64 key line or the whole `.pub` file.
pub(super) fn parse_key(text: &str) -> Option<PublicKey> {
    let text = text.trim();
    PublicKey::from_base64(text)
        .or_else(|_| PublicKey::decode(text))
        .ok()
}

/// Rollback protection: an index whose `sequence` is lower than the last one seen is refused.
/// The same sequence is fine (a re-fetch of the current index).
pub fn check_sequence(sequence: u64, last_seen: Option<u64>) -> Result<(), VerifyFailure> {
    match last_seen {
        Some(last) if sequence < last => Err(VerifyFailure::Rollback),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_keys_parse_and_test_key_is_optional() {
        assert_eq!(RegistryKeys::official_with(None).0.len(), 2);
        assert_eq!(
            RegistryKeys::official_with(Some(OFFICIAL_ACTIVE_KEY))
                .0
                .len(),
            3
        );
        assert_eq!(RegistryKeys::official_with(Some("garbage")).0.len(), 2);
    }

    #[test]
    fn sequence_never_goes_back() {
        assert!(check_sequence(5, None).is_ok());
        assert!(check_sequence(5, Some(5)).is_ok());
        assert!(check_sequence(6, Some(5)).is_ok());
        assert!(matches!(
            check_sequence(4, Some(5)),
            Err(VerifyFailure::Rollback)
        ));
    }
}
