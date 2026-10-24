//! Per-server trust decisions, followed channels and per-person settings.

use rusqlite::{OptionalExtension, params};

use crate::db::{Store, UnixMillis};
use crate::error::{Error, Result};
use crate::servers::ServerId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowedChannel {
    pub channel_path: String,
    pub last_read_at: UnixMillis,
}

/// Local volume and mute for a person, keyed by (server, certificate hash) (FR-1.19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersonSetting {
    /// Percent, 0–200.
    pub volume: u16,
    pub local_mute: bool,
}

impl Default for PersonSetting {
    fn default() -> Self {
        Self {
            volume: 100,
            local_mute: false,
        }
    }
}

impl Store {
    /// The server certificate fingerprint the user trusted, if any (FR-1.10).
    pub fn trusted_cert(&self, server: ServerId) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT fingerprint FROM trusted_cert WHERE server_id = ?1",
                [server],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Records (or replaces, after a certificate change was accepted) the trusted
    /// fingerprint of a server.
    pub fn set_trusted_cert(&self, server: ServerId, fingerprint: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO trusted_cert (server_id, fingerprint) VALUES (?1, ?2)
             ON CONFLICT (server_id) DO UPDATE SET fingerprint = excluded.fingerprint",
            params![server, fingerprint],
        )?;
        Ok(())
    }

    pub fn followed_channels(&self, server: ServerId) -> Result<Vec<FollowedChannel>> {
        let mut stmt = self.conn.prepare(
            "SELECT channel_path, last_read_at FROM followed_channel
             WHERE server_id = ?1 ORDER BY channel_path",
        )?;
        let rows = stmt.query_map([server], |row| {
            Ok(FollowedChannel {
                channel_path: row.get(0)?,
                last_read_at: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Follows a channel; following an already followed channel is a no-op.
    pub fn follow_channel(
        &self,
        server: ServerId,
        channel_path: &str,
        now: UnixMillis,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO followed_channel (server_id, channel_path, last_read_at)
             VALUES (?1, ?2, ?3) ON CONFLICT DO NOTHING",
            params![server, channel_path, now],
        )?;
        Ok(())
    }

    pub fn unfollow_channel(&self, server: ServerId, channel_path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM followed_channel WHERE server_id = ?1 AND channel_path = ?2",
            params![server, channel_path],
        )?;
        Ok(())
    }

    pub fn mark_channel_read(
        &self,
        server: ServerId,
        channel_path: &str,
        at: UnixMillis,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE followed_channel SET last_read_at = ?3
             WHERE server_id = ?1 AND channel_path = ?2",
            params![server, channel_path, at],
        )?;
        Ok(())
    }

    pub fn person_setting(
        &self,
        server: ServerId,
        cert_hash: &str,
    ) -> Result<Option<PersonSetting>> {
        Ok(self
            .conn
            .query_row(
                "SELECT volume, local_mute FROM person_setting
                 WHERE server_id = ?1 AND person_cert_hash = ?2",
                params![server, cert_hash],
                |row| {
                    Ok(PersonSetting {
                        volume: row.get(0)?,
                        local_mute: row.get(1)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn set_person_setting(
        &self,
        server: ServerId,
        cert_hash: &str,
        setting: &PersonSetting,
    ) -> Result<()> {
        if setting.volume > 200 {
            return Err(Error::InvalidVolume(setting.volume));
        }
        self.conn.execute(
            "INSERT INTO person_setting (server_id, person_cert_hash, volume, local_mute)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (server_id, person_cert_hash)
             DO UPDATE SET volume = excluded.volume, local_mute = excluded.local_mute",
            params![server, cert_hash, setting.volume, setting.local_mute],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewServer;
    use crate::db::tests::temp_store;

    #[test]
    fn trust_is_stored_and_replaced() {
        let (_dir, store) = temp_store();
        let id = store
            .add_server(&NewServer::new("A", "a", "u"))
            .expect("add")
            .id;
        assert_eq!(store.trusted_cert(id).expect("get"), None);
        store.set_trusted_cert(id, "old").expect("set");
        store.set_trusted_cert(id, "new").expect("replace");
        assert_eq!(store.trusted_cert(id).expect("get").as_deref(), Some("new"));
    }

    #[test]
    fn follow_mark_read_unfollow() {
        let (_dir, store) = temp_store();
        let id = store
            .add_server(&NewServer::new("A", "a", "u"))
            .expect("add")
            .id;
        store.follow_channel(id, "Root/B", 10).expect("follow");
        store.follow_channel(id, "Root/A", 10).expect("follow");
        store
            .follow_channel(id, "Root/A", 99)
            .expect("follow again");
        store.mark_channel_read(id, "Root/B", 20).expect("read");
        assert_eq!(
            store.followed_channels(id).expect("list"),
            vec![
                FollowedChannel {
                    channel_path: "Root/A".into(),
                    last_read_at: 10
                },
                FollowedChannel {
                    channel_path: "Root/B".into(),
                    last_read_at: 20
                },
            ]
        );
        store.unfollow_channel(id, "Root/A").expect("unfollow");
        assert_eq!(store.followed_channels(id).expect("list").len(), 1);
    }

    #[test]
    fn person_settings_are_keyed_by_server_and_hash() {
        let (_dir, store) = temp_store();
        let a = store
            .add_server(&NewServer::new("A", "a", "u"))
            .expect("a")
            .id;
        let b = store
            .add_server(&NewServer::new("B", "b", "u"))
            .expect("b")
            .id;
        let loud = PersonSetting {
            volume: 200,
            local_mute: false,
        };
        store.set_person_setting(a, "h1", &loud).expect("set");
        assert_eq!(store.person_setting(a, "h1").expect("get"), Some(loud));
        assert_eq!(store.person_setting(b, "h1").expect("get"), None);

        let muted = PersonSetting {
            volume: 0,
            local_mute: true,
        };
        store.set_person_setting(a, "h1", &muted).expect("update");
        assert_eq!(store.person_setting(a, "h1").expect("get"), Some(muted));

        let too_loud = PersonSetting {
            volume: 201,
            local_mute: false,
        };
        assert!(matches!(
            store.set_person_setting(a, "h1", &too_loud),
            Err(Error::InvalidVolume(201))
        ));
    }
}
