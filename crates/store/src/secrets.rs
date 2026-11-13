//! OS secure storage (Secret Service / KWallet on Linux, Keychain on macOS, Credential
//! Manager on Windows) for the identity and server passwords. FreeBSD has no Secret Service in
//! most installs (no gnome-keyring or KWallet session service), so there the same items live in
//! an encrypted file in the profile directory instead — see [`vault`].
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
//!
//! Android and iOS have no OS backend in this crate: the embedding app implements
//! [`SecretBackend`] on top of the phone's secure storage and injects it with
//! [`SecretStore::with_backend`]. The layout, the caching and the migrations above are the
//! same for every backend.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

#[cfg(any(test, feature = "test-util"))]
use crate::error::SecretBackendError;
use crate::error::{Error, Result};
use crate::identities::IdentityId;
use crate::identity::Identity;
use crate::servers::ServerId;

/// Keyring service name, derived from the app identifier `app.mumcord.client`.
pub const KEYRING_SERVICE: &str = "app.mumcord.client";

/// Account of the single identity older versions stored; only read for the migration.
const LEGACY_IDENTITY_ACCOUNT: &str = "identity";
const CHUNK_SIZE: usize = if cfg!(windows) { 2048 } else { usize::MAX };

/// The secure storage a [`SecretStore`] keeps its items in: a flat map from an account name to
/// a secret of arbitrary bytes.
///
/// The desktop platforms use the OS store ([`SecretStore::os`]). An app on another platform
/// implements this trait on top of the platform's secure storage (Android Keystore-protected
/// storage, iOS Keychain) and injects it with [`SecretStore::with_backend`].
///
/// Rules for an implementation:
/// - Account names are short ASCII paths such as `identity/3/0` or `server/12`. They are chosen
///   by the store and must be kept exactly; they are not secret.
/// - Secrets are opaque bytes of any length (a PKCS#12 file of several kilobytes, a JSON text)
///   and must come back unchanged.
/// - A missing item is `Ok(None)` on [`get`](Self::get) and `Ok(())` on
///   [`delete`](Self::delete), never an error.
/// - Every other failure (storage locked, access denied, …) is reported as
///   [`Error::SecureStorage`] with a [`SecretBackendError`](crate::SecretBackendError). Its
///   message is logged, so it must never contain the secret.
/// - Calls may block and come from any thread. The store caches what it reads, so each item is
///   read at most once per [`SecretStore`] and written only when its value changes.
/// - Secrets must not leave the device: no backups, no cloud sync.
///
/// ```
/// use std::collections::HashMap;
/// use std::sync::{Arc, Mutex};
///
/// use mumcord_store::{Error, Result, SecretBackend, SecretBackendError, SecretStore};
///
/// /// Stands in for the platform's secure storage.
/// #[derive(Default)]
/// struct PhoneStorage(Mutex<HashMap<String, Vec<u8>>>);
///
/// impl PhoneStorage {
///     fn items(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, Vec<u8>>>> {
///         self.0
///             .lock()
///             .map_err(|_| Error::SecureStorage(SecretBackendError::new("storage is locked")))
///     }
/// }
///
/// impl SecretBackend for PhoneStorage {
///     fn set(&self, account: &str, secret: &[u8]) -> Result<()> {
///         self.items()?.insert(account.to_owned(), secret.to_vec());
///         Ok(())
///     }
///     fn get(&self, account: &str) -> Result<Option<Vec<u8>>> {
///         Ok(self.items()?.get(account).cloned())
///     }
///     fn delete(&self, account: &str) -> Result<()> {
///         self.items()?.remove(account);
///         Ok(())
///     }
/// }
///
/// let secrets = SecretStore::with_backend(Arc::new(PhoneStorage::default()));
/// secrets.set_server_password(1, "hunter2")?;
/// assert_eq!(secrets.server_password(1)?.as_deref(), Some("hunter2"));
/// # Ok::<(), Error>(())
/// ```
pub trait SecretBackend: Send + Sync {
    /// Stores `secret` under `account`, replacing an existing item.
    fn set(&self, account: &str, secret: &[u8]) -> Result<()>;
    /// The secret stored under `account`, or `None` if there is none.
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>>;
    /// Removes the item under `account`. Deleting a missing entry is not an error.
    fn delete(&self, account: &str) -> Result<()>;
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
struct KeyringBackend {
    service: String,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl KeyringBackend {
    fn entry(&self, account: &str) -> Result<keyring::Entry> {
        Ok(keyring::Entry::new(&self.service, account)?)
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
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

/// Files in the profile directory that hold secret material and only the user may touch.
///
/// The vault key and blob are trusted only when they are regular files (not symbolic links)
/// owned by the user and closed to group and others: a file restored from a backup with 0644, or
/// one another local user planted, is refused with an error that says how to fix it instead of
/// being used as it is. The checks run on the opened file, so swapping the path between check and
/// read does not help. Kept apart from the vault so they run in the Linux test suite as well.
#[cfg(any(target_os = "freebsd", all(test, unix)))]
mod private_file {
    use std::fs;
    use std::io::{self, Read, Write};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    use std::path::{Path, PathBuf};

    /// Contents of `path`, or `None` if it does not exist.
    pub(super) fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(refused(path, "is a symbolic link"));
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        }
        // O_NOFOLLOW: a link put in place after the check above is not followed either.
        let mut file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(refused(path, "is not a regular file"));
        }
        check_owner(path, &meta)?;
        if meta.mode() & 0o077 != 0 {
            return Err(refused(path, "can be read or changed by other users"));
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }

    /// Writes `bytes` to `path` so that only the user can read it.
    ///
    /// The bytes go into a temporary file beside `path` that is renamed onto it once they are
    /// flushed, so a crash or a full disk half way leaves the previous file intact. That file
    /// holds every stored secret, including the identity, so a truncated one would be the end of
    /// the profile. The temporary file is always created anew: a stale one left with wider
    /// permissions would otherwise keep them through the rename.
    pub(super) fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        let dir = path.parent().unwrap_or(Path::new("."));
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let dir_meta = fs::metadata(dir)?;
        check_owner(dir, &dir_meta)?;
        if dir_meta.mode() & 0o022 != 0 {
            // Another user could replace the files in it.
            return Err(refused(dir, "can be changed by other users"));
        }
        let temp = temp_path(path);
        match fs::remove_file(&temp) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(bytes)?;
        // On the disk before the rename: a rename that outlived its data would leave a file of
        // zeroes behind the name.
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    }

    fn temp_path(path: &Path) -> PathBuf {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(".tmp");
        path.with_file_name(name)
    }

    fn check_owner(path: &Path, meta: &fs::Metadata) -> io::Result<()> {
        // Safety: geteuid cannot fail and has no preconditions.
        if meta.uid() != unsafe { libc::geteuid() } {
            return Err(refused(path, "belongs to another user"));
        }
        Ok(())
    }

    /// The error names only the file, not its full path, which would contain the user name.
    fn refused(path: &Path, reason: &str) -> io::Error {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{name} {reason}; it must be a file of your own that only you can read (chmod 600)"
            ),
        )
    }
}

/// Which store a profile keeps its secrets in where a Secret Service may or may not answer.
///
/// The choice is sticky per profile so that a Secret Service that is merely late or locked at one
/// start (D-Bus not up yet, an unlock prompt dismissed) does not silently send new secrets to the
/// file while the old ones stay in the wallet, or the other way round:
/// - a profile with a secret file keeps using it;
/// - a profile marked as using the Secret Service keeps using it, and a failing service surfaces
///   as the usual "secure storage could not be read" error instead of an empty file;
/// - only a profile that has used neither is probed, and the result is remembered.
#[cfg(any(target_os = "freebsd", all(test, unix)))]
mod backend_choice {
    use std::path::Path;

    use super::private_file;

    pub(super) const BLOB_FILE: &str = "secrets.vault";
    pub(super) const KEY_FILE: &str = "secrets.key";
    /// Empty marker: this profile's secrets are in the OS Secret Service.
    pub(super) const OS_MARKER_FILE: &str = "secrets.os-store";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Choice {
        Os,
        File,
    }

    /// Picks the store for the profile in `dir`; `probe` checks whether a Secret Service works and
    /// runs only for a profile that has not used either store yet.
    pub(super) fn choose(dir: &Path, probe: impl FnOnce() -> bool) -> Choice {
        if dir.join(BLOB_FILE).exists() || dir.join(KEY_FILE).exists() {
            if dir.join(OS_MARKER_FILE).exists() {
                tracing::warn!(
                    "this profile has secrets in the OS secret service and in the encrypted file; using the file"
                );
            }
            return Choice::File;
        }
        if dir.join(OS_MARKER_FILE).exists() {
            return Choice::Os;
        }
        if probe() {
            if let Err(e) = private_file::write(&dir.join(OS_MARKER_FILE), &[]) {
                tracing::warn!(error = %e, "could not remember that the profile uses the OS secret service");
            }
            Choice::Os
        } else {
            tracing::warn!(
                "no usable OS secret service; secrets go to the encrypted file in the profile, which is weaker"
            );
            Choice::File
        }
    }
}

/// The encrypted secret file used where no Secret Service answers (FreeBSD).
///
/// One AES-256-GCM blob holds every item as `account -> secret`; the 32-byte random key sits in a
/// second file next to it, unprotected: no passphrase or key derivation is involved. Both files
/// are 0600 in the profile directory and refused otherwise (see [`private_file`]), so other users
/// cannot read them and a backup that leaks only the blob stays useless. This protects the secrets
/// the same way a browser's "basic" password store does: against other users and against backups
/// that leak only one of the two files, not against anyone or anything that can read the user's
/// whole profile directory. A Secret Service, where the session has one, is always preferred (see
/// [`SecretStore::os_with_profile`]).
#[cfg(target_os = "freebsd")]
mod vault {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey};
    use ring::rand::{SecureRandom, SystemRandom};

    use super::backend_choice::{BLOB_FILE, KEY_FILE};
    use super::private_file;
    use super::{Result, SecretBackend};
    use crate::error::Error;

    /// Prefix of the blob, so a foreign file is refused instead of decrypted into nonsense.
    const MAGIC: &[u8; 8] = b"MCVLT1\0\0";
    const KEY_LEN: usize = 32;
    /// AES-256-GCM nonce; a fresh random one per write, which is safe for the number of writes a
    /// profile sees.
    const NONCE_LEN: usize = 12;
    /// AES-GCM authentication tag length.
    const TAG_LEN: usize = 16;

    /// Secrets in an encrypted file inside the profile directory.
    pub(super) struct VaultBackend {
        dir: PathBuf,
        /// Serialises the read-modify-write of the blob.
        write: std::sync::Mutex<()>,
    }

    impl VaultBackend {
        pub(super) fn new(dir: PathBuf) -> Self {
            Self {
                dir,
                write: std::sync::Mutex::new(()),
            }
        }

        fn blob_path(&self) -> PathBuf {
            self.dir.join(BLOB_FILE)
        }

        fn key_path(&self) -> PathBuf {
            self.dir.join(KEY_FILE)
        }

        /// The key file, created with a fresh key on first use.
        fn key(&self) -> Result<LessSafeKey> {
            let key = match private_file::read(&self.key_path()).map_err(Error::SecretFile)? {
                Some(key) => key,
                None => {
                    let mut key = vec![0u8; KEY_LEN];
                    SystemRandom::new()
                        .fill(&mut key)
                        .map_err(|_| Error::SecretVault("no random bytes".to_owned()))?;
                    private_file::write(&self.key_path(), &key).map_err(Error::SecretFile)?;
                    key
                }
            };
            let unbound = UnboundKey::new(&ring::aead::AES_256_GCM, &key)
                .map_err(|_| Error::SecretVault("the key file is corrupt".to_owned()))?;
            Ok(LessSafeKey::new(unbound))
        }

        fn read(&self) -> Result<BTreeMap<String, Vec<u8>>> {
            let Some(blob) = private_file::read(&self.blob_path()).map_err(Error::SecretFile)?
            else {
                return Ok(BTreeMap::new());
            };
            if blob.len() < MAGIC.len() + NONCE_LEN + TAG_LEN || &blob[..MAGIC.len()] != MAGIC {
                return Err(Error::SecretVault("not a secret file".to_owned()));
            }
            let key = self.key()?;
            let (nonce, rest) = blob[MAGIC.len()..].split_at(NONCE_LEN);
            let mut plain = rest.to_vec();
            // `open_in_place` authenticates and returns only the plaintext, without the tag.
            let plain = key
                .open_in_place(
                    Nonce::try_assume_unique_for_key(nonce)
                        .map_err(|_| Error::SecretVault("bad nonce".to_owned()))?,
                    Aad::empty(),
                    &mut plain,
                )
                .map_err(|_| Error::SecretVault("the secret file does not decrypt".to_owned()))?;
            serde_json::from_slice(plain).map_err(Error::from)
        }

        fn write_map(&self, items: &BTreeMap<String, Vec<u8>>) -> Result<()> {
            let key = self.key()?;
            let plain = serde_json::to_vec(items)?;
            let mut nonce = [0u8; NONCE_LEN];
            SystemRandom::new()
                .fill(&mut nonce)
                .map_err(|_| Error::SecretVault("no random bytes".to_owned()))?;
            let mut sealed = plain;
            key.seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::empty(),
                &mut sealed,
            )
            .map_err(|_| Error::SecretVault("sealing failed".to_owned()))?;
            let mut blob = Vec::with_capacity(MAGIC.len() + NONCE_LEN + sealed.len());
            blob.extend_from_slice(MAGIC);
            blob.extend_from_slice(&nonce);
            blob.extend_from_slice(&sealed);
            private_file::write(&self.blob_path(), &blob).map_err(Error::SecretFile)
        }
    }

    impl SecretBackend for VaultBackend {
        fn set(&self, account: &str, secret: &[u8]) -> Result<()> {
            let _guard = self
                .write
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut items = self.read()?;
            items.insert(account.to_owned(), secret.to_vec());
            self.write_map(&items)
        }

        fn get(&self, account: &str) -> Result<Option<Vec<u8>>> {
            Ok(self.read()?.remove(account))
        }

        fn delete(&self, account: &str) -> Result<()> {
            let _guard = self
                .write
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut items = self.read()?;
            if items.remove(account).is_some() {
                self.write_map(&items)?;
            }
            Ok(())
        }
    }

    /// Whether the OS secret service really works. A service that only answers reads is not
    /// enough — a wallet that was never set up (KWallet on a fresh KDE profile, for example) takes
    /// the request and drops it — so the probe stores, reads back and removes one item. Without
    /// this every identity and server password would fail instead of landing in the file.
    pub(super) fn secret_service_usable(service: &str) -> bool {
        let probe = || -> keyring::Result<()> {
            let entry = keyring::Entry::new(service, "mumcord-probe")?;
            entry.set_secret(b"probe")?;
            entry.get_secret()?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(e),
            }
        };
        match probe() {
            Ok(()) => true,
            Err(e) => {
                tracing::info!(error = %e, "the OS secret service probe failed");
                false
            }
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

/// Access to secrets in the secure storage: the OS store on desktop, an injected
/// [`SecretBackend`] elsewhere.
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
    /// The OS secure storage under [`KEYRING_SERVICE`]. Not available on Android and iOS; use
    /// [`with_backend`](Self::with_backend) there.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn os() -> Self {
        Self::os_with_service(KEYRING_SERVICE)
    }

    /// The OS secure storage under [`KEYRING_SERVICE`], falling back to the encrypted secret file
    /// in `profile_dir` where no Secret Service answers (FreeBSD). Once a profile has used one
    /// of the two it sticks with it (see `backend_choice`). Every other platform ignores the
    /// directory and uses the OS store only. Not available on Android and iOS; use
    /// [`with_backend`](Self::with_backend) there.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn os_with_profile(profile_dir: impl Into<std::path::PathBuf>) -> Self {
        #[cfg(target_os = "freebsd")]
        {
            let dir = profile_dir.into();
            match backend_choice::choose(&dir, || vault::secret_service_usable(KEYRING_SERVICE)) {
                backend_choice::Choice::Os => Self::os_with_service(KEYRING_SERVICE),
                backend_choice::Choice::File => {
                    Self::with_backend(Arc::new(vault::VaultBackend::new(dir)))
                }
            }
        }
        #[cfg(not(target_os = "freebsd"))]
        {
            let _ = profile_dir;
            Self::os()
        }
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn os_with_service(service: &str) -> Self {
        Self::with_backend(Arc::new(KeyringBackend {
            service: service.to_owned(),
        }))
    }

    /// A store on top of the given secure storage, for platforms without an OS backend in this
    /// crate (Android, iOS): the embedding app implements [`SecretBackend`] and passes it here.
    pub fn with_backend(backend: Arc<dyn SecretBackend>) -> Self {
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

/// The error of a denied keychain prompt, for the test backends.
#[cfg(any(test, feature = "test-util"))]
fn denied() -> Error {
    Error::SecureStorage(SecretBackendError::new("denied"))
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
                return Err(denied());
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

    /// The FreeBSD fallback: the same items survive a restart, the blob is unreadable without
    /// its key, and both files stay private.
    #[cfg(target_os = "freebsd")]
    #[test]
    fn vault_round_trip() {
        let dir = tempfile::tempdir().expect("temp dir");
        let backend = Arc::new(super::vault::VaultBackend::new(dir.path().to_path_buf()));
        let store = SecretStore::with_backend(backend.clone());
        assert_eq!(store.server_password(5).expect("none"), None);
        store.set_server_password(5, "hunter2").expect("set");
        store
            .set_access_tokens(5, &["token".to_owned()])
            .expect("tokens");

        // A new session reads the same file.
        let reopened = SecretStore::with_backend(Arc::new(super::vault::VaultBackend::new(
            dir.path().to_path_buf(),
        )));
        assert_eq!(
            reopened.server_password(5).expect("get").as_deref(),
            Some("hunter2")
        );
        assert_eq!(reopened.access_tokens(5).expect("tokens"), ["token"]);

        for name in ["secrets.vault", "secrets.key"] {
            let path = dir.path().join(name);
            let mode = std::os::unix::fs::PermissionsExt::mode(
                &std::fs::metadata(&path).expect("metadata").permissions(),
            );
            assert_eq!(
                mode & 0o077,
                0,
                "{name} must not be group or world readable"
            );
        }

        // Rewrites go through a temporary file and a rename: no leftovers, no window in which
        // the stored secrets are missing.
        store.set_server_password(5, "hunter3").expect("overwrite");
        let third = SecretStore::with_backend(Arc::new(super::vault::VaultBackend::new(
            dir.path().to_path_buf(),
        )));
        assert_eq!(
            third.server_password(5).expect("get").as_deref(),
            Some("hunter3")
        );
        let leftovers: Vec<String> = std::fs::read_dir(dir.path())
            .expect("dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert_eq!(leftovers, Vec::<String>::new());
        let blob = std::fs::read(dir.path().join("secrets.vault")).expect("blob");
        assert!(
            !blob.windows(7).any(|w| w == b"hunter2"),
            "the password must not be in the file in clear text"
        );

        store.delete_server_password(5).expect("clear");
        // A fresh session sees the removal; the open one still has its cached copy.
        let third = SecretStore::with_backend(Arc::new(super::vault::VaultBackend::new(
            dir.path().to_path_buf(),
        )));
        assert_eq!(third.server_password(5).expect("get"), None);
    }

    #[cfg(unix)]
    fn mode_of(path: &std::path::Path) -> u32 {
        std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(path).expect("metadata").permissions(),
        ) & 0o777
    }

    #[cfg(unix)]
    fn set_mode(path: &std::path::Path, mode: u32) {
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(mode))
            .expect("chmod");
    }

    #[cfg(unix)]
    #[test]
    fn fresh_secret_files_are_private() {
        let dir = tempfile::tempdir().expect("temp dir");
        let profile = dir.path().join("profile");
        let key = profile.join("secrets.key");
        super::private_file::write(&key, b"key").expect("write");
        assert_eq!(mode_of(&key), 0o600);
        assert_eq!(mode_of(&profile) & 0o077, 0, "a new profile folder is 0700");
        assert_eq!(
            super::private_file::read(&key).expect("read").as_deref(),
            Some(&b"key"[..])
        );
        assert_eq!(
            super::private_file::read(&profile.join("missing")).expect("missing"),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_stale_temp_file_does_not_pass_on_its_permissions() {
        let dir = tempfile::tempdir().expect("temp dir");
        let temp = dir.path().join("secrets.key.tmp");
        std::fs::write(&temp, b"old").expect("stale temp");
        set_mode(&temp, 0o644);
        let key = dir.path().join("secrets.key");
        super::private_file::write(&key, b"key").expect("write");
        assert_eq!(mode_of(&key), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn a_secret_file_open_to_others_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let key = dir.path().join("secrets.key");
        super::private_file::write(&key, b"key").expect("write");
        for mode in [0o644, 0o640, 0o604, 0o620, 0o602] {
            set_mode(&key, mode);
            let error = super::private_file::read(&key).expect_err("refused");
            assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
            let message = error.to_string();
            assert!(message.contains("secrets.key"), "{message}");
            assert!(
                !message.contains(&*dir.path().to_string_lossy()),
                "the full path is not in the error: {message}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_secret_file_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("elsewhere");
        super::private_file::write(&target, b"key").expect("write");
        let key = dir.path().join("secrets.key");
        std::os::unix::fs::symlink(&target, &key).expect("symlink");
        let error = super::private_file::read(&key).expect_err("refused");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(error.to_string().contains("symbolic link"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn secret_files_are_not_written_into_a_folder_others_can_change() {
        let dir = tempfile::tempdir().expect("temp dir");
        set_mode(dir.path(), 0o777);
        let error = super::private_file::write(&dir.path().join("secrets.key"), b"key")
            .expect_err("refused");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(!dir.path().join("secrets.key").exists());
        set_mode(dir.path(), 0o700);
    }

    /// A profile sticks with the store it used: a Secret Service that is late or locked at one
    /// start does not send new secrets to the file, and an existing file is not abandoned when a
    /// Secret Service appears.
    #[cfg(unix)]
    #[test]
    fn the_secret_store_choice_sticks_to_the_profile() {
        use super::backend_choice::{Choice, choose};

        let fresh = tempfile::tempdir().expect("temp dir");
        assert_eq!(choose(fresh.path(), || false), Choice::File);
        // Nothing was stored yet, so the next start may still pick the Secret Service.
        assert_eq!(choose(fresh.path(), || true), Choice::Os);
        // From now on the profile uses the Secret Service, even when it does not answer.
        assert_eq!(
            choose(fresh.path(), || panic!("not probed again")),
            Choice::Os
        );

        let with_file = tempfile::tempdir().expect("temp dir");
        super::private_file::write(&with_file.path().join("secrets.vault"), b"blob")
            .expect("write");
        assert_eq!(
            choose(with_file.path(), || panic!("not probed")),
            Choice::File
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

    /// A backend written against the public interface only, like the one of a phone app: its
    /// own storage and its own error type.
    #[derive(Default)]
    struct EmbedderBackend {
        items: Mutex<HashMap<String, Vec<u8>>>,
        locked: std::sync::atomic::AtomicBool,
    }

    #[derive(Debug)]
    struct DeviceLocked;

    impl std::fmt::Display for DeviceLocked {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("the device is locked")
        }
    }

    impl std::error::Error for DeviceLocked {}

    impl EmbedderBackend {
        fn items(&self) -> Result<MutexGuard<'_, HashMap<String, Vec<u8>>>> {
            if self.locked.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(Error::SecureStorage(crate::SecretBackendError::new(
                    DeviceLocked,
                )));
            }
            Ok(lock(&self.items))
        }
    }

    impl SecretBackend for EmbedderBackend {
        fn set(&self, account: &str, secret: &[u8]) -> Result<()> {
            self.items()?.insert(account.to_owned(), secret.to_vec());
            Ok(())
        }
        fn get(&self, account: &str) -> Result<Option<Vec<u8>>> {
            Ok(self.items()?.get(account).cloned())
        }
        fn delete(&self, account: &str) -> Result<()> {
            self.items()?.remove(account);
            Ok(())
        }
    }

    #[test]
    fn an_injected_backend_holds_all_secrets() {
        let backend = Arc::new(EmbedderBackend::default());
        exercise(&SecretStore::with_backend(backend.clone()));

        // Identities and server secrets are found again after a restart.
        let store = SecretStore::with_backend(backend.clone());
        store.save_identity(3, &IDENTITY).expect("save");
        store.set_server_password(12, "hunter2").expect("set");
        let restarted = SecretStore::with_backend(backend);
        assert_eq!(
            restarted.load_identity(3).expect("load").as_ref(),
            Some(&*IDENTITY)
        );
        assert_eq!(
            restarted.server_password(12).expect("get").as_deref(),
            Some("hunter2")
        );
    }

    #[test]
    fn an_injected_backend_reports_its_own_error() {
        let backend = Arc::new(EmbedderBackend::default());
        backend
            .locked
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let store = SecretStore::with_backend(backend);

        let error = store
            .set_server_password(12, "hunter2")
            .expect_err("locked storage");
        assert!(matches!(error, Error::SecureStorage(_)), "{error:?}");
        assert_eq!(
            error.to_string(),
            "secure storage error: the device is locked"
        );
    }

    /// Reads succeed, writes fail like a denied prompt for an item of another app version.
    struct FailingWrites(MemoryBackend);

    impl SecretBackend for FailingWrites {
        fn set(&self, _account: &str, _secret: &[u8]) -> Result<()> {
            Err(denied())
        }
        fn get(&self, account: &str) -> Result<Option<Vec<u8>>> {
            self.0.get(account)
        }
        fn delete(&self, _account: &str) -> Result<()> {
            Err(denied())
        }
    }

    /// Runs against the real OS secure storage under a separate test service name and
    /// removes its entries afterwards. Run with:
    /// `cargo test -p mumcord-store -- --ignored os_keyring`
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
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
