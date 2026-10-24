use std::path::Path;

use rusqlite::Connection;

use crate::error::{Error, Result};

/// File name of the database inside the app data directory.
pub const DB_FILE_NAME: &str = "mumcord.sqlite3";

/// Milliseconds since the Unix epoch (UTC). All timestamps are stored in this form.
pub type UnixMillis = i64;

/// Ordered schema migrations. Entry `n` upgrades the schema from version `n` to `n + 1`;
/// the current version is kept in SQLite's `user_version`. Never edit a released
/// migration – append a new one.
const MIGRATIONS: &[&str] = &[
    // v1: initial schema (spec section 7).
    r"
    CREATE TABLE saved_server (
        id           INTEGER PRIMARY KEY,
        label        TEXT    NOT NULL,
        address      TEXT    NOT NULL,
        port         INTEGER NOT NULL DEFAULT 64738 CHECK (port BETWEEN 1 AND 65535),
        username     TEXT    NOT NULL,
        last_channel TEXT,
        save_history INTEGER NOT NULL DEFAULT 1 CHECK (save_history IN (0, 1)),
        sort_order   INTEGER NOT NULL
    );

    CREATE TABLE trusted_cert (
        server_id   INTEGER PRIMARY KEY REFERENCES saved_server (id) ON DELETE CASCADE,
        fingerprint TEXT    NOT NULL
    );

    CREATE TABLE followed_channel (
        server_id    INTEGER NOT NULL REFERENCES saved_server (id) ON DELETE CASCADE,
        channel_path TEXT    NOT NULL,
        last_read_at INTEGER NOT NULL,
        PRIMARY KEY (server_id, channel_path)
    );

    CREATE TABLE chat_message (
        id                INTEGER PRIMARY KEY,
        server_id         INTEGER NOT NULL REFERENCES saved_server (id) ON DELETE CASCADE,
        conversation_kind TEXT    NOT NULL CHECK (conversation_kind IN ('channel', 'private')),
        conversation_key  TEXT    NOT NULL,
        sender_name       TEXT    NOT NULL,
        sender_cert_hash  TEXT,
        sent_at           INTEGER NOT NULL,
        body              TEXT    NOT NULL
    );
    CREATE INDEX chat_message_conversation
        ON chat_message (server_id, conversation_kind, conversation_key, sent_at);

    CREATE TABLE person_setting (
        server_id        INTEGER NOT NULL REFERENCES saved_server (id) ON DELETE CASCADE,
        person_cert_hash TEXT    NOT NULL,
        volume           INTEGER NOT NULL DEFAULT 100 CHECK (volume BETWEEN 0 AND 200),
        local_mute       INTEGER NOT NULL DEFAULT 0 CHECK (local_mute IN (0, 1)),
        PRIMARY KEY (server_id, person_cert_hash)
    );

    -- Single settings record, stored as JSON so new settings need no migration.
    CREATE TABLE settings (
        id   INTEGER PRIMARY KEY CHECK (id = 1),
        data TEXT    NOT NULL
    );
    ",
    // v2: remember per server whether the default avatar was handled (FR-1.47).
    r"
    ALTER TABLE saved_server ADD COLUMN default_avatar_sent INTEGER NOT NULL DEFAULT 0
        CHECK (default_avatar_sent IN (0, 1));
    ",
];

/// Handle to the local database. Not `Sync`; callers share it behind a mutex.
pub struct Store {
    pub(crate) conn: Connection,
}

impl Store {
    /// Opens (or creates) the database in `data_dir` and applies pending migrations.
    /// The directory must exist; the caller decides where app data lives.
    pub fn open(data_dir: &Path) -> Result<Self> {
        Self::from_connection(Connection::open(data_dir.join(DB_FILE_NAME))?)
    }

    fn from_connection(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        migrate(&mut conn)?;
        Ok(Self { conn })
    }

    /// Current schema version (number of applied migrations).
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let supported = MIGRATIONS.len() as i64;
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current > supported {
        return Err(Error::SchemaTooNew {
            found: current,
            supported,
        });
    }
    for (version, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path()).expect("open store");
        (dir, store)
    }

    #[test]
    fn fresh_database_is_fully_migrated() {
        let (_dir, store) = temp_store();
        assert_eq!(
            store.schema_version().expect("version"),
            MIGRATIONS.len() as i64
        );
        let fk: bool = store
            .conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .expect("pragma");
        assert!(fk);
    }

    #[test]
    fn reopening_keeps_data_and_does_not_rerun_migrations() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let store = Store::open(dir.path()).expect("open");
            store
                .add_server(&crate::NewServer::new("Home", "localhost", "Tester"))
                .expect("add");
        }
        let store = Store::open(dir.path()).expect("reopen");
        assert_eq!(store.servers().expect("servers").len(), 1);
        assert_eq!(
            store.schema_version().expect("version"),
            MIGRATIONS.len() as i64
        );
    }

    #[test]
    fn version_1_database_upgrades_and_keeps_servers() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut conn = Connection::open(dir.path().join(DB_FILE_NAME)).expect("open");
            let tx = conn.transaction().expect("tx");
            tx.execute_batch(MIGRATIONS[0]).expect("v1");
            tx.pragma_update(None, "user_version", 1).expect("pragma");
            tx.execute(
                "INSERT INTO saved_server (label, address, username, sort_order)
                 VALUES ('Old', 'localhost', 'Tester', 0)",
                [],
            )
            .expect("insert");
            tx.commit().expect("commit");
        }
        let store = Store::open(dir.path()).expect("upgrade");
        assert_eq!(
            store.schema_version().expect("version"),
            MIGRATIONS.len() as i64
        );
        let servers = store.servers().expect("servers");
        assert_eq!(servers.len(), 1);
        assert!(!store.default_avatar_sent(servers[0].id).expect("flag"));
    }

    #[test]
    fn newer_schema_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let conn = Connection::open(dir.path().join(DB_FILE_NAME)).expect("open");
            conn.pragma_update(None, "user_version", 99)
                .expect("pragma");
        }
        assert!(matches!(
            Store::open(dir.path()),
            Err(Error::SchemaTooNew { found: 99, .. })
        ));
    }
}
