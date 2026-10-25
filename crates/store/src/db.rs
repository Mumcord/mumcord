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
    // v3: the user's last own mute/deafen choice, restored on an explicit join (QA.7). Kept
    // out of the settings JSON so saving settings from the UI can never overwrite it.
    r"
    CREATE TABLE own_voice (
        id   INTEGER PRIMARY KEY CHECK (id = 1),
        mute INTEGER NOT NULL CHECK (mute IN (0, 1)),
        deaf INTEGER NOT NULL CHECK (deaf IN (0, 1))
    );
    ",
    // v4: the channel the user is joined to for voice, kept when the app quits so the next
    // start can offer to rejoin it (QA.9). `last_channel` also follows the read-only default
    // channel, so it cannot tell whether voice was on.
    r"
    ALTER TABLE saved_server ADD COLUMN voice_channel TEXT;
    ",
    // v5: the avatar remembered per server (hand-set or the default avatar, as a scaled PNG),
    // sent again on connect when the server keeps none for the user (FR-1.20, QA.10).
    // Servers that already got the default avatar are marked for a one-time backfill with
    // it; the image lives in a file, so the engine fills it in at start.
    r"
    ALTER TABLE saved_server ADD COLUMN avatar BLOB;
    ALTER TABLE saved_server ADD COLUMN avatar_backfill INTEGER NOT NULL DEFAULT 0
        CHECK (avatar_backfill IN (0, 1));
    UPDATE saved_server SET avatar_backfill = 1 WHERE default_avatar_sent = 1;
    ",
    // v6: preview cards the user dismissed (QA.42), per message and link; they go away with
    // the message. The timestamp index makes deleting old history an index range scan.
    r"
    CREATE TABLE dismissed_preview (
        message_id INTEGER NOT NULL REFERENCES chat_message (id) ON DELETE CASCADE,
        url        TEXT    NOT NULL,
        PRIMARY KEY (message_id, url)
    );
    CREATE INDEX chat_message_sent_at ON chat_message (sent_at);
    ",
    // v7: GIF favorites (FR-1.52, QA.43): saved image links the user can send again. The
    // image itself comes from the link-preview image cache, so only the link is stored.
    r"
    CREATE TABLE gif_favorite (
        url          TEXT    PRIMARY KEY,
        added_at     INTEGER NOT NULL,
        last_used_at INTEGER NOT NULL
    );
    CREATE INDEX gif_favorite_last_used ON gif_favorite (last_used_at DESC);
    ",
    // v8: private notes about people (FR-1.63), keyed like the person settings. Kept apart
    // from them because a note is free text and has nothing to do with playback.
    r"
    CREATE TABLE person_note (
        server_id        INTEGER NOT NULL REFERENCES saved_server (id) ON DELETE CASCADE,
        person_cert_hash TEXT    NOT NULL,
        note             TEXT    NOT NULL,
        PRIMARY KEY (server_id, person_cert_hash)
    );
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
    fn version_5_database_upgrades_and_keeps_messages() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut conn = Connection::open(dir.path().join(DB_FILE_NAME)).expect("open");
            let tx = conn.transaction().expect("tx");
            for sql in &MIGRATIONS[..5] {
                tx.execute_batch(sql).expect("migration");
            }
            tx.pragma_update(None, "user_version", 5).expect("pragma");
            tx.execute(
                "INSERT INTO saved_server (label, address, username, sort_order)
                 VALUES ('Old', 'localhost', 'Tester', 0)",
                [],
            )
            .expect("server");
            tx.execute(
                "INSERT INTO chat_message
                     (server_id, conversation_kind, conversation_key, sender_name, sent_at, body)
                 VALUES (1, 'channel', 'Lobby', 'Alice', 5, 'hi')",
                [],
            )
            .expect("message");
            tx.commit().expect("commit");
        }
        let store = Store::open(dir.path()).expect("upgrade");
        assert_eq!(
            store.schema_version().expect("version"),
            MIGRATIONS.len() as i64
        );
        store
            .dismiss_preview(1, "https://example.org/")
            .expect("dismiss");
        assert_eq!(store.dismissed_previews(&[1]).expect("flags")[&1].len(), 1);
    }

    #[test]
    fn version_7_database_upgrades_with_empty_notes_and_keeps_person_settings() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut conn = Connection::open(dir.path().join(DB_FILE_NAME)).expect("open");
            let tx = conn.transaction().expect("tx");
            for sql in &MIGRATIONS[..7] {
                tx.execute_batch(sql).expect("migration");
            }
            tx.pragma_update(None, "user_version", 7).expect("pragma");
            tx.execute(
                "INSERT INTO saved_server (label, address, username, sort_order)
                 VALUES ('Old', 'localhost', 'Tester', 0)",
                [],
            )
            .expect("server");
            tx.execute(
                "INSERT INTO person_setting (server_id, person_cert_hash, volume, local_mute)
                 VALUES (1, 'h1', 50, 1)",
                [],
            )
            .expect("person setting");
            tx.commit().expect("commit");
        }
        let store = Store::open(dir.path()).expect("upgrade");
        assert_eq!(
            store.schema_version().expect("version"),
            MIGRATIONS.len() as i64
        );
        assert_eq!(
            store.person_setting(1, "h1").expect("setting"),
            Some(crate::PersonSetting {
                volume: 50,
                local_mute: true
            })
        );
        assert_eq!(store.person_note(1, "h1").expect("note"), "");
        store
            .set_person_note(1, "h1", "plays bass")
            .expect("set note");
        assert_eq!(store.person_note(1, "h1").expect("note"), "plays bass");
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
        assert_eq!(store.server_avatar(servers[0].id).expect("avatar"), None);
    }

    #[test]
    fn version_4_servers_with_the_default_avatar_sent_are_backfilled_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut conn = Connection::open(dir.path().join(DB_FILE_NAME)).expect("open");
            let tx = conn.transaction().expect("tx");
            for sql in &MIGRATIONS[..4] {
                tx.execute_batch(sql).expect("old schema");
            }
            tx.pragma_update(None, "user_version", 4).expect("pragma");
            tx.execute_batch(
                "INSERT INTO saved_server (label, address, username, sort_order, default_avatar_sent)
                 VALUES ('Sent', 'localhost', 'Tester', 0, 1), ('Fresh', 'other', 'Tester', 1, 0);",
            )
            .expect("insert");
            tx.commit().expect("commit");
        }
        let store = Store::open(dir.path()).expect("upgrade");
        let ids: Vec<_> = store
            .servers()
            .expect("servers")
            .iter()
            .map(|s| s.id)
            .collect();
        let (sent, fresh) = (ids[0], ids[1]);

        assert_eq!(
            store
                .backfill_server_avatars(Some(b"default"))
                .expect("backfill"),
            1
        );
        assert_eq!(
            store.server_avatar(sent).expect("avatar").as_deref(),
            Some(&b"default"[..])
        );
        assert_eq!(store.server_avatar(fresh).expect("avatar"), None);
        assert!(!store.default_avatar_sent(fresh).expect("flag"));

        // Only once: a later removal by hand is not undone by the next start.
        store.set_server_avatar(sent, None).expect("remove");
        assert_eq!(
            store.backfill_server_avatars(Some(b"new")).expect("again"),
            0
        );
        assert_eq!(store.server_avatar(sent).expect("avatar"), None);
    }

    #[test]
    fn backfill_without_a_default_avatar_remembers_nothing() {
        let (_dir, store) = temp_store();
        let id = store
            .add_server(&crate::NewServer::new("A", "a", "Tester"))
            .expect("add")
            .id;
        store.set_default_avatar_sent(id).expect("sent");
        store
            .conn
            .execute("UPDATE saved_server SET avatar_backfill = 1", [])
            .expect("mark");
        assert_eq!(store.backfill_server_avatars(None).expect("backfill"), 1);
        assert_eq!(store.server_avatar(id).expect("avatar"), None);
        assert_eq!(store.backfill_server_avatars(Some(b"x")).expect("again"), 0);
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
