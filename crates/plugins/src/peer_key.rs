//! `peer.key` (FR-6.18): a stable per-plugin identity for other users that never reveals their
//! certificate. Key = HMAC-SHA256(per-install secret, plugin id ‖ 0x00 ‖ certificate hash), so
//! the same user keeps their key across sessions and restarts, but two plugins cannot match
//! their keys to each other. Users without a certificate get no key.

use std::fmt;
use std::io::Write;
use std::path::Path;

use hmac::{Hmac, KeyInit, Mac};
use rand::TryRng;
use sha2::Sha256;

/// File in the plugins folder that holds the per-install secret.
pub const SECRET_FILE: &str = "peer-key.secret";
const SECRET_BYTES: usize = 32;

/// Computes peer keys from the per-install secret.
#[derive(Clone)]
pub struct PeerKeys {
    secret: [u8; SECRET_BYTES],
}

impl fmt::Debug for PeerKeys {
    // The secret never ends up in a log line.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PeerKeys")
    }
}

impl PeerKeys {
    pub fn new(secret: [u8; SECRET_BYTES]) -> Self {
        Self { secret }
    }

    /// Reads the secret from `<plugins_dir>/peer-key.secret`, or creates it (owner-only on
    /// Unix). A file of the wrong size is an error rather than silently replaced: replacing it
    /// would change every key plugins have stored.
    pub fn load_or_create(plugins_dir: &Path) -> std::io::Result<Self> {
        let path = plugins_dir.join(SECRET_FILE);
        match std::fs::read(&path) {
            Ok(bytes) => {
                let secret = <[u8; SECRET_BYTES]>::try_from(bytes.as_slice()).map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "the plugin peer-key secret has the wrong size",
                    )
                })?;
                Ok(Self::new(secret))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut secret = [0; SECRET_BYTES];
                rand::rngs::SysRng
                    .try_fill_bytes(&mut secret)
                    .map_err(std::io::Error::other)?;
                std::fs::create_dir_all(plugins_dir)?;
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
                let mut file = options.open(&path)?;
                file.write_all(&secret)?;
                file.sync_all()?;
                Ok(Self::new(secret))
            }
            Err(error) => Err(error),
        }
    }

    /// 64 lowercase hex characters, or `None` for a user without a certificate.
    pub fn key(&self, plugin_id: &str, cert_hash: Option<&str>) -> Option<String> {
        let cert_hash = cert_hash.filter(|hash| !hash.is_empty())?;
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&self.secret)
            .expect("HMAC accepts keys of any length");
        mac.update(plugin_id.as_bytes());
        mac.update(&[0]);
        mac.update(cert_hash.as_bytes());
        Some(
            mac.finalize()
                .into_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_stable_per_plugin_and_absent_without_certificate() {
        let keys = PeerKeys::new([7; SECRET_BYTES]);
        let a = keys.key("poll", Some("abc123")).expect("test value");
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
        assert_eq!(keys.key("poll", Some("abc123")).expect("test value"), a);
        assert_ne!(keys.key("dice", Some("abc123")).expect("test value"), a);
        assert_ne!(keys.key("poll", Some("abc124")).expect("test value"), a);
        assert_ne!(
            PeerKeys::new([8; SECRET_BYTES])
                .key("poll", Some("abc123"))
                .expect("test value"),
            a
        );
        assert_eq!(keys.key("poll", None), None);
        assert_eq!(keys.key("poll", Some("")), None);
    }

    #[test]
    fn id_and_hash_cannot_be_shifted_into_each_other() {
        let keys = PeerKeys::new([1; SECRET_BYTES]);
        assert_ne!(keys.key("ab", Some("c")), keys.key("a", Some("bc")));
    }

    #[test]
    fn secret_survives_a_restart() {
        let dir = tempfile::tempdir().expect("test value");
        let first = PeerKeys::load_or_create(dir.path()).expect("test value");
        let second = PeerKeys::load_or_create(dir.path()).expect("test value");
        assert_eq!(first.key("poll", Some("x")), second.key("poll", Some("x")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(SECRET_FILE))
                .expect("test value")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::write(dir.path().join(SECRET_FILE), b"short").expect("test value");
        assert!(PeerKeys::load_or_create(dir.path()).is_err());
        assert_eq!(format!("{first:?}"), "PeerKeys");
    }
}
