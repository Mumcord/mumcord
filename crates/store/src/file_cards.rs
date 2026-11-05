//! The end state of a file download, kept with the chat message that carried the offer (spec 07,
//! section 7.2), so the card still shows "Downloaded" or the mismatch after a restart. Only
//! finished downloads are stored; every other state is worked out again from the live session.

use std::collections::HashMap;

use rusqlite::params;

use crate::db::Store;
use crate::error::Result;

/// How a download of a file offer ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileCardRecord {
    /// Downloaded and checked; `path` is where the file was saved.
    Done { path: String },
    /// The file didn't match the offer and was deleted.
    Mismatch,
}

impl Store {
    /// Saves how the download of the offer in message `message_id` ended. Unknown messages are
    /// ignored; a later download replaces the earlier result.
    pub fn save_file_card(&self, message_id: i64, record: &FileCardRecord) -> Result<()> {
        let (state, path) = match record {
            FileCardRecord::Done { path } => ("done", Some(path.as_str())),
            FileCardRecord::Mismatch => ("mismatch", None),
        };
        self.conn.execute(
            // An upsert, not `INSERT OR REPLACE`: replacing deletes the row and would lose
            // columns a newer Mumcord added (issue #521).
            "INSERT INTO file_card (message_id, state, path)
             SELECT id, ?2, ?3 FROM chat_message WHERE id = ?1
             ON CONFLICT (message_id) DO UPDATE SET state = excluded.state, path = excluded.path",
            params![message_id, state, path],
        )?;
        Ok(())
    }

    /// The saved download results of the given messages (messages without one are absent).
    pub fn file_cards(&self, message_ids: &[i64]) -> Result<HashMap<i64, FileCardRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT state, path FROM file_card WHERE message_id = ?1")?;
        let mut found = HashMap::new();
        for &id in message_ids {
            let mut rows = stmt.query([id])?;
            if let Some(row) = rows.next()? {
                let state: String = row.get(0)?;
                let path: Option<String> = row.get(1)?;
                let record = match (state.as_str(), path) {
                    ("done", Some(path)) => FileCardRecord::Done { path },
                    _ => FileCardRecord::Mismatch,
                };
                found.insert(id, record);
            }
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_store;
    use crate::{Conversation, NewChatMessage, NewServer};

    fn message(store: &Store) -> i64 {
        let server = store
            .add_server(&NewServer::new("A", "a", "u"))
            .expect("add")
            .id;
        store
            .insert_message(
                server,
                &NewChatMessage {
                    conversation: Conversation::Channel("Root".into()),
                    sender_name: "Alice".into(),
                    sender_cert_hash: Some("aa".into()),
                    sent_at: 1,
                    body: "offer".into(),
                },
            )
            .expect("insert")
            .expect("stored")
    }

    #[test]
    fn download_results_round_trip_and_replace() {
        let (_dir, store) = temp_store();
        let id = message(&store);
        assert!(store.file_cards(&[id]).expect("read").is_empty());
        let done = FileCardRecord::Done {
            path: "/home/a/Downloads/x.txt".into(),
        };
        store.save_file_card(id, &done).expect("save");
        assert_eq!(store.file_cards(&[id, 999]).expect("read")[&id], done);
        store
            .save_file_card(id, &FileCardRecord::Mismatch)
            .expect("save");
        assert_eq!(
            store.file_cards(&[id]).expect("read")[&id],
            FileCardRecord::Mismatch
        );
    }

    #[test]
    fn replacing_a_result_keeps_columns_from_a_newer_app() {
        let (_dir, store) = temp_store();
        let id = message(&store);
        store
            .save_file_card(id, &FileCardRecord::Mismatch)
            .expect("save");
        store
            .conn
            .execute_batch("ALTER TABLE file_card ADD COLUMN future TEXT; UPDATE file_card SET future = 'kept';")
            .expect("newer column");
        let done = FileCardRecord::Done { path: "/x".into() };
        store.save_file_card(id, &done).expect("save");
        let future: String = store
            .conn
            .query_row("SELECT future FROM file_card", [], |row| row.get(0))
            .expect("future");
        assert_eq!(future, "kept");
        assert_eq!(store.file_cards(&[id]).expect("read")[&id], done);
    }

    #[test]
    fn results_go_away_with_their_message_and_unknown_messages_are_ignored() {
        let (_dir, store) = temp_store();
        store
            .save_file_card(42, &FileCardRecord::Mismatch)
            .expect("ignored");
        assert!(store.file_cards(&[42]).expect("read").is_empty());
        let id = message(&store);
        store
            .save_file_card(id, &FileCardRecord::Mismatch)
            .expect("save");
        store
            .conn
            .execute("DELETE FROM chat_message WHERE id = ?1", [id])
            .expect("delete");
        assert!(store.file_cards(&[id]).expect("read").is_empty());
    }
}
