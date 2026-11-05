//! Local trust data of the plugin platform (spec 09, section 7.1): approved origins, daily
//! request counters, when a registry key was first seen, the permission history per plugin
//! version, the high-risk plugins the user confirmed and the plugins whose web content is off.
//! Nothing here leaves the device.

use rusqlite::{OptionalExtension, params};

use crate::db::{Store, UnixMillis};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginApproval {
    pub plugin_id: String,
    pub origin: String,
    pub purpose_key: String,
    pub approved_at: UnixMillis,
}

/// One plugin's network counters for one local calendar day (`YYYY-MM-DD`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginCounters {
    pub plugin_id: String,
    pub day: String,
    pub requests: i64,
    pub blocked: i64,
    pub last_request_at: Option<UnixMillis>,
    pub last_host: Option<String>,
}

/// What a plugin version declared when it was first seen; `permissions` and `origins` are
/// JSON arrays kept as text so the store does not depend on the plugin crates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionHistoryEntry {
    pub plugin_id: String,
    pub version: String,
    pub permissions: String,
    pub origins: String,
    pub seen_at: UnixMillis,
}

impl Store {
    /// Approves `origin` for the plugin; an existing approval is replaced (approved update).
    pub fn approve_plugin_origin(&self, approval: &OriginApproval) -> Result<()> {
        self.conn.execute(
            "INSERT INTO plugin_origin_approval (plugin_id, origin, purpose_key, approved_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (plugin_id, origin) DO UPDATE SET
                 purpose_key = excluded.purpose_key, approved_at = excluded.approved_at",
            params![
                approval.plugin_id,
                approval.origin,
                approval.purpose_key,
                approval.approved_at
            ],
        )?;
        Ok(())
    }

    /// The plugin's approved origins, oldest first.
    pub fn plugin_origin_approvals(&self, plugin_id: &str) -> Result<Vec<OriginApproval>> {
        let mut stmt = self.conn.prepare(
            "SELECT plugin_id, origin, purpose_key, approved_at FROM plugin_origin_approval
             WHERE plugin_id = ?1 ORDER BY approved_at, origin",
        )?;
        let rows = stmt.query_map([plugin_id], |row| {
            Ok(OriginApproval {
                plugin_id: row.get(0)?,
                origin: row.get(1)?,
                purpose_key: row.get(2)?,
                approved_at: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Drops every approval of the plugin, e.g. on uninstall.
    pub fn clear_plugin_origin_approvals(&self, plugin_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM plugin_origin_approval WHERE plugin_id = ?1",
            [plugin_id],
        )?;
        Ok(())
    }

    /// Counts one request (`blocked` or allowed) to `host` for the plugin on `day`.
    pub fn record_plugin_request(
        &self,
        plugin_id: &str,
        day: &str,
        host: &str,
        blocked: bool,
        at: UnixMillis,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO plugin_origin_counters
                 (plugin_id, day, requests, blocked, last_request_at, last_host)
             VALUES (?1, ?2, 1, ?3, ?4, ?5)
             ON CONFLICT (plugin_id, day) DO UPDATE SET
                 requests = requests + 1,
                 blocked = blocked + excluded.blocked,
                 last_request_at = excluded.last_request_at,
                 last_host = excluded.last_host",
            params![plugin_id, day, i64::from(blocked), at, host],
        )?;
        Ok(())
    }

    /// The plugin's counters, newest day first.
    pub fn plugin_origin_counters(&self, plugin_id: &str) -> Result<Vec<OriginCounters>> {
        let mut stmt = self.conn.prepare(
            "SELECT plugin_id, day, requests, blocked, last_request_at, last_host
             FROM plugin_origin_counters WHERE plugin_id = ?1 ORDER BY day DESC",
        )?;
        let rows = stmt.query_map([plugin_id], |row| {
            Ok(OriginCounters {
                plugin_id: row.get(0)?,
                day: row.get(1)?,
                requests: row.get(2)?,
                blocked: row.get(3)?,
                last_request_at: row.get(4)?,
                last_host: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Removes counters of days before `oldest_kept_day` (`YYYY-MM-DD`, which sorts as text).
    /// The caller passes today minus 6 days so that 7 days remain. Returns the rows removed.
    pub fn prune_plugin_origin_counters(&self, oldest_kept_day: &str) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM plugin_origin_counters WHERE day < ?1",
            [oldest_kept_day],
        )?)
    }

    /// Records when a registry key first showed up in a refreshed index. Written once: a key
    /// that is already known keeps its first time. Returns whether this call wrote it.
    pub fn note_repo_key_seen(&self, key_id: &str, at: UnixMillis) -> Result<bool> {
        let written = self.conn.execute(
            "INSERT INTO repo_key_first_seen (key_id, first_seen_at) VALUES (?1, ?2)
             ON CONFLICT (key_id) DO NOTHING",
            params![key_id, at],
        )?;
        Ok(written > 0)
    }

    pub fn repo_key_first_seen(&self, key_id: &str) -> Result<Option<UnixMillis>> {
        Ok(self
            .conn
            .query_row(
                "SELECT first_seen_at FROM repo_key_first_seen WHERE key_id = ?1",
                [key_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Remembers what a version declared. A version already recorded keeps its first entry.
    pub fn record_plugin_version(&self, entry: &VersionHistoryEntry) -> Result<()> {
        self.conn.execute(
            "INSERT INTO plugin_version_history
                 (plugin_id, version, permissions, origins, seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (plugin_id, version) DO NOTHING",
            params![
                entry.plugin_id,
                entry.version,
                entry.permissions,
                entry.origins,
                entry.seen_at
            ],
        )?;
        Ok(())
    }

    /// The plugin's recorded versions, oldest first.
    pub fn plugin_version_history(&self, plugin_id: &str) -> Result<Vec<VersionHistoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT plugin_id, version, permissions, origins, seen_at
             FROM plugin_version_history WHERE plugin_id = ?1 ORDER BY seen_at, version",
        )?;
        let rows = stmt.query_map([plugin_id], |row| {
            Ok(VersionHistoryEntry {
                plugin_id: row.get(0)?,
                version: row.get(1)?,
                permissions: row.get(2)?,
                origins: row.get(3)?,
                seen_at: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Remembers that the user confirmed installing this high-risk version.
    pub fn confirm_plugin_high_risk(
        &self,
        plugin_id: &str,
        version: &str,
        at: UnixMillis,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO plugin_high_risk_confirmed (plugin_id, version, confirmed_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (plugin_id, version) DO UPDATE SET confirmed_at = excluded.confirmed_at",
            params![plugin_id, version, at],
        )?;
        Ok(())
    }

    /// Remembers that the user skipped this version of a plugin ("Skip this version").
    pub fn skip_plugin_update(&self, plugin_id: &str, version: &str, at: UnixMillis) -> Result<()> {
        self.conn.execute(
            "INSERT INTO plugin_skipped_update (plugin_id, version, skipped_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (plugin_id) DO UPDATE SET
                 version = excluded.version, skipped_at = excluded.skipped_at",
            params![plugin_id, version, at],
        )?;
        Ok(())
    }

    /// The version the user skipped last for this plugin, if any.
    pub fn plugin_skipped_update(&self, plugin_id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT version FROM plugin_skipped_update WHERE plugin_id = ?1",
                params![plugin_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Turns a plugin's web content off (`disabled`) or on again.
    pub fn set_plugin_web_disabled(
        &self,
        plugin_id: &str,
        disabled: bool,
        at: UnixMillis,
    ) -> Result<()> {
        if disabled {
            self.conn.execute(
                "INSERT INTO plugin_web_disabled (plugin_id, disabled_at) VALUES (?1, ?2)
                 ON CONFLICT (plugin_id) DO NOTHING",
                params![plugin_id, at],
            )?;
        } else {
            self.conn.execute(
                "DELETE FROM plugin_web_disabled WHERE plugin_id = ?1",
                params![plugin_id],
            )?;
        }
        Ok(())
    }

    /// Whether the plugin's web content is turned off.
    pub fn plugin_web_disabled(&self, plugin_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM plugin_web_disabled WHERE plugin_id = ?1",
                params![plugin_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// When the user confirmed this version as high risk, if they did.
    pub fn plugin_high_risk_confirmed_at(
        &self,
        plugin_id: &str,
        version: &str,
    ) -> Result<Option<UnixMillis>> {
        Ok(self
            .conn
            .query_row(
                "SELECT confirmed_at FROM plugin_high_risk_confirmed
                 WHERE plugin_id = ?1 AND version = ?2",
                params![plugin_id, version],
                |row| row.get(0),
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_store;

    #[test]
    fn approvals_are_replaced_per_origin_and_cleared_per_plugin() {
        let (_dir, store) = temp_store();
        let mut approval = OriginApproval {
            plugin_id: "dice".into(),
            origin: "https://api.example.com:443".into(),
            purpose_key: "p.one".into(),
            approved_at: 1,
        };
        store.approve_plugin_origin(&approval).expect("approve");
        approval.purpose_key = "p.two".into();
        approval.approved_at = 2;
        store.approve_plugin_origin(&approval).expect("replace");
        let other = OriginApproval {
            plugin_id: "other".into(),
            ..approval.clone()
        };
        store.approve_plugin_origin(&other).expect("other");
        assert_eq!(
            store.plugin_origin_approvals("dice").expect("read"),
            vec![approval]
        );
        store.clear_plugin_origin_approvals("dice").expect("clear");
        assert!(
            store
                .plugin_origin_approvals("dice")
                .expect("read")
                .is_empty()
        );
        assert_eq!(
            store.plugin_origin_approvals("other").expect("read").len(),
            1
        );
    }

    #[test]
    fn counters_add_up_per_day_and_old_days_are_pruned() {
        let (_dir, store) = temp_store();
        store
            .record_plugin_request("p", "2026-11-01", "a.example", false, 10)
            .expect("record");
        store
            .record_plugin_request("p", "2026-11-01", "b.example", true, 20)
            .expect("record");
        store
            .record_plugin_request("p", "2026-10-20", "a.example", false, 5)
            .expect("record");
        let rows = store.plugin_origin_counters("p").expect("read");
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            OriginCounters {
                plugin_id: "p".into(),
                day: "2026-11-01".into(),
                requests: 2,
                blocked: 1,
                last_request_at: Some(20),
                last_host: Some("b.example".into()),
            }
        );
        assert_eq!(
            store
                .prune_plugin_origin_counters("2026-10-26")
                .expect("prune"),
            1
        );
        assert_eq!(store.plugin_origin_counters("p").expect("read").len(), 1);
    }

    #[test]
    fn a_registry_key_keeps_its_first_seen_time() {
        let (_dir, store) = temp_store();
        assert_eq!(store.repo_key_first_seen("k").expect("read"), None);
        assert!(store.note_repo_key_seen("k", 100).expect("write"));
        assert!(!store.note_repo_key_seen("k", 200).expect("write"));
        assert_eq!(store.repo_key_first_seen("k").expect("read"), Some(100));
    }

    #[test]
    fn version_history_keeps_the_first_entry_per_version() {
        let (_dir, store) = temp_store();
        let first = VersionHistoryEntry {
            plugin_id: "p".into(),
            version: "1.0.0".into(),
            permissions: r#"["net.http"]"#.into(),
            origins: "[]".into(),
            seen_at: 1,
        };
        store.record_plugin_version(&first).expect("record");
        store
            .record_plugin_version(&VersionHistoryEntry {
                permissions: "[]".into(),
                seen_at: 9,
                ..first.clone()
            })
            .expect("record again");
        let second = VersionHistoryEntry {
            version: "1.1.0".into(),
            seen_at: 5,
            ..first.clone()
        };
        store.record_plugin_version(&second).expect("record");
        assert_eq!(
            store.plugin_version_history("p").expect("read"),
            vec![first, second]
        );
    }

    #[test]
    fn web_content_stays_off_until_turned_on_again() {
        let (_dir, store) = temp_store();
        assert!(!store.plugin_web_disabled("p").expect("store"));
        store.set_plugin_web_disabled("p", true, 1).expect("store");
        store.set_plugin_web_disabled("p", true, 2).expect("store");
        assert!(store.plugin_web_disabled("p").expect("store"));
        assert!(!store.plugin_web_disabled("q").expect("store"));
        store.set_plugin_web_disabled("p", false, 3).expect("store");
        assert!(!store.plugin_web_disabled("p").expect("store"));
    }

    #[test]
    fn skipped_update_is_remembered_per_plugin_and_replaced() {
        let (_dir, store) = temp_store();
        assert_eq!(store.plugin_skipped_update("p").expect("store"), None);
        store.skip_plugin_update("p", "1.1.0", 1).expect("store");
        store.skip_plugin_update("q", "2.0.0", 2).expect("store");
        store.skip_plugin_update("p", "1.2.0", 3).expect("store");
        assert_eq!(
            store.plugin_skipped_update("p").expect("store").as_deref(),
            Some("1.2.0")
        );
        assert_eq!(
            store.plugin_skipped_update("q").expect("store").as_deref(),
            Some("2.0.0")
        );
    }

    #[test]
    fn high_risk_confirmation_is_per_version() {
        let (_dir, store) = temp_store();
        assert_eq!(
            store
                .plugin_high_risk_confirmed_at("p", "1.0.0")
                .expect("read"),
            None
        );
        store
            .confirm_plugin_high_risk("p", "1.0.0", 42)
            .expect("confirm");
        assert_eq!(
            store
                .plugin_high_risk_confirmed_at("p", "1.0.0")
                .expect("read"),
            Some(42)
        );
        assert_eq!(
            store
                .plugin_high_risk_confirmed_at("p", "1.1.0")
                .expect("read"),
            None
        );
    }
}
