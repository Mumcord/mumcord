//! OS secure storage (Secret Service / KWallet on Linux, Keychain on macOS, Credential
//! Manager on Windows) for the identity and server passwords.
//!
//! Layout under service [`KEYRING_SERVICE`]:
//! - `identity/<id>/count` – number of chunks of identity `‹id›` (decimal text)
//! - `identity/<id>/<n>` – chunk `n` of the identity as PKCS#12 (empty password; protection
//!   comes from the OS store)
//! - `server/<server id>` – password and channel passwords (Mumble access tokens) of a saved
//!   server, as JSON `{"password": "…", "tokens": ["…"]}`
//!
//! Older versions kept exactly one identity under `identity` and `identity/<n>`; it is read
//! once with [`SecretStore::legacy_identity`] and moved to the current layout (issue #148).
//!
//! Only Windows needs more than one identity chunk: Credential Manager rejects secrets larger
//! than 2560 bytes and an RSA PKCS#12 can exceed that.
//!
//! Every item can cost the user a prompt (an unsigned macOS app is asked for the login
//! password per item and per access), so each item is read at most once per session, cached
//! in memory and written only when its value changes. A failed read (e.g. a denied prompt)
//! is remembered too and not repeated before the next start, so callers that ask often
//! (server list, notifications, reconnects) cannot turn one prompt into a flood. Older versions kept a server's password
//! and tokens in two items (`server-password/<id>`, `access-tokens/<id>`) and split the
//! identity on every platform; both are moved to the current layout on first read.
//!
//! The OS calls can block (e.g. while a wallet unlock prompt is open); async callers
//! should run them on a blocking thread.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::identities::IdentityId;
use crate::identity::Identity;
use crate::servers::ServerId;

/// Keyring service name, derived from the app identifier `app.mumcord.client`.
pub const KEYRING_SERVICE: &str = "app.mumcord.client";

/// Account of the single identity older versions stored; only read for the migration.
const LEGACY_IDENTITY_ACCOUNT: &str = "identity";
const CHUNK_SIZE: usize = if cfg!(windows) { 2048 } else { usize::MAX };

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

/// Password and channel passwords of one saved server, kept in one item so a connect reads
/// (and may prompt for) a single secret.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ServerSecrets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    password: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tokens: Vec<String>,
}

/// Outcome of the first read of an item this session.
enum Cached<T> {
    Read(T),
    /// The read failed; later calls fail without asking the OS (and the user) again.
    Unavailable,
}

impl ServerSecrets {
    fn is_empty(&self) -> bool {
        self.password.is_none() && self.tokens.is_empty()
    }
}

/// Access to secrets in the OS secure storage.
pub struct SecretStore {
    backend: Arc<dyn SecretBackend>,
    /// Server items read this session; a server without item is cached as empty.
    servers: Mutex<HashMap<ServerId, Cached<ServerSecrets>>>,
    /// Identities read this session, so a list of identities does not read (and prompt for)
    /// the same item twice.
    identities: Mutex<HashMap<IdentityId, Cached<Option<Identity>>>>,
    /// The identity of older versions once read this session.
    legacy: Mutex<Option<Cached<Option<Identity>>>>,
}

impl SecretStore {
    /// The OS secure storage under [`KEYRING_SERVICE`].
    pub fn os() -> Self {
        Self::os_with_service(KEYRING_SERVICE)
    }

    fn os_with_service(service: &str) -> Self {
        Self::with_backend(Arc::new(KeyringBackend {
            service: service.to_owned(),
        }))
    }

    fn with_backend(backend: Arc<dyn SecretBackend>) -> Self {
        Self {
            backend,
            servers: Mutex::new(HashMap::new()),
            identities: Mutex::new(HashMap::new()),
            legacy: Mutex::new(None),
        }
    }

    /// Stores identity `id`, replacing any previous certificate stored for it.
    pub fn save_identity(&self, id: IdentityId, identity: &Identity) -> Result<()> {
        // Held across the OS calls so concurrent callers wait for one write.
        let mut cache = lock(&self.identities);
        let p12 = identity.to_pkcs12("")?;
        let old_count = self.identity_chunk_count(id)?;
        self.write_identity(id, &p12, old_count)?;
        cache.insert(id, Cached::Read(Some(identity.clone())));
        Ok(())
    }

    /// Identity `id`, or `None` if nothing is stored for it. Read once per session.
    pub fn load_identity(&self, id: IdentityId) -> Result<Option<Identity>> {
        let mut cache = lock(&self.identities);
        match cache.get(&id) {
            Some(Cached::Read(identity)) => return Ok(identity.clone()),
            Some(Cached::Unavailable) => return Err(Error::SecretUnavailable),
            None => {}
        }
        match self.read_identity(id) {
            Ok(identity) => {
                cache.insert(id, Cached::Read(identity.clone()));
                Ok(identity)
            }
            Err(e) => {
                log_read_failure("an identity", &e);
                cache.insert(id, Cached::Unavailable);
                Err(e)
            }
        }
    }

    fn read_identity(&self, id: IdentityId) -> Result<Option<Identity>> {
        let Some(count) = self.identity_chunk_count(id)? else {
            return Ok(None);
        };
        let mut p12 = Vec::new();
        for index in 0..count {
            let chunk = self
                .backend
                .get(&identity_chunk_account(id, index))?
                .ok_or(Error::CorruptStoredIdentity)?;
            p12.extend(chunk);
        }
        let identity = Identity::from_pkcs12(&p12, "").map_err(|_| Error::CorruptStoredIdentity)?;
        if count > p12.len().div_ceil(CHUNK_SIZE) {
            // Written by a version that split the identity on every platform; one item
            // means one prompt per start instead of several.
            if let Err(e) = self.write_identity(id, &p12, Some(count)) {
                tracing::warn!(error = %e, "merging the identity chunks failed");
            }
        }
        Ok(Some(identity))
    }

    /// Removes identity `id` with its key material; a missing identity is not an error.
    pub fn delete_identity(&self, id: IdentityId) -> Result<()> {
        let mut cache = lock(&self.identities);
        let count = self.identity_chunk_count(id)?;
        self.backend.delete(&identity_count_account(id))?;
        for index in 0..count.unwrap_or(0) {
            self.backend.delete(&identity_chunk_account(id, index))?;
        }
        cache.insert(id, Cached::Read(None));
        Ok(())
    }

    /// The single identity older versions stored, for the migration to the identity list;
    /// `None` when there is none (new installation or already migrated). Read once per
    /// session.
    pub fn legacy_identity(&self) -> Result<Option<Identity>> {
        let mut cache = lock(&self.legacy);
        match &*cache {
            Some(Cached::Read(identity)) => return Ok(identity.clone()),
            Some(Cached::Unavailable) => return Err(Error::SecretUnavailable),
            None => {}
        }
        match self.read_legacy_identity() {
            Ok(identity) => {
                *cache = Some(Cached::Read(identity.clone()));
                Ok(identity)
            }
            Err(e) => {
                log_read_failure("the identity of an older version", &e);
                *cache = Some(Cached::Unavailable);
                Err(e)
            }
        }
    }

    fn read_legacy_identity(&self) -> Result<Option<Identity>> {
        let Some(count) = self.legacy_chunk_count()? else {
            return Ok(None);
        };
        let mut p12 = Vec::new();
        for index in 0..count {
            let chunk = self
                .backend
                .get(&legacy_chunk_account(index))?
                .ok_or(Error::CorruptStoredIdentity)?;
            p12.extend(chunk);
        }
        Identity::from_pkcs12(&p12, "")
            .map(Some)
            .map_err(|_| Error::CorruptStoredIdentity)
    }

    /// Removes the items of the single-identity layout after a successful migration.
    pub fn delete_legacy_identity(&self) -> Result<()> {
        let mut cache = lock(&self.legacy);
        let count = self.legacy_chunk_count()?;
        self.backend.delete(LEGACY_IDENTITY_ACCOUNT)?;
        for index in 0..count.unwrap_or(0) {
            self.backend.delete(&legacy_chunk_account(index))?;
        }
        *cache = Some(Cached::Read(None));
        Ok(())
    }

    pub fn set_server_password(&self, server: ServerId, password: &str) -> Result<()> {
        self.update_server(server, |s| s.password = Some(password.to_owned()))
    }

    pub fn server_password(&self, server: ServerId) -> Result<Option<String>> {
        self.update_server(server, |s| s.password.clone())
    }

    /// Removes a server password; a server without password is not an error.
    pub fn delete_server_password(&self, server: ServerId) -> Result<()> {
        self.update_server(server, |s| s.password = None)
    }

    /// Access tokens (channel passwords) of a saved server, oldest first; none is not an error.
    pub fn access_tokens(&self, server: ServerId) -> Result<Vec<String>> {
        self.update_server(server, |s| s.tokens.clone())
    }

    /// Replaces the access tokens of a saved server.
    pub fn set_access_tokens(&self, server: ServerId, tokens: &[String]) -> Result<()> {
        self.update_server(server, |s| s.tokens = tokens.to_vec())
    }

    /// Removes the password and access tokens of a removed server without reading them first.
    pub fn delete_server_secrets(&self, server: ServerId) -> Result<()> {
        let mut cache = lock(&self.servers);
        match cache.get(&server) {
            Some(Cached::Read(cached)) if cached.is_empty() => return Ok(()),
            Some(Cached::Read(_)) => self.backend.delete(&server_account(server))?,
            // Removing a server is a deliberate action, so it may ask again.
            None | Some(Cached::Unavailable) => {
                self.backend.delete(&server_account(server))?;
                self.backend.delete(&legacy_password_account(server))?;
                self.backend.delete(&legacy_tokens_account(server))?;
            }
        }
        cache.insert(server, Cached::Read(ServerSecrets::default()));
        Ok(())
    }

    /// Runs `change` on the server's secrets (read once per session) and writes the item only
    /// if they changed.
    fn update_server<T>(
        &self,
        server: ServerId,
        change: impl FnOnce(&mut ServerSecrets) -> T,
    ) -> Result<T> {
        // Held across the OS calls so concurrent callers wait for one read instead of each
        // reading (and prompting) for the same item.
        let mut cache = lock(&self.servers);
        let current = match cache.get(&server) {
            Some(Cached::Read(cached)) => cached.clone(),
            Some(Cached::Unavailable) => return Err(Error::SecretUnavailable),
            None => match self.read_server(server) {
                Ok(read) => {
                    cache.insert(server, Cached::Read(read.clone()));
                    read
                }
                Err(e) => {
                    log_read_failure("a server's password", &e);
                    cache.insert(server, Cached::Unavailable);
                    return Err(e);
                }
            },
        };
        let mut next = current.clone();
        let result = change(&mut next);
        if next != current {
            if next.is_empty() {
                self.backend.delete(&server_account(server))?;
            } else {
                self.backend
                    .set(&server_account(server), &serde_json::to_vec(&next)?)?;
            }
            cache.insert(server, Cached::Read(next));
        }
        Ok(result)
    }

    fn read_server(&self, server: ServerId) -> Result<ServerSecrets> {
        if let Some(bytes) = self.backend.get(&server_account(server))? {
            return Ok(serde_json::from_slice(&bytes)?);
        }
        let legacy_password = self.backend.get(&legacy_password_account(server))?;
        let legacy_tokens = self.backend.get(&legacy_tokens_account(server))?;
        let secrets = ServerSecrets {
            password: legacy_password
                .clone()
                .map(String::from_utf8)
                .transpose()
                .map_err(|_| Error::CorruptStoredIdentity)?,
            tokens: match &legacy_tokens {
                Some(bytes) => serde_json::from_slice(bytes)?,
                None => Vec::new(),
            },
        };
        if !secrets.is_empty() {
            // The secrets were read, so a failed merge (e.g. a denied prompt for the old
            // items) must not fail the read; the next start tries again.
            if let Err(e) = self.merge_legacy_server(
                server,
                &secrets,
                legacy_password.is_some(),
                legacy_tokens.is_some(),
            ) {
                tracing::warn!(error = %e, "merging a server's old keychain items failed");
            }
        }
        Ok(secrets)
    }

    fn merge_legacy_server(
        &self,
        server: ServerId,
        secrets: &ServerSecrets,
        had_password: bool,
        had_tokens: bool,
    ) -> Result<()> {
        // Merged before the old items go, so a failure in between loses nothing.
        self.backend
            .set(&server_account(server), &serde_json::to_vec(secrets)?)?;
        if had_password {
            self.backend.delete(&legacy_password_account(server))?;
        }
        if had_tokens {
            self.backend.delete(&legacy_tokens_account(server))?;
        }
        Ok(())
    }

    fn write_identity(&self, id: IdentityId, p12: &[u8], old_count: Option<usize>) -> Result<()> {
        let chunks: Vec<&[u8]> = p12.chunks(CHUNK_SIZE).collect();
        for (index, chunk) in chunks.iter().enumerate() {
            self.backend
                .set(&identity_chunk_account(id, index), chunk)?;
        }
        // The count is written last so a failure above never leaves a count that
        // points at chunks of two different identities.
        self.backend.set(
            &identity_count_account(id),
            chunks.len().to_string().as_bytes(),
        )?;
        for index in chunks.len()..old_count.unwrap_or(0) {
            self.backend.delete(&identity_chunk_account(id, index))?;
        }
        Ok(())
    }

    fn identity_chunk_count(&self, id: IdentityId) -> Result<Option<usize>> {
        chunk_count(&*self.backend, &identity_count_account(id))
    }

    fn legacy_chunk_count(&self) -> Result<Option<usize>> {
        chunk_count(&*self.backend, LEGACY_IDENTITY_ACCOUNT)
    }
}

/// Reads the number of chunks stored under `account`.
fn chunk_count(backend: &dyn SecretBackend, account: &str) -> Result<Option<usize>> {
    let Some(raw) = backend.get(account)? else {
        return Ok(None);
    };
    std::str::from_utf8(&raw)
        .ok()
        .and_then(|text| text.parse().ok())
        .map(Some)
        .ok_or(Error::CorruptStoredIdentity)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while holding the lock leaves the cache itself intact.
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn log_read_failure(what: &str, error: &Error) {
    // Only OS errors are shown: a parse error could quote part of the secret.
    match error {
        Error::SecureStorage(e) => {
            tracing::warn!(error = %e, "reading {what} from secure storage failed; not asking again until restart");
        }
        _ => {
            tracing::warn!("{what} in secure storage is unreadable; not asking again until restart")
        }
    }
}

fn identity_count_account(id: IdentityId) -> String {
    format!("identity/{id}/count")
}

fn identity_chunk_account(id: IdentityId, index: usize) -> String {
    format!("identity/{id}/{index}")
}

fn legacy_chunk_account(index: usize) -> String {
    format!("{LEGACY_IDENTITY_ACCOUNT}/{index}")
}

fn server_account(server: ServerId) -> String {
    format!("server/{server}")
}

fn legacy_password_account(server: ServerId) -> String {
    format!("server-password/{server}")
}

fn legacy_tokens_account(server: ServerId) -> String {
    format!("access-tokens/{server}")
}

#[cfg(any(test, feature = "test-util"))]
mod memory {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;

    /// Shared so a test can keep a handle and count the OS calls after handing the backend
    /// to a [`SecretStore`].
    #[derive(Clone, Default)]
    pub(super) struct MemoryBackend(Arc<MemoryState>);

    #[derive(Default)]
    pub(super) struct MemoryState {
        items: Mutex<HashMap<String, Vec<u8>>>,
        reads: AtomicUsize,
        writes: AtomicUsize,
        /// Makes every call fail like a denied keychain prompt.
        denied: AtomicBool,
    }

    #[cfg(test)]
    impl MemoryBackend {
        /// Number of `get` calls so far.
        pub(super) fn reads(&self) -> usize {
            self.0.reads.load(Ordering::Relaxed)
        }

        /// Number of `set` and `delete` calls so far.
        pub(super) fn writes(&self) -> usize {
            self.0.writes.load(Ordering::Relaxed)
        }

        pub(super) fn deny(&self, denied: bool) {
            self.0.denied.store(denied, Ordering::Relaxed);
        }

        pub(super) fn accounts(&self) -> Vec<String> {
            let mut accounts: Vec<String> =
                self.0.items.lock().expect("lock").keys().cloned().collect();
            accounts.sort();
            accounts
        }
    }

    impl MemoryBackend {
        fn check_denied(&self) -> Result<()> {
            if self.0.denied.load(Ordering::Relaxed) {
                return Err(keyring::Error::NoStorageAccess("denied".into()).into());
            }
            Ok(())
        }
    }

    impl SecretBackend for MemoryBackend {
        fn set(&self, account: &str, secret: &[u8]) -> Result<()> {
            self.0.writes.fetch_add(1, Ordering::Relaxed);
            self.check_denied()?;
            self.0
                .items
                .lock()
                .expect("lock")
                .insert(account.to_owned(), secret.to_vec());
            Ok(())
        }
        fn get(&self, account: &str) -> Result<Option<Vec<u8>>> {
            self.0.reads.fetch_add(1, Ordering::Relaxed);
            self.check_denied()?;
            Ok(self.0.items.lock().expect("lock").get(account).cloned())
        }
        fn delete(&self, account: &str) -> Result<()> {
            self.0.writes.fetch_add(1, Ordering::Relaxed);
            self.check_denied()?;
            self.0.items.lock().expect("lock").remove(account);
            Ok(())
        }
    }

    impl SecretStore {
        /// In-memory store for tests, so they never touch the user's OS secure storage.
        pub fn in_memory() -> Self {
            Self::with_backend(Arc::new(MemoryBackend::default()))
        }

        /// Another handle to the same in-memory items with fresh caches, like the next start
        /// of the app. Tests that restart the engine reuse the items this way.
        pub fn reopen(&self) -> Self {
            Self::with_backend(self.backend.clone())
        }

        /// Writes an identity in the single-identity layout of older versions, to test the
        /// migration to the identity list (issue #148).
        pub fn seed_legacy_identity(&self, identity: &Identity) -> Result<()> {
            let p12 = identity.to_pkcs12("")?;
            self.backend.set(LEGACY_IDENTITY_ACCOUNT, b"1")?;
            self.backend.set(&legacy_chunk_account(0), &p12)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::memory::MemoryBackend;
    use super::*;

    static IDENTITY: LazyLock<Identity> =
        LazyLock::new(|| Identity::generate("Secret Tester").expect("generate"));

    fn counted() -> (SecretStore, MemoryBackend) {
        let backend = MemoryBackend::default();
        (
            SecretStore::with_backend(Arc::new(backend.clone())),
            backend,
        )
    }

    fn exercise(store: &SecretStore) {
        assert_eq!(store.load_identity(1).expect("empty"), None);
        store.save_identity(1, &IDENTITY).expect("save");
        assert_eq!(
            store.load_identity(1).expect("load").as_ref(),
            Some(&*IDENTITY)
        );
        // Saving again (e.g. after import) replaces cleanly.
        store.save_identity(1, &IDENTITY).expect("save again");
        assert_eq!(
            store.load_identity(1).expect("load").as_ref(),
            Some(&*IDENTITY)
        );
        assert_eq!(store.legacy_identity().expect("no legacy"), None);

        assert_eq!(store.server_password(7).expect("none"), None);
        store.set_server_password(7, "hunter2").expect("set");
        assert_eq!(
            store.server_password(7).expect("get").as_deref(),
            Some("hunter2")
        );
        store.delete_server_password(7).expect("delete");
        store.delete_server_password(7).expect("delete missing");
        assert_eq!(store.server_password(7).expect("gone"), None);

        assert!(store.access_tokens(7).expect("none").is_empty());
        let tokens = ["secret".to_owned(), "with \"quote\"".to_owned()];
        store.set_access_tokens(7, &tokens).expect("set tokens");
        assert_eq!(store.access_tokens(7).expect("get tokens"), tokens);
        assert!(store.access_tokens(8).expect("other server").is_empty());
        store.set_access_tokens(7, &[]).expect("clear tokens");
        assert!(store.access_tokens(7).expect("cleared").is_empty());

        store
            .set_server_password(9, "pw")
            .expect("set before removal");
        store.delete_server_secrets(9).expect("remove server");
        assert_eq!(store.server_password(9).expect("removed"), None);

        store.delete_identity(1).expect("delete identity");
        assert_eq!(store.load_identity(1).expect("gone"), None);
    }

    #[test]
    fn identities_are_stored_apart() {
        let (store, backend) = counted();
        let other = Identity::generate("Second Tester").expect("generate");
        store.save_identity(1, &IDENTITY).expect("save 1");
        store.save_identity(2, &other).expect("save 2");
        assert_eq!(
            store.load_identity(1).expect("load 1").as_ref(),
            Some(&*IDENTITY)
        );
        assert_eq!(
            store.load_identity(2).expect("load 2").as_ref(),
            Some(&other)
        );
        store.delete_identity(1).expect("delete 1");
        assert_eq!(store.load_identity(1).expect("gone"), None);
        assert!(store.load_identity(2).expect("kept").is_some());
        let accounts = backend.accounts();
        assert!(!accounts.iter().any(|a| a.starts_with("identity/1")));
        assert!(accounts.iter().any(|a| a.starts_with("identity/2")));
    }

    #[test]
    fn the_legacy_single_identity_is_read_and_removed() {
        let backend = MemoryBackend::default();
        let p12 = IDENTITY.to_pkcs12("").expect("p12");
        backend
            .set(LEGACY_IDENTITY_ACCOUNT, b"1")
            .expect("seed count");
        backend
            .set(&legacy_chunk_account(0), &p12)
            .expect("seed chunk");
        let store = SecretStore::with_backend(Arc::new(backend.clone()));
        assert_eq!(
            store.legacy_identity().expect("legacy").as_ref(),
            Some(&*IDENTITY)
        );
        // The old identity is not visible under the new layout until it is migrated.
        assert_eq!(store.load_identity(1).expect("none"), None);

        store.save_identity(1, &IDENTITY).expect("migrate");
        store.delete_legacy_identity().expect("clean up");
        assert_eq!(store.legacy_identity().expect("gone"), None);
        assert!(
            backend
                .accounts()
                .iter()
                .all(|a| a.starts_with("identity/1"))
        );
        assert_eq!(
            store.load_identity(1).expect("load").as_ref(),
            Some(&*IDENTITY)
        );
    }

    #[test]
    fn in_memory_round_trip() {
        exercise(&SecretStore::in_memory());
    }

    #[test]
    fn server_secrets_are_read_once_per_session_and_written_only_on_change() {
        let (store, backend) = counted();
        store.set_server_password(7, "hunter2").expect("set");
        store
            .set_access_tokens(7, &["token".to_owned()])
            .expect("set tokens");
        assert_eq!(backend.accounts(), ["server/7"]);

        // A new session: servers list, connect, reconnect and the edit dialog all ask again.
        let store = SecretStore::with_backend(Arc::new(backend.clone()));
        let (reads, writes) = (backend.reads(), backend.writes());
        for _ in 0..5 {
            assert_eq!(
                store.server_password(7).expect("get").as_deref(),
                Some("hunter2")
            );
            assert_eq!(store.access_tokens(7).expect("tokens"), ["token"]);
        }
        store.set_server_password(7, "hunter2").expect("unchanged");
        store
            .set_access_tokens(7, &["token".to_owned()])
            .expect("unchanged tokens");
        assert_eq!(backend.reads() - reads, 1);
        assert_eq!(backend.writes(), writes);

        store.set_server_password(7, "new").expect("change");
        assert_eq!(backend.writes() - writes, 1);
        assert_eq!(backend.reads() - reads, 1);
    }

    #[test]
    fn server_without_secrets_is_read_once_and_never_written() {
        let (store, backend) = counted();
        for _ in 0..3 {
            assert_eq!(store.server_password(3).expect("none"), None);
            assert!(store.access_tokens(3).expect("none").is_empty());
        }
        store.delete_server_password(3).expect("nothing to delete");
        store.set_access_tokens(3, &[]).expect("nothing to clear");
        // The current item plus the two items of older versions.
        assert_eq!(backend.reads(), 3);
        assert_eq!(backend.writes(), 0);
        store.delete_server_secrets(3).expect("remove");
        assert_eq!(backend.writes(), 0);
    }

    #[test]
    fn legacy_server_items_are_merged_into_one() {
        let backend = MemoryBackend::default();
        backend
            .set(&legacy_password_account(4), b"hunter2")
            .expect("seed password");
        backend
            .set(&legacy_tokens_account(4), br#"["a","b"]"#)
            .expect("seed tokens");
        let store = SecretStore::with_backend(Arc::new(backend.clone()));
        assert_eq!(
            store.server_password(4).expect("get").as_deref(),
            Some("hunter2")
        );
        assert_eq!(store.access_tokens(4).expect("tokens"), ["a", "b"]);
        assert_eq!(backend.accounts(), ["server/4"]);

        let reopened = SecretStore::with_backend(Arc::new(backend.clone()));
        let reads = backend.reads();
        assert_eq!(reopened.access_tokens(4).expect("tokens"), ["a", "b"]);
        assert_eq!(backend.reads() - reads, 1);
    }

    #[test]
    fn removing_a_server_deletes_current_and_legacy_items() {
        let backend = MemoryBackend::default();
        backend
            .set(&legacy_password_account(5), b"old")
            .expect("seed legacy");
        backend
            .set(&server_account(5), b"{}")
            .expect("seed current");
        let store = SecretStore::with_backend(Arc::new(backend.clone()));
        let reads = backend.reads();
        store.delete_server_secrets(5).expect("remove");
        assert_eq!(backend.reads(), reads);
        assert!(backend.accounts().is_empty());
    }

    #[test]
    fn identity_chunks_fit_the_platform() {
        let (store, backend) = counted();
        store.save_identity(1, &IDENTITY).expect("save");
        let count = store.identity_chunk_count(1).expect("count").expect("some");
        if cfg!(windows) {
            for index in 0..count {
                let chunk = backend.get(&identity_chunk_account(1, index)).expect("get");
                assert!(chunk.expect("chunk").len() <= 2560);
            }
        } else {
            assert_eq!(count, 1);
        }
    }

    #[test]
    fn stale_chunks_are_removed_and_split_identity_is_merged() {
        let backend = MemoryBackend::default();
        let p12 = IDENTITY.to_pkcs12("").expect("p12");
        let pieces: Vec<&[u8]> = p12.chunks(1024).collect();
        backend
            .set(
                &identity_count_account(3),
                pieces.len().to_string().as_bytes(),
            )
            .expect("seed count");
        for (index, piece) in pieces.iter().enumerate() {
            backend
                .set(&identity_chunk_account(3, index), piece)
                .expect("seed chunk");
        }
        let store = SecretStore::with_backend(Arc::new(backend.clone()));
        assert_eq!(
            store.load_identity(3).expect("load").as_ref(),
            Some(&*IDENTITY)
        );
        let expected = p12.len().div_ceil(CHUNK_SIZE);
        assert_eq!(
            store.identity_chunk_count(3).expect("count"),
            Some(expected)
        );
        assert_eq!(backend.accounts().len(), expected + 1);
        assert_eq!(
            store.load_identity(3).expect("reload").as_ref(),
            Some(&*IDENTITY)
        );
    }

    #[test]
    fn missing_chunk_is_reported_as_corrupt() {
        let (store, backend) = counted();
        store.save_identity(1, &IDENTITY).expect("save");
        backend
            .delete(&identity_chunk_account(1, 0))
            .expect("drop chunk");
        let store = SecretStore::with_backend(Arc::new(backend));
        assert!(matches!(
            store.load_identity(1),
            Err(Error::CorruptStoredIdentity)
        ));
    }

    #[test]
    fn identity_is_read_once_per_session() {
        let (store, backend) = counted();
        for _ in 0..3 {
            assert_eq!(store.load_identity(1).expect("first start"), None);
        }
        assert_eq!(backend.reads(), 1);
        store.save_identity(1, &IDENTITY).expect("save");

        // A new session: start-up, every connect and reconnect and the identity settings ask.
        let store = SecretStore::with_backend(Arc::new(backend.clone()));
        let chunks = store.identity_chunk_count(1).expect("count").expect("some");
        let reads = backend.reads();
        for _ in 0..5 {
            assert_eq!(
                store.load_identity(1).expect("load").as_ref(),
                Some(&*IDENTITY)
            );
        }
        // The chunk count and the chunks (one outside Windows).
        assert_eq!(backend.reads() - reads, 1 + chunks);
    }

    #[test]
    fn denied_reads_are_not_repeated_until_restart() {
        let (store, backend) = counted();
        store.save_identity(1, &IDENTITY).expect("save");
        store.set_server_password(7, "hunter2").expect("set");

        let store = SecretStore::with_backend(Arc::new(backend.clone()));
        backend.deny(true);
        let reads = backend.reads();
        for _ in 0..5 {
            assert!(store.load_identity(1).is_err());
            assert!(store.server_password(7).is_err());
            assert!(store.access_tokens(7).is_err());
        }
        assert_eq!(backend.reads() - reads, 2);

        // Allowed after a restart.
        backend.deny(false);
        let store = SecretStore::with_backend(Arc::new(backend.clone()));
        assert_eq!(
            store.server_password(7).expect("get").as_deref(),
            Some("hunter2")
        );
        assert!(store.load_identity(1).expect("load").is_some());
    }

    #[test]
    fn denied_merge_of_legacy_items_still_returns_the_secrets() {
        let backend = MemoryBackend::default();
        backend
            .set(&legacy_password_account(4), b"hunter2")
            .expect("seed password");
        let store = SecretStore::with_backend(Arc::new(backend.clone()));
        let failing = FailingWrites(backend.clone());
        let store_denied_writes = SecretStore::with_backend(Arc::new(failing));
        for _ in 0..3 {
            assert_eq!(
                store_denied_writes
                    .server_password(4)
                    .expect("read despite failed merge")
                    .as_deref(),
                Some("hunter2")
            );
        }
        assert_eq!(backend.accounts(), ["server-password/4"]);
        // The next start merges.
        assert_eq!(
            store.server_password(4).expect("get").as_deref(),
            Some("hunter2")
        );
        assert_eq!(backend.accounts(), ["server/4"]);
    }

    /// Reads succeed, writes fail like a denied prompt for an item of another app version.
    struct FailingWrites(MemoryBackend);

    impl SecretBackend for FailingWrites {
        fn set(&self, _account: &str, _secret: &[u8]) -> Result<()> {
            Err(keyring::Error::NoStorageAccess("denied".into()).into())
        }
        fn get(&self, account: &str) -> Result<Option<Vec<u8>>> {
            self.0.get(account)
        }
        fn delete(&self, _account: &str) -> Result<()> {
            Err(keyring::Error::NoStorageAccess("denied".into()).into())
        }
    }

    /// Runs against the real OS secure storage under a separate test service name and
    /// removes its entries afterwards. Run with:
    /// `cargo test -p mumcord-store -- --ignored os_keyring`
    #[test]
    #[ignore = "touches the real OS secure storage"]
    fn os_keyring_round_trip() {
        let store = SecretStore::os_with_service("app.mumcord.client.test");
        store.delete_identity(1).expect("pre-clean identity");
        store.delete_legacy_identity().expect("pre-clean legacy");
        store.delete_server_secrets(7).expect("pre-clean server 7");
        store.delete_server_secrets(9).expect("pre-clean server 9");
        exercise(&SecretStore::os_with_service("app.mumcord.client.test"));
    }
}
