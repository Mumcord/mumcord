//! Store side of the history sync between Mumcord clients (FR-1.60, QA.48): time-range
//! queries for summaries and hash lists, and inserting messages that came from another
//! person's history together with that person's name.

use std::collections::HashMap;

use rusqlite::params;

use crate::db::{Store, UnixMillis};
use crate::error::Result;
use crate::history::{ChatMessage, Conversation, NewChatMessage, message_from_row};
use crate::servers::ServerId;

/// Count and time span of the stored messages of a conversation in a time range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeSummary {
    pub count: u64,
    /// `sent_at` of the oldest and newest message; `None` when the range is empty.
    pub oldest: Option<UnixMillis>,
    pub newest: Option<UnixMillis>,
}

impl Store {
    /// Stores a message from another person's history, remembering whose (`holder`). Like
    /// [`Store::insert_message`], nothing is stored while the server's "Save history" switch
    /// is off; returns the new id, or `None` when not stored.
    pub fn insert_synced_message(
        &self,
        server: ServerId,
        message: &NewChatMessage,
        holder: &str,
    ) -> Result<Option<i64>> {
        let (kind, key) = message.conversation.parts();
        let inserted = self.conn.execute(
            "INSERT INTO chat_message
                 (server_id, conversation_kind, conversation_key, sender_name, sender_cert_hash,
                  sent_at, body, synced_from)
             SELECT id, ?2, ?3, ?4, ?5, ?6, ?7, ?8 FROM saved_server
             WHERE id = ?1 AND save_history = 1",
            params![
                server,
                kind,
                key,
                message.sender_name,
                message.sender_cert_hash,
                message.sent_at,
                message.body,
                holder
            ],
        )?;
        Ok((inserted == 1).then(|| self.conn.last_insert_rowid()))
    }

    /// The newest `limit` messages of a conversation sent in `[from, to)`, oldest first.
    pub fn messages_between(
        &self,
        server: ServerId,
        conversation: &Conversation,
        from: UnixMillis,
        to: UnixMillis,
        limit: u32,
    ) -> Result<Vec<ChatMessage>> {
        let (kind, key) = conversation.parts();
        let mut stmt = self.conn.prepare(
            "SELECT * FROM (
                 SELECT id, conversation_kind, conversation_key, sender_name, sender_cert_hash,
                        sent_at, body
                 FROM chat_message
                 WHERE server_id = ?1 AND conversation_kind = ?2 AND conversation_key = ?3
                   AND sent_at >= ?4 AND sent_at < ?5
                 ORDER BY sent_at DESC, id DESC
                 LIMIT ?6)
             ORDER BY sent_at, id",
        )?;
        let rows = stmt.query_map(
            params![server, kind, key, from, to, limit],
            message_from_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Count and time span of a conversation's messages sent in `[from, to)`.
    pub fn range_summary(
        &self,
        server: ServerId,
        conversation: &Conversation,
        from: UnixMillis,
        to: UnixMillis,
    ) -> Result<RangeSummary> {
        let (kind, key) = conversation.parts();
        let (count, oldest, newest): (i64, Option<i64>, Option<i64>) = self.conn.query_row(
            "SELECT COUNT(*), MIN(sent_at), MAX(sent_at) FROM chat_message
             WHERE server_id = ?1 AND conversation_kind = ?2 AND conversation_key = ?3
               AND sent_at >= ?4 AND sent_at < ?5",
            params![server, kind, key, from, to],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok(RangeSummary {
            count: count.unsigned_abs(),
            oldest,
            newest,
        })
    }

    /// Whose history the given messages came from; messages received live are absent.
    pub fn synced_origins(&self, message_ids: &[i64]) -> Result<HashMap<i64, String>> {
        let mut stmt = self.conn.prepare(
            "SELECT synced_from FROM chat_message WHERE id = ?1 AND synced_from IS NOT NULL",
        )?;
        let mut found = HashMap::new();
        for &id in message_ids {
            let mut rows = stmt.query_map([id], |row| row.get::<_, String>(0))?;
            if let Some(holder) = rows.next().transpose()? {
                found.insert(id, holder);
            }
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewServer;
    use crate::db::tests::temp_store;

    fn msg(sent_at: UnixMillis, body: &str) -> NewChatMessage {
        NewChatMessage {
            conversation: Conversation::Channel("Lobby".into()),
            sender_name: "Alice".into(),
            sender_cert_hash: Some("aa".into()),
            sent_at,
            body: body.into(),
        }
    }

    fn server(store: &Store) -> ServerId {
        store
            .add_server(&NewServer {
                label: "Test".into(),
                address: "localhost".into(),
                port: 64738,
                username: "Tester".into(),
                save_history: true,
            })
            .expect("server")
            .id
    }

    #[test]
    fn range_queries_cover_half_open_ranges_newest_first_window() {
        let (_dir, store) = temp_store();
        let id = server(&store);
        for t in [10, 20, 30, 40] {
            store.append_message(id, &msg(t, "x")).expect("append");
        }
        let lobby = Conversation::Channel("Lobby".into());
        let window = store
            .messages_between(id, &lobby, 10, 40, 2)
            .expect("between");
        let times: Vec<_> = window.iter().map(|m| m.sent_at).collect();
        assert_eq!(times, [20, 30], "newest two before 40, oldest first");
        assert_eq!(
            store.range_summary(id, &lobby, 10, 40).expect("summary"),
            RangeSummary {
                count: 3,
                oldest: Some(10),
                newest: Some(30)
            }
        );
        let empty = store.range_summary(id, &lobby, 50, 60).expect("empty");
        assert_eq!((empty.count, empty.oldest), (0, None));
        let private = Conversation::Private("aa".into());
        assert!(
            store
                .messages_between(id, &private, 0, 100, 10)
                .expect("private")
                .is_empty()
        );
    }

    #[test]
    fn synced_messages_remember_the_holder_and_respect_save_history() {
        let (_dir, store) = temp_store();
        let id = server(&store);
        let live = store.insert_message(id, &msg(5, "live")).expect("live");
        let synced = store
            .insert_synced_message(id, &msg(1, "old"), "Bob")
            .expect("synced")
            .expect("stored");
        let origins = store
            .synced_origins(&[live.expect("stored"), synced])
            .expect("origins");
        assert_eq!(origins.len(), 1);
        assert_eq!(origins[&synced], "Bob");
        let lobby = Conversation::Channel("Lobby".into());
        let all = store.messages(id, &lobby, 10).expect("all");
        assert_eq!(all[0].body, "old", "ordered by send time");

        store.set_save_history(id, false).expect("off");
        assert_eq!(
            store
                .insert_synced_message(id, &msg(2, "skipped"), "Bob")
                .expect("insert"),
            None
        );
    }
}
