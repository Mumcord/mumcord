//! Friends (FR-1.64): people the user marked as friends, kept locally by certificate hash so
//! they stay the same person across servers and renames. People without a certificate
//! cannot be friends.

use rusqlite::{OptionalExtension, params};

use crate::db::{Store, UnixMillis};
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Friend {
    pub cert_hash: String,
    /// The name the friend was last seen with.
    pub name: String,
    pub added_at: UnixMillis,
    /// When the friend was last seen online; `None` before they were seen after adding.
    pub last_seen_at: Option<UnixMillis>,
}

impl Store {
    /// All friends, by name.
    pub fn friends(&self) -> Result<Vec<Friend>> {
        let mut stmt = self.conn.prepare(
            "SELECT cert_hash, name, added_at, last_seen_at FROM friend
             ORDER BY name COLLATE NOCASE, cert_hash",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Friend {
                cert_hash: row.get(0)?,
                name: row.get(1)?,
                added_at: row.get(2)?,
                last_seen_at: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn is_friend(&self, cert_hash: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM friend WHERE cert_hash = ?1",
                [cert_hash],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Adds a friend; adding a known friend only updates the name.
    pub fn add_friend(&self, cert_hash: &str, name: &str, now: UnixMillis) -> Result<()> {
        self.conn.execute(
            "INSERT INTO friend (cert_hash, name, added_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (cert_hash) DO UPDATE SET name = excluded.name",
            params![cert_hash, name, now],
        )?;
        Ok(())
    }

    /// Removes a friend; an unknown hash is a no-op. Their private history stays until it is
    /// deleted like any other history.
    pub fn remove_friend(&self, cert_hash: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM friend WHERE cert_hash = ?1", [cert_hash])?;
        Ok(())
    }

    /// Records that the friend was seen online under `name` at `at`; unknown hashes are
    /// ignored.
    pub fn friend_seen(&self, cert_hash: &str, name: &str, at: UnixMillis) -> Result<()> {
        self.conn.execute(
            "UPDATE friend SET name = ?2, last_seen_at = ?3 WHERE cert_hash = ?1",
            params![cert_hash, name, at],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{Conversation, NewChatMessage, NewServer, Store};

    fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path()).expect("open");
        (dir, store)
    }

    fn private(hash: &str, at: i64) -> NewChatMessage {
        NewChatMessage {
            conversation: Conversation::Private(hash.into()),
            sender_name: "Bot".into(),
            sender_cert_hash: Some(hash.into()),
            sent_at: at,
            body: "hi".into(),
        }
    }

    #[test]
    fn add_rename_seen_and_remove() {
        let (_dir, store) = temp_store();
        assert!(store.friends().expect("empty").is_empty());
        store.add_friend("h1", "Bot", 10).expect("add");
        store.add_friend("h1", "Bot2", 20).expect("re-add");
        let friends = store.friends().expect("list");
        assert_eq!(friends.len(), 1);
        assert_eq!(friends[0].name, "Bot2");
        assert_eq!(friends[0].added_at, 10);
        assert_eq!(friends[0].last_seen_at, None);
        assert!(store.is_friend("h1").expect("is"));

        store.friend_seen("h1", "Robot", 30).expect("seen");
        store
            .friend_seen("unknown", "X", 30)
            .expect("unknown ignored");
        let friends = store.friends().expect("list");
        assert_eq!(friends.len(), 1);
        assert_eq!(friends[0].name, "Robot");
        assert_eq!(friends[0].last_seen_at, Some(30));

        store.remove_friend("h1").expect("remove");
        assert!(!store.is_friend("h1").expect("is"));
        store.remove_friend("h1").expect("remove again");
    }

    #[test]
    fn automatic_pruning_keeps_private_history_with_friends() {
        let (_dir, store) = temp_store();
        let server = store
            .add_server(&NewServer::new("A", "a", "u"))
            .expect("server")
            .id;
        store.add_friend("friend", "Bot", 1).expect("add");
        store
            .insert_message(server, &private("friend", 5))
            .expect("insert");
        store
            .insert_message(server, &private("stranger", 5))
            .expect("insert");
        assert_eq!(store.delete_messages_before(100).expect("prune"), 1);
        let kept = Conversation::Private("friend".into());
        assert_eq!(store.messages(server, &kept, 10).expect("kept").len(), 1);

        // Manual deletion still works.
        store
            .clear_conversation_history(server, &kept)
            .expect("clear");
        assert!(store.messages(server, &kept, 10).expect("gone").is_empty());
    }

    #[test]
    fn private_history_with_a_friend_spans_servers() {
        let (_dir, store) = temp_store();
        let a = store
            .add_server(&NewServer::new("A", "a", "u"))
            .expect("a")
            .id;
        let b = store
            .add_server(&NewServer::new("B", "b", "u"))
            .expect("b")
            .id;
        let chat = Conversation::Private("h".into());
        store.insert_message(a, &private("h", 1)).expect("a1");
        let last = store
            .insert_message(b, &private("h", 2))
            .expect("b1")
            .expect("stored");
        assert_eq!(store.messages(b, &chat, 10).expect("b only").len(), 1);

        store.add_friend("h", "Bot", 3).expect("add");
        let both = store.messages(b, &chat, 10).expect("both");
        assert_eq!(both.iter().map(|m| m.sent_at).collect::<Vec<_>>(), [1, 2]);
        let older = store.messages_before(b, &chat, last, 10).expect("older");
        assert_eq!(older.len(), 1);
        assert_eq!(older[0].sent_at, 1);
    }
}
