//! Persistence for the plugin system (spec 06, section 7.1): installed plugins, their storage,
//! third-party repositories and plugin suggestions made by servers.
//!
//! Plugin storage values are opaque to Mumcord and never logged. Secrets of plugins do not
//! live here but in the OS keychain.

use rusqlite::{OptionalExtension, Row, params};

use crate::db::{Store, UnixMillis};
use crate::error::{Error, Result};
use crate::identities::IdentityId;
use crate::servers::ServerId;

/// Longest storage key a plugin may use, in bytes (FR-6.20).
pub const MAX_PLUGIN_KEY_BYTES: usize = 128;
/// Largest storage value a plugin may write, in bytes (FR-6.20).
pub const MAX_PLUGIN_VALUE_BYTES: usize = 64 * 1024;
/// Most third-party repositories a user can add (FR-6.74).
pub const MAX_PLUGIN_REPOSITORIES: usize = 10;
/// How long the data of a removed plugin is kept before it is purged (FR-6.62).
pub const REMOVED_DATA_RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// Who may see that the user runs a plugin (FR-6.35).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PluginVisibility {
    #[default]
    Everyone,
    Friends,
    Nobody,
}

impl PluginVisibility {
    fn as_str(self) -> &'static str {
        match self {
            Self::Everyone => "everyone",
            Self::Friends => "friends",
            Self::Nobody => "nobody",
        }
    }

    fn from_db(value: &str) -> Self {
        match value {
            "friends" => Self::Friends,
            "nobody" => Self::Nobody,
            _ => Self::Everyone,
        }
    }
}

/// One installed plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRecord {
    pub id: String,
    pub version: String,
    pub previous_version: Option<String>,
    /// `official`, `repo:<repository id>` or `dev:<folder path>`.
    pub source: String,
    /// Package hash; `None` for dev plugins.
    pub sha256: Option<String>,
    /// Pinned author key; `None` for dev plugins.
    pub author_key: Option<String>,
    pub enabled: bool,
    /// `crashes`, `revoked`, `tampered`, `user` or `permission`.
    pub disabled_reason: Option<String>,
    pub visibility: PluginVisibility,
    /// Granted permission ids.
    pub granted: Vec<String>,
    pub pinned: bool,
    pub notifications: bool,
    /// Set when removed with its data kept.
    pub removed_at: Option<UnixMillis>,
    pub installed_at: UnixMillis,
}

impl PluginRecord {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let granted: String = row.get(9)?;
        Ok(Self {
            id: row.get(0)?,
            version: row.get(1)?,
            previous_version: row.get(2)?,
            source: row.get(3)?,
            sha256: row.get(4)?,
            author_key: row.get(5)?,
            enabled: row.get(6)?,
            disabled_reason: row.get(7)?,
            visibility: PluginVisibility::from_db(&row.get::<_, String>(8)?),
            // An unreadable list grants nothing, the safe side.
            granted: serde_json::from_str(&granted).unwrap_or_default(),
            pinned: row.get(10)?,
            notifications: row.get(11)?,
            removed_at: row.get(12)?,
            installed_at: row.get(13)?,
        })
    }
}

const PLUGIN_COLUMNS: &str = "id, version, previous_version, source, sha256, author_key, enabled, \
     disabled_reason, visibility, granted, pinned, notifications, removed_at, installed_at";

/// A third-party plugin repository (FR-6.74).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRepository {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub public_key: String,
    pub pinned_at: UnixMillis,
    pub enabled: bool,
    /// `key-changed` while the repository is paused after a key change.
    pub paused_reason: Option<String>,
    pub last_sequence: i64,
    pub last_checked: Option<UnixMillis>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPluginRepository {
    pub url: String,
    pub name: String,
    pub public_key: String,
}

/// What a server suggested to one own identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSuggestion {
    pub server_id: ServerId,
    pub identity_id: IdentityId,
    pub plugin_id: String,
    pub seq: i64,
    /// `new`, `not-now`, `dismissed` or `installed`.
    pub state: String,
    /// End of a "Not now" snooze.
    pub until: Option<UnixMillis>,
    pub sender_pin: String,
}

impl Store {
    /// Installed plugins by id; plugins removed with their data kept are left out.
    pub fn plugins(&self) -> Result<Vec<PluginRecord>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {PLUGIN_COLUMNS} FROM plugin WHERE removed_at IS NULL ORDER BY id"
        ))?;
        let rows = stmt.query_map([], PluginRecord::from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// A plugin, also one that is removed with its data kept (so a reinstall can restore it).
    pub fn plugin(&self, id: &str) -> Result<Option<PluginRecord>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {PLUGIN_COLUMNS} FROM plugin WHERE id = ?1"),
                params![id],
                PluginRecord::from_row,
            )
            .optional()?)
    }

    /// Inserts or replaces a plugin. Its storage is kept, which restores the data of a plugin
    /// reinstalled within the retention period.
    pub fn save_plugin(&self, plugin: &PluginRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO plugin (id, version, previous_version, source, sha256, author_key,
                                 enabled, disabled_reason, visibility, granted, pinned,
                                 notifications, removed_at, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT (id) DO UPDATE SET
                 version = excluded.version, previous_version = excluded.previous_version,
                 source = excluded.source, sha256 = excluded.sha256,
                 author_key = excluded.author_key, enabled = excluded.enabled,
                 disabled_reason = excluded.disabled_reason, visibility = excluded.visibility,
                 granted = excluded.granted, pinned = excluded.pinned,
                 notifications = excluded.notifications, removed_at = excluded.removed_at,
                 installed_at = excluded.installed_at",
            params![
                plugin.id,
                plugin.version,
                plugin.previous_version,
                plugin.source,
                plugin.sha256,
                plugin.author_key,
                plugin.enabled,
                plugin.disabled_reason,
                plugin.visibility.as_str(),
                serde_json::to_string(&plugin.granted)?,
                plugin.pinned,
                plugin.notifications,
                plugin.removed_at,
                plugin.installed_at,
            ],
        )?;
        Ok(())
    }

    /// Removes a plugin but keeps its storage for 30 days (FR-6.62). Disables it too.
    pub fn mark_plugin_removed(&self, id: &str, now: UnixMillis) -> Result<()> {
        self.conn.execute(
            "UPDATE plugin SET removed_at = ?2, enabled = 0 WHERE id = ?1",
            params![id, now],
        )?;
        Ok(())
    }

    /// Deletes a plugin together with its storage ("Also delete its data").
    pub fn delete_plugin(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM plugin WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Deletes plugins removed more than 30 days ago, with their storage; returns their ids
    /// so the caller can also drop keychain secrets and caches.
    pub fn purge_expired_plugins(&self, now: UnixMillis) -> Result<Vec<String>> {
        let cutoff = now - REMOVED_DATA_RETENTION_MS;
        let mut stmt = self
            .conn
            .prepare("DELETE FROM plugin WHERE removed_at <= ?1 RETURNING id")?;
        let ids = stmt
            .query_map(params![cutoff], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    pub fn plugin_kv_get(
        &self,
        plugin_id: &str,
        scope_key: &str,
        key: &str,
    ) -> Result<Option<Vec<u8>>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM plugin_kv WHERE plugin_id = ?1 AND scope_key = ?2 AND key = ?3",
                params![plugin_id, scope_key, key],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Writes a value. `quota_bytes` is the plugin's whole quota across scopes; a key counts
    /// with its own length plus the value length (FR-6.20).
    pub fn plugin_kv_set(
        &self,
        plugin_id: &str,
        scope_key: &str,
        key: &str,
        value: &[u8],
        quota_bytes: u64,
        now: UnixMillis,
    ) -> Result<()> {
        if key.is_empty() || key.len() > MAX_PLUGIN_KEY_BYTES {
            return Err(Error::InvalidPluginKey {
                max: MAX_PLUGIN_KEY_BYTES,
            });
        }
        if value.len() > MAX_PLUGIN_VALUE_BYTES {
            return Err(Error::PluginValueTooLarge {
                max: MAX_PLUGIN_VALUE_BYTES,
            });
        }
        let other_usage: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(length(CAST(key AS BLOB)) + length(value)), 0) FROM plugin_kv
             WHERE plugin_id = ?1 AND NOT (scope_key = ?2 AND key = ?3)",
            params![plugin_id, scope_key, key],
            |row| row.get(0),
        )?;
        let new_usage = other_usage as u64 + (key.len() + value.len()) as u64;
        if new_usage > quota_bytes {
            return Err(Error::PluginQuotaExceeded);
        }
        self.conn.execute(
            "INSERT INTO plugin_kv (plugin_id, scope_key, key, value, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (plugin_id, scope_key, key)
             DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![plugin_id, scope_key, key, value, now],
        )?;
        Ok(())
    }

    pub fn plugin_kv_delete(&self, plugin_id: &str, scope_key: &str, key: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM plugin_kv WHERE plugin_id = ?1 AND scope_key = ?2 AND key = ?3",
            params![plugin_id, scope_key, key],
        )?;
        Ok(())
    }

    /// Keys in one scope, sorted.
    pub fn plugin_kv_keys(&self, plugin_id: &str, scope_key: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT key FROM plugin_kv WHERE plugin_id = ?1 AND scope_key = ?2 ORDER BY key",
        )?;
        let keys = stmt
            .query_map(params![plugin_id, scope_key], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(keys)
    }

    /// Every `(scope, key, value)` of a plugin, for "Export data" (FR-6.62).
    pub fn plugin_kv_all(&self, plugin_id: &str) -> Result<Vec<(String, String, Vec<u8>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT scope_key, key, value FROM plugin_kv WHERE plugin_id = ?1
             ORDER BY scope_key, key",
        )?;
        let rows = stmt
            .query_map(params![plugin_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Bytes used by a plugin across all scopes.
    pub fn plugin_kv_usage(&self, plugin_id: &str) -> Result<u64> {
        let used: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(length(CAST(key AS BLOB)) + length(value)), 0) FROM plugin_kv
             WHERE plugin_id = ?1",
            params![plugin_id],
            |row| row.get(0),
        )?;
        Ok(used as u64)
    }

    /// "Reset data": drops all storage of a plugin.
    pub fn plugin_kv_clear(&self, plugin_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM plugin_kv WHERE plugin_id = ?1",
            params![plugin_id],
        )?;
        Ok(())
    }

    pub fn plugin_repositories(&self) -> Result<Vec<PluginRepository>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, url, name, public_key, pinned_at, enabled, paused_reason,
                    last_sequence, last_checked
             FROM plugin_repository ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(PluginRepository {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    name: row.get(2)?,
                    public_key: row.get(3)?,
                    pinned_at: row.get(4)?,
                    enabled: row.get(5)?,
                    paused_reason: row.get(6)?,
                    last_sequence: row.get(7)?,
                    last_checked: row.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Adds a repository and pins its key (FR-6.74); at most 10.
    pub fn add_plugin_repository(
        &self,
        repository: &NewPluginRepository,
        now: UnixMillis,
    ) -> Result<i64> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM plugin_repository", [], |row| {
                    row.get(0)
                })?;
        if count as usize >= MAX_PLUGIN_REPOSITORIES {
            return Err(Error::TooManyPluginRepositories {
                max: MAX_PLUGIN_REPOSITORIES,
            });
        }
        let inserted = self.conn.execute(
            "INSERT INTO plugin_repository (url, name, public_key, pinned_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![repository.url, repository.name, repository.public_key, now],
        );
        match inserted {
            Ok(_) => Ok(self.conn.last_insert_rowid()),
            Err(rusqlite::Error::SqliteFailure(failure, _))
                if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(Error::DuplicatePluginRepository)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Turns a repository on or off; `paused_reason` is cleared when it is turned on.
    pub fn set_plugin_repository_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE plugin_repository
             SET enabled = ?2, paused_reason = CASE WHEN ?2 THEN NULL ELSE paused_reason END
             WHERE id = ?1",
            params![id, enabled],
        )?;
        Ok(())
    }

    /// Pauses a repository, e.g. with `key-changed` (FR-6.75).
    pub fn pause_plugin_repository(&self, id: i64, reason: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE plugin_repository SET enabled = 0, paused_reason = ?2 WHERE id = ?1",
            params![id, reason],
        )?;
        Ok(())
    }

    /// Records a successful check: the newest index sequence seen and when.
    pub fn record_plugin_repository_check(
        &self,
        id: i64,
        sequence: i64,
        now: UnixMillis,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE plugin_repository SET last_sequence = ?2, last_checked = ?3 WHERE id = ?1",
            params![id, sequence, now],
        )?;
        Ok(())
    }

    /// Pins a new key after the user accepted a key change.
    pub fn replace_plugin_repository_key(
        &self,
        id: i64,
        public_key: &str,
        now: UnixMillis,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE plugin_repository
             SET public_key = ?2, pinned_at = ?3, enabled = 1, paused_reason = NULL
             WHERE id = ?1",
            params![id, public_key, now],
        )?;
        Ok(())
    }

    pub fn remove_plugin_repository(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM plugin_repository WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Inserts or replaces the suggestion of `plugin_id` for a server and own identity.
    pub fn save_plugin_suggestion(&self, suggestion: &PluginSuggestion) -> Result<()> {
        self.conn.execute(
            "INSERT INTO server_plugin_suggestion
                 (server_id, identity_id, plugin_id, seq, state, until, sender_pin)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (server_id, identity_id, plugin_id) DO UPDATE SET
                 seq = excluded.seq, state = excluded.state, until = excluded.until,
                 sender_pin = excluded.sender_pin",
            params![
                suggestion.server_id,
                suggestion.identity_id,
                suggestion.plugin_id,
                suggestion.seq,
                suggestion.state,
                suggestion.until,
                suggestion.sender_pin,
            ],
        )?;
        Ok(())
    }

    pub fn plugin_suggestions(
        &self,
        server: ServerId,
        identity: IdentityId,
    ) -> Result<Vec<PluginSuggestion>> {
        let mut stmt = self.conn.prepare(
            "SELECT server_id, identity_id, plugin_id, seq, state, until, sender_pin
             FROM server_plugin_suggestion WHERE server_id = ?1 AND identity_id = ?2
             ORDER BY plugin_id",
        )?;
        let rows = stmt
            .query_map(params![server, identity], |row| {
                Ok(PluginSuggestion {
                    server_id: row.get(0)?,
                    identity_id: row.get(1)?,
                    plugin_id: row.get(2)?,
                    seq: row.get(3)?,
                    state: row.get(4)?,
                    until: row.get(5)?,
                    sender_pin: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_store;
    use crate::identities::NewIdentity;
    use crate::servers::NewServer;

    fn plugin(id: &str) -> PluginRecord {
        PluginRecord {
            id: id.into(),
            version: "1.0.0".into(),
            previous_version: None,
            source: "official".into(),
            sha256: Some("ab".into()),
            author_key: Some("key".into()),
            enabled: true,
            disabled_reason: None,
            visibility: PluginVisibility::Friends,
            granted: vec!["chat.read".into()],
            pinned: false,
            notifications: true,
            removed_at: None,
            installed_at: 100,
        }
    }

    #[test]
    fn plugin_round_trips_and_is_replaced() {
        let (_dir, store) = temp_store();
        store.save_plugin(&plugin("poll")).expect("save");
        assert_eq!(store.plugin("poll").expect("read"), Some(plugin("poll")));
        let mut changed = plugin("poll");
        changed.version = "1.1.0".into();
        changed.previous_version = Some("1.0.0".into());
        changed.disabled_reason = Some("user".into());
        store.save_plugin(&changed).expect("replace");
        assert_eq!(store.plugins().expect("list"), vec![changed]);
    }

    #[test]
    fn removed_plugin_keeps_data_for_30_days_then_is_purged() {
        let (_dir, store) = temp_store();
        store.save_plugin(&plugin("poll")).expect("save");
        store.save_plugin(&plugin("dice")).expect("save");
        store
            .plugin_kv_set("poll", "global", "k", b"v", 1000, 1)
            .expect("set");
        store.mark_plugin_removed("poll", 1_000).expect("remove");
        assert!(
            store
                .plugins()
                .expect("list")
                .iter()
                .all(|p| p.id != "poll")
        );
        assert!(!store.plugin("poll").expect("read").expect("row").enabled);

        let almost = 1_000 + REMOVED_DATA_RETENTION_MS - 1;
        assert!(
            store
                .purge_expired_plugins(almost)
                .expect("purge")
                .is_empty()
        );
        assert!(
            store
                .plugin_kv_get("poll", "global", "k")
                .expect("get")
                .is_some()
        );

        let due = 1_000 + REMOVED_DATA_RETENTION_MS;
        assert_eq!(store.purge_expired_plugins(due).expect("purge"), ["poll"]);
        assert_eq!(
            store.plugin_kv_get("poll", "global", "k").expect("get"),
            None
        );
        assert!(store.plugin("dice").expect("read").is_some());
    }

    #[test]
    fn reinstalling_a_removed_plugin_restores_its_data() {
        let (_dir, store) = temp_store();
        store.save_plugin(&plugin("poll")).expect("save");
        store
            .plugin_kv_set("poll", "global", "k", b"v", 1000, 1)
            .expect("set");
        store.mark_plugin_removed("poll", 5).expect("remove");
        store.save_plugin(&plugin("poll")).expect("reinstall");
        assert_eq!(store.plugins().expect("list").len(), 1);
        assert_eq!(
            store.plugin_kv_get("poll", "global", "k").expect("get"),
            Some(b"v".to_vec())
        );
    }

    #[test]
    fn delete_plugin_drops_its_storage() {
        let (_dir, store) = temp_store();
        store.save_plugin(&plugin("poll")).expect("save");
        store
            .plugin_kv_set("poll", "7", "k", b"v", 1000, 1)
            .expect("set");
        store.delete_plugin("poll").expect("delete");
        assert_eq!(store.plugin_kv_usage("poll").expect("usage"), 0);
    }

    #[test]
    fn kv_is_scoped_and_counts_quota_across_scopes() {
        let (_dir, store) = temp_store();
        store.save_plugin(&plugin("poll")).expect("save");
        store
            .plugin_kv_set("poll", "global", "ab", &[0; 10], 30, 1)
            .expect("set");
        store
            .plugin_kv_set("poll", "7", "ab", &[1; 10], 30, 1)
            .expect("same key, other scope");
        assert_eq!(store.plugin_kv_usage("poll").expect("usage"), 24);
        assert!(matches!(
            store.plugin_kv_set("poll", "7", "cd", &[0; 10], 30, 2),
            Err(Error::PluginQuotaExceeded)
        ));
        // Overwriting counts the new size only, so shrinking and growing within quota work.
        store
            .plugin_kv_set("poll", "7", "ab", &[1; 16], 30, 3)
            .expect("overwrite within quota");
        assert_eq!(store.plugin_kv_usage("poll").expect("usage"), 30);
        assert_eq!(
            store.plugin_kv_keys("poll", "7").expect("keys"),
            ["ab".to_string()]
        );
        store.plugin_kv_delete("poll", "7", "ab").expect("delete");
        assert_eq!(store.plugin_kv_usage("poll").expect("usage"), 12);
        assert_eq!(store.plugin_kv_all("poll").expect("all").len(), 1);
        store.plugin_kv_clear("poll").expect("clear");
        assert_eq!(store.plugin_kv_usage("poll").expect("usage"), 0);
    }

    #[test]
    fn kv_rejects_bad_keys_and_oversized_values() {
        let (_dir, store) = temp_store();
        store.save_plugin(&plugin("poll")).expect("save");
        let long_key = "k".repeat(MAX_PLUGIN_KEY_BYTES + 1);
        for key in ["", long_key.as_str()] {
            assert!(matches!(
                store.plugin_kv_set("poll", "global", key, b"v", 1 << 20, 1),
                Err(Error::InvalidPluginKey { .. })
            ));
        }
        let big = vec![0; MAX_PLUGIN_VALUE_BYTES + 1];
        assert!(matches!(
            store.plugin_kv_set("poll", "global", "k", &big, 1 << 20, 1),
            Err(Error::PluginValueTooLarge { .. })
        ));
        let max = vec![0; MAX_PLUGIN_VALUE_BYTES];
        store
            .plugin_kv_set("poll", "global", "k", &max, 1 << 20, 1)
            .expect("max value fits");
    }

    #[test]
    fn at_most_ten_repositories_without_duplicates() {
        let (_dir, store) = temp_store();
        let new = |n: usize| NewPluginRepository {
            url: format!("https://r{n}.example.org/"),
            name: format!("Repo {n}"),
            public_key: "key".into(),
        };
        for n in 0..MAX_PLUGIN_REPOSITORIES {
            store.add_plugin_repository(&new(n), 5).expect("add");
        }
        assert!(matches!(
            store.add_plugin_repository(&new(99), 5),
            Err(Error::TooManyPluginRepositories { max: 10 })
        ));
        let first = store.plugin_repositories().expect("list")[0].id;
        store.remove_plugin_repository(first).expect("remove");
        assert!(matches!(
            store.add_plugin_repository(&new(1), 5),
            Err(Error::DuplicatePluginRepository)
        ));
        store
            .add_plugin_repository(&new(99), 5)
            .expect("room again");
    }

    #[test]
    fn repository_pause_resume_and_key_replacement() {
        let (_dir, store) = temp_store();
        let id = store
            .add_plugin_repository(
                &NewPluginRepository {
                    url: "https://r.example.org/".into(),
                    name: "Repo".into(),
                    public_key: "old".into(),
                },
                5,
            )
            .expect("add");
        store
            .record_plugin_repository_check(id, 9, 50)
            .expect("check");
        store
            .pause_plugin_repository(id, "key-changed")
            .expect("pause");
        let paused = &store.plugin_repositories().expect("list")[0];
        assert!(!paused.enabled);
        assert_eq!(paused.paused_reason.as_deref(), Some("key-changed"));
        assert_eq!((paused.last_sequence, paused.last_checked), (9, Some(50)));

        store
            .replace_plugin_repository_key(id, "new", 60)
            .expect("replace");
        let resumed = &store.plugin_repositories().expect("list")[0];
        assert!(resumed.enabled && resumed.paused_reason.is_none());
        assert_eq!(
            (resumed.public_key.as_str(), resumed.pinned_at),
            ("new", 60)
        );
        store.set_plugin_repository_enabled(id, false).expect("off");
        assert!(!store.plugin_repositories().expect("list")[0].enabled);
    }

    #[test]
    fn suggestions_are_per_server_and_identity() {
        let (_dir, store) = temp_store();
        let server = store
            .add_server(&NewServer::new("A", "localhost", "Tester"))
            .expect("server")
            .id;
        let one = store
            .add_identity(&NewIdentity::new("One", "one", "h1", 1))
            .expect("identity")
            .id;
        let two = store
            .add_identity(&NewIdentity::new("Two", "two", "h2", 2))
            .expect("identity")
            .id;
        let mut suggestion = PluginSuggestion {
            server_id: server,
            identity_id: one,
            plugin_id: "poll".into(),
            seq: 1,
            state: "new".into(),
            until: None,
            sender_pin: "pin".into(),
        };
        store.save_plugin_suggestion(&suggestion).expect("save");
        suggestion.seq = 2;
        suggestion.state = "not-now".into();
        suggestion.until = Some(99);
        store.save_plugin_suggestion(&suggestion).expect("replace");
        assert_eq!(
            store.plugin_suggestions(server, one).expect("list"),
            vec![suggestion]
        );
        assert!(
            store
                .plugin_suggestions(server, two)
                .expect("list")
                .is_empty()
        );
    }
}
