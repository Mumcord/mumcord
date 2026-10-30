use rusqlite::{OptionalExtension, Row, params};

use crate::db::{Store, UnixMillis};
use crate::error::{Error, Result};
use crate::identities::IdentityId;
use crate::secrets::SecretStore;

/// Mumble's default server port.
pub const DEFAULT_PORT: u16 = 64738;

/// Database id of a saved server; also used to name its password in secure storage.
pub type ServerId = i64;

/// Input for adding a server. The password is not part of it: it goes to
/// [`SecretStore::set_server_password`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewServer {
    pub label: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub save_history: bool,
    /// Identity to connect with; `None` uses the default identity (issue #148).
    pub identity_id: Option<IdentityId>,
}

impl NewServer {
    /// Server with the default port, history saving on and the default identity.
    pub fn new(label: &str, address: &str, username: &str) -> Self {
        Self {
            label: label.to_owned(),
            address: address.to_owned(),
            port: DEFAULT_PORT,
            username: username.to_owned(),
            save_history: true,
            identity_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedServer {
    pub id: ServerId,
    pub label: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    /// Channel path to rejoin on the next connection (FR-1.14).
    pub last_channel: Option<String>,
    pub save_history: bool,
    pub sort_order: i64,
    /// Connect this server when the app starts (FR-2.7).
    pub always_connect_at_start: bool,
    /// When the app was quit while connected to this server; `None` if it was not connected.
    /// Set with [`Store::set_quit_state`], not by [`Store::update_server`].
    pub connected_at_quit: Option<UnixMillis>,
    /// This was the voice server when the app was quit, for the rejoin prompt (FR-2.3).
    pub was_voice_server_at_quit: bool,
    /// No notifications from this server (FR-2.10).
    pub notifications_muted: bool,
    /// Theme for this server; `None` uses the app theme (FR-2.17).
    pub theme_id: Option<String>,
    /// Identity to connect with; `None` uses the default identity (issue #148).
    pub identity_id: Option<IdentityId>,
}

const SERVER_COLUMNS: &str = "id, label, address, port, username, last_channel, save_history, \
     sort_order, always_connect_at_start, connected_at_quit, was_voice_server_at_quit, \
     notifications_muted, theme_id, identity_id";

fn server_from_row(row: &Row<'_>) -> rusqlite::Result<SavedServer> {
    Ok(SavedServer {
        id: row.get(0)?,
        label: row.get(1)?,
        address: row.get(2)?,
        port: row.get(3)?,
        username: row.get(4)?,
        last_channel: row.get(5)?,
        save_history: row.get(6)?,
        sort_order: row.get(7)?,
        always_connect_at_start: row.get(8)?,
        connected_at_quit: row.get(9)?,
        was_voice_server_at_quit: row.get(10)?,
        notifications_muted: row.get(11)?,
        theme_id: row.get(12)?,
        identity_id: row.get(13)?,
    })
}

impl Store {
    /// Adds a server at the end of the server bar.
    pub fn add_server(&self, server: &NewServer) -> Result<SavedServer> {
        if server.port == 0 {
            return Err(Error::InvalidPort);
        }
        if let Some(identity) = server.identity_id
            && self.identity(identity)?.is_none()
        {
            return Err(Error::UnknownIdentity(identity));
        }
        self.conn.execute(
            "INSERT INTO saved_server (label, address, port, username, save_history, identity_id,
                                      sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6,
                     (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM saved_server))",
            params![
                server.label,
                server.address,
                server.port,
                server.username,
                server.save_history,
                server.identity_id
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.server(id)?.ok_or(Error::UnknownServer(id))
    }

    /// All saved servers in server-bar order.
    pub fn servers(&self) -> Result<Vec<SavedServer>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SERVER_COLUMNS} FROM saved_server ORDER BY sort_order, id"
        ))?;
        let rows = stmt.query_map([], server_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn server(&self, id: ServerId) -> Result<Option<SavedServer>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {SERVER_COLUMNS} FROM saved_server WHERE id = ?1"),
                [id],
                server_from_row,
            )
            .optional()?)
    }

    /// Updates label, address, port, username, last channel, the save-history switch, the
    /// multi-server settings (always connect, muted notifications, theme) and the identity.
    /// The order is changed with [`Store::reorder_servers`], the quit state with
    /// [`Store::set_quit_state`].
    pub fn update_server(&self, server: &SavedServer) -> Result<()> {
        if server.port == 0 {
            return Err(Error::InvalidPort);
        }
        let changed = self.conn.execute(
            "UPDATE saved_server
             SET label = ?2, address = ?3, port = ?4, username = ?5, last_channel = ?6,
                 save_history = ?7, always_connect_at_start = ?8, notifications_muted = ?9,
                 theme_id = ?10, identity_id = ?11
             WHERE id = ?1",
            params![
                server.id,
                server.label,
                server.address,
                server.port,
                server.username,
                server.last_channel,
                server.save_history,
                server.always_connect_at_start,
                server.notifications_muted,
                server.theme_id,
                server.identity_id
            ],
        )?;
        expect_one(changed, server.id)
    }

    /// Records, for every server, whether it was connected at quit and which one was the voice
    /// server. `connected` holds the connected servers; `voice` is at most one of them. Servers
    /// not listed are marked as not connected.
    pub fn set_quit_state(
        &self,
        connected: &[ServerId],
        voice: Option<ServerId>,
        quit_at: UnixMillis,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE saved_server SET connected_at_quit = NULL, was_voice_server_at_quit = 0",
            [],
        )?;
        for id in connected {
            let changed = tx.execute(
                "UPDATE saved_server SET connected_at_quit = ?2, was_voice_server_at_quit = ?3
                 WHERE id = ?1",
                params![id, quit_at, voice == Some(*id)],
            )?;
            expect_one(changed, *id)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn set_last_channel(&self, id: ServerId, channel_path: Option<&str>) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE saved_server SET last_channel = ?2 WHERE id = ?1",
            params![id, channel_path],
        )?;
        expect_one(changed, id)
    }

    /// Servers with a remembered voice channel and its path (QA.9).
    pub fn voice_channels(&self) -> Result<Vec<(ServerId, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, voice_channel FROM saved_server WHERE voice_channel IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The channel the user is joined to for voice on this server (QA.9).
    pub fn set_voice_channel(&self, id: ServerId, channel_path: &str) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE saved_server SET voice_channel = ?2 WHERE id = ?1",
            params![id, channel_path],
        )?;
        expect_one(changed, id)
    }

    /// Forgets the voice channel on every server: voice is on for at most one connection.
    pub fn clear_voice_channels(&self) -> Result<()> {
        self.conn.execute(
            "UPDATE saved_server SET voice_channel = NULL WHERE voice_channel IS NOT NULL",
            [],
        )?;
        Ok(())
    }

    /// The per-server "Save history" switch (FR-1.33). Turning it off stops storing
    /// new messages; existing history stays until cleared.
    pub fn set_save_history(&self, id: ServerId, save: bool) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE saved_server SET save_history = ?2 WHERE id = ?1",
            params![id, save],
        )?;
        expect_one(changed, id)
    }

    /// Whether the default avatar was already handled for this server (FR-1.47).
    pub fn default_avatar_sent(&self, id: ServerId) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT default_avatar_sent FROM saved_server WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(Error::UnknownServer(id))
    }

    /// Marks the default avatar as handled for this server. Never reset: manual avatar
    /// changes on the server must not make it send again (FR-1.47).
    pub fn set_default_avatar_sent(&self, id: ServerId) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE saved_server SET default_avatar_sent = 1 WHERE id = ?1",
            [id],
        )?;
        expect_one(changed, id)
    }

    /// The avatar remembered for this server as image bytes; `None` if there is none
    /// (FR-1.20, QA.10).
    pub fn server_avatar(&self, id: ServerId) -> Result<Option<Vec<u8>>> {
        self.conn
            .query_row(
                "SELECT avatar FROM saved_server WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(Error::UnknownServer(id))
    }

    /// Remembers the avatar the user has on this server; `None` remembers "no avatar". Also
    /// marks the default avatar as handled, so it never replaces a choice made here.
    pub fn set_server_avatar(&self, id: ServerId, avatar: Option<&[u8]>) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE saved_server SET avatar = ?2, default_avatar_sent = 1, avatar_backfill = 0
             WHERE id = ?1",
            params![id, avatar],
        )?;
        expect_one(changed, id)
    }

    /// One-time backfill for servers that got the default avatar before avatars were
    /// remembered per server: they remember `default` (the current default avatar; `None`
    /// if none is set). Returns how many servers were handled (QA.10).
    pub fn backfill_server_avatars(&self, default: Option<&[u8]>) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE saved_server SET avatar = COALESCE(avatar, ?1), avatar_backfill = 0
             WHERE avatar_backfill = 1",
            [default],
        )?)
    }

    /// Sets the server-bar order to the given sequence of ids. Servers not listed
    /// keep their relative order after the listed ones.
    pub fn reorder_servers(&self, ordered_ids: &[ServerId]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let mut rest: Vec<ServerId> = {
            let mut stmt = tx.prepare("SELECT id FROM saved_server ORDER BY sort_order, id")?;
            let ids = stmt.query_map([], |row| row.get(0))?;
            ids.collect::<rusqlite::Result<_>>()?
        };
        for id in ordered_ids {
            if !rest.contains(id) {
                return Err(Error::UnknownServer(*id));
            }
        }
        rest.retain(|id| !ordered_ids.contains(id));
        for (position, id) in ordered_ids.iter().chain(rest.iter()).enumerate() {
            tx.execute(
                "UPDATE saved_server SET sort_order = ?2 WHERE id = ?1",
                params![id, position as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Removes a server with all its local data (trust decision, follows, history,
    /// person settings) and its password and channel passwords in secure storage.
    pub fn remove_server(&self, id: ServerId, secrets: &SecretStore) -> Result<()> {
        // Secrets first: if secure storage fails, the server stays and the user can retry
        // instead of leaving an orphaned secret behind.
        secrets.delete_server_secrets(id)?;
        let changed = self
            .conn
            .execute("DELETE FROM saved_server WHERE id = ?1", [id])?;
        expect_one(changed, id)
    }
}

fn expect_one(changed: usize, id: ServerId) -> Result<()> {
    if changed == 0 {
        Err(Error::UnknownServer(id))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_store;

    #[test]
    fn voice_channel_is_set_kept_by_edits_and_cleared_everywhere() {
        let (_dir, store) = temp_store();
        let a = store
            .add_server(&NewServer::new("A", "localhost", "Tester"))
            .expect("add");
        let b = store
            .add_server(&NewServer::new("B", "localhost", "Tester"))
            .expect("add");
        assert!(store.voice_channels().expect("read").is_empty());
        store.set_voice_channel(a.id, "Root/Raid").expect("set");
        store.set_voice_channel(b.id, "Root").expect("set");
        store.update_server(&a).expect("edit");
        store.set_last_channel(a.id, Some("Root")).expect("last");
        assert_eq!(
            store.voice_channels().expect("read"),
            [(a.id, "Root/Raid".to_owned()), (b.id, "Root".to_owned())]
        );
        store.clear_voice_channels().expect("clear");
        assert!(store.voice_channels().expect("read").is_empty());
        assert!(matches!(
            store.set_voice_channel(999, "Root"),
            Err(Error::UnknownServer(999))
        ));
    }

    #[test]
    fn multi_server_fields_default_off_and_round_trip() {
        let (_dir, store) = temp_store();
        let added = store
            .add_server(&NewServer::new("A", "localhost", "Tester"))
            .expect("add");
        assert!(!added.always_connect_at_start && !added.notifications_muted);
        assert!(!added.was_voice_server_at_quit);
        assert_eq!(
            (added.connected_at_quit, added.theme_id.clone()),
            (None, None)
        );

        let mut edited = added.clone();
        edited.always_connect_at_start = true;
        edited.notifications_muted = true;
        edited.theme_id = Some("forest".into());
        store.update_server(&edited).expect("update");
        assert_eq!(store.server(added.id).expect("get"), Some(edited.clone()));

        // A stale struct saved later must not wipe the quit state.
        store
            .set_quit_state(&[added.id], Some(added.id), 42)
            .expect("quit");
        store.update_server(&edited).expect("update again");
        let loaded = store.server(added.id).expect("get").expect("exists");
        assert_eq!(loaded.connected_at_quit, Some(42));
        assert!(loaded.was_voice_server_at_quit);
    }

    #[test]
    fn quit_state_replaces_the_previous_one() {
        let (_dir, store) = temp_store();
        let a = store
            .add_server(&NewServer::new("A", "localhost", "T"))
            .expect("add");
        let b = store
            .add_server(&NewServer::new("B", "localhost", "T"))
            .expect("add");
        store
            .set_quit_state(&[a.id, b.id], Some(b.id), 10)
            .expect("quit");
        store.set_quit_state(&[b.id], None, 20).expect("quit");
        let a = store.server(a.id).expect("get").expect("a");
        let b = store.server(b.id).expect("get").expect("b");
        assert_eq!(
            (a.connected_at_quit, a.was_voice_server_at_quit),
            (None, false)
        );
        assert_eq!(
            (b.connected_at_quit, b.was_voice_server_at_quit),
            (Some(20), false)
        );
        assert!(matches!(
            store.set_quit_state(&[999], None, 1),
            Err(Error::UnknownServer(999))
        ));
    }

    #[test]
    fn add_list_update_and_defaults() {
        let (_dir, store) = temp_store();
        let added = store
            .add_server(&NewServer::new("Home", "localhost", "Tester"))
            .expect("add");
        assert_eq!(added.port, DEFAULT_PORT);
        assert!(added.save_history);
        assert_eq!(added.last_channel, None);

        let mut edited = added.clone();
        edited.label = "Home server".into();
        edited.port = 1234;
        edited.last_channel = Some("Root/Lobby".into());
        store.update_server(&edited).expect("update");
        assert_eq!(store.server(added.id).expect("get"), Some(edited));

        store
            .set_last_channel(added.id, None)
            .expect("last channel");
        assert_eq!(
            store
                .server(added.id)
                .expect("get")
                .expect("some")
                .last_channel,
            None
        );
        assert!(matches!(
            store.set_save_history(999, false),
            Err(Error::UnknownServer(999))
        ));
    }

    #[test]
    fn default_avatar_flag_starts_unset_and_sticks() {
        let (_dir, store) = temp_store();
        let a = store
            .add_server(&NewServer::new("A", "a.example", "Tester"))
            .expect("add");
        let b = store
            .add_server(&NewServer::new("B", "b.example", "Tester"))
            .expect("add");
        assert!(!store.default_avatar_sent(a.id).expect("flag"));
        store.set_default_avatar_sent(a.id).expect("set");
        store.set_default_avatar_sent(a.id).expect("set twice");
        let mut edited = store.server(a.id).expect("load").expect("exists");
        edited.label = "A2".into();
        store.update_server(&edited).expect("update");
        assert!(store.default_avatar_sent(a.id).expect("flag"));
        assert!(!store.default_avatar_sent(b.id).expect("flag"));
        assert!(matches!(
            store.default_avatar_sent(999),
            Err(Error::UnknownServer(999))
        ));
        assert!(matches!(
            store.set_default_avatar_sent(999),
            Err(Error::UnknownServer(999))
        ));
    }

    #[test]
    fn server_avatar_is_remembered_per_server_and_marks_the_default_handled() {
        let (_dir, store) = temp_store();
        let a = store
            .add_server(&NewServer::new("A", "a.example", "Tester"))
            .expect("add");
        let b = store
            .add_server(&NewServer::new("B", "b.example", "Tester"))
            .expect("add");
        assert_eq!(store.server_avatar(a.id).expect("avatar"), None);

        store.set_server_avatar(a.id, Some(b"png")).expect("set");
        assert_eq!(
            store.server_avatar(a.id).expect("avatar").as_deref(),
            Some(&b"png"[..])
        );
        assert!(store.default_avatar_sent(a.id).expect("flag"));
        let mut edited = store.server(a.id).expect("load").expect("exists");
        edited.label = "A2".into();
        store.update_server(&edited).expect("update");
        assert!(store.server_avatar(a.id).expect("avatar").is_some());
        assert_eq!(store.server_avatar(b.id).expect("avatar"), None);
        assert!(!store.default_avatar_sent(b.id).expect("flag"));

        // Removed by hand: "no avatar" is remembered and the default stays handled.
        store.set_server_avatar(a.id, None).expect("clear");
        assert_eq!(store.server_avatar(a.id).expect("avatar"), None);
        assert!(store.default_avatar_sent(a.id).expect("flag"));

        assert!(matches!(
            store.server_avatar(999),
            Err(Error::UnknownServer(999))
        ));
        assert!(matches!(
            store.set_server_avatar(999, None),
            Err(Error::UnknownServer(999))
        ));
    }

    #[test]
    fn port_zero_is_rejected() {
        let (_dir, store) = temp_store();
        let mut server = NewServer::new("A", "a", "u");
        server.port = 0;
        assert!(matches!(store.add_server(&server), Err(Error::InvalidPort)));
    }

    #[test]
    fn new_servers_append_and_reorder_works() {
        let (_dir, store) = temp_store();
        let a = store
            .add_server(&NewServer::new("A", "a", "u"))
            .expect("a")
            .id;
        let b = store
            .add_server(&NewServer::new("B", "b", "u"))
            .expect("b")
            .id;
        let c = store
            .add_server(&NewServer::new("C", "c", "u"))
            .expect("c")
            .id;
        let order = |s: &Store| -> Vec<ServerId> {
            s.servers().expect("servers").iter().map(|s| s.id).collect()
        };
        assert_eq!(order(&store), vec![a, b, c]);
        store.reorder_servers(&[c, a]).expect("reorder");
        assert_eq!(order(&store), vec![c, a, b]);
        assert!(matches!(
            store.reorder_servers(&[42]),
            Err(Error::UnknownServer(42))
        ));
    }

    #[test]
    fn removing_a_server_cascades_and_deletes_password() {
        let (_dir, store) = temp_store();
        let secrets = SecretStore::in_memory();
        let id = store
            .add_server(&NewServer::new("A", "a", "u"))
            .expect("add")
            .id;
        secrets.set_server_password(id, "pw").expect("pw");
        secrets
            .set_access_tokens(id, &["secret".to_owned()])
            .expect("tokens");
        store.set_trusted_cert(id, "ab:cd").expect("trust");
        store.follow_channel(id, "Root/Games", 1).expect("follow");
        store
            .set_person_setting(
                id,
                "hash",
                &crate::PersonSetting {
                    volume: 50,
                    local_mute: true,
                },
            )
            .expect("person");
        store
            .append_message(
                id,
                &crate::NewChatMessage {
                    conversation: crate::Conversation::Channel("Root".into()),
                    sender_name: "x".into(),
                    sender_cert_hash: None,
                    sent_at: 1,
                    body: "hi".into(),
                },
            )
            .expect("msg");

        store.remove_server(id, &secrets).expect("remove");
        assert_eq!(secrets.server_password(id).expect("read"), None);
        assert!(secrets.access_tokens(id).expect("tokens").is_empty());
        for table in [
            "trusted_cert",
            "followed_channel",
            "person_setting",
            "chat_message",
        ] {
            let count: i64 = store
                .conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .expect("count");
            assert_eq!(count, 0, "{table} not cascaded");
        }
    }
}
