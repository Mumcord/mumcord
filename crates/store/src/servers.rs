use rusqlite::{OptionalExtension, Row, params};

use crate::db::Store;
use crate::error::{Error, Result};
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
}

impl NewServer {
    /// Server with the default port and history saving on.
    pub fn new(label: &str, address: &str, username: &str) -> Self {
        Self {
            label: label.to_owned(),
            address: address.to_owned(),
            port: DEFAULT_PORT,
            username: username.to_owned(),
            save_history: true,
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
}

const SERVER_COLUMNS: &str =
    "id, label, address, port, username, last_channel, save_history, sort_order";

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
    })
}

impl Store {
    /// Adds a server at the end of the server bar.
    pub fn add_server(&self, server: &NewServer) -> Result<SavedServer> {
        if server.port == 0 {
            return Err(Error::InvalidPort);
        }
        self.conn.execute(
            "INSERT INTO saved_server (label, address, port, username, save_history, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5,
                     (SELECT COALESCE(MAX(sort_order), -1) + 1 FROM saved_server))",
            params![
                server.label,
                server.address,
                server.port,
                server.username,
                server.save_history
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

    /// Updates label, address, port, username, last channel and the save-history
    /// switch. The order is changed with [`Store::reorder_servers`].
    pub fn update_server(&self, server: &SavedServer) -> Result<()> {
        if server.port == 0 {
            return Err(Error::InvalidPort);
        }
        let changed = self.conn.execute(
            "UPDATE saved_server
             SET label = ?2, address = ?3, port = ?4, username = ?5, last_channel = ?6,
                 save_history = ?7
             WHERE id = ?1",
            params![
                server.id,
                server.label,
                server.address,
                server.port,
                server.username,
                server.last_channel,
                server.save_history
            ],
        )?;
        expect_one(changed, server.id)
    }

    pub fn set_last_channel(&self, id: ServerId, channel_path: Option<&str>) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE saved_server SET last_channel = ?2 WHERE id = ?1",
            params![id, channel_path],
        )?;
        expect_one(changed, id)
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
    /// person settings) and its password in secure storage.
    pub fn remove_server(&self, id: ServerId, secrets: &SecretStore) -> Result<()> {
        // Password first: if secure storage fails, the server stays and the user can retry
        // instead of leaving an orphaned secret behind.
        secrets.delete_server_password(id)?;
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
