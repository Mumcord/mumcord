use rusqlite::{Row, params};

use crate::db::{Store, UnixMillis};
use crate::error::Result;
use crate::servers::ServerId;

/// A chat conversation on one server.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Conversation {
    /// A channel (joined or followed), identified by its channel path.
    Channel(String),
    /// A private chat, identified by the other person's certificate hash.
    Private(String),
}

impl Conversation {
    fn parts(&self) -> (&'static str, &str) {
        match self {
            Self::Channel(path) => ("channel", path),
            Self::Private(cert_hash) => ("private", cert_hash),
        }
    }

    fn from_parts(kind: &str, key: String) -> rusqlite::Result<Self> {
        match kind {
            "channel" => Ok(Self::Channel(key)),
            "private" => Ok(Self::Private(key)),
            other => Err(rusqlite::Error::InvalidColumnType(
                0,
                format!("conversation_kind = {other}"),
                rusqlite::types::Type::Text,
            )),
        }
    }
}

/// A message to store. `body` must already be sanitized HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewChatMessage {
    pub conversation: Conversation,
    pub sender_name: String,
    /// Missing for users without a certificate.
    pub sender_cert_hash: Option<String>,
    /// Local receive (or send) time.
    pub sent_at: UnixMillis,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub id: i64,
    pub conversation: Conversation,
    pub sender_name: String,
    pub sender_cert_hash: Option<String>,
    pub sent_at: UnixMillis,
    pub body: String,
}

fn message_from_row(row: &Row<'_>) -> rusqlite::Result<ChatMessage> {
    Ok(ChatMessage {
        id: row.get(0)?,
        conversation: Conversation::from_parts(&row.get::<_, String>(1)?, row.get(2)?)?,
        sender_name: row.get(3)?,
        sender_cert_hash: row.get(4)?,
        sent_at: row.get(5)?,
        body: row.get(6)?,
    })
}

impl Store {
    /// Stores a message unless the server's "Save history" switch is off (or the
    /// server is unknown). Returns whether the message was stored.
    pub fn append_message(&self, server: ServerId, message: &NewChatMessage) -> Result<bool> {
        Ok(self.insert_message(server, message)?.is_some())
    }

    /// Like [`Store::append_message`], but returns the new message id (`None` when the
    /// message was not stored).
    pub fn insert_message(
        &self,
        server: ServerId,
        message: &NewChatMessage,
    ) -> Result<Option<i64>> {
        let (kind, key) = message.conversation.parts();
        // The save-history check is part of the INSERT so it cannot race a toggle.
        let inserted = self.conn.execute(
            "INSERT INTO chat_message
                 (server_id, conversation_kind, conversation_key, sender_name, sender_cert_hash,
                  sent_at, body)
             SELECT id, ?2, ?3, ?4, ?5, ?6, ?7 FROM saved_server
             WHERE id = ?1 AND save_history = 1",
            params![
                server,
                kind,
                key,
                message.sender_name,
                message.sender_cert_hash,
                message.sent_at,
                message.body
            ],
        )?;
        Ok((inserted == 1).then(|| self.conn.last_insert_rowid()))
    }

    /// The newest `limit` messages of a conversation, oldest first.
    pub fn messages(
        &self,
        server: ServerId,
        conversation: &Conversation,
        limit: u32,
    ) -> Result<Vec<ChatMessage>> {
        let (kind, key) = conversation.parts();
        let mut stmt = self.conn.prepare(
            "SELECT * FROM (
                 SELECT id, conversation_kind, conversation_key, sender_name, sender_cert_hash,
                        sent_at, body
                 FROM chat_message
                 WHERE server_id = ?1 AND conversation_kind = ?2 AND conversation_key = ?3
                 ORDER BY sent_at DESC, id DESC
                 LIMIT ?4)
             ORDER BY sent_at, id",
        )?;
        let rows = stmt.query_map(params![server, kind, key, limit], message_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Number of stored messages in a conversation newer than `since` (unread badges).
    /// Up to `limit` messages of a conversation older than message `before` (by send time,
    /// then id), oldest first. Used to page backwards through history.
    pub fn messages_before(
        &self,
        server: ServerId,
        conversation: &Conversation,
        before: i64,
        limit: u32,
    ) -> Result<Vec<ChatMessage>> {
        let (kind, key) = conversation.parts();
        let mut stmt = self.conn.prepare(
            "SELECT * FROM (
                 SELECT m.id, m.conversation_kind, m.conversation_key, m.sender_name,
                        m.sender_cert_hash, m.sent_at, m.body
                 FROM chat_message m, chat_message c
                 WHERE c.id = ?4 AND m.server_id = ?1 AND m.conversation_kind = ?2
                   AND m.conversation_key = ?3
                   AND (m.sent_at < c.sent_at OR (m.sent_at = c.sent_at AND m.id < c.id))
                 ORDER BY m.sent_at DESC, m.id DESC
                 LIMIT ?5)
             ORDER BY sent_at, id",
        )?;
        let rows = stmt.query_map(params![server, kind, key, before, limit], message_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn count_messages_since(
        &self,
        server: ServerId,
        conversation: &Conversation,
        since: UnixMillis,
    ) -> Result<u64> {
        let (kind, key) = conversation.parts();
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM chat_message
             WHERE server_id = ?1 AND conversation_kind = ?2 AND conversation_key = ?3
               AND sent_at > ?4",
            params![server, kind, key, since],
            |row| row.get(0),
        )?;
        Ok(count.unsigned_abs())
    }

    /// "Clear history" for one channel or private chat.
    pub fn clear_conversation_history(
        &self,
        server: ServerId,
        conversation: &Conversation,
    ) -> Result<()> {
        let (kind, key) = conversation.parts();
        self.conn.execute(
            "DELETE FROM chat_message
             WHERE server_id = ?1 AND conversation_kind = ?2 AND conversation_key = ?3",
            params![server, kind, key],
        )?;
        Ok(())
    }

    /// "Clear history" for a whole server.
    pub fn clear_server_history(&self, server: ServerId) -> Result<()> {
        self.conn
            .execute("DELETE FROM chat_message WHERE server_id = ?1", [server])?;
        Ok(())
    }

    /// "Clear all history".
    pub fn clear_all_history(&self) -> Result<()> {
        self.conn.execute("DELETE FROM chat_message", [])?;
        // Deleted chat contents must not linger in free pages of the file.
        self.conn.execute_batch("VACUUM")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewServer;
    use crate::db::tests::temp_store;

    fn msg(conversation: &Conversation, sent_at: UnixMillis, body: &str) -> NewChatMessage {
        NewChatMessage {
            conversation: conversation.clone(),
            sender_name: "Alice".into(),
            sender_cert_hash: Some("aa".into()),
            sent_at,
            body: body.into(),
        }
    }

    #[test]
    fn messages_are_returned_newest_window_in_order() {
        let (_dir, store) = temp_store();
        let id = store
            .add_server(&NewServer::new("A", "a", "u"))
            .expect("add")
            .id;
        let lobby = Conversation::Channel("Root/Lobby".into());
        let private = Conversation::Private("bb".into());
        for t in 1..=5 {
            assert!(
                store
                    .append_message(id, &msg(&lobby, t, &format!("m{t}")))
                    .expect("append")
            );
        }
        store
            .append_message(id, &msg(&private, 3, "secret"))
            .expect("append");

        let last3 = store.messages(id, &lobby, 3).expect("messages");
        let bodies: Vec<_> = last3.iter().map(|m| m.body.as_str()).collect();
        assert_eq!(bodies, ["m3", "m4", "m5"]);
        assert_eq!(last3[0].conversation, lobby);
        assert_eq!(last3[0].sender_cert_hash.as_deref(), Some("aa"));
        assert_eq!(store.messages(id, &private, 10).expect("private").len(), 1);
        assert_eq!(store.count_messages_since(id, &lobby, 2).expect("count"), 3);
    }

    #[test]
    fn paging_backwards_from_a_message() {
        let (_dir, store) = temp_store();
        let id = store
            .add_server(&NewServer::new("A", "a", "u"))
            .expect("add")
            .id;
        let lobby = Conversation::Channel("Lobby".into());
        let ids: Vec<i64> = (1..=5)
            .map(|t| {
                store
                    .insert_message(id, &msg(&lobby, t, &format!("m{t}")))
                    .expect("insert")
                    .expect("stored")
            })
            .collect();
        let page = store.messages_before(id, &lobby, ids[3], 2).expect("page");
        let bodies: Vec<_> = page.iter().map(|m| m.body.as_str()).collect();
        assert_eq!(bodies, ["m2", "m3"]);
        assert!(
            store
                .messages_before(id, &lobby, ids[0], 10)
                .expect("page")
                .is_empty()
        );
    }

    #[test]
    fn save_history_off_stores_nothing() {
        let (_dir, store) = temp_store();
        let id = store
            .add_server(&NewServer::new("A", "a", "u"))
            .expect("add")
            .id;
        let lobby = Conversation::Channel("Root".into());
        store.set_save_history(id, false).expect("toggle");
        assert!(
            !store
                .append_message(id, &msg(&lobby, 1, "x"))
                .expect("append")
        );
        assert!(store.messages(id, &lobby, 10).expect("messages").is_empty());
        store.set_save_history(id, true).expect("toggle");
        assert!(
            store
                .append_message(id, &msg(&lobby, 2, "y"))
                .expect("append")
        );
        assert!(
            !store
                .append_message(9999, &msg(&lobby, 3, "z"))
                .expect("unknown server")
        );
    }

    #[test]
    fn clearing_per_conversation_server_and_all() {
        let (_dir, store) = temp_store();
        let a = store
            .add_server(&NewServer::new("A", "a", "u"))
            .expect("a")
            .id;
        let b = store
            .add_server(&NewServer::new("B", "b", "u"))
            .expect("b")
            .id;
        let lobby = Conversation::Channel("Root/Lobby".into());
        let games = Conversation::Channel("Root/Games".into());
        for server in [a, b] {
            for conv in [&lobby, &games] {
                store
                    .append_message(server, &msg(conv, 1, "x"))
                    .expect("append");
            }
        }
        let count = |s: &Store| -> i64 {
            s.conn
                .query_row("SELECT COUNT(*) FROM chat_message", [], |r| r.get(0))
                .expect("count")
        };

        store
            .clear_conversation_history(a, &lobby)
            .expect("clear conv");
        assert!(store.messages(a, &lobby, 10).expect("m").is_empty());
        assert_eq!(store.messages(a, &games, 10).expect("m").len(), 1);
        assert_eq!(store.messages(b, &lobby, 10).expect("m").len(), 1);
        assert_eq!(count(&store), 3);

        store.clear_server_history(b).expect("clear server");
        assert_eq!(count(&store), 1);

        store.clear_all_history().expect("clear all");
        assert_eq!(count(&store), 0);
    }
}
