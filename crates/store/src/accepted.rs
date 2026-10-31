//! Accepted private conversations (issue #141): people who may keep sending private messages
//! while the "Private messages from" rule would filter them. Kept per own identity (by the own
//! certificate hash), because that is who the other side is talking to.

use rusqlite::{OptionalExtension, params};

use crate::db::{Store, UnixMillis};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedConversation {
    /// Certificate hash of the own identity the conversation was accepted with.
    pub own_cert_hash: String,
    pub cert_hash: String,
    /// The name the person had when the conversation was accepted.
    pub name: String,
    pub accepted_at: UnixMillis,
}

impl Store {
    /// All accepted conversations of every identity, by name.
    pub fn accepted_conversations(&self) -> Result<Vec<AcceptedConversation>> {
        let mut stmt = self.conn.prepare(
            "SELECT own_cert_hash, cert_hash, name, accepted_at FROM accepted_conversation
             ORDER BY name COLLATE NOCASE, cert_hash",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(AcceptedConversation {
                own_cert_hash: row.get(0)?,
                cert_hash: row.get(1)?,
                name: row.get(2)?,
                accepted_at: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn is_conversation_accepted(&self, own_cert_hash: &str, cert_hash: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM accepted_conversation WHERE own_cert_hash = ?1 AND cert_hash = ?2",
                [own_cert_hash, cert_hash],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Accepts a conversation; accepting it again keeps the first date and updates the name.
    pub fn accept_conversation(
        &self,
        own_cert_hash: &str,
        cert_hash: &str,
        name: &str,
        now: UnixMillis,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO accepted_conversation (own_cert_hash, cert_hash, name, accepted_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (own_cert_hash, cert_hash) DO UPDATE SET name = excluded.name",
            params![own_cert_hash, cert_hash, name, now],
        )?;
        Ok(())
    }

    /// Revokes an accepted conversation; an unknown pair is a no-op.
    pub fn revoke_conversation(&self, own_cert_hash: &str, cert_hash: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM accepted_conversation WHERE own_cert_hash = ?1 AND cert_hash = ?2",
            [own_cert_hash, cert_hash],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::db::tests::temp_store;

    #[test]
    fn accept_is_per_identity_and_revocable() {
        let (_dir, store) = temp_store();
        store
            .accept_conversation("me", "bob", "Bob", 10)
            .expect("accept");
        store
            .accept_conversation("me", "bob", "Bobby", 20)
            .expect("accept again");
        assert!(store.is_conversation_accepted("me", "bob").expect("query"));
        assert!(!store.is_conversation_accepted("alt", "bob").expect("query"));
        let all = store.accepted_conversations().expect("list");
        assert_eq!(all.len(), 1);
        assert_eq!((all[0].name.as_str(), all[0].accepted_at), ("Bobby", 10));
        store.revoke_conversation("me", "bob").expect("revoke");
        assert!(!store.is_conversation_accepted("me", "bob").expect("query"));
    }
}
