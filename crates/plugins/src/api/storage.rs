//! `storage` (FR-6.20): a small key–value store per plugin, kept in the app database
//! (`plugin_kv`). Scope `global` is shared by all identities; scope `identity` follows the
//! identity the service sets with [`Storage::set_identity`]. Also the data helpers the plugin
//! service uses for "Export data", "Reset data" and removing a plugin with its data (FR-6.62).

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use mumcord_store::{IdentityId, MAX_PLUGIN_KEY_BYTES, Store, UnixMillis};
use serde::Serialize;
use wasmtime::component::{HasSelf, Linker};

use crate::api::secrets::SecretsError;
use crate::runtime::bindings::mumcord::plugin::storage::{self, Scope};
use crate::runtime::bindings::mumcord::plugin::types::Error;
use crate::runtime::{HostApi, HostContext, PluginState};

/// `list-keys` returns at most this many keys per call.
pub const MAX_LIST_KEYS: usize = 1000;
const GLOBAL_SCOPE: &str = "global";

/// The database behind `storage` plus the identity whose `identity` scope plugins see.
#[derive(Default)]
pub struct Storage {
    store: Option<Arc<Mutex<Store>>>,
    identity: RwLock<Option<IdentityId>>,
}

impl fmt::Debug for Storage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Storage")
            .field("store", &self.store.is_some())
            .field("identity", &self.identity())
            .finish()
    }
}

impl Storage {
    pub fn new(store: Arc<Mutex<Store>>) -> Self {
        Self {
            store: Some(store),
            identity: RwLock::new(None),
        }
    }

    /// The identity scope plugins see: the focused server's identity, else the default
    /// identity. `None` falls back to the stored default identity; without any identity every
    /// `identity` scope call fails with `invalid`.
    pub fn set_identity(&self, identity: Option<IdentityId>) {
        *self
            .identity
            .write()
            .unwrap_or_else(PoisonError::into_inner) = identity;
    }

    fn identity(&self) -> Option<IdentityId> {
        *self.identity.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn scope_key(&self, scope: Scope) -> Result<String, Error> {
        match scope {
            Scope::Global => Ok(GLOBAL_SCOPE.to_owned()),
            Scope::Identity => self
                .identity()
                .or_else(|| self.default_identity())
                .map(|id| id.to_string())
                .ok_or(Error::Invalid),
        }
    }

    /// Until the host has resolved the focused server's identity (no server focused yet, or
    /// the app is still starting), plugins use the default identity (spec 06 decision #6).
    fn default_identity(&self) -> Option<IdentityId> {
        let store = self.store.as_ref()?;
        let store = store.lock().unwrap_or_else(PoisonError::into_inner);
        match store.default_identity() {
            Ok(record) => record.map(|r| r.id),
            Err(error) => {
                tracing::warn!(%error, "reading the default identity for plugin storage failed");
                None
            }
        }
    }

    /// Runs `f` on the database. A missing or failing database is logged without keys or
    /// values; the WIT has no I/O error, so the plugin gets `quota-exceeded` ("nothing was
    /// stored"), which it has to handle anyway.
    fn with_store<T>(
        &self,
        plugin: &str,
        f: impl FnOnce(&Store) -> mumcord_store::Result<T>,
    ) -> Result<T, Error> {
        let Some(store) = &self.store else {
            tracing::warn!(plugin, "plugin storage is not available");
            return Err(Error::QuotaExceeded);
        };
        let store = store.lock().unwrap_or_else(PoisonError::into_inner);
        f(&store).map_err(|error| match error {
            mumcord_store::Error::PluginQuotaExceeded => Error::QuotaExceeded,
            mumcord_store::Error::PluginValueTooLarge { .. } => Error::TooLarge,
            mumcord_store::Error::InvalidPluginKey { .. } => Error::Invalid,
            error => {
                tracing::warn!(plugin, %error, "plugin storage failed");
                Error::QuotaExceeded
            }
        })
    }

    pub(crate) fn get(
        &self,
        plugin: &str,
        scope: Scope,
        key: &str,
    ) -> Result<Option<Vec<u8>>, Error> {
        check_key(key)?;
        let scope = self.scope_key(scope)?;
        self.with_store(plugin, |s| s.plugin_kv_get(plugin, &scope, key))
    }

    pub(crate) fn set(
        &self,
        plugin: &str,
        quota_bytes: u64,
        scope: Scope,
        key: &str,
        value: &[u8],
    ) -> Result<(), Error> {
        check_key(key)?;
        let scope = self.scope_key(scope)?;
        self.with_store(plugin, |s| {
            s.plugin_kv_set(plugin, &scope, key, value, quota_bytes, now())
        })
    }

    pub(crate) fn delete(&self, plugin: &str, scope: Scope, key: &str) -> Result<(), Error> {
        check_key(key)?;
        let scope = self.scope_key(scope)?;
        self.with_store(plugin, |s| s.plugin_kv_delete(plugin, &scope, key))
    }

    pub(crate) fn list_keys(
        &self,
        plugin: &str,
        scope: Scope,
        prefix: &str,
    ) -> Result<Vec<String>, Error> {
        let scope = self.scope_key(scope)?;
        let keys = self.with_store(plugin, |s| s.plugin_kv_keys(plugin, &scope))?;
        Ok(keys
            .into_iter()
            .filter(|key| key.starts_with(prefix))
            .take(MAX_LIST_KEYS)
            .collect())
    }
}

/// Empty keys are malformed; keys over the limit are `too-large` (the WIT contract), not
/// `invalid` as the store would say.
fn check_key(key: &str) -> Result<(), Error> {
    if key.is_empty() {
        Err(Error::Invalid)
    } else if key.len() > MAX_PLUGIN_KEY_BYTES {
        Err(Error::TooLarge)
    } else {
        Ok(())
    }
}

fn now() -> UnixMillis {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

pub(crate) struct StorageApi;

impl HostApi for StorageApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
        storage::add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
    }
}

impl storage::Host for PluginState {
    fn get(&mut self, scope: Scope, key: String) -> Result<Option<Vec<u8>>, Error> {
        let (ctx, id) = (&self.ctx, &self.id);
        self.clock.waiting(|| ctx.storage.get(id, scope, &key))
    }

    fn set(&mut self, scope: Scope, key: String, value: Vec<u8>) -> Result<(), Error> {
        let (ctx, id, quota) = (&self.ctx, &self.id, self.storage_quota);
        self.clock
            .waiting(|| ctx.storage.set(id, quota, scope, &key, &value))
    }

    fn delete(&mut self, scope: Scope, key: String) -> Result<(), Error> {
        let (ctx, id) = (&self.ctx, &self.id);
        self.clock.waiting(|| ctx.storage.delete(id, scope, &key))
    }

    fn list_keys(&mut self, scope: Scope, prefix: String) -> Result<Vec<String>, Error> {
        let (ctx, id) = (&self.ctx, &self.id);
        self.clock
            .waiting(|| ctx.storage.list_keys(id, scope, &prefix))
    }
}

/// Why a data helper failed.
#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("plugin storage is not available")]
    NoStore,
    #[error(transparent)]
    Store(#[from] mumcord_store::Error),
    #[error(transparent)]
    Secrets(#[from] SecretsError),
    #[error("could not write the export: {0}")]
    Export(#[from] serde_json::Error),
}

/// The "Export data" file (FR-6.62): every stored value of a plugin. Secrets are never
/// exported.
#[derive(Debug, Serialize)]
struct Export<'a> {
    format: u32,
    plugin: &'a str,
    entries: Vec<ExportEntry>,
}

#[derive(Debug, Serialize)]
struct ExportEntry {
    /// `global` or `identity:<id>`.
    scope: String,
    key: String,
    /// The value as text when it is UTF-8 (plugins mostly store JSON), else as base64.
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    base64: Option<String>,
}

impl HostContext {
    fn store(&self) -> Result<std::sync::MutexGuard<'_, Store>, DataError> {
        let store = self.storage.store.as_ref().ok_or(DataError::NoStore)?;
        Ok(store.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The pretty-printed JSON for "Export data" of plugin `id`.
    pub fn export_plugin_data(&self, id: &str) -> Result<String, DataError> {
        let rows = self.store()?.plugin_kv_all(id)?;
        let entries = rows
            .into_iter()
            .map(|(scope, key, value)| {
                let scope = if scope == GLOBAL_SCOPE {
                    scope
                } else {
                    format!("identity:{scope}")
                };
                let (text, base64) = match String::from_utf8(value) {
                    Ok(text) => (Some(text), None),
                    Err(e) => (
                        None,
                        Some(base64::engine::general_purpose::STANDARD.encode(e.into_bytes())),
                    ),
                };
                ExportEntry {
                    scope,
                    key,
                    text,
                    base64,
                }
            })
            .collect();
        Ok(serde_json::to_string_pretty(&Export {
            format: 1,
            plugin: id,
            entries,
        })?)
    }

    /// "Reset data": deletes all stored values and secrets of plugin `id`; the plugin stays
    /// installed.
    pub fn reset_plugin_data(&self, id: &str) -> Result<(), DataError> {
        self.store()?.plugin_kv_clear(id)?;
        self.delete_plugin_secrets(id)
    }

    /// Remove with "Also delete its data": the plugin's record, stored values and secrets.
    pub fn delete_plugin_data(&self, id: &str) -> Result<(), DataError> {
        // plugin_kv rows go with the record (ON DELETE CASCADE).
        self.store()?.delete_plugin(id)?;
        self.delete_plugin_secrets(id)
    }

    /// Deletes plugins removed more than 30 days ago together with their secrets (FR-6.62).
    /// Returns their ids.
    pub fn purge_removed_plugins(&self, now: UnixMillis) -> Result<Vec<String>, DataError> {
        let ids = self.store()?.purge_expired_plugins(now)?;
        for id in &ids {
            self.delete_plugin_secrets(id)?;
        }
        Ok(ids)
    }

    fn delete_plugin_secrets(&self, id: &str) -> Result<(), DataError> {
        if let Some(secrets) = &self.secrets {
            secrets.delete_all(id)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use mumcord_store::{MAX_PLUGIN_VALUE_BYTES, NewIdentity, PluginRecord, PluginVisibility};

    use super::*;
    use crate::api::secrets::PluginSecrets;

    const QUOTA: u64 = 256 * 1024;

    fn record(id: &str) -> PluginRecord {
        PluginRecord {
            id: id.into(),
            version: "1.0.0".into(),
            previous_version: None,
            source: "official".into(),
            sha256: None,
            author_key: None,
            enabled: true,
            disabled_reason: None,
            visibility: PluginVisibility::Friends,
            granted: vec![],
            pinned: false,
            notifications: true,
            removed_at: None,
            installed_at: 0,
        }
    }

    fn context() -> (tempfile::TempDir, HostContext) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path()).expect("store");
        store.save_plugin(&record("pet")).expect("save");
        store.save_plugin(&record("other")).expect("save");
        let ctx = HostContext {
            storage: Storage::new(Arc::new(Mutex::new(store))),
            secrets: Some(Arc::new(PluginSecrets::in_memory())),
            ..HostContext::default()
        };
        ctx.storage.set_identity(Some(7));
        (dir, ctx)
    }

    #[test]
    fn values_round_trip_per_scope_and_plugin() {
        let (_dir, ctx) = context();
        let s = &ctx.storage;
        s.set("pet", QUOTA, Scope::Identity, "name", b"Pip")
            .expect("set");
        s.set("pet", QUOTA, Scope::Global, "name", b"shared")
            .expect("set");
        assert_eq!(
            s.get("pet", Scope::Identity, "name"),
            Ok(Some(b"Pip".to_vec()))
        );
        assert_eq!(
            s.get("pet", Scope::Global, "name"),
            Ok(Some(b"shared".to_vec()))
        );
        assert_eq!(s.get("other", Scope::Global, "name"), Ok(None));

        s.set_identity(Some(8));
        assert_eq!(s.get("pet", Scope::Identity, "name"), Ok(None));

        s.delete("pet", Scope::Global, "name").expect("delete");
        s.delete("pet", Scope::Global, "missing")
            .expect("missing is fine");
        assert_eq!(s.get("pet", Scope::Global, "name"), Ok(None));
    }

    /// F51: a plugin that starts before any server is focused (or before the host resolved
    /// the focused server's identity) uses the default identity instead of failing.
    #[test]
    fn without_a_focused_identity_the_default_identity_is_used() {
        let (_dir, ctx) = context();
        let s = &ctx.storage;
        let default = {
            let store = s.store.as_ref().expect("store");
            let store = store.lock().expect("lock");
            let default = store
                .add_identity(&NewIdentity::new("Main", "main", "aa", 1))
                .expect("identity");
            store
                .add_identity(&NewIdentity::new("Work", "work", "bb", 2))
                .expect("identity");
            default.id
        };
        s.set_identity(None);
        s.set("pet", QUOTA, Scope::Identity, "name", b"Pip")
            .expect("set");

        s.set_identity(Some(default));
        assert_eq!(
            s.get("pet", Scope::Identity, "name"),
            Ok(Some(b"Pip".to_vec())),
            "the fallback wrote to the default identity"
        );
        s.set_identity(Some(default + 1));
        assert_eq!(
            s.get("pet", Scope::Identity, "name"),
            Ok(None),
            "focusing a server with another identity switches the scope"
        );
    }

    #[test]
    fn without_an_identity_the_identity_scope_is_invalid() {
        let (_dir, ctx) = context();
        ctx.storage.set_identity(None);
        assert_eq!(
            ctx.storage.get("pet", Scope::Identity, "k"),
            Err(Error::Invalid)
        );
        assert_eq!(
            ctx.storage.set("pet", QUOTA, Scope::Identity, "k", b"v"),
            Err(Error::Invalid)
        );
        assert_eq!(ctx.storage.get("pet", Scope::Global, "k"), Ok(None));
    }

    #[test]
    fn keys_and_values_are_limited() {
        let (_dir, ctx) = context();
        let s = &ctx.storage;
        assert_eq!(s.get("pet", Scope::Global, ""), Err(Error::Invalid));
        let long_key = "k".repeat(MAX_PLUGIN_KEY_BYTES + 1);
        assert_eq!(s.get("pet", Scope::Global, &long_key), Err(Error::TooLarge));
        assert_eq!(
            s.set("pet", QUOTA, Scope::Global, &long_key, b"v"),
            Err(Error::TooLarge)
        );
        assert_eq!(
            s.delete("pet", Scope::Global, &long_key),
            Err(Error::TooLarge)
        );
        let max_key = "k".repeat(MAX_PLUGIN_KEY_BYTES);
        s.set("pet", QUOTA, Scope::Global, &max_key, b"v")
            .expect("128 bytes is fine");
        let big = vec![0; MAX_PLUGIN_VALUE_BYTES + 1];
        assert_eq!(
            s.set("pet", QUOTA, Scope::Global, "big", &big),
            Err(Error::TooLarge)
        );
        s.set("pet", QUOTA, Scope::Global, "big", &big[1..])
            .expect("64 KiB is fine");
    }

    #[test]
    fn writes_over_the_quota_fail_and_store_nothing() {
        let (_dir, ctx) = context();
        let s = &ctx.storage;
        let quota = 100;
        s.set("pet", quota, Scope::Global, "a", &[1; 60])
            .expect("fits");
        assert_eq!(
            s.set("pet", quota, Scope::Identity, "b", &[2; 60]),
            Err(Error::QuotaExceeded)
        );
        assert_eq!(s.get("pet", Scope::Identity, "b"), Ok(None));
        // Replacing a value only counts the new size.
        s.set("pet", quota, Scope::Global, "a", &[3; 90])
            .expect("replace fits");
    }

    #[test]
    fn list_keys_filters_by_prefix_sorted_and_capped() {
        let (_dir, ctx) = context();
        let s = &ctx.storage;
        for key in ["bots/b", "bots/a", "pet", "botsy"] {
            s.set("pet", QUOTA, Scope::Global, key, b"1").expect("set");
        }
        s.set("pet", QUOTA, Scope::Identity, "bots/c", b"1")
            .expect("set");
        assert_eq!(
            s.list_keys("pet", Scope::Global, "bots/"),
            Ok(vec!["bots/a".to_owned(), "bots/b".to_owned()])
        );
        assert_eq!(
            s.list_keys("pet", Scope::Global, "").map(|k| k.len()),
            Ok(4)
        );

        for i in 0..MAX_LIST_KEYS + 5 {
            s.set("other", u64::MAX, Scope::Global, &format!("k{i:05}"), b"")
                .expect("set");
        }
        let keys = s.list_keys("other", Scope::Global, "").expect("list");
        assert_eq!(keys.len(), MAX_LIST_KEYS);
        assert_eq!(keys[0], "k00000");
    }

    #[test]
    fn without_a_database_calls_fail_cleanly() {
        let ctx = HostContext::default();
        assert_eq!(
            ctx.storage.get("pet", Scope::Global, "k"),
            Err(Error::QuotaExceeded)
        );
        assert!(matches!(
            ctx.export_plugin_data("pet"),
            Err(DataError::NoStore)
        ));
    }

    #[test]
    fn export_lists_text_and_binary_values_but_no_secrets() {
        let (_dir, ctx) = context();
        let s = &ctx.storage;
        s.set("pet", QUOTA, Scope::Global, "cfg", br#"{"a":1}"#)
            .expect("set");
        s.set("pet", QUOTA, Scope::Identity, "raw", &[0xff, 0x00])
            .expect("set");
        let secrets = ctx.secrets.as_ref().expect("secrets");
        secrets.set("pet", "token", "hunter2").expect("secret");

        let json: serde_json::Value =
            serde_json::from_str(&ctx.export_plugin_data("pet").expect("export")).expect("json");
        assert_eq!(
            json,
            serde_json::json!({
                "format": 1,
                "plugin": "pet",
                "entries": [
                    {"scope": "identity:7", "key": "raw", "base64": "/wA="},
                    {"scope": "global", "key": "cfg", "text": "{\"a\":1}"},
                ],
            })
        );
    }

    #[test]
    fn reset_clears_values_and_secrets_but_keeps_the_plugin() {
        let (_dir, ctx) = context();
        ctx.storage
            .set("pet", QUOTA, Scope::Global, "k", b"v")
            .expect("set");
        ctx.storage
            .set("other", QUOTA, Scope::Global, "k", b"v")
            .expect("set");
        let secrets = ctx.secrets.as_ref().expect("secrets");
        secrets.set("pet", "token", "x").expect("secret");

        ctx.reset_plugin_data("pet").expect("reset");
        assert_eq!(ctx.storage.get("pet", Scope::Global, "k"), Ok(None));
        assert_eq!(secrets.exists("pet", "token"), Ok(false));
        assert_eq!(
            ctx.storage.get("other", Scope::Global, "k"),
            Ok(Some(b"v".to_vec()))
        );
        assert!(
            ctx.store()
                .expect("store")
                .plugin("pet")
                .expect("read")
                .is_some()
        );
    }

    #[test]
    fn delete_and_purge_remove_record_values_and_secrets() {
        let (_dir, ctx) = context();
        let secrets = ctx.secrets.as_ref().expect("secrets");
        for id in ["pet", "other"] {
            ctx.storage
                .set(id, QUOTA, Scope::Global, "k", b"v")
                .expect("set");
            secrets.set(id, "token", "x").expect("secret");
        }

        ctx.delete_plugin_data("pet").expect("delete");
        assert!(
            ctx.store()
                .expect("store")
                .plugin("pet")
                .expect("read")
                .is_none()
        );
        assert!(
            ctx.store()
                .expect("store")
                .plugin_kv_all("pet")
                .expect("read")
                .is_empty()
        );
        assert_eq!(secrets.exists("pet", "token"), Ok(false));

        ctx.store()
            .expect("store")
            .mark_plugin_removed("other", 1_000)
            .expect("remove");
        let day = 24 * 60 * 60 * 1000;
        assert_eq!(
            ctx.purge_removed_plugins(1_000 + 29 * day).expect("purge"),
            Vec::<String>::new()
        );
        assert_eq!(secrets.exists("other", "token"), Ok(true));
        assert_eq!(
            ctx.purge_removed_plugins(1_000 + 31 * day).expect("purge"),
            vec!["other".to_owned()]
        );
        assert_eq!(secrets.exists("other", "token"), Ok(false));
    }
}
