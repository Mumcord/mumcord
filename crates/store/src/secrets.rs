//! OS secure storage (Secret Service / KWallet on Linux, Keychain on macOS, Credential
//! Manager on Windows) for the identity and server passwords.
//!
//! Layout under service [`KEYRING_SERVICE`]:
//! - `identity` – number of chunks of the stored identity (decimal text)
//! - `identity/<n>` – chunk `n` of the identity as PKCS#12 (empty password; protection
//!   comes from the OS store)
//! - `server-password/<server id>` – password of a saved server
//!
//! The identity is chunked because Windows Credential Manager rejects secrets larger
//! than 2560 bytes and an RSA PKCS#12 can exceed that.
//!
//! The OS calls can block (e.g. while a wallet unlock prompt is open); async callers
//! should run them on a blocking thread.

use crate::error::{Error, Result};
use crate::identity::Identity;
use crate::servers::ServerId;

/// Keyring service name, derived from the app identifier `app.mumcord.client`.
pub const KEYRING_SERVICE: &str = "app.mumcord.client";

const IDENTITY_ACCOUNT: &str = "identity";
const CHUNK_SIZE: usize = 2048;

trait SecretBackend: Send + Sync {
    fn set(&self, account: &str, secret: &[u8]) -> Result<()>;
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>>;
    /// Deleting a missing entry is not an error.
    fn delete(&self, account: &str) -> Result<()>;
}

struct KeyringBackend {
    service: String,
}

impl KeyringBackend {
    fn entry(&self, account: &str) -> Result<keyring::Entry> {
        Ok(keyring::Entry::new(&self.service, account)?)
    }
}

impl SecretBackend for KeyringBackend {
    fn set(&self, account: &str, secret: &[u8]) -> Result<()> {
        Ok(self.entry(account)?.set_secret(secret)?)
    }

    fn get(&self, account: &str) -> Result<Option<Vec<u8>>> {
        match self.entry(account)?.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn delete(&self, account: &str) -> Result<()> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// Access to secrets in the OS secure storage.
pub struct SecretStore {
    backend: Box<dyn SecretBackend>,
}

impl SecretStore {
    /// The OS secure storage under [`KEYRING_SERVICE`].
    pub fn os() -> Self {
        Self::os_with_service(KEYRING_SERVICE)
    }

    fn os_with_service(service: &str) -> Self {
        Self {
            backend: Box::new(KeyringBackend {
                service: service.to_owned(),
            }),
        }
    }

    /// Stores the identity, replacing any previous one.
    pub fn save_identity(&self, identity: &Identity) -> Result<()> {
        let p12 = identity.to_pkcs12("")?;
        let old_count = self.identity_chunk_count()?;
        let chunks: Vec<&[u8]> = p12.chunks(CHUNK_SIZE).collect();
        for (index, chunk) in chunks.iter().enumerate() {
            self.backend.set(&chunk_account(index), chunk)?;
        }
        // The count is written last so a failure above never leaves a count that
        // points at chunks of two different identities.
        self.backend
            .set(IDENTITY_ACCOUNT, chunks.len().to_string().as_bytes())?;
        for index in chunks.len()..old_count.unwrap_or(0) {
            self.backend.delete(&chunk_account(index))?;
        }
        Ok(())
    }

    /// The stored identity, or `None` if none was saved yet (first start).
    pub fn load_identity(&self) -> Result<Option<Identity>> {
        let Some(count) = self.identity_chunk_count()? else {
            return Ok(None);
        };
        let mut p12 = Vec::new();
        for index in 0..count {
            let chunk = self
                .backend
                .get(&chunk_account(index))?
                .ok_or(Error::CorruptStoredIdentity)?;
            p12.extend(chunk);
        }
        Identity::from_pkcs12(&p12, "")
            .map(Some)
            .map_err(|_| Error::CorruptStoredIdentity)
    }

    pub fn delete_identity(&self) -> Result<()> {
        let count = self.identity_chunk_count()?;
        self.backend.delete(IDENTITY_ACCOUNT)?;
        for index in 0..count.unwrap_or(0) {
            self.backend.delete(&chunk_account(index))?;
        }
        Ok(())
    }

    pub fn set_server_password(&self, server: ServerId, password: &str) -> Result<()> {
        self.backend
            .set(&password_account(server), password.as_bytes())
    }

    pub fn server_password(&self, server: ServerId) -> Result<Option<String>> {
        match self.backend.get(&password_account(server))? {
            Some(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| Error::CorruptStoredIdentity),
            None => Ok(None),
        }
    }

    /// Removes a server password; a server without password is not an error.
    pub fn delete_server_password(&self, server: ServerId) -> Result<()> {
        self.backend.delete(&password_account(server))
    }

    fn identity_chunk_count(&self) -> Result<Option<usize>> {
        let Some(raw) = self.backend.get(IDENTITY_ACCOUNT)? else {
            return Ok(None);
        };
        std::str::from_utf8(&raw)
            .ok()
            .and_then(|text| text.parse().ok())
            .map(Some)
            .ok_or(Error::CorruptStoredIdentity)
    }
}

fn chunk_account(index: usize) -> String {
    format!("{IDENTITY_ACCOUNT}/{index}")
}

fn password_account(server: ServerId) -> String {
    format!("server-password/{server}")
}

#[cfg(any(test, feature = "test-util"))]
mod memory {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    pub(super) struct MemoryBackend(pub(super) Mutex<HashMap<String, Vec<u8>>>);

    impl SecretBackend for MemoryBackend {
        fn set(&self, account: &str, secret: &[u8]) -> Result<()> {
            self.0
                .lock()
                .expect("lock")
                .insert(account.to_owned(), secret.to_vec());
            Ok(())
        }
        fn get(&self, account: &str) -> Result<Option<Vec<u8>>> {
            Ok(self.0.lock().expect("lock").get(account).cloned())
        }
        fn delete(&self, account: &str) -> Result<()> {
            self.0.lock().expect("lock").remove(account);
            Ok(())
        }
    }

    impl SecretStore {
        /// In-memory store for tests, so they never touch the user's OS secure storage.
        pub fn in_memory() -> Self {
            Self {
                backend: Box::new(MemoryBackend::default()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;

    static IDENTITY: LazyLock<Identity> =
        LazyLock::new(|| Identity::generate("Secret Tester").expect("generate"));

    fn exercise(store: &SecretStore) {
        assert_eq!(store.load_identity().expect("empty"), None);
        store.save_identity(&IDENTITY).expect("save");
        assert_eq!(
            store.load_identity().expect("load").as_ref(),
            Some(&*IDENTITY)
        );
        // Saving again (e.g. after import) replaces cleanly.
        store.save_identity(&IDENTITY).expect("save again");
        assert_eq!(
            store.load_identity().expect("load").as_ref(),
            Some(&*IDENTITY)
        );

        assert_eq!(store.server_password(7).expect("none"), None);
        store.set_server_password(7, "hunter2").expect("set");
        assert_eq!(
            store.server_password(7).expect("get").as_deref(),
            Some("hunter2")
        );
        store.delete_server_password(7).expect("delete");
        store.delete_server_password(7).expect("delete missing");
        assert_eq!(store.server_password(7).expect("gone"), None);

        store.delete_identity().expect("delete identity");
        assert_eq!(store.load_identity().expect("gone"), None);
    }

    #[test]
    fn in_memory_round_trip() {
        exercise(&SecretStore::in_memory());
    }

    #[test]
    fn identity_is_chunked_below_windows_limit_and_stale_chunks_removed() {
        let backend = memory::MemoryBackend::default();
        backend.set("identity", b"9").expect("seed count");
        for index in 0..9 {
            backend
                .set(&chunk_account(index), b"stale")
                .expect("seed chunk");
        }
        let store = SecretStore {
            backend: Box::new(backend),
        };
        store.save_identity(&IDENTITY).expect("save");
        assert_eq!(
            store.load_identity().expect("load").as_ref(),
            Some(&*IDENTITY)
        );

        let count = store.identity_chunk_count().expect("count").expect("some");
        assert!(count < 9);
        for index in 0..9 {
            let chunk = store.backend.get(&chunk_account(index)).expect("get");
            match chunk {
                Some(bytes) => assert!(index < count && bytes.len() <= 2560),
                None => assert!(index >= count),
            }
        }
    }

    #[test]
    fn missing_chunk_is_reported_as_corrupt() {
        let store = SecretStore::in_memory();
        store.save_identity(&IDENTITY).expect("save");
        store.backend.delete(&chunk_account(0)).expect("drop chunk");
        assert!(matches!(
            store.load_identity(),
            Err(Error::CorruptStoredIdentity)
        ));
    }

    /// Runs against the real OS secure storage under a separate test service name and
    /// removes its entries afterwards. Run with:
    /// `cargo test -p mumcord-store -- --ignored os_keyring`
    #[test]
    #[ignore = "touches the real OS secure storage"]
    fn os_keyring_round_trip() {
        let store = SecretStore::os_with_service("app.mumcord.client.test");
        store.delete_identity().expect("pre-clean identity");
        store.delete_server_password(7).expect("pre-clean password");
        exercise(&store);
    }
}
