//! The private messages overview (issue #173): pinned and muted private conversations, and the
//! newest stored message of each private conversation for the list's preview.

use rusqlite::params;

use crate::db::{Store, UnixMillis};
use crate::error::Result;
use crate::history::{ChatMessage, message_from_row};
use crate::servers::ServerId;

/// `muted_until` of a conversation muted "until I turn it back on".
pub const MUTED_FOREVER: UnixMillis = UnixMillis::MAX;

/// Pin and mute state of one private conversation on one server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PmConversationPrefs {
    pub server_id: ServerId,
    /// Certificate hash of the other person.
    pub cert_hash: String,
    pub pinned: bool,
    /// Muted until this time; [`MUTED_FOREVER`] until turned back on, `None` when not muted.
    pub muted_until: Option<UnixMillis>,
}

/// The newest stored message of a private conversation and the name the other person last wrote
/// under (`None` when only own messages are stored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateConversationSummary {
    pub last: ChatMessage,
    pub peer_name: Option<String>,
}

impl Store {
    /// Pin and mute state of every private conversation that has any.
    pub fn pm_conversation_prefs(&self) -> Result<Vec<PmConversationPrefs>> {
        let mut stmt = self.conn.prepare(
            "SELECT server_id, cert_hash, pinned, muted_until FROM pm_conversation
             ORDER BY server_id, cert_hash",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PmConversationPrefs {
                server_id: row.get(0)?,
                cert_hash: row.get(1)?,
                pinned: row.get(2)?,
                muted_until: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Pins or unpins a private conversation.
    pub fn set_pm_conversation_pinned(
        &self,
        server: ServerId,
        cert_hash: &str,
        pinned: bool,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO pm_conversation (server_id, cert_hash, pinned) VALUES (?1, ?2, ?3)
             ON CONFLICT (server_id, cert_hash) DO UPDATE SET pinned = excluded.pinned",
            params![server, cert_hash, pinned],
        )?;
        self.drop_default_pm_conversation(server, cert_hash)
    }

    /// Mutes a private conversation until `until` ([`MUTED_FOREVER`] for good), or unmutes it.
    pub fn set_pm_conversation_muted(
        &self,
        server: ServerId,
        cert_hash: &str,
        until: Option<UnixMillis>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO pm_conversation (server_id, cert_hash, muted_until) VALUES (?1, ?2, ?3)
             ON CONFLICT (server_id, cert_hash) DO UPDATE SET muted_until = excluded.muted_until",
            params![server, cert_hash, until],
        )?;
        self.drop_default_pm_conversation(server, cert_hash)
    }

    /// A conversation that is neither pinned nor muted needs no row.
    fn drop_default_pm_conversation(&self, server: ServerId, cert_hash: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM pm_conversation
             WHERE server_id = ?1 AND cert_hash = ?2 AND pinned = 0 AND muted_until IS NULL",
            params![server, cert_hash],
        )?;
        Ok(())
    }

    /// The newest stored message of every private conversation on `server`, newest first.
    pub fn private_conversation_summaries(
        &self,
        server: ServerId,
    ) -> Result<Vec<PrivateConversationSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, m.conversation_kind, m.conversation_key, m.sender_name,
                    m.sender_cert_hash, m.sent_at, m.body,
                    (SELECT p.sender_name FROM chat_message p
                     WHERE p.server_id = m.server_id AND p.conversation_kind = 'private'
                       AND p.conversation_key = m.conversation_key
                       AND p.sender_cert_hash = m.conversation_key
                     ORDER BY p.sent_at DESC, p.id DESC LIMIT 1)
             FROM chat_message m
             WHERE m.server_id = ?1 AND m.conversation_kind = 'private'
               AND m.id = (SELECT n.id FROM chat_message n
                           WHERE n.server_id = m.server_id AND n.conversation_kind = 'private'
                             AND n.conversation_key = m.conversation_key
                           ORDER BY n.sent_at DESC, n.id DESC LIMIT 1)
             ORDER BY m.sent_at DESC, m.id DESC",
        )?;
        let rows = stmt.query_map([server], |row| {
            Ok(PrivateConversationSummary {
                last: message_from_row(row)?,
                peer_name: row.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_store;
    use crate::{Conversation, NewChatMessage, NewServer};

    fn server(store: &Store) -> ServerId {
        store
            .add_server(&NewServer::new("A", "a", "u"))
            .expect("add")
            .id
    }

    #[test]
    fn pin_and_mute_round_trip_and_reset_drops_the_row() {
        let (_dir, store) = temp_store();
        let id = server(&store);
        store
            .set_pm_conversation_pinned(id, "bob", true)
            .expect("pin");
        store
            .set_pm_conversation_muted(id, "bob", Some(MUTED_FOREVER))
            .expect("mute");
        store
            .set_pm_conversation_muted(id, "eve", Some(5_000))
            .expect("mute eve");
        assert_eq!(
            store.pm_conversation_prefs().expect("list"),
            vec![
                PmConversationPrefs {
                    server_id: id,
                    cert_hash: "bob".into(),
                    pinned: true,
                    muted_until: Some(MUTED_FOREVER),
                },
                PmConversationPrefs {
                    server_id: id,
                    cert_hash: "eve".into(),
                    pinned: false,
                    muted_until: Some(5_000),
                },
            ]
        );
        store
            .set_pm_conversation_pinned(id, "bob", false)
            .expect("unpin");
        store
            .set_pm_conversation_muted(id, "bob", None)
            .expect("unmute");
        store
            .set_pm_conversation_muted(id, "eve", None)
            .expect("unmute eve");
        assert!(store.pm_conversation_prefs().expect("list").is_empty());
    }

    #[test]
    fn prefs_go_with_their_server() {
        let (_dir, store) = temp_store();
        let id = server(&store);
        store
            .set_pm_conversation_pinned(id, "bob", true)
            .expect("pin");
        store
            .remove_server(id, &crate::SecretStore::in_memory())
            .expect("remove");
        assert!(store.pm_conversation_prefs().expect("list").is_empty());
    }

    #[test]
    fn summaries_hold_the_newest_message_and_the_peer_name() {
        let (_dir, store) = temp_store();
        let id = server(&store);
        let add = |key: &str, sender: &str, hash: &str, at: UnixMillis, body: &str| {
            store
                .append_message(
                    id,
                    &NewChatMessage {
                        conversation: Conversation::Private(key.into()),
                        sender_name: sender.into(),
                        sender_cert_hash: Some(hash.into()),
                        sent_at: at,
                        body: body.into(),
                    },
                )
                .expect("append");
        };
        add("bob", "Bob", "bob", 10, "hi");
        add("bob", "Me", "me", 20, "hello Bob");
        add("eve", "Me", "me", 15, "only mine");
        store
            .append_message(
                id,
                &NewChatMessage {
                    conversation: Conversation::Channel("Root".into()),
                    sender_name: "Bob".into(),
                    sender_cert_hash: Some("bob".into()),
                    sent_at: 30,
                    body: "channel".into(),
                },
            )
            .expect("channel");
        let all = store.private_conversation_summaries(id).expect("list");
        let view: Vec<_> = all
            .iter()
            .map(|s| (s.last.body.as_str(), s.peer_name.as_deref()))
            .collect();
        assert_eq!(view, vec![("hello Bob", Some("Bob")), ("only mine", None)]);
    }
}
