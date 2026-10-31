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
    // v9: history sync between Mumcord clients (FR-1.60, QA.48): the display name of the
    // person whose history a channel message came from; NULL for messages received live.
    r"
    ALTER TABLE chat_message ADD COLUMN synced_from TEXT;
    ",
    // v10: friends by certificate hash (FR-1.64), local only and across servers. Private
    // chats with a friend are read across servers (FR-1.65), so private history also gets an
    // index that does not start with the server.
    r"
    CREATE TABLE friend (
        cert_hash    TEXT    PRIMARY KEY,
        name         TEXT    NOT NULL,
        added_at     INTEGER NOT NULL,
        last_seen_at INTEGER
    );
    CREATE INDEX chat_message_by_key
        ON chat_message (conversation_kind, conversation_key, sent_at);
    ",
    // v11: multi-server state (FR-2.3, FR-2.7, FR-2.10, FR-2.17). `connected_at_quit` is the
    // quit time of a server that was connected then (NULL otherwise); the most recent one
    // wins when more servers than the limit were connected (AC-2.6).
    r"
    ALTER TABLE saved_server ADD COLUMN always_connect_at_start INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE saved_server ADD COLUMN connected_at_quit INTEGER;
    ALTER TABLE saved_server ADD COLUMN was_voice_server_at_quit INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE saved_server ADD COLUMN notifications_muted INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE saved_server ADD COLUMN theme_id TEXT;
    ",
    // v12: when the user last cleared channel history (issue #119), so history sync does not
    // bring the deleted messages back from other people's copies. `server_id` NULL: all
    // servers; `channel_key` NULL: every channel of the server.
    r"
    CREATE TABLE history_cleared (
        server_id   INTEGER REFERENCES saved_server (id) ON DELETE CASCADE,
        channel_key TEXT,
        cleared_at  INTEGER NOT NULL
    );
    ",
    // v13: non-secret details of the saved channel passwords (issue #146), by the token's
    // position in the server's token list in secure storage. `channels` is a JSON array of
    // `{channel_id, path}`. Tokens saved before have no row and keep their default name.
    r"
    CREATE TABLE channel_token (
        server_id    INTEGER NOT NULL REFERENCES saved_server (id) ON DELETE CASCADE,
        position     INTEGER NOT NULL,
        label        TEXT,
        channels     TEXT    NOT NULL DEFAULT '[]',
        last_used_at INTEGER,
        PRIMARY KEY (server_id, position)
    );
    ",
    // v14: several identities (issue #148). Certificates and keys stay in the OS secure
    // storage; this table holds the local label, the certificate's common name and hash, when
    // it was added and which one is the default. `saved_server.identity_id` NULL means "use
    // the default identity", so existing servers keep their identity after the upgrade.
    r"
    CREATE TABLE identity (
        id         INTEGER PRIMARY KEY,
        label      TEXT    NOT NULL,
        cert_name  TEXT    NOT NULL,
        cert_hash  TEXT    NOT NULL UNIQUE,
        created_at INTEGER NOT NULL,
        is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1))
    );
    CREATE UNIQUE INDEX identity_one_default ON identity (is_default) WHERE is_default = 1;

    ALTER TABLE saved_server ADD COLUMN identity_id INTEGER REFERENCES identity (id)
        ON DELETE SET NULL;
    ",
    // v15: turning off `@all` / `@channel` mentions per server (issue #70, Q1.6).
    r"
    ALTER TABLE saved_server ADD COLUMN group_mentions_muted INTEGER NOT NULL DEFAULT 0;
    ",
    // v16: accepted private conversations (issue #141). Kept per own identity, keyed by the
    // own certificate hash, so the same person can message one identity but not another.
    r"
    CREATE TABLE accepted_conversation (
        own_cert_hash TEXT    NOT NULL,
        cert_hash     TEXT    NOT NULL,
        name          TEXT    NOT NULL,
        accepted_at   INTEGER NOT NULL,
        PRIMARY KEY (own_cert_hash, cert_hash)
    );
    ",
    // v17: pinned and muted private conversations in the private messages overview (issue
    // #173), by server and the other person's certificate hash. `muted_until` is Unix millis,
    // i64::MAX for "until turned back on", NULL when not muted.
    r"
    CREATE TABLE pm_conversation (
        server_id   INTEGER NOT NULL REFERENCES saved_server (id) ON DELETE CASCADE,
        cert_hash   TEXT    NOT NULL,
        pinned      INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
        muted_until INTEGER,
        PRIMARY KEY (server_id, cert_hash)
    );
    ",
    // v18: audio profiles per channel (issue #171): overrides of Music quality, low-latency mode
    // and the bitrate cap; NULL keeps the global setting. Keyed by channel id, with the channel
    // path kept so a renamed or recreated channel still matches.
    r"
    CREATE TABLE channel_audio_profile (
        server_id     INTEGER NOT NULL REFERENCES saved_server (id) ON DELETE CASCADE,
        channel_id    INTEGER NOT NULL,
        channel_path  TEXT    NOT NULL,
        music_quality INTEGER CHECK (music_quality IN (0, 1)),
        low_latency   INTEGER CHECK (low_latency IN (0, 1)),
        bitrate_kbps  INTEGER CHECK (bitrate_kbps > 0),
        PRIMARY KEY (server_id, channel_id)
    );
    ",
    // v19: blocked users (issue #140), kept per own identity (FR-3.79). Lands after #218,
    // whose `channel_audio_profile` (v18, above) keeps its version number so the version
    // numbers match. A certificate block (`cert_hash` set) covers every server of
    // that identity (Q1.4); a name block (`cert_hash` NULL) is for people without a
    // certificate and only applies on `server_id`. `server_label` keeps where the block was
    // set readable after the server is removed.
    r"
    CREATE TABLE blocked_user (
        id            INTEGER PRIMARY KEY,
        own_cert_hash TEXT    NOT NULL,
        cert_hash     TEXT,
        name          TEXT    NOT NULL,
        server_id     INTEGER REFERENCES saved_server (id) ON DELETE SET NULL,
        server_label  TEXT    NOT NULL,
        blocked_at    INTEGER NOT NULL
    );
    CREATE UNIQUE INDEX blocked_user_cert ON blocked_user (own_cert_hash, cert_hash)
        WHERE cert_hash IS NOT NULL;
    CREATE UNIQUE INDEX blocked_user_name
        ON blocked_user (own_cert_hash, server_id, name COLLATE NOCASE)
        WHERE cert_hash IS NULL;
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

    /// Testers' local databases already have #218's `channel_audio_profile` as v18, so
    /// `blocked_user` must stay v19.
    #[test]
    fn blocked_user_is_v19_after_channel_audio_profile() {
        assert_eq!(MIGRATIONS.len(), 19);
        assert!(MIGRATIONS[16].contains("CREATE TABLE pm_conversation"));
        assert!(MIGRATIONS[17].contains("CREATE TABLE channel_audio_profile"));
        assert!(MIGRATIONS[18].contains("CREATE TABLE blocked_user"));
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
    fn version_9_database_upgrades_with_no_friends_and_keeps_private_history() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut conn = Connection::open(dir.path().join(DB_FILE_NAME)).expect("open");
            let tx = conn.transaction().expect("tx");
            for sql in &MIGRATIONS[..9] {
                tx.execute_batch(sql).expect("migration");
            }
            tx.pragma_update(None, "user_version", 9).expect("pragma");
            tx.execute(
                "INSERT INTO saved_server (label, address, username, sort_order)
                 VALUES ('Old', 'localhost', 'Tester', 0)",
                [],
            )
            .expect("server");
            tx.execute(
                "INSERT INTO chat_message
                     (server_id, conversation_kind, conversation_key, sender_name,
                      sender_cert_hash, sent_at, body)
                 VALUES (1, 'private', 'h1', 'Bot', 'h1', 5, 'hi')",
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
        assert!(store.friends().expect("friends").is_empty());
        store.add_friend("h1", "Bot", 6).expect("add");
        let chat = crate::Conversation::Private("h1".into());
        assert_eq!(store.messages(1, &chat, 10).expect("messages").len(), 1);
    }

    #[test]
    fn version_8_database_upgrades_and_marks_old_messages_as_live() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut conn = Connection::open(dir.path().join(DB_FILE_NAME)).expect("open");
            let tx = conn.transaction().expect("tx");
            for sql in &MIGRATIONS[..8] {
                tx.execute_batch(sql).expect("migration");
            }
            tx.pragma_update(None, "user_version", 8).expect("pragma");
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
        let lobby = crate::Conversation::Channel("Lobby".into());
        let kept = store.messages(1, &lobby, 10).expect("messages");
        assert_eq!(kept.len(), 1);
        assert!(
            store
                .synced_origins(&[kept[0].id])
                .expect("origins")
                .is_empty()
        );
    }

    #[test]
    fn version_10_database_upgrades_with_multi_server_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut conn = Connection::open(dir.path().join(DB_FILE_NAME)).expect("open");
            let tx = conn.transaction().expect("tx");
            for sql in &MIGRATIONS[..10] {
                tx.execute_batch(sql).expect("migration");
            }
            tx.pragma_update(None, "user_version", 10).expect("pragma");
            tx.execute(
                "INSERT INTO saved_server (label, address, username, sort_order)
                 VALUES ('Old', 'localhost', 'Tester', 0)",
                [],
            )
            .expect("insert");
            tx.commit().expect("commit");
        }
        let store = Store::open(dir.path()).expect("upgrade");
        let server = &store.servers().expect("servers")[0];
        assert_eq!(server.label, "Old");
        assert!(!server.always_connect_at_start && !server.notifications_muted);
        assert!(!server.group_mentions_muted);
        assert!(!server.was_voice_server_at_quit);
        assert_eq!(
            (server.connected_at_quit, server.theme_id.as_deref()),
            (None, None)
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
