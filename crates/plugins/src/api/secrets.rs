//! `secrets` (FR-6.25, official plugins only): named secrets in the OS keychain under service
//! [`SERVICE`], account `<plugin id>/<name>`. Only the user writes them, through a `password`
//! field ([`PluginSecrets::set`]); plugins can ask whether one exists and delete it, never read
//! it. `net.http` references them by name. Without a working keychain the permission is
//! unavailable ([`UNAVAILABLE_TEXT`]); there is no plaintext fallback.
//!
//! The keychain cannot list items, so each plugin also has an index item at account
//! `<plugin id>` (plugin ids have no `/`, so it cannot clash with a secret) holding the names as
//! a JSON array. "Reset data" and removing with data delete everything the index names.
//!
//! Every keychain item can cost the user a prompt (unsigned macOS builds), so the index is read
//! at most once per plugin per session and cached; `exists` answers from it. The OS calls can
//! block (e.g. while a wallet unlock prompt is open); they run on the plugin's own thread or
//! a blocking thread of the service, never on the audio path.

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use wasmtime::component::{HasSelf, Linker};

use crate::runtime::bindings::mumcord::plugin::secrets;
use crate::runtime::bindings::mumcord::plugin::types::Error;
use crate::runtime::{HostApi, PluginState};

/// Keychain service of plugin secrets.
pub const SERVICE: &str = "Mumcord plugins";
/// The catalogue permission every `secrets` call needs.
const PERMISSION: &str = "secrets";
/// Longest secret name in bytes.
pub const MAX_NAME_BYTES: usize = 128;
/// Shown instead of the permission when the keychain does not work.
pub const UNAVAILABLE_TEXT: &str = "Secure storage is not available on this system";
/// Account read once to learn whether the keychain works; never written.
const PROBE_ACCOUNT: &str = "availability-check";

/// Why a keychain operation failed. Never carries a secret value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretsError {
    #[error("{UNAVAILABLE_TEXT}")]
    Unavailable,
    #[error("the secret name is empty, too long or contains control characters")]
    InvalidName,
    #[error("the keychain failed: {0}")]
    Keychain(String),
}

trait Backend: Send + Sync {
    fn set(&self, account: &str, secret: &[u8]) -> Result<(), SecretsError>;
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, SecretsError>;
    /// Deleting a missing entry is not an error.
    fn delete(&self, account: &str) -> Result<(), SecretsError>;
}

struct KeyringBackend;

fn entry(account: &str) -> Result<keyring::Entry, SecretsError> {
    keyring::Entry::new(SERVICE, account).map_err(keychain_error)
}

fn keychain_error(error: keyring::Error) -> SecretsError {
    match error {
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => {
            SecretsError::Unavailable
        }
        error => SecretsError::Keychain(error.to_string()),
    }
}

impl Backend for KeyringBackend {
    fn set(&self, account: &str, secret: &[u8]) -> Result<(), SecretsError> {
        entry(account)?.set_secret(secret).map_err(keychain_error)
    }

    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, SecretsError> {
        match entry(account)?.get_secret() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keychain_error(e)),
        }
    }

    fn delete(&self, account: &str) -> Result<(), SecretsError> {
        match entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keychain_error(e)),
        }
    }
}

/// The plugin secrets in the OS keychain.
pub struct PluginSecrets {
    backend: Box<dyn Backend>,
    available: OnceLock<bool>,
    /// Names per plugin, read from the index item once. The lock also serialises the index
    /// read-modify-write.
    names: Mutex<HashMap<String, BTreeSet<String>>>,
}

impl fmt::Debug for PluginSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginSecrets")
            .field("available", &self.available.get())
            .finish_non_exhaustive()
    }
}

impl PluginSecrets {
    /// The OS keychain under [`SERVICE`].
    pub fn os() -> Self {
        Self::with_backend(Box::new(KeyringBackend))
    }

    fn with_backend(backend: Box<dyn Backend>) -> Self {
        Self {
            backend,
            available: OnceLock::new(),
            names: Mutex::new(HashMap::new()),
        }
    }

    /// Whether the keychain works; checked once per session. When it does not, the `secrets`
    /// permission is unavailable and the UI shows [`UNAVAILABLE_TEXT`].
    pub fn available(&self) -> bool {
        *self.available.get_or_init(|| {
            let ok = self.backend.get(PROBE_ACCOUNT).is_ok();
            if !ok {
                tracing::warn!("the keychain is not available; plugin secrets are off");
            }
            ok
        })
    }

    fn check(&self, name: &str) -> Result<(), SecretsError> {
        if !self.available() {
            return Err(SecretsError::Unavailable);
        }
        if name.is_empty() || name.len() > MAX_NAME_BYTES || name.chars().any(char::is_control) {
            return Err(SecretsError::InvalidName);
        }
        Ok(())
    }

    /// Stores what the user typed into a plugin's `password` field. The only way a secret is
    /// written.
    pub fn set(&self, plugin: &str, name: &str, value: &str) -> Result<(), SecretsError> {
        self.check(name)?;
        let mut names = self.lock();
        self.backend.set(&account(plugin, name), value.as_bytes())?;
        let mut list = self.names_of(&mut names, plugin)?.clone();
        if list.insert(name.to_owned()) {
            self.save_index(&mut names, plugin, list)?;
        }
        Ok(())
    }

    /// The secret for a `net.http` header (FR-6.37). Never handed to a plugin.
    #[allow(dead_code, reason = "read by the net.http executor (B19)")]
    pub(crate) fn get(&self, plugin: &str, name: &str) -> Result<Option<String>, SecretsError> {
        self.check(name)?;
        let _names = self.lock();
        Ok(self
            .backend
            .get(&account(plugin, name))?
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
    }

    pub fn exists(&self, plugin: &str, name: &str) -> Result<bool, SecretsError> {
        self.check(name)?;
        let mut names = self.lock();
        Ok(self.names_of(&mut names, plugin)?.contains(name))
    }

    /// Deletes one secret; a missing one is not an error.
    pub fn delete(&self, plugin: &str, name: &str) -> Result<(), SecretsError> {
        self.check(name)?;
        let mut names = self.lock();
        self.backend.delete(&account(plugin, name))?;
        let mut list = self.names_of(&mut names, plugin)?.clone();
        if list.remove(name) {
            self.save_index(&mut names, plugin, list)?;
        }
        Ok(())
    }

    /// Deletes every secret of a plugin ("Reset data", remove with data). Does nothing without
    /// a keychain, since nothing can have been stored.
    pub fn delete_all(&self, plugin: &str) -> Result<(), SecretsError> {
        if !self.available() {
            return Ok(());
        }
        let mut names = self.lock();
        let list = self.names_of(&mut names, plugin)?.clone();
        for name in &list {
            self.backend.delete(&account(plugin, name))?;
        }
        self.backend.delete(plugin)?;
        names.insert(plugin.to_owned(), BTreeSet::new());
        Ok(())
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, BTreeSet<String>>> {
        self.names.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn names_of<'a>(
        &self,
        names: &'a mut HashMap<String, BTreeSet<String>>,
        plugin: &str,
    ) -> Result<&'a BTreeSet<String>, SecretsError> {
        if !names.contains_key(plugin) {
            let list = match self.backend.get(plugin)? {
                // A damaged index lists nothing; its secrets are overwritten when set again.
                Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                    tracing::warn!(plugin, %error, "unreadable plugin secrets index");
                    BTreeSet::new()
                }),
                None => BTreeSet::new(),
            };
            names.insert(plugin.to_owned(), list);
        }
        Ok(&names[plugin])
    }

    fn save_index(
        &self,
        names: &mut HashMap<String, BTreeSet<String>>,
        plugin: &str,
        list: BTreeSet<String>,
    ) -> Result<(), SecretsError> {
        if list.is_empty() {
            self.backend.delete(plugin)?;
        } else {
            let json =
                serde_json::to_vec(&list).map_err(|e| SecretsError::Keychain(e.to_string()))?;
            self.backend.set(plugin, &json)?;
        }
        names.insert(plugin.to_owned(), list);
        Ok(())
    }
}

fn account(plugin: &str, name: &str) -> String {
    format!("{plugin}/{name}")
}

/// What a plugin sees of a keychain failure: `not-permitted` when there is no keychain (the
/// permission is unavailable), `invalid` for a bad name. Other failures are logged without the
/// name and also reported as `not-permitted`, the one error every caller handles.
fn plugin_error(plugin: &str, error: SecretsError) -> Error {
    match error {
        SecretsError::InvalidName => Error::Invalid,
        SecretsError::Unavailable => Error::NotPermitted,
        SecretsError::Keychain(message) => {
            tracing::warn!(plugin, %message, "plugin secrets failed");
            Error::NotPermitted
        }
    }
}

pub(crate) struct SecretsApi;

impl HostApi for SecretsApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
        secrets::add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
    }
}

impl PluginState {
    /// The shared permission gate: grants can change while the plugin runs.
    fn permit_secrets(&self) -> Result<(), Error> {
        self.ctx.grants.check(&self.id, PERMISSION)
    }

    fn plugin_secrets(&self) -> Result<&PluginSecrets, Error> {
        self.ctx.secrets.as_deref().ok_or(Error::NotPermitted)
    }
}

impl secrets::Host for PluginState {
    fn exists(&mut self, name: String) -> Result<bool, Error> {
        self.permit_secrets()?;
        if name.len() > MAX_NAME_BYTES {
            return Err(Error::TooLarge);
        }
        self.plugin_secrets()?
            .exists(&self.id, &name)
            .map_err(|e| plugin_error(&self.id, e))
    }

    fn delete(&mut self, name: String) -> Result<(), Error> {
        self.permit_secrets()?;
        if name.len() > MAX_NAME_BYTES {
            return Err(Error::TooLarge);
        }
        self.plugin_secrets()?
            .delete(&self.id, &name)
            .map_err(|e| plugin_error(&self.id, e))
    }
}

#[cfg(test)]
mod memory {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Default)]
    pub(super) struct State {
        pub items: Mutex<HashMap<String, Vec<u8>>>,
        pub reads: AtomicUsize,
        pub broken: AtomicBool,
    }

    pub(super) struct MemoryBackend(pub Arc<State>);

    impl MemoryBackend {
        fn fail(&self) -> Result<(), SecretsError> {
            if self.0.broken.load(Ordering::Relaxed) {
                Err(SecretsError::Unavailable)
            } else {
                Ok(())
            }
        }
    }

    impl Backend for MemoryBackend {
        fn set(&self, account: &str, secret: &[u8]) -> Result<(), SecretsError> {
            self.fail()?;
            self.0
                .items
                .lock()
                .expect("lock")
                .insert(account.to_owned(), secret.to_vec());
            Ok(())
        }

        fn get(&self, account: &str) -> Result<Option<Vec<u8>>, SecretsError> {
            self.fail()?;
            self.0.reads.fetch_add(1, Ordering::Relaxed);
            Ok(self.0.items.lock().expect("lock").get(account).cloned())
        }

        fn delete(&self, account: &str) -> Result<(), SecretsError> {
            self.fail()?;
            self.0.items.lock().expect("lock").remove(account);
            Ok(())
        }
    }

    impl PluginSecrets {
        pub(crate) fn in_memory() -> Self {
            Self::with_backend(Box::new(MemoryBackend(Arc::default())))
        }

        pub(super) fn in_memory_with(state: Arc<State>) -> Self {
            Self::with_backend(Box::new(MemoryBackend(state)))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use super::memory::State;
    use super::*;

    fn secrets() -> (Arc<State>, PluginSecrets) {
        let state = Arc::new(State::default());
        let secrets = PluginSecrets::in_memory_with(Arc::clone(&state));
        (state, secrets)
    }

    fn items(state: &State) -> Vec<String> {
        let mut keys: Vec<_> = state.items.lock().expect("lock").keys().cloned().collect();
        keys.sort();
        keys
    }

    #[test]
    fn secrets_live_under_plugin_and_name_and_are_indexed() {
        let (state, s) = secrets();
        s.set("mumbot-music", "bot/1/abc", "key-1").expect("set");
        s.set("mumbot-music", "bot/1/abc", "key-2")
            .expect("replace");
        s.set("other", "token", "x").expect("set");
        assert_eq!(
            items(&state),
            [
                "mumbot-music",
                "mumbot-music/bot/1/abc",
                "other",
                "other/token"
            ]
        );
        assert_eq!(s.get("mumbot-music", "bot/1/abc"), Ok(Some("key-2".into())));
        assert_eq!(s.exists("mumbot-music", "bot/1/abc"), Ok(true));
        assert_eq!(s.exists("mumbot-music", "token"), Ok(false));

        s.delete("mumbot-music", "bot/1/abc").expect("delete");
        s.delete("mumbot-music", "missing")
            .expect("missing is fine");
        assert_eq!(items(&state), ["other", "other/token"]);
    }

    #[test]
    fn the_index_survives_a_restart_and_is_read_once() {
        let (state, s) = secrets();
        s.set("pet", "a", "1").expect("set");
        s.set("pet", "b", "2").expect("set");
        drop(s);

        let s = PluginSecrets::in_memory_with(Arc::clone(&state));
        let before = state.reads.load(Ordering::Relaxed);
        assert_eq!(s.exists("pet", "a"), Ok(true));
        assert_eq!(s.exists("pet", "b"), Ok(true));
        assert_eq!(s.exists("pet", "c"), Ok(false));
        // One probe and one index read.
        assert_eq!(state.reads.load(Ordering::Relaxed) - before, 2);

        s.delete_all("pet").expect("delete all");
        assert!(items(&state).is_empty());
        assert_eq!(s.exists("pet", "a"), Ok(false));
    }

    #[test]
    fn names_are_checked() {
        let (_state, s) = secrets();
        assert_eq!(s.set("pet", "", "x"), Err(SecretsError::InvalidName));
        assert_eq!(s.set("pet", "a\nb", "x"), Err(SecretsError::InvalidName));
        let long = "n".repeat(MAX_NAME_BYTES + 1);
        assert_eq!(s.exists("pet", &long), Err(SecretsError::InvalidName));
        s.set("pet", &"n".repeat(MAX_NAME_BYTES), "x")
            .expect("128 bytes is fine");
    }

    #[test]
    fn without_a_keychain_nothing_is_stored_anywhere() {
        let (state, s) = secrets();
        state.broken.store(true, Ordering::Relaxed);
        assert!(!s.available());
        assert_eq!(s.set("pet", "token", "x"), Err(SecretsError::Unavailable));
        assert_eq!(s.exists("pet", "token"), Err(SecretsError::Unavailable));
        assert_eq!(s.delete_all("pet"), Ok(()));
        assert!(items(&state).is_empty());
        assert_eq!(
            plugin_error("pet", SecretsError::Unavailable),
            Error::NotPermitted
        );
        assert_eq!(
            plugin_error("pet", SecretsError::InvalidName),
            Error::Invalid
        );
    }
}
