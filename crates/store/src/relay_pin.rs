//! Trust-on-first-use pins of a server's mumpeer relay (spec 04, FR-4.9, §7.1), kept per
//! saved server and own identity. Only the pin is stored; announcements, tokens and relay
//! addresses stay in memory for the connection.

use rusqlite::{OptionalExtension, params};

use crate::db::{Store, UnixMillis};
use crate::error::{Error, Result};
use crate::identities::IdentityId;
use crate::servers::ServerId;

/// Hex digits of a bot certificate hash (SHA-1, as Mumble shows certificate hashes).
const BOT_CERT_HASH_LEN: usize = 40;
/// Hex digits of a relay certificate fingerprint (SHA-256).
const RELAY_FP_LEN: usize = 64;

/// The bot and relay first accepted for one server entry and identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayPin {
    /// Certificate hash of the bot that announced the relay, 40 lowercase hex digits.
    pub bot_cert_hash: String,
    /// Certificate fingerprint of the relay, 64 lowercase hex digits.
    pub relay_fp: String,
    /// When the pair was first accepted; shown in the server info panel.
    pub first_seen: UnixMillis,
}

/// Lowercases `value` if it is exactly `len` hex digits.
fn normalized_hex(value: &str, len: usize) -> Result<String> {
    if value.len() == len && value.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(Error::InvalidRelayPin)
    }
}

impl Store {
    /// The relay pin of a server entry and identity, if one was accepted.
    pub fn relay_pin(&self, server: ServerId, identity: IdentityId) -> Result<Option<RelayPin>> {
        Ok(self
            .conn
            .query_row(
                "SELECT bot_cert_hash, relay_fp, first_seen FROM relay_pin
                 WHERE server_id = ?1 AND identity_id = ?2",
                [server, identity],
                |row| {
                    Ok(RelayPin {
                        bot_cert_hash: row.get(0)?,
                        relay_fp: row.get(1)?,
                        first_seen: row.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// Pins a relay on first acceptance, or replaces the pin when the user trusts a changed
    /// relay. Hashes are checked for length and hex digits and stored lowercase.
    pub fn set_relay_pin(
        &self,
        server: ServerId,
        identity: IdentityId,
        pin: &RelayPin,
    ) -> Result<()> {
        let bot_cert_hash = normalized_hex(&pin.bot_cert_hash, BOT_CERT_HASH_LEN)?;
        let relay_fp = normalized_hex(&pin.relay_fp, RELAY_FP_LEN)?;
        if self.server(server)?.is_none() {
            return Err(Error::UnknownServer(server));
        }
        if self.identity(identity)?.is_none() {
            return Err(Error::UnknownIdentity(identity));
        }
        self.conn.execute(
            "INSERT INTO relay_pin (server_id, identity_id, bot_cert_hash, relay_fp, first_seen)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (server_id, identity_id) DO UPDATE SET
                 bot_cert_hash = excluded.bot_cert_hash,
                 relay_fp = excluded.relay_fp,
                 first_seen = excluded.first_seen",
            params![server, identity, bot_cert_hash, relay_fp, pin.first_seen],
        )?;
        Ok(())
    }

    /// Removes the relay pin ("Forget relay"); does nothing if there is none.
    pub fn forget_relay_pin(&self, server: ServerId, identity: IdentityId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM relay_pin WHERE server_id = ?1 AND identity_id = ?2",
            [server, identity],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_store;
    use crate::{NewIdentity, NewServer, SecretStore};

    fn pin(bot: char, relay: char, first_seen: UnixMillis) -> RelayPin {
        RelayPin {
            bot_cert_hash: bot.to_string().repeat(BOT_CERT_HASH_LEN),
            relay_fp: relay.to_string().repeat(RELAY_FP_LEN),
            first_seen,
        }
    }

    fn setup(store: &Store) -> (ServerId, IdentityId, IdentityId) {
        let server = store
            .add_server(&NewServer::new("A", "localhost", "Tester"))
            .expect("server")
            .id;
        let first = store
            .add_identity(&NewIdentity::new("One", "one", "h1", 1))
            .expect("identity")
            .id;
        let second = store
            .add_identity(&NewIdentity::new("Two", "two", "h2", 2))
            .expect("identity")
            .id;
        (server, first, second)
    }

    #[test]
    fn pin_is_set_replaced_and_forgotten_per_identity() {
        let (_dir, store) = temp_store();
        let (server, one, two) = setup(&store);
        assert_eq!(store.relay_pin(server, one).expect("read"), None);

        store
            .set_relay_pin(server, one, &pin('A', 'b', 10))
            .expect("pin");
        assert_eq!(
            store.relay_pin(server, one).expect("read"),
            Some(pin('a', 'b', 10)),
            "hashes are stored lowercase"
        );
        assert_eq!(store.relay_pin(server, two).expect("read"), None);

        store
            .set_relay_pin(server, one, &pin('c', 'd', 20))
            .expect("replace");
        assert_eq!(
            store.relay_pin(server, one).expect("read"),
            Some(pin('c', 'd', 20))
        );

        store.forget_relay_pin(server, one).expect("forget");
        assert_eq!(store.relay_pin(server, one).expect("read"), None);
        store.forget_relay_pin(server, one).expect("forget again");
    }

    #[test]
    fn malformed_hashes_and_unknown_owners_are_rejected() {
        let (_dir, store) = temp_store();
        let (server, one, _) = setup(&store);
        let mut short = pin('a', 'b', 1);
        short.bot_cert_hash.pop();
        let mut not_hex = pin('a', 'b', 1);
        not_hex.relay_fp.replace_range(0..1, "g");
        for bad in [short, not_hex] {
            assert!(matches!(
                store.set_relay_pin(server, one, &bad),
                Err(Error::InvalidRelayPin)
            ));
        }
        assert!(matches!(
            store.set_relay_pin(999, one, &pin('a', 'b', 1)),
            Err(Error::UnknownServer(999))
        ));
        assert!(matches!(
            store.set_relay_pin(server, 999, &pin('a', 'b', 1)),
            Err(Error::UnknownIdentity(999))
        ));
    }

    #[test]
    fn pins_go_with_their_server_and_identity() {
        let (_dir, store) = temp_store();
        let secrets = SecretStore::in_memory();
        let (server, one, two) = setup(&store);
        let other = store
            .add_server(&NewServer::new("B", "localhost", "Tester"))
            .expect("server")
            .id;
        store
            .set_relay_pin(server, one, &pin('a', 'b', 1))
            .expect("pin");
        store
            .set_relay_pin(other, one, &pin('a', 'b', 1))
            .expect("pin");
        store
            .set_relay_pin(other, two, &pin('a', 'b', 1))
            .expect("pin");

        store
            .remove_server(server, &secrets)
            .expect("remove server");
        assert_eq!(store.relay_pin(server, one).expect("read"), None);
        assert!(store.relay_pin(other, one).expect("read").is_some());

        store
            .remove_identity(one, &secrets)
            .expect("remove identity");
        assert_eq!(store.relay_pin(other, one).expect("read"), None);
        assert!(store.relay_pin(other, two).expect("read").is_some());
    }
}
