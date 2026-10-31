//! Blocked users (issue #140): people whose private messages, voice and Mumcord plugin data
//! the user no longer wants. Kept per own identity (FR-3.79). A block by certificate hash
//! covers every server of that identity (Q1.4); a name block, for people without a
//! certificate, only applies on the server where it was set.

use rusqlite::params;

use crate::db::{Store, UnixMillis};
use crate::error::Result;
use crate::servers::ServerId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedUser {
    pub id: i64,
    /// Certificate hash of the own identity that set the block.
    pub own_cert_hash: String,
    /// `None` for a name block.
    pub cert_hash: Option<String>,
    /// The name the person had when they were blocked.
    pub name: String,
    /// The server the block was set on; `None` once that server was removed.
    pub server_id: Option<ServerId>,
    /// Label of that server when the block was set.
    pub server_label: String,
    pub blocked_at: UnixMillis,
}

impl BlockedUser {
    /// Whether this block covers a person seen on `server` while connected as `own_cert_hash`.
    pub fn covers(
        &self,
        own_cert_hash: &str,
        server: ServerId,
        cert_hash: Option<&str>,
        name: &str,
    ) -> bool {
        if self.own_cert_hash != own_cert_hash {
            return false;
        }
        match &self.cert_hash {
            Some(blocked) => cert_hash == Some(blocked.as_str()),
            // Mumble servers treat user names case-insensitively.
            None => {
                self.server_id == Some(server) && self.name.to_lowercase() == name.to_lowercase()
            }
        }
    }
}

impl Store {
    /// Blocked users of every identity, by name.
    pub fn blocked_users(&self) -> Result<Vec<BlockedUser>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, own_cert_hash, cert_hash, name, server_id, server_label, blocked_at
             FROM blocked_user ORDER BY name COLLATE NOCASE, id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(BlockedUser {
                id: row.get(0)?,
                own_cert_hash: row.get(1)?,
                cert_hash: row.get(2)?,
                name: row.get(3)?,
                server_id: row.get(4)?,
                server_label: row.get(5)?,
                blocked_at: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Blocks a certificate for one own identity on every server. Blocking again keeps the
    /// first date and server and updates the name.
    pub fn block_cert(
        &self,
        own_cert_hash: &str,
        cert_hash: &str,
        name: &str,
        server: ServerId,
        server_label: &str,
        now: UnixMillis,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO blocked_user
                 (own_cert_hash, cert_hash, name, server_id, server_label, blocked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (own_cert_hash, cert_hash) WHERE cert_hash IS NOT NULL
             DO UPDATE SET name = excluded.name",
            params![own_cert_hash, cert_hash, name, server, server_label, now],
        )?;
        Ok(())
    }

    /// Blocks a name on one server for one own identity; blocking it again is a no-op.
    pub fn block_name(
        &self,
        own_cert_hash: &str,
        name: &str,
        server: ServerId,
        server_label: &str,
        now: UnixMillis,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO blocked_user
                 (own_cert_hash, cert_hash, name, server_id, server_label, blocked_at)
             VALUES (?1, NULL, ?2, ?3, ?4, ?5)
             ON CONFLICT (own_cert_hash, server_id, name COLLATE NOCASE) WHERE cert_hash IS NULL
             DO NOTHING",
            params![own_cert_hash, name, server, server_label, now],
        )?;
        Ok(())
    }

    /// Removes a block; an unknown id is a no-op.
    pub fn unblock(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM blocked_user WHERE id = ?1", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::NewServer;
    use crate::db::tests::temp_store;

    #[test]
    fn certificate_block_is_per_identity_and_covers_every_server() {
        let (_dir, store) = temp_store();
        let server = store
            .add_server(&NewServer::new("Home", "localhost", "me"))
            .expect("server")
            .id;
        store
            .block_cert("me", "bob", "Bob", server, "Home", 10)
            .expect("block");
        store
            .block_cert("me", "bob", "Bobby", server, "Home", 20)
            .expect("block again");
        let all = store.blocked_users().expect("list");
        assert_eq!(all.len(), 1);
        let block = &all[0];
        assert_eq!(
            (block.name.as_str(), block.blocked_at, block.server_id),
            ("Bobby", 10, Some(server))
        );
        assert!(block.covers("me", server, Some("bob"), "Renamed"));
        assert!(block.covers("me", server + 1, Some("bob"), "Bob"));
        assert!(!block.covers("alt", server, Some("bob"), "Bob"));
        assert!(!block.covers("me", server, None, "Bob"));
        assert!(!block.covers("me", server, Some("carol"), "Bob"));
    }

    #[test]
    fn name_block_only_applies_on_its_server() {
        let (_dir, store) = temp_store();
        let server = store
            .add_server(&NewServer::new("Home", "localhost", "me"))
            .expect("server")
            .id;
        store
            .block_name("me", "Guest", server, "Home", 10)
            .expect("block");
        store
            .block_name("me", "guest", server, "Home", 20)
            .expect("block again");
        let all = store.blocked_users().expect("list");
        assert_eq!(all.len(), 1);
        let block = &all[0];
        assert_eq!(block.cert_hash, None);
        assert!(block.covers("me", server, None, "GUEST"));
        assert!(!block.covers("me", server + 1, None, "Guest"));
        assert!(!block.covers("alt", server, None, "Guest"));

        // The block outlives the server, but no longer applies anywhere.
        store
            .conn
            .execute("DELETE FROM saved_server WHERE id = ?1", [server])
            .expect("remove server");
        let all = store.blocked_users().expect("list");
        assert_eq!(
            (all[0].server_id, all[0].server_label.as_str()),
            (None, "Home")
        );
        assert!(!all[0].covers("me", server, None, "Guest"));
    }

    #[test]
    fn unblock_removes_only_that_block() {
        let (_dir, store) = temp_store();
        let server = store
            .add_server(&NewServer::new("Home", "localhost", "me"))
            .expect("server")
            .id;
        store
            .block_cert("me", "bob", "Bob", server, "Home", 10)
            .expect("block");
        store
            .block_cert("alt", "bob", "Bob", server, "Home", 10)
            .expect("block");
        let id = store.blocked_users().expect("list")[0].id;
        store.unblock(id).expect("unblock");
        store.unblock(id).expect("unblock again");
        let left = store.blocked_users().expect("list");
        assert_eq!(left.len(), 1);
        assert_ne!(left[0].id, id);
    }
}
