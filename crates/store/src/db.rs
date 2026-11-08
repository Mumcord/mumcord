use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::error::{Error, Result};

/// File name of the database inside the app data directory.
pub const DB_FILE_NAME: &str = "mumcord.sqlite3";

/// Milliseconds since the Unix epoch (UTC). All timestamps are stored in this form.
pub type UnixMillis = i64;

/// Whether apps that know only the schema before a migration can still use the profile
/// after it (issue #521, see `docs/profile-database.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    /// Only adds tables, indexes or columns with a default (and fills columns it added).
    /// An older app ignores them and keeps working.
    Additive,
    /// Renames, removes or changes the meaning or format of something an older app uses.
    /// Older apps must refuse the profile.
    Breaking,
}

/// How an older app undoes a migration when it rolls a profile back (issue #521, see
/// `docs/profile-database.md`). The step is stored in the profile when the migration runs,
/// so an app that does not know the migration can still undo it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Down {
    /// Restores the schema from before the migration. It may only drop what the migration
    /// added, so only data written since then is lost.
    Sql(&'static str),
    /// Undoing it would lose data from before the migration, or certificate or identity data
    /// an additive migration created. A profile that needs it undone is never rolled back.
    Irreversible,
}

struct Migration {
    change: Change,
    sql: &'static str,
    down: Down,
}

const fn additive(sql: &'static str, down: Down) -> Migration {
    Migration {
        change: Change::Additive,
        sql,
        down,
    }
}

const fn breaking(sql: &'static str, down: Down) -> Migration {
    Migration {
        change: Change::Breaking,
        sql,
        down,
    }
}

/// Ordered schema migrations. Entry `n` upgrades the schema from version `n` to `n + 1`;
/// the current version is kept in SQLite's `user_version`. Never edit a released
/// migration – append a new one, marked [`additive`] or [`breaking`], with its [`Down`] step.
const MIGRATIONS: &[Migration] = &[
    // v1: initial schema (spec section 7).
    additive(
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
        // The initial schema holds everything, trusted server certificates included.
        Down::Irreversible,
    ),
    // v2: remember per server whether the default avatar was handled (FR-1.47).
    additive(
        r"
    ALTER TABLE saved_server ADD COLUMN default_avatar_sent INTEGER NOT NULL DEFAULT 0
        CHECK (default_avatar_sent IN (0, 1));
    ",
        Down::Sql("ALTER TABLE saved_server DROP COLUMN default_avatar_sent;"),
    ),
    // v3: the user's last own mute/deafen choice, restored on an explicit join (QA.7). Kept
    // out of the settings JSON so saving settings from the UI can never overwrite it.
    additive(
        r"
    CREATE TABLE own_voice (
        id   INTEGER PRIMARY KEY CHECK (id = 1),
        mute INTEGER NOT NULL CHECK (mute IN (0, 1)),
        deaf INTEGER NOT NULL CHECK (deaf IN (0, 1))
    );
    ",
        Down::Sql("DROP TABLE own_voice;"),
    ),
    // v4: the channel the user is joined to for voice, kept when the app quits so the next
    // start can offer to rejoin it (QA.9). `last_channel` also follows the read-only default
    // channel, so it cannot tell whether voice was on.
    additive(
        r"
    ALTER TABLE saved_server ADD COLUMN voice_channel TEXT;
    ",
        Down::Sql("ALTER TABLE saved_server DROP COLUMN voice_channel;"),
    ),
    // v5: the avatar remembered per server (hand-set or the default avatar, as a scaled PNG),
    // sent again on connect when the server keeps none for the user (FR-1.20, QA.10).
    // Servers that already got the default avatar are marked for a one-time backfill with
    // it; the image lives in a file, so the engine fills it in at start.
    additive(
        r"
    ALTER TABLE saved_server ADD COLUMN avatar BLOB;
    ALTER TABLE saved_server ADD COLUMN avatar_backfill INTEGER NOT NULL DEFAULT 0
        CHECK (avatar_backfill IN (0, 1));
    UPDATE saved_server SET avatar_backfill = 1 WHERE default_avatar_sent = 1;
    ",
        Down::Sql(
            r"
            ALTER TABLE saved_server DROP COLUMN avatar_backfill;
            ALTER TABLE saved_server DROP COLUMN avatar;
            ",
        ),
    ),
    // v6: preview cards the user dismissed (QA.42), per message and link; they go away with
    // the message. The timestamp index makes deleting old history an index range scan.
    additive(
        r"
    CREATE TABLE dismissed_preview (
        message_id INTEGER NOT NULL REFERENCES chat_message (id) ON DELETE CASCADE,
        url        TEXT    NOT NULL,
        PRIMARY KEY (message_id, url)
    );
    CREATE INDEX chat_message_sent_at ON chat_message (sent_at);
    ",
        Down::Sql(
            r"
            DROP INDEX chat_message_sent_at;
            DROP TABLE dismissed_preview;
            ",
        ),
    ),
    // v7: GIF favorites (FR-1.52, QA.43): saved image links the user can send again. The
    // image itself comes from the link-preview image cache, so only the link is stored.
    additive(
        r"
    CREATE TABLE gif_favorite (
        url          TEXT    PRIMARY KEY,
        added_at     INTEGER NOT NULL,
        last_used_at INTEGER NOT NULL
    );
    CREATE INDEX gif_favorite_last_used ON gif_favorite (last_used_at DESC);
    ",
        Down::Sql("DROP TABLE gif_favorite;"),
    ),
    // v8: private notes about people (FR-1.63), keyed like the person settings. Kept apart
    // from them because a note is free text and has nothing to do with playback.
    additive(
        r"
    CREATE TABLE person_note (
        server_id        INTEGER NOT NULL REFERENCES saved_server (id) ON DELETE CASCADE,
        person_cert_hash TEXT    NOT NULL,
        note             TEXT    NOT NULL,
        PRIMARY KEY (server_id, person_cert_hash)
    );
    ",
        Down::Sql("DROP TABLE person_note;"),
    ),
    // v9: history sync between Mumcord clients (FR-1.60, QA.48): the display name of the
    // person whose history a channel message came from; NULL for messages received live.
    additive(
        r"
    ALTER TABLE chat_message ADD COLUMN synced_from TEXT;
    ",
        Down::Sql("ALTER TABLE chat_message DROP COLUMN synced_from;"),
    ),
    // v10: friends by certificate hash (FR-1.64), local only and across servers. Private
    // chats with a friend are read across servers (FR-1.65), so private history also gets an
    // index that does not start with the server.
    additive(
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
        Down::Sql(
            r"
            DROP INDEX chat_message_by_key;
            DROP TABLE friend;
            ",
        ),
    ),
    // v11: multi-server state (FR-2.3, FR-2.7, FR-2.10, FR-2.17). `connected_at_quit` is the
    // quit time of a server that was connected then (NULL otherwise); the most recent one
    // wins when more servers than the limit were connected (AC-2.6).
    additive(
        r"
    ALTER TABLE saved_server ADD COLUMN always_connect_at_start INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE saved_server ADD COLUMN connected_at_quit INTEGER;
    ALTER TABLE saved_server ADD COLUMN was_voice_server_at_quit INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE saved_server ADD COLUMN notifications_muted INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE saved_server ADD COLUMN theme_id TEXT;
    ",
        Down::Sql(
            r"
            ALTER TABLE saved_server DROP COLUMN theme_id;
            ALTER TABLE saved_server DROP COLUMN notifications_muted;
            ALTER TABLE saved_server DROP COLUMN was_voice_server_at_quit;
            ALTER TABLE saved_server DROP COLUMN connected_at_quit;
            ALTER TABLE saved_server DROP COLUMN always_connect_at_start;
            ",
        ),
    ),
    // v12: when the user last cleared channel history (issue #119), so history sync does not
    // bring the deleted messages back from other people's copies. `server_id` NULL: all
    // servers; `channel_key` NULL: every channel of the server.
    additive(
        r"
    CREATE TABLE history_cleared (
        server_id   INTEGER REFERENCES saved_server (id) ON DELETE CASCADE,
        channel_key TEXT,
        cleared_at  INTEGER NOT NULL
    );
    ",
        Down::Sql("DROP TABLE history_cleared;"),
    ),
    // v13: non-secret details of the saved channel passwords (issue #146), by the token's
    // position in the server's token list in secure storage. `channels` is a JSON array of
    // `{channel_id, path}`. Tokens saved before have no row and keep their default name.
    additive(
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
        Down::Sql("DROP TABLE channel_token;"),
    ),
    // v14: several identities (issue #148). Certificates and keys stay in the OS secure
    // storage; this table holds the local label, the certificate's common name and hash, when
    // it was added and which one is the default. `saved_server.identity_id` NULL means "use
    // the default identity", so existing servers keep their identity after the upgrade.
    additive(
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
        // Dropping `identity` would delete the identities' metadata.
        Down::Irreversible,
    ),
    // v15: turning off `@all` / `@channel` mentions per server (issue #70, Q1.6).
    additive(
        r"
    ALTER TABLE saved_server ADD COLUMN group_mentions_muted INTEGER NOT NULL DEFAULT 0;
    ",
        Down::Sql("ALTER TABLE saved_server DROP COLUMN group_mentions_muted;"),
    ),
    // v16: accepted private conversations (issue #141). Kept per own identity, keyed by the
    // own certificate hash, so the same person can message one identity but not another.
    additive(
        r"
    CREATE TABLE accepted_conversation (
        own_cert_hash TEXT    NOT NULL,
        cert_hash     TEXT    NOT NULL,
        name          TEXT    NOT NULL,
        accepted_at   INTEGER NOT NULL,
        PRIMARY KEY (own_cert_hash, cert_hash)
    );
    ",
        Down::Sql("DROP TABLE accepted_conversation;"),
    ),
    // v17: pinned and muted private conversations in the private messages overview (issue
    // #173), by server and the other person's certificate hash. `muted_until` is Unix millis,
    // i64::MAX for "until turned back on", NULL when not muted.
    additive(
        r"
    CREATE TABLE pm_conversation (
        server_id   INTEGER NOT NULL REFERENCES saved_server (id) ON DELETE CASCADE,
        cert_hash   TEXT    NOT NULL,
        pinned      INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
        muted_until INTEGER,
        PRIMARY KEY (server_id, cert_hash)
    );
    ",
        Down::Sql("DROP TABLE pm_conversation;"),
    ),
    // v18: audio profiles per channel (issue #171): overrides of Music quality, low-latency mode
    // and the bitrate cap; NULL keeps the global setting. Keyed by channel id, with the channel
    // path kept so a renamed or recreated channel still matches.
    additive(
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
        Down::Sql("DROP TABLE channel_audio_profile;"),
    ),
    // v19: blocked users (issue #140), kept per own identity (FR-3.79). Lands after #218,
    // whose `channel_audio_profile` (v18, above) keeps its version number so the version
    // numbers match. A certificate block (`cert_hash` set) covers every server of
    // that identity (Q1.4); a name block (`cert_hash` NULL) is for people without a
    // certificate and only applies on `server_id`. `server_label` keeps where the block was
    // set readable after the server is removed.
    additive(
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
        Down::Sql("DROP TABLE blocked_user;"),
    ),
    // v20: mumpeer relay (spec 04). Lands after #218 (v18 `channel_audio_profile`) and #224
    // (v19 `blocked_user`), both on main above, so the version numbers match testers.
    // databases. `saved_server.p2p_mode_override` holds a `P2pMode` in snake_case, NULL
    // uses the global setting (FR-4.2). `relay_pin` is the trust-on-first-use pin of a server's
    // relay per server entry and own identity (FR-4.9, §7.1): the bot's certificate hash
    // (40 hex), the relay's certificate fingerprint (64 hex) and when it was first accepted.
    additive(
        r"
    ALTER TABLE saved_server ADD COLUMN p2p_mode_override TEXT CHECK (p2p_mode_override IN
        ('direct_relay', 'relay_first', 'direct_only', 'relay_only', 'off'));

    CREATE TABLE relay_pin (
        server_id     INTEGER NOT NULL REFERENCES saved_server (id) ON DELETE CASCADE,
        identity_id   INTEGER NOT NULL REFERENCES identity (id) ON DELETE CASCADE,
        bot_cert_hash TEXT    NOT NULL,
        relay_fp      TEXT    NOT NULL,
        first_seen    INTEGER NOT NULL,
        PRIMARY KEY (server_id, identity_id)
    );
    ",
        // `relay_pin` holds relay certificate pins.
        Down::Irreversible,
    ),
    // v21: plugin system (Epic 3.1, spec 06 section 7.1). `plugin` is one row per installed
    // plugin; `removed_at` is set when it is removed with its data kept (purged after 30
    // days, FR-6.62). `plugin_kv` holds plugin storage per scope (`global` or the identity id).
    // `server_plugin_suggestion` remembers what a server suggested to which identity.
    additive(
        r"
    CREATE TABLE plugin (
        id               TEXT    PRIMARY KEY,
        version          TEXT    NOT NULL,
        previous_version TEXT,
        source           TEXT    NOT NULL,
        sha256           TEXT,
        author_key       TEXT,
        enabled          INTEGER NOT NULL DEFAULT 0,
        disabled_reason  TEXT CHECK (disabled_reason IN
            ('crashes', 'revoked', 'tampered', 'user', 'permission')),
        visibility       TEXT    NOT NULL DEFAULT 'everyone'
            CHECK (visibility IN ('everyone', 'friends', 'nobody')),
        granted          TEXT    NOT NULL DEFAULT '[]',
        pinned           INTEGER NOT NULL DEFAULT 0,
        notifications    INTEGER NOT NULL DEFAULT 1,
        removed_at       INTEGER,
        installed_at     INTEGER NOT NULL
    );

    CREATE TABLE plugin_kv (
        plugin_id  TEXT    NOT NULL REFERENCES plugin (id) ON DELETE CASCADE,
        scope_key  TEXT    NOT NULL,
        key        TEXT    NOT NULL,
        value      BLOB    NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (plugin_id, scope_key, key)
    );

    CREATE TABLE plugin_repository (
        id            INTEGER PRIMARY KEY,
        url           TEXT    NOT NULL UNIQUE,
        name          TEXT    NOT NULL,
        public_key    TEXT    NOT NULL,
        pinned_at     INTEGER NOT NULL,
        enabled       INTEGER NOT NULL DEFAULT 1,
        paused_reason TEXT CHECK (paused_reason IN ('key-changed')),
        last_sequence INTEGER NOT NULL DEFAULT 0,
        last_checked  INTEGER
    );

    CREATE TABLE server_plugin_suggestion (
        server_id   INTEGER NOT NULL REFERENCES saved_server (id) ON DELETE CASCADE,
        identity_id INTEGER NOT NULL REFERENCES identity (id) ON DELETE CASCADE,
        plugin_id   TEXT    NOT NULL,
        seq         INTEGER NOT NULL,
        state       TEXT    NOT NULL
            CHECK (state IN ('new', 'not-now', 'dismissed', 'installed')),
        until       INTEGER,
        sender_pin  TEXT    NOT NULL,
        PRIMARY KEY (server_id, identity_id, plugin_id)
    );

    CREATE TABLE plugin_sender_pin (
        server_id   INTEGER NOT NULL REFERENCES saved_server (id) ON DELETE CASCADE,
        identity_id INTEGER NOT NULL REFERENCES identity (id) ON DELETE CASCADE,
        cert_hash   TEXT    NOT NULL,
        first_seen  INTEGER NOT NULL,
        PRIMARY KEY (server_id, identity_id)
    );
    ",
        // `plugin_sender_pin` holds certificate pins.
        Down::Irreversible,
    ),
    // v22: a person's screen share sound gets its own volume, separate from their voice
    // (issue #333). Existing rows keep their voice volume; their share starts at 100 %.
    additive(
        r"
    ALTER TABLE person_setting ADD COLUMN share_volume INTEGER NOT NULL DEFAULT 100
        CHECK (share_volume BETWEEN 0 AND 200);
    ",
        Down::Sql("ALTER TABLE person_setting DROP COLUMN share_volume;"),
    ),
    // v23: screen share encoder probe results, one set per machine (spec 07, FR-7.55). `meta`
    // holds the driver and app version the results were measured with; a change of either
    // means the probe runs again. `selftest` is the software encoder's measured time per
    // (resolution, frame rate).
    additive(
        r"
    CREATE TABLE encoder_cache_meta (
        id          INTEGER PRIMARY KEY CHECK (id = 1),
        driver_id   TEXT NOT NULL,
        app_version TEXT NOT NULL
    );
    CREATE TABLE encoder_cache (
        encoder  TEXT PRIMARY KEY,
        available INTEGER NOT NULL,
        smoke_ok  INTEGER NOT NULL
    );
    CREATE TABLE encoder_selftest (
        resolution     INTEGER NOT NULL,
        fps            INTEGER NOT NULL,
        svt_selftest_ms INTEGER NOT NULL,
        PRIMARY KEY (resolution, fps)
    );
    ",
        Down::Sql(
            r"
            DROP TABLE encoder_selftest;
            DROP TABLE encoder_cache;
            DROP TABLE encoder_cache_meta;
            ",
        ),
    ),
    // v24: how a file download ended, per chat message carrying the offer (spec 07, section
    // 7.2); it goes away with the message.
    additive(
        r"
    CREATE TABLE file_card (
        message_id INTEGER PRIMARY KEY REFERENCES chat_message (id) ON DELETE CASCADE,
        state      TEXT NOT NULL CHECK (state IN ('done', 'mismatch')),
        path       TEXT
    );
    ",
        Down::Sql("DROP TABLE file_card;"),
    ),
    // v25: plugin platform 1.1 trust data (spec 09, section 7.1). Origin approvals, per-day
    // request counters (kept 7 days), the first time a registry key was seen, per-version
    // permission history and confirmed high-risk plugins. v24 is the file-card table of the
    // file-sharing epic.
    additive(
        r"
    CREATE TABLE plugin_origin_approval (
        plugin_id   TEXT NOT NULL,
        origin      TEXT NOT NULL,
        purpose_key TEXT NOT NULL,
        approved_at INTEGER NOT NULL,
        PRIMARY KEY (plugin_id, origin)
    );
    CREATE TABLE plugin_origin_counters (
        plugin_id       TEXT NOT NULL,
        day             TEXT NOT NULL,
        requests        INTEGER NOT NULL DEFAULT 0,
        blocked         INTEGER NOT NULL DEFAULT 0,
        last_request_at INTEGER,
        last_host       TEXT,
        PRIMARY KEY (plugin_id, day)
    );
    CREATE TABLE repo_key_first_seen (
        key_id        TEXT PRIMARY KEY,
        first_seen_at INTEGER NOT NULL
    );
    CREATE TABLE plugin_version_history (
        plugin_id   TEXT NOT NULL,
        version     TEXT NOT NULL,
        permissions TEXT NOT NULL,
        origins     TEXT NOT NULL,
        seen_at     INTEGER NOT NULL,
        PRIMARY KEY (plugin_id, version)
    );
    CREATE TABLE plugin_high_risk_confirmed (
        plugin_id    TEXT NOT NULL,
        version      TEXT NOT NULL,
        confirmed_at INTEGER NOT NULL,
        PRIMARY KEY (plugin_id, version)
    );
    ",
        Down::Sql(
            r"
            DROP TABLE plugin_high_risk_confirmed;
            DROP TABLE plugin_version_history;
            DROP TABLE repo_key_first_seen;
            DROP TABLE plugin_origin_counters;
            DROP TABLE plugin_origin_approval;
            ",
        ),
    ),
    // v26: the software AV1 self-test also depends on "Optimize for" (spec 07, FR-7.56): Sharp
    // text costs about four times as much CPU, so a Balanced pass must not count for it. The
    // key changes, so the table is rebuilt; old results are dropped and simply measured again.
    // Breaking: an app before v26 writes rows without the new NOT NULL `optimize` column.
    breaking(
        r"
    DROP TABLE encoder_selftest;
    CREATE TABLE encoder_selftest (
        resolution      INTEGER NOT NULL,
        fps             INTEGER NOT NULL,
        optimize        TEXT NOT NULL CHECK (optimize IN ('balanced', 'text', 'motion')),
        svt_selftest_ms INTEGER NOT NULL,
        PRIMARY KEY (resolution, fps, optimize)
    );
    ",
        // Balanced results are what an app before v26 means by a result, so they are kept.
        Down::Sql(
            r"
        CREATE TEMP TABLE encoder_selftest_down AS
            SELECT resolution, fps, svt_selftest_ms FROM encoder_selftest
            WHERE optimize = 'balanced';
        DROP TABLE encoder_selftest;
        CREATE TABLE encoder_selftest (
            resolution     INTEGER NOT NULL,
            fps            INTEGER NOT NULL,
            svt_selftest_ms INTEGER NOT NULL,
            PRIMARY KEY (resolution, fps)
        );
        INSERT INTO encoder_selftest (resolution, fps, svt_selftest_ms)
            SELECT resolution, fps, svt_selftest_ms FROM temp.encoder_selftest_down;
        DROP TABLE temp.encoder_selftest_down;
        ",
        ),
    ),
    // v27: whether the "Expected key" entered when a repository was added matched the pinned
    // key (FR-6.74), for the security score's trust factor. Existing repositories default to 0.
    additive(
        r"
    ALTER TABLE plugin_repository ADD COLUMN expected_key_matched INTEGER NOT NULL DEFAULT 0;
    ",
        Down::Sql("ALTER TABLE plugin_repository DROP COLUMN expected_key_matched;"),
    ),
    // v28: "Skip this version" in the plugin Updates tab (FR-9.83): the newest version the user
    // skipped per plugin. That version and older ones are no longer offered; a newer one is.
    additive(
        r"
    CREATE TABLE plugin_skipped_update (
        plugin_id  TEXT PRIMARY KEY,
        version    TEXT NOT NULL,
        skipped_at INTEGER NOT NULL
    );
    ",
        Down::Sql("DROP TABLE plugin_skipped_update;"),
    ),
    // v29: plugins whose web content was turned off because Mumcord's window stopped answering
    // while one of their frames was open (FR-9.42). It stays off across restarts until the user
    // turns it on again in Settings → Plugins; the row is deleted then.
    additive(
        r"
    CREATE TABLE plugin_web_disabled (
        plugin_id   TEXT PRIMARY KEY,
        disabled_at INTEGER NOT NULL
    );
    ",
        Down::Sql("DROP TABLE plugin_web_disabled;"),
    ),
    // v30: the down step of every applied migration (issue #521), so an older app can roll a
    // profile back past migrations it does not know. `sql` NULL: the migration is
    // irreversible. Rows for v1–v30 are filled in from the code when this migration runs.
    additive(
        r"
    CREATE TABLE migration_down (
        version  INTEGER PRIMARY KEY,
        breaking INTEGER NOT NULL CHECK (breaking IN (0, 1)),
        sql      TEXT
    );
    ",
        Down::Sql("DROP TABLE migration_down;"),
    ),
];

/// The migration that creates `migration_down`. From here on every migration stores its down
/// step as it runs.
const DOWN_STEPS_VERSION: usize = 30;

/// Handle to the local database. Not `Sync`; callers share it behind a mutex.
pub struct Store {
    pub(crate) conn: Connection,
}

impl Store {
    /// Opens (or creates) the database in `data_dir` and applies pending migrations.
    /// The directory must exist; the caller decides where app data lives.
    pub fn open(data_dir: &Path) -> Result<Self> {
        // A read-write open already writes (journal mode, WAL files), which a newer
        // profile must never see: an older app may not change it (issue #510).
        check_schema(data_dir)?;
        let store = Self::from_connection(Connection::open(data_dir.join(DB_FILE_NAME))?)?;
        // The durability switch (spec 12 FR-12.8) is applied after opening and migrating:
        // migrations always run under NORMAL, so a schema change is never written with OFF.
        if !store.settings()?.durable_saves {
            store.conn.pragma_update(None, "synchronous", "OFF")?;
        }
        Ok(store)
    }

    fn from_connection(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // In WAL mode NORMAL only syncs at checkpoints: a commit cannot corrupt the database,
        // a power cut can lose the last commits, and each write stops waiting for the disk
        // (plugin storage writes run inside a plugin's 50 ms call budget).
        conn.pragma_update(None, "synchronous", "NORMAL")?;
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

/// The oldest schema an app must know to use a profile that has `applied` migrations: the
/// number of the newest breaking migration among them (at least 1). It is kept in SQLite's
/// `application_id`, a header field Mumcord has no other use for: it sits beside
/// `user_version`, changes in the same transaction as a migration and can be read from the
/// file header without opening the database (issue #521). 0 means "not recorded", as in
/// profiles last migrated before #521.
fn min_reader_version(applied: usize) -> i64 {
    MIGRATIONS[..applied]
        .iter()
        .rposition(|m| m.change == Change::Breaking)
        .map_or(1, |index| index as i64 + 1)
}

/// The schema versions stored in a profile.
#[derive(Debug, Clone, Copy)]
struct Versions {
    /// Number of applied migrations (`user_version`).
    schema: i64,
    /// See [`min_reader_version`] (`application_id`).
    min_reader: i64,
}

fn stored_versions(conn: &Connection) -> rusqlite::Result<Versions> {
    Ok(Versions {
        schema: conn.query_row("PRAGMA user_version", [], |row| row.get(0))?,
        min_reader: conn.query_row("PRAGMA application_id", [], |row| row.get(0))?,
    })
}

/// Fails with [`Error::SchemaTooNew`] if the profile in `data_dir` was upgraded by a newer
/// Mumcord in a way this app cannot work with. A newer profile whose later migrations were
/// all additive is fine. Reads only: the database file and its WAL files stay exactly as they
/// are, so the newer app can still open the profile. No database yet is fine (first start).
pub fn check_schema(data_dir: &Path) -> Result<()> {
    let path = data_dir.join(DB_FILE_NAME);
    if !path.is_file() {
        return Ok(());
    }
    let mut wal = path.clone().into_os_string();
    wal.push("-wal");
    if Path::new(&wal).exists() {
        // The newest version may still sit in the WAL, so SQLite has to read it. Its WAL
        // files already exist; a read-only connection leaves the database file alone.
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        return ensure_supported(stored_versions(&conn)?);
    }
    // Without a WAL the file header is the whole truth. Reading it directly avoids SQLite,
    // which would create WAL files beside a closed WAL-mode database even when read-only.
    match header_versions(&path)? {
        Some(versions) => ensure_supported(versions),
        // Not an SQLite file: the normal open reports that properly.
        None => Ok(()),
    }
}

/// `user_version` and `application_id` from the database header (big-endian integers at
/// byte offsets 60 and 68, see <https://www.sqlite.org/fileformat.html#the_database_header>),
/// or `None` if `path` holds no SQLite header.
fn header_versions(path: &Path) -> Result<Option<Versions>> {
    use std::io::Read;

    const MAGIC: &[u8; 16] = b"SQLite format 3\0";
    let mut header = [0u8; 72];
    let mut file = std::fs::File::open(path).map_err(Error::ProfileRead)?;
    if file.read_exact(&mut header).is_err() || !header.starts_with(MAGIC) {
        return Ok(None);
    }
    let field = |offset: usize| {
        let bytes: [u8; 4] = header[offset..offset + 4]
            .try_into()
            .expect("four header bytes");
        i64::from(i32::from_be_bytes(bytes))
    };
    Ok(Some(Versions {
        schema: field(60),
        min_reader: field(68),
    }))
}

fn ensure_supported(stored: Versions) -> Result<()> {
    let supported = MIGRATIONS.len() as i64;
    // A newer profile without a recorded minimum comes from an app before #521, which did not
    // mark its migrations, so nothing says this app can use it.
    let readable =
        stored.schema <= supported || (stored.min_reader > 0 && stored.min_reader <= supported);
    if !readable {
        return Err(Error::SchemaTooNew {
            found: stored.schema,
            supported,
        });
    }
    Ok(())
}

/// Applies the pending migrations. A profile already past this app's schema is left exactly
/// as it is: migrations never run backwards on their own and newer tables and columns are not
/// touched (see [`roll_back`] for the explicit way back).
fn migrate(conn: &mut Connection) -> Result<()> {
    let stored = stored_versions(conn)?;
    ensure_supported(stored)?;
    for index in stored.schema as usize..MIGRATIONS.len() {
        apply(conn, index)?;
    }
    if stored.schema == MIGRATIONS.len() as i64 && stored.min_reader == 0 {
        // Up to date but last migrated before #521: record the minimum once.
        conn.pragma_update(None, "application_id", min_reader_version(MIGRATIONS.len()))?;
    }
    Ok(())
}

/// Runs migration `index` (version `index + 1`) in one transaction, together with its stored
/// down step and the new version numbers.
fn apply(conn: &mut Connection, index: usize) -> Result<()> {
    let version = index + 1;
    let tx = conn.transaction()?;
    tx.execute_batch(MIGRATIONS[index].sql)?;
    if version >= DOWN_STEPS_VERSION {
        // The migration that creates the table also stores the steps of all before it.
        let first = if version == DOWN_STEPS_VERSION {
            1
        } else {
            version
        };
        for stored in first..=version {
            let migration = &MIGRATIONS[stored - 1];
            let sql = match migration.down {
                Down::Sql(sql) => Some(sql),
                Down::Irreversible => None,
            };
            tx.execute(
                "INSERT INTO migration_down (version, breaking, sql) VALUES (?1, ?2, ?3)",
                rusqlite::params![stored as i64, migration.change == Change::Breaking, sql],
            )?;
        }
    }
    tx.pragma_update(None, "user_version", version as i64)?;
    tx.pragma_update(None, "application_id", min_reader_version(version))?;
    tx.commit()?;
    Ok(())
}

/// A rollback that makes a profile from a newer app usable by this one: undoing the newer
/// migrations down to `to`, the version before the oldest breaking migration this app does
/// not know. Additive migrations below that one stay, as this app can work with them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rollback {
    /// The profile's schema version now.
    pub found: i64,
    /// Its schema version after the rollback.
    pub to: i64,
}

/// The rollback that would make the profile in `data_dir` usable by this app, if one is
/// needed and possible: every migration it has to undo must have a stored down step that is
/// not irreversible. Reads only, like [`check_schema`].
pub fn rollback_plan(data_dir: &Path) -> Result<Option<Rollback>> {
    let path = data_dir.join(DB_FILE_NAME);
    if check_schema(data_dir).is_ok() || !path.is_file() {
        return Ok(None);
    }
    let conn = open_untouched(&path)?;
    plan(&conn)
}

fn plan(conn: &Connection) -> Result<Option<Rollback>> {
    let own = MIGRATIONS.len() as i64;
    let found = stored_versions(conn)?.schema;
    let has_steps: bool = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'migration_down'",
        [],
        |row| row.get(0),
    )?;
    if found <= own || !has_steps {
        return Ok(None);
    }
    let steps: Vec<(i64, bool, bool)> = conn
        .prepare(
            "SELECT version, breaking, sql IS NOT NULL FROM migration_down
             WHERE version > ?1 AND version <= ?2 ORDER BY version",
        )?
        .query_map([own, found], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    if steps.len() as i64 != found - own {
        return Ok(None);
    }
    let Some(first_breaking) = steps.iter().position(|&(_, breaking, _)| breaking) else {
        return Ok(None);
    };
    if !steps[first_breaking..]
        .iter()
        .all(|&(_, _, reversible)| reversible)
    {
        return Ok(None);
    }
    Ok(Some(Rollback {
        found,
        to: steps[first_breaking].0 - 1,
    }))
}

/// Opens the database read-only without creating or changing any file: with its WAL files
/// when they exist (SQLite needs them for the newest data), as an immutable file otherwise.
fn open_untouched(path: &Path) -> Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    if Path::new(&wal).exists() {
        return Ok(Connection::open_with_flags(path, flags)?);
    }
    Ok(Connection::open_with_flags(
        file_uri(path)? + "?immutable=1",
        flags | OpenFlags::SQLITE_OPEN_URI,
    )?)
}

/// `path` as an SQLite `file:` URI (<https://www.sqlite.org/uri.html>).
fn file_uri(path: &Path) -> Result<String> {
    let absolute = std::path::absolute(path).map_err(Error::ProfileRead)?;
    let text = absolute.to_str().ok_or_else(|| {
        Error::ProfileRead(std::io::Error::other("the profile path is not valid UTF-8"))
    })?;
    let mut uri = String::from("file://");
    if !text.starts_with('/') {
        // A Windows drive path: `file:///C:/...`.
        uri.push('/');
    }
    for byte in text.replace('\\', "/").bytes() {
        if byte.is_ascii_alphanumeric() || b"/:-._~".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(uri)
}

/// Rolls the profile in `data_dir` back as [`rollback_plan`] describes, after saving a full
/// copy of the database file next to it, and returns that copy's path. The down steps run
/// from the newest migration on, all in one transaction: if one fails, the profile stays
/// exactly as it was.
pub fn roll_back(data_dir: &Path) -> Result<std::path::PathBuf> {
    let path = data_dir.join(DB_FILE_NAME);
    let plan = rollback_plan(data_dir)?.ok_or(Error::RollbackUnavailable)?;
    // Foreign keys stay off (the default): a down step that rebuilds a table must not
    // cascade into the older tables that reference it.
    let mut conn = Connection::open(&path)?;
    // Everything still in the WAL goes into the file, so the copy is complete.
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    let backup = backup(&path)?;
    let tx = conn.transaction()?;
    undo(&tx, plan)?;
    tx.pragma_update(None, "application_id", min_reader_version(MIGRATIONS.len()))?;
    tx.commit()?;
    Ok(backup)
}

/// Copies the database file to `mumcord.sqlite3.backup-<local time>` beside it, never over an
/// existing file.
fn backup(path: &Path) -> Result<std::path::PathBuf> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let target = path.with_file_name(format!("{DB_FILE_NAME}.backup-{stamp}"));
    let copy = || -> std::io::Result<()> {
        let mut source = std::fs::File::open(path)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        std::io::copy(&mut source, &mut file)?;
        file.sync_all()
    };
    copy().map_err(Error::ProfileBackup)?;
    Ok(target)
}

/// Runs the stored down steps from `plan.found` down to `plan.to + 1` and forgets them.
fn undo(tx: &rusqlite::Transaction<'_>, plan: Rollback) -> Result<()> {
    for version in (plan.to + 1..=plan.found).rev() {
        let sql: Option<String> = tx.query_row(
            "SELECT sql FROM migration_down WHERE version = ?1",
            [version],
            |row| row.get(0),
        )?;
        tx.execute_batch(&sql.ok_or(Error::RollbackUnavailable)?)?;
    }
    tx.execute("DELETE FROM migration_down WHERE version > ?1", [plan.to])?;
    tx.pragma_update(None, "user_version", plan.to)?;
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
    fn commits_do_not_wait_for_the_disk() {
        let (_dir, store) = temp_store();
        let mode: String = store
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("journal mode");
        let sync: i64 = store
            .conn
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .expect("synchronous");
        assert_eq!(
            (mode.as_str(), sync),
            ("wal", 1),
            "WAL with synchronous=NORMAL"
        );
    }

    #[test]
    fn durable_saves_off_sets_synchronous_off() {
        // FR-12.8 / FR-12.9: the switch is read after open + migration and applied then.
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path()).expect("open store");
        let mut settings = store.settings().expect("settings");
        settings.durable_saves = false;
        store.save_settings(&settings).expect("save");
        drop(store);

        let store = Store::open(dir.path()).expect("reopen");
        let sync: i64 = store
            .conn
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .expect("synchronous");
        assert_eq!(sync, 0, "synchronous=OFF when durable saves are off");
    }

    /// Testers' local databases already have #218's `channel_audio_profile` as v18, so
    /// `blocked_user` must stay v19.
    #[test]
    fn blocked_user_is_v19_after_channel_audio_profile() {
        assert!(MIGRATIONS[16].sql.contains("CREATE TABLE pm_conversation"));
        assert!(
            MIGRATIONS[17]
                .sql
                .contains("CREATE TABLE channel_audio_profile")
        );
        assert!(MIGRATIONS[18].sql.contains("CREATE TABLE blocked_user"));
        assert!(MIGRATIONS[19].sql.contains("CREATE TABLE relay_pin"));
        assert!(MIGRATIONS[20].sql.contains("CREATE TABLE plugin "));
        assert!(MIGRATIONS[21].sql.contains("share_volume"));
        assert!(MIGRATIONS[23].sql.contains("CREATE TABLE file_card"));
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
            for migration in &MIGRATIONS[..5] {
                tx.execute_batch(migration.sql).expect("migration");
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
            for migration in &MIGRATIONS[..7] {
                tx.execute_batch(migration.sql).expect("migration");
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
                local_mute: true,
                share_volume: 100,
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
            for migration in &MIGRATIONS[..9] {
                tx.execute_batch(migration.sql).expect("migration");
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
            for migration in &MIGRATIONS[..8] {
                tx.execute_batch(migration.sql).expect("migration");
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
            for migration in &MIGRATIONS[..10] {
                tx.execute_batch(migration.sql).expect("migration");
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

    /// Testers' databases already have #218's v18 and #224's v19; the relay migration must
    /// stay v20 after them.
    #[test]
    fn relay_migration_is_v20_after_audio_profile_and_blocked_user() {
        assert!(MIGRATIONS.len() >= 22);
        assert!(MIGRATIONS[16].sql.contains("CREATE TABLE pm_conversation"));
        assert!(
            MIGRATIONS[17]
                .sql
                .contains("CREATE TABLE channel_audio_profile")
        );
        assert!(MIGRATIONS[18].sql.contains("CREATE TABLE blocked_user"));
        assert!(MIGRATIONS[20].sql.contains("CREATE TABLE plugin "));
        assert!(MIGRATIONS[19].sql.contains("CREATE TABLE relay_pin"));
    }

    /// The repository "Expected key" flag (v27) defaults to false for existing rows.
    #[test]
    fn expected_key_migration_defaults_to_false() {
        assert!(MIGRATIONS.len() >= 27);
        assert!(MIGRATIONS[26].sql.contains("expected_key_matched"));
        assert!(MIGRATIONS[26].sql.contains("DEFAULT 0"));
    }

    /// The plugin trust tables (v25) come right after the file-card table (v24).
    #[test]
    fn plugin_trust_migration_creates_its_five_tables() {
        assert!(MIGRATIONS.len() >= 25);
        assert!(MIGRATIONS[23].sql.contains("CREATE TABLE file_card"));
        // v26 (the AV1 self-test keyed by "Optimize for") comes after it.
        assert!(MIGRATIONS[25].sql.contains("optimize"));
        let sql = MIGRATIONS[24].sql;
        for table in [
            "plugin_origin_approval",
            "plugin_origin_counters",
            "repo_key_first_seen",
            "plugin_version_history",
            "plugin_high_risk_confirmed",
        ] {
            assert!(sql.contains(&format!("CREATE TABLE {table} ")), "{table}");
        }
    }

    #[test]
    fn version_19_database_upgrades_with_no_override_and_no_relay_pins() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut conn = Connection::open(dir.path().join(DB_FILE_NAME)).expect("open");
            let tx = conn.transaction().expect("tx");
            for migration in &MIGRATIONS[..19] {
                tx.execute_batch(migration.sql).expect("migration");
            }
            tx.pragma_update(None, "user_version", 19).expect("pragma");
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
        assert_eq!(server.p2p_mode_override, None);
        let pins: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM relay_pin", [], |r| r.get(0))
            .expect("count");
        assert_eq!(pins, 0);
    }

    /// A v25 database gets both later migrations: the AV1 self-test rebuild and the
    /// repository "Expected key" flag, which defaults to false for existing rows.
    #[test]
    fn version_25_database_upgrades_through_v26_and_v27() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut conn = Connection::open(dir.path().join(DB_FILE_NAME)).expect("open");
            let tx = conn.transaction().expect("tx");
            for migration in &MIGRATIONS[..25] {
                tx.execute_batch(migration.sql).expect("migration");
            }
            tx.pragma_update(None, "user_version", 25).expect("pragma");
            tx.execute(
                "INSERT INTO plugin_repository (url, name, public_key, pinned_at)
                 VALUES ('https://example.invalid/r', 'Repo', 'key', 0)",
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
        let matched: i64 = store
            .conn
            .query_row(
                "SELECT expected_key_matched FROM plugin_repository",
                [],
                |r| r.get(0),
            )
            .expect("flag");
        assert_eq!(matched, 0);
        store
            .conn
            .execute(
                "INSERT INTO encoder_selftest (resolution, fps, optimize, svt_selftest_ms)
                 VALUES (1080, 30, 'text', 5)",
                [],
            )
            .expect("selftest table has the optimize key");
    }

    #[test]
    fn version_1_database_upgrades_and_keeps_servers() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut conn = Connection::open(dir.path().join(DB_FILE_NAME)).expect("open");
            let tx = conn.transaction().expect("tx");
            tx.execute_batch(MIGRATIONS[0].sql).expect("v1");
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
            for migration in &MIGRATIONS[..4] {
                tx.execute_batch(migration.sql).expect("old schema");
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

    /// A profile a newer Mumcord already upgraded with a breaking migration, closed cleanly as
    /// that app leaves it.
    fn newer_profile(dir: &Path) -> std::path::PathBuf {
        drop(Store::open(dir).expect("open"));
        let path = dir.join(DB_FILE_NAME);
        let conn = Connection::open(&path).expect("open");
        conn.pragma_update(None, "user_version", MIGRATIONS.len() as i64 + 4)
            .expect("pragma");
        conn.pragma_update(None, "application_id", MIGRATIONS.len() as i64 + 2)
            .expect("pragma");
        drop(conn);
        path
    }

    fn sidecars(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("read dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn opening_a_newer_profile_leaves_it_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = newer_profile(dir.path());
        let before = std::fs::read(&path).expect("read");
        let files = sidecars(dir.path());

        let supported = MIGRATIONS.len() as i64;
        let found = supported + 4;
        assert!(matches!(
            check_schema(dir.path()),
            Err(Error::SchemaTooNew { found: f, supported: s }) if f == found && s == supported
        ));
        assert!(matches!(
            Store::open(dir.path()),
            Err(Error::SchemaTooNew { .. })
        ));

        assert_eq!(std::fs::read(&path).expect("read"), before);
        assert_eq!(sidecars(dir.path()), files);
    }

    #[test]
    fn schema_check_reads_a_version_still_in_the_wal() {
        let dir = tempfile::tempdir().expect("tempdir");
        drop(Store::open(dir.path()).expect("open"));
        // The newer app still runs (or crashed): its upgrade is not checkpointed yet.
        let conn = Connection::open(dir.path().join(DB_FILE_NAME)).expect("open");
        conn.pragma_update(None, "wal_autocheckpoint", 0)
            .expect("pragma");
        conn.pragma_update(None, "user_version", MIGRATIONS.len() as i64 + 1)
            .expect("pragma");
        conn.pragma_update(None, "application_id", MIGRATIONS.len() as i64 + 1)
            .expect("pragma");
        assert!(matches!(
            check_schema(dir.path()),
            Err(Error::SchemaTooNew { .. })
        ));
    }

    #[test]
    fn schema_check_leaves_a_non_database_to_the_open() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(DB_FILE_NAME), b"not a database").expect("write");
        check_schema(dir.path()).expect("the open reports it");
    }

    #[test]
    fn schema_check_accepts_a_current_or_missing_profile() {
        let dir = tempfile::tempdir().expect("tempdir");
        check_schema(dir.path()).expect("no profile yet");
        assert!(sidecars(dir.path()).is_empty(), "the check creates no file");
        drop(Store::open(dir.path()).expect("open"));
        check_schema(dir.path()).expect("current profile");
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

    #[test]
    fn only_v26_is_breaking_so_the_minimum_reader_is_26() {
        let breaking: Vec<usize> = MIGRATIONS
            .iter()
            .enumerate()
            .filter(|(_, m)| m.change == Change::Breaking)
            .map(|(index, _)| index + 1)
            .collect();
        assert_eq!(breaking, [26]);
        assert_eq!(min_reader_version(1), 1);
        assert_eq!(min_reader_version(25), 1);
        assert_eq!(min_reader_version(26), 26);
        assert_eq!(min_reader_version(MIGRATIONS.len()), 26);
    }

    /// The CI check for migration authors (`docs/profile-database.md`): a migration marked
    /// additive may only create tables and indexes, add columns and fill columns it added.
    /// Anything else must be marked breaking.
    #[test]
    fn additive_migrations_only_add() {
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            if migration.change == Change::Breaking {
                continue;
            }
            let code: String = migration
                .sql
                .lines()
                .map(|line| line.split("--").next().unwrap_or_default())
                .collect::<Vec<_>>()
                .join(" ");
            for statement in code.split(';') {
                let words: Vec<String> = statement
                    .split_whitespace()
                    .take(5)
                    .map(str::to_ascii_uppercase)
                    .collect();
                let words: Vec<&str> = words.iter().map(String::as_str).collect();
                let allowed = match words.as_slice() {
                    [] | ["CREATE", "TABLE", ..] | ["CREATE", "INDEX", ..] => true,
                    ["CREATE", "UNIQUE", "INDEX", ..] => true,
                    ["ALTER", "TABLE", _, "ADD", "COLUMN"] => true,
                    ["UPDATE", table, "SET", column, ..] => migration.sql.contains(&format!(
                        "ALTER TABLE {} ADD COLUMN {} ",
                        table.to_ascii_lowercase(),
                        column.to_ascii_lowercase()
                    )),
                    _ => false,
                };
                assert!(
                    allowed,
                    "v{} is marked additive but runs `{}`; mark it breaking",
                    index + 1,
                    statement.trim()
                );
            }
        }
    }

    #[test]
    fn opening_records_the_minimum_reader_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        drop(Store::open(dir.path()).expect("open"));
        let path = dir.path().join(DB_FILE_NAME);
        let id = |conn: &Connection| -> i64 {
            conn.query_row("PRAGMA application_id", [], |row| row.get(0))
                .expect("pragma")
        };
        let conn = Connection::open(&path).expect("open");
        assert_eq!(id(&conn), 26);
        // A profile last migrated before #521 gets it on the next open.
        conn.pragma_update(None, "application_id", 0)
            .expect("pragma");
        drop(conn);
        drop(Store::open(dir.path()).expect("open"));
        assert_eq!(id(&Connection::open(&path).expect("open")), 26);
    }

    #[test]
    fn a_newer_profile_from_before_the_minimum_was_recorded_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = newer_profile(dir.path());
        let conn = Connection::open(&path).expect("open");
        conn.pragma_update(None, "application_id", 0)
            .expect("pragma");
        drop(conn);
        assert!(matches!(
            check_schema(dir.path()),
            Err(Error::SchemaTooNew { .. })
        ));
    }

    /// Every row of `table`, each column quoted as SQL, so values compare byte for byte.
    fn dump(conn: &Connection, table: &str) -> Vec<String> {
        let columns: Vec<String> = conn
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .expect("prepare")
            .query_map([], |row| row.get(0))
            .expect("query")
            .collect::<rusqlite::Result<_>>()
            .expect("columns");
        let quoted: Vec<String> = columns.iter().map(|c| format!("quote({c})")).collect();
        conn.prepare(&format!(
            "SELECT {} FROM {table} ORDER BY rowid",
            quoted.join(" || '|' || ")
        ))
        .expect("prepare")
        .query_map([], |row| row.get(0))
        .expect("query")
        .collect::<rusqlite::Result<_>>()
        .expect("rows")
    }

    /// Acceptance criteria of issue #521: an app opens a profile from a newer app whose later
    /// migrations were all additive, works with it, and leaves the newer data untouched.
    #[test]
    fn an_additive_newer_profile_opens_and_keeps_the_newer_data() {
        use crate::history::{Conversation, NewChatMessage};
        use crate::servers::NewServer;

        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path()).expect("open");
        let server = store
            .add_server(&NewServer::new("Home", "localhost", "alice"))
            .expect("server");
        drop(store);

        // A newer Mumcord adds a table, a column and settings this app does not know.
        let path = dir.path().join(DB_FILE_NAME);
        let newer = MIGRATIONS.len() as i64 + 3;
        let conn = Connection::open(&path).expect("open");
        conn.execute_batch(
            "CREATE TABLE future_thing (id INTEGER PRIMARY KEY, payload BLOB NOT NULL);
             INSERT INTO future_thing (payload) VALUES (x'00ff10'), (zeroblob(3));
             ALTER TABLE saved_server ADD COLUMN future_color TEXT;
             UPDATE saved_server SET future_color = 'teal';
             INSERT INTO settings (id, data) VALUES (1,
                 '{\"wizard_completed\":true,\"theme\":\"neon\",\"future_flag\":{\"a\":1}}');",
        )
        .expect("newer schema");
        conn.pragma_update(None, "user_version", newer)
            .expect("pragma");
        drop(conn);

        check_schema(dir.path()).expect("additive changes are readable");
        let store = Store::open(dir.path()).expect("open");
        assert_eq!(store.schema_version().expect("version"), newer);
        let mut saved = store.servers().expect("servers").remove(0);
        saved.label = "Renamed".into();
        store.update_server(&saved).expect("update");
        let message = NewChatMessage {
            conversation: Conversation::Channel("Root".into()),
            sender_name: "alice".into(),
            sender_cert_hash: None,
            sent_at: 1,
            body: "hi".into(),
        };
        assert!(store.append_message(server.id, &message).expect("append"));
        let mut settings = store.settings().expect("settings");
        assert!(settings.wizard_completed);
        settings.wizard_completed = false;
        store.save_settings(&settings).expect("save");
        drop(store);

        // The newer app finds its data unchanged.
        let conn = Connection::open(&path).expect("open");
        let versions = stored_versions(&conn).expect("versions");
        assert_eq!((versions.schema, versions.min_reader), (newer, 26));
        assert_eq!(dump(&conn, "future_thing"), ["1|X'00FF10'", "2|X'000000'"]);
        let color: String = conn
            .query_row("SELECT future_color FROM saved_server", [], |row| {
                row.get(0)
            })
            .expect("column");
        assert_eq!(color, "teal");
        let data: String = conn
            .query_row("SELECT data FROM settings", [], |row| row.get(0))
            .expect("settings");
        let data: serde_json::Value = serde_json::from_str(&data).expect("json");
        assert_eq!(data["theme"], "neon");
        assert_eq!(data["future_flag"], serde_json::json!({ "a": 1 }));
        assert_eq!(data["wizard_completed"], false);
    }

    /// Rows in the tables of the initial schema, which every later migration has to keep.
    fn seed(conn: &Connection) {
        conn.execute_batch(
            "INSERT INTO saved_server (id, label, address, port, username, sort_order)
                 VALUES (1, 'Home', 'localhost', 64738, 'alice', 0);
             INSERT INTO trusted_cert (server_id, fingerprint) VALUES (1, 'ab12');
             INSERT INTO followed_channel (server_id, channel_path, last_read_at)
                 VALUES (1, 'Root/Games', 5);
             INSERT INTO chat_message (id, server_id, conversation_kind, conversation_key,
                 sender_name, sender_cert_hash, sent_at, body)
                 VALUES (1, 1, 'channel', 'Root', 'bob', 'cd34', 7, 'hello');
             INSERT INTO person_setting (server_id, person_cert_hash, volume, local_mute)
                 VALUES (1, 'cd34', 150, 1);
             INSERT INTO settings (id, data) VALUES (1, '{\"theme\":\"dark\"}');",
        )
        .expect("seed");
    }

    /// Schema (whitespace-insensitive), version numbers and every row of a database.
    fn snapshot(conn: &Connection) -> Vec<String> {
        let mut schema: Vec<(String, String, String)> = conn
            .prepare("SELECT type, name, coalesce(sql, '') FROM sqlite_master")
            .expect("prepare")
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .expect("query")
            .collect::<rusqlite::Result<_>>()
            .expect("schema");
        schema.sort();
        let versions = stored_versions(conn).expect("versions");
        let mut out = vec![format!("v{} min{}", versions.schema, versions.min_reader)];
        for (kind, name, sql) in schema {
            let sql = sql
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .replace(" ,", ",")
                .replace("( ", "(")
                .replace(" )", ")")
                .replace("\"", "");
            out.push(format!("{kind} {name}: {sql}"));
            if kind == "table" {
                out.extend(
                    dump(conn, &name)
                        .into_iter()
                        .map(|row| format!("{name}: {row}")),
                );
            }
        }
        out
    }

    fn down(conn: &mut Connection, index: usize) {
        let Down::Sql(sql) = MIGRATIONS[index].down else {
            panic!("v{} is irreversible", index + 1);
        };
        let tx = conn.transaction().expect("tx");
        tx.execute_batch(sql).expect("down step");
        tx.pragma_update(None, "user_version", index as i64)
            .expect("pragma");
        tx.pragma_update(None, "application_id", min_reader_version(index))
            .expect("pragma");
        tx.commit().expect("commit");
    }

    /// Every reversible migration's down step restores the schema and data from before it,
    /// and the migration runs again afterwards with the same result.
    #[test]
    fn every_down_step_round_trips_on_a_seeded_profile() {
        for (index, migration) in MIGRATIONS.iter().enumerate().skip(1) {
            if migration.down == Down::Irreversible {
                continue;
            }
            let mut conn = Connection::open_in_memory().expect("open");
            apply(&mut conn, 0).expect("v1");
            seed(&conn);
            for earlier in 1..index {
                apply(&mut conn, earlier).expect("earlier migration");
            }
            let before = snapshot(&conn);
            apply(&mut conn, index).expect("up");
            let after = snapshot(&conn);
            down(&mut conn, index);
            assert_eq!(snapshot(&conn), before, "v{} down", index + 1);
            apply(&mut conn, index).expect("up again");
            assert_eq!(snapshot(&conn), after, "v{} up again", index + 1);
        }
    }

    #[test]
    fn only_migrations_with_certificate_or_identity_tables_are_irreversible() {
        let irreversible: Vec<usize> = MIGRATIONS
            .iter()
            .enumerate()
            .filter(|(_, m)| m.down == Down::Irreversible)
            .map(|(index, _)| index + 1)
            .collect();
        assert_eq!(irreversible, [1, 14, 20, 21]);
    }

    #[test]
    fn the_down_steps_are_stored_for_every_applied_migration() {
        let (_dir, store) = temp_store();
        let steps: Vec<(i64, bool, Option<String>)> = store
            .conn
            .prepare("SELECT version, breaking, sql FROM migration_down ORDER BY version")
            .expect("prepare")
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .expect("query")
            .collect::<rusqlite::Result<_>>()
            .expect("rows");
        assert_eq!(steps.len(), MIGRATIONS.len());
        assert!(
            MIGRATIONS[DOWN_STEPS_VERSION - 1]
                .sql
                .contains("CREATE TABLE migration_down")
        );
        for ((version, breaking, sql), migration) in steps.iter().zip(MIGRATIONS) {
            assert_eq!(
                *breaking,
                migration.change == Change::Breaking,
                "v{version}"
            );
            let expected = match migration.down {
                Down::Sql(sql) => Some(sql),
                Down::Irreversible => None,
            };
            assert_eq!(sql.as_deref(), expected, "v{version}");
        }
    }

    /// What a newer Mumcord leaves behind: v+1 additive (`future_note`), v+2 breaking
    /// (`saved_server.label` renamed), v+3 additive (`saved_server.color`), each with its down
    /// step stored, and the given down step for v+2.
    fn profile_from_a_newer_app(dir: &Path, breaking_down: Option<&str>) -> std::path::PathBuf {
        use crate::servers::NewServer;

        let store = Store::open(dir).expect("open");
        store
            .add_server(&NewServer::new("Home", "localhost", "alice"))
            .expect("server");
        drop(store);
        let path = dir.join(DB_FILE_NAME);
        let own = MIGRATIONS.len() as i64;
        let conn = Connection::open(&path).expect("open");
        conn.execute_batch(
            "CREATE TABLE future_note (id INTEGER PRIMARY KEY, text TEXT NOT NULL);
             INSERT INTO future_note (text) VALUES ('kept');
             ALTER TABLE saved_server RENAME COLUMN label TO title;
             ALTER TABLE saved_server ADD COLUMN color TEXT;
             UPDATE saved_server SET color = 'teal';",
        )
        .expect("newer schema");
        let mut insert = conn
            .prepare("INSERT INTO migration_down (version, breaking, sql) VALUES (?1, ?2, ?3)")
            .expect("prepare");
        insert
            .execute(rusqlite::params![own + 1, false, "DROP TABLE future_note;"])
            .expect("step");
        insert
            .execute(rusqlite::params![own + 2, true, breaking_down])
            .expect("step");
        insert
            .execute(rusqlite::params![
                own + 3,
                false,
                "ALTER TABLE saved_server DROP COLUMN color;"
            ])
            .expect("step");
        drop(insert);
        conn.pragma_update(None, "user_version", own + 3)
            .expect("pragma");
        conn.pragma_update(None, "application_id", own + 2)
            .expect("pragma");
        drop(conn);
        path
    }

    const RENAME_BACK: &str = "ALTER TABLE saved_server RENAME COLUMN title TO label;";

    /// Acceptance criteria of the rollback (issue #521): this app undoes a newer app's breaking
    /// migration with nothing but the stored down steps, saves a full copy first, keeps the
    /// additive migration below it and starts normally afterwards.
    #[test]
    fn an_older_app_rolls_back_a_breaking_change_with_the_stored_steps() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = profile_from_a_newer_app(dir.path(), Some(RENAME_BACK));
        let before = std::fs::read(&path).expect("read");
        let files = sidecars(dir.path());
        let own = MIGRATIONS.len() as i64;

        let plan = rollback_plan(dir.path()).expect("plan");
        assert_eq!(
            plan,
            Some(Rollback {
                found: own + 3,
                to: own + 1
            })
        );
        assert_eq!(
            std::fs::read(&path).expect("read"),
            before,
            "planning reads only"
        );
        assert_eq!(sidecars(dir.path()), files);

        let backup = roll_back(dir.path()).expect("roll back");
        assert_eq!(backup.parent(), Some(dir.path()));
        let name = backup.file_name().expect("name").to_string_lossy();
        assert!(
            name.starts_with(&format!("{DB_FILE_NAME}.backup-")),
            "{name}"
        );
        assert_eq!(std::fs::read(&backup).expect("backup"), before);

        check_schema(dir.path()).expect("readable again");
        let store = Store::open(dir.path()).expect("open");
        assert_eq!(store.schema_version().expect("version"), own + 1);
        assert_eq!(store.servers().expect("servers")[0].label, "Home");
        let conn = &store.conn;
        assert_eq!(stored_versions(conn).expect("versions").min_reader, 26);
        assert_eq!(
            dump(conn, "future_note"),
            ["1|'kept'"],
            "additive change stays"
        );
        let color: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('saved_server') WHERE name = 'color'",
                [],
                |row| row.get(0),
            )
            .expect("columns");
        assert_eq!(color, 0);
        let newest: i64 = conn
            .query_row("SELECT max(version) FROM migration_down", [], |row| {
                row.get(0)
            })
            .expect("steps");
        assert_eq!(newest, own + 1);
    }

    #[test]
    fn a_failing_down_step_leaves_the_profile_byte_identical() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = profile_from_a_newer_app(
            dir.path(),
            Some("ALTER TABLE saved_server RENAME COLUMN missing TO label;"),
        );
        let before = std::fs::read(&path).expect("read");

        assert!(rollback_plan(dir.path()).expect("plan").is_some());
        assert!(matches!(roll_back(dir.path()), Err(Error::Database(_))));

        assert_eq!(std::fs::read(&path).expect("read"), before);
        assert!(!dir.path().join(format!("{DB_FILE_NAME}-wal")).exists());
        assert!(matches!(
            check_schema(dir.path()),
            Err(Error::SchemaTooNew { .. })
        ));
    }

    #[test]
    fn an_irreversible_or_unknown_step_offers_no_rollback() {
        let dir = tempfile::tempdir().expect("tempdir");
        profile_from_a_newer_app(dir.path(), None);
        assert_eq!(rollback_plan(dir.path()).expect("plan"), None);
        assert!(matches!(
            roll_back(dir.path()),
            Err(Error::RollbackUnavailable)
        ));

        // A newer profile without stored steps for its migrations.
        let dir = tempfile::tempdir().expect("tempdir");
        newer_profile(dir.path());
        assert_eq!(rollback_plan(dir.path()).expect("plan"), None);
    }

    #[test]
    fn a_readable_profile_needs_no_rollback() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(rollback_plan(dir.path()).expect("no profile"), None);
        drop(Store::open(dir.path()).expect("open"));
        assert_eq!(rollback_plan(dir.path()).expect("current"), None);
    }
}
