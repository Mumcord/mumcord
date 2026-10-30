//! What Mumcord knows about each saved channel password (issue #146): the user's own label,
//! the channels it was entered for and when it was last used. The tokens themselves stay in
//! secure storage ([`crate::SecretStore`]); this table only holds non-secret details and is
//! keyed by the token's position in the server's token list, which only grows at the end and
//! shrinks by removal, so positions are kept in step on every change.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::{Store, UnixMillis};
use crate::error::Result;
use crate::servers::ServerId;

/// A channel a token was entered for, as it was named then.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenChannel {
    pub channel_id: u32,
    /// Channel path below the root at the time the token was entered.
    pub path: Vec<String>,
}

/// The details of one token. Tokens saved before these details existed have none: no label,
/// no channels and no last use, until they are used again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelToken {
    /// The user's own name for the token; `None` shows the channels or "Password N".
    pub label: Option<String>,
    pub channels: Vec<TokenChannel>,
    pub last_used_at: Option<UnixMillis>,
}

impl Store {
    /// The details of the server's first `count` tokens, in token order.
    pub fn channel_tokens(&self, server: ServerId, count: usize) -> Result<Vec<ChannelToken>> {
        let mut tokens = self.all_channel_tokens(server)?;
        tokens.resize_with(count, ChannelToken::default);
        Ok(tokens)
    }

    /// Records that the token at `position` was entered for a channel; a channel it already
    /// lists gets its current path. Also counts as a use.
    pub fn attach_token_channel(
        &self,
        server: ServerId,
        position: usize,
        channel: TokenChannel,
        now: UnixMillis,
    ) -> Result<()> {
        self.change_channel_tokens(server, |tokens| {
            if tokens.len() <= position {
                tokens.resize_with(position + 1, ChannelToken::default);
            }
            let token = &mut tokens[position];
            match token
                .channels
                .iter_mut()
                .find(|c| c.channel_id == channel.channel_id)
            {
                Some(known) => known.path = channel.path,
                None => token.channels.push(channel),
            }
            token.last_used_at = Some(now);
        })
    }

    /// Notes a use of every token entered for `channel_id`, e.g. when the user joins it.
    pub fn touch_token_channel(
        &self,
        server: ServerId,
        channel_id: u32,
        now: UnixMillis,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE channel_token SET last_used_at = ?3
             WHERE server_id = ?1 AND EXISTS (
                 SELECT 1 FROM json_each(channels) WHERE json_extract(value, '$.channel_id') = ?2)",
            params![server, channel_id, now],
        )?;
        Ok(())
    }

    /// Sets the user's own label of the token at `position`; `None` goes back to the default.
    pub fn rename_channel_token(
        &self,
        server: ServerId,
        position: usize,
        label: Option<&str>,
    ) -> Result<()> {
        self.change_channel_tokens(server, |tokens| {
            if tokens.len() <= position {
                tokens.resize_with(position + 1, ChannelToken::default);
            }
            tokens[position].label = label.map(str::to_owned);
        })
    }

    /// Forgets the details of the tokens at `positions`; later tokens move up, as they do in
    /// the token list.
    pub fn remove_channel_tokens(&self, server: ServerId, positions: &[usize]) -> Result<()> {
        self.change_channel_tokens(server, |tokens| {
            let mut index = 0;
            tokens.retain(|_| {
                let keep = !positions.contains(&index);
                index += 1;
                keep
            });
        })
    }

    /// Forgets the details from `position` on, so a token newly added there starts without
    /// details left over from a list that changed outside Mumcord.
    pub fn truncate_channel_tokens(&self, server: ServerId, position: usize) -> Result<()> {
        self.conn.execute(
            "DELETE FROM channel_token WHERE server_id = ?1 AND position >= ?2",
            params![server, i64::try_from(position).unwrap_or(i64::MAX)],
        )?;
        Ok(())
    }

    fn all_channel_tokens(&self, server: ServerId) -> Result<Vec<ChannelToken>> {
        let mut stmt = self.conn.prepare(
            "SELECT position, label, channels, last_used_at FROM channel_token
             WHERE server_id = ?1 ORDER BY position",
        )?;
        let rows = stmt.query_map([server], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<UnixMillis>>(3)?,
            ))
        })?;
        let mut tokens = Vec::new();
        for row in rows {
            let (position, label, channels, last_used_at) = row?;
            let Ok(position) = usize::try_from(position) else {
                continue;
            };
            if tokens.len() <= position {
                tokens.resize_with(position + 1, ChannelToken::default);
            }
            tokens[position] = ChannelToken {
                label,
                channels: serde_json::from_str(&channels)?,
                last_used_at,
            };
        }
        Ok(tokens)
    }

    /// Rewrites all details of a server in one transaction after `change`.
    fn change_channel_tokens(
        &self,
        server: ServerId,
        change: impl FnOnce(&mut Vec<ChannelToken>),
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let mut tokens = self.all_channel_tokens(server)?;
        change(&mut tokens);
        tx.execute("DELETE FROM channel_token WHERE server_id = ?1", [server])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO channel_token (server_id, position, label, channels, last_used_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (position, token) in tokens.iter().enumerate() {
                if *token == ChannelToken::default() {
                    continue;
                }
                stmt.execute(params![
                    server,
                    i64::try_from(position).unwrap_or(i64::MAX),
                    token.label,
                    serde_json::to_string(&token.channels)?,
                    token.last_used_at,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewServer;
    use crate::db::tests::temp_store;

    fn channel(id: u32, path: &[&str]) -> TokenChannel {
        TokenChannel {
            channel_id: id,
            path: path.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn server(store: &Store) -> ServerId {
        store
            .add_server(&NewServer::new("Home", "localhost", "Tester"))
            .expect("add")
            .id
    }

    #[test]
    fn tokens_without_details_have_none() {
        let (_dir, store) = temp_store();
        let id = server(&store);
        assert_eq!(
            store.channel_tokens(id, 2).expect("read"),
            vec![ChannelToken::default(); 2]
        );
    }

    #[test]
    fn a_token_lists_every_channel_it_was_entered_for() {
        let (_dir, store) = temp_store();
        let id = server(&store);
        store
            .attach_token_channel(id, 1, channel(5, &["Raid", "Officers"]), 10)
            .expect("attach");
        store
            .attach_token_channel(id, 1, channel(6, &["Raid", "Guests"]), 20)
            .expect("attach");
        store
            .attach_token_channel(id, 1, channel(5, &["Raid", "Leads"]), 30)
            .expect("attach again");
        let tokens = store.channel_tokens(id, 2).expect("read");
        assert_eq!(tokens[0], ChannelToken::default());
        assert_eq!(
            tokens[1].channels,
            [
                channel(5, &["Raid", "Leads"]),
                channel(6, &["Raid", "Guests"])
            ]
        );
        assert_eq!(tokens[1].last_used_at, Some(30));
    }

    #[test]
    fn renaming_sets_and_clears_the_label() {
        let (_dir, store) = temp_store();
        let id = server(&store);
        store
            .rename_channel_token(id, 0, Some("Raid password"))
            .expect("rename");
        assert_eq!(
            store.channel_tokens(id, 1).expect("read")[0]
                .label
                .as_deref(),
            Some("Raid password")
        );
        store.rename_channel_token(id, 0, None).expect("clear");
        assert_eq!(store.channel_tokens(id, 1).expect("read")[0].label, None);
    }

    #[test]
    fn removing_moves_later_tokens_up() {
        let (_dir, store) = temp_store();
        let id = server(&store);
        for position in 0..3 {
            store
                .rename_channel_token(id, position, Some(&format!("T{position}")))
                .expect("rename");
        }
        store.remove_channel_tokens(id, &[0, 2]).expect("remove");
        let tokens = store.channel_tokens(id, 1).expect("read");
        assert_eq!(tokens[0].label.as_deref(), Some("T1"));
        assert_eq!(store.all_channel_tokens(id).expect("all").len(), 1);
    }

    #[test]
    fn using_a_channel_touches_its_tokens_only() {
        let (_dir, store) = temp_store();
        let id = server(&store);
        store
            .attach_token_channel(id, 0, channel(5, &["A"]), 1)
            .expect("attach");
        store
            .attach_token_channel(id, 1, channel(6, &["B"]), 1)
            .expect("attach");
        store.touch_token_channel(id, 6, 99).expect("touch");
        let tokens = store.channel_tokens(id, 2).expect("read");
        assert_eq!(tokens[0].last_used_at, Some(1));
        assert_eq!(tokens[1].last_used_at, Some(99));
    }

    #[test]
    fn truncating_forgets_details_of_a_reused_position() {
        let (_dir, store) = temp_store();
        let id = server(&store);
        store
            .attach_token_channel(id, 0, channel(5, &["A"]), 1)
            .expect("attach");
        store
            .attach_token_channel(id, 1, channel(6, &["B"]), 1)
            .expect("attach");
        store.truncate_channel_tokens(id, 1).expect("truncate");
        let tokens = store.channel_tokens(id, 2).expect("read");
        assert_eq!(tokens[0].channels, [channel(5, &["A"])]);
        assert_eq!(tokens[1], ChannelToken::default());
    }

    #[test]
    fn removing_the_server_removes_the_details() {
        let (_dir, store) = temp_store();
        let id = server(&store);
        store
            .rename_channel_token(id, 0, Some("x"))
            .expect("rename");
        store
            .remove_server(id, &crate::SecretStore::in_memory())
            .expect("remove");
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM channel_token", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 0);
    }

    #[test]
    fn details_leave_the_saved_tokens_alone() {
        let (_dir, store) = temp_store();
        let secrets = crate::SecretStore::in_memory();
        let id = server(&store);
        let tokens = vec!["old one".to_owned(), "old two".to_owned()];
        secrets.set_access_tokens(id, &tokens).expect("tokens");
        assert_eq!(
            store.channel_tokens(id, tokens.len()).expect("read"),
            vec![ChannelToken::default(); 2],
            "tokens saved before have no details yet"
        );
        store
            .attach_token_channel(id, 1, channel(5, &["Raid"]), 7)
            .expect("attach");
        store
            .rename_channel_token(id, 0, Some("Mine"))
            .expect("rename");
        assert_eq!(secrets.access_tokens(id).expect("tokens"), tokens);
    }
}
