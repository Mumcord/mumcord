//! The identity registry (issue #148): local labels, certificate names and hashes, which
//! identity is the default and which saved server uses which identity.
//!
//! The certificates and their private keys themselves live in the OS secure storage
//! ([`crate::SecretStore`]); nothing secret is stored here.

use rusqlite::{OptionalExtension, Row, params};

use crate::db::{Store, UnixMillis};
use crate::error::{Error, Result};
use crate::secrets::SecretStore;
use crate::servers::ServerId;

/// Database id of an identity; also names its items in secure storage.
pub type IdentityId = i64;

/// Longest local label, in characters.
pub const MAX_IDENTITY_LABEL: usize = 40;

/// Input for adding an identity. The certificate stays in secure storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewIdentity {
    /// Local label the user sees; the certificate is not changed by it.
    pub label: String,
    /// Common name inside the certificate (the default user name on servers).
    pub cert_name: String,
    /// SHA-1 over the DER certificate, as Mumble identifies users.
    pub cert_hash: String,
    pub added_at: UnixMillis,
}

impl NewIdentity {
    pub fn new(label: &str, cert_name: &str, cert_hash: &str, added_at: UnixMillis) -> Self {
        Self {
            label: label.to_owned(),
            cert_name: cert_name.to_owned(),
            cert_hash: cert_hash.to_owned(),
            added_at,
        }
    }
}

/// One identity as the local database knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityRecord {
    pub id: IdentityId,
    pub label: String,
    pub cert_name: String,
    pub cert_hash: String,
    pub added_at: UnixMillis,
    pub is_default: bool,
}

const IDENTITY_COLUMNS: &str = "id, label, cert_name, cert_hash, created_at, is_default";

fn identity_from_row(row: &Row<'_>) -> rusqlite::Result<IdentityRecord> {
    Ok(IdentityRecord {
        id: row.get(0)?,
        label: row.get(1)?,
        cert_name: row.get(2)?,
        cert_hash: row.get(3)?,
        added_at: row.get(4)?,
        is_default: row.get(5)?,
    })
}

/// Trims a label and checks it is usable; an empty or over-long label is refused.
fn checked_label(label: &str) -> Result<String> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > MAX_IDENTITY_LABEL {
        return Err(Error::InvalidIdentityLabel {
            max: MAX_IDENTITY_LABEL,
        });
    }
    Ok(label.to_owned())
}

impl Store {
    /// Adds an identity. The first identity becomes the default; a certificate that is
    /// already in the list is refused.
    pub fn add_identity(&self, identity: &NewIdentity) -> Result<IdentityRecord> {
        let label = checked_label(&identity.label)?;
        let tx = self.conn.unchecked_transaction()?;
        let existing: i64 = tx.query_row("SELECT COUNT(*) FROM identity", [], |row| row.get(0))?;
        let duplicate: i64 = tx.query_row(
            "SELECT COUNT(*) FROM identity WHERE cert_hash = ?1",
            [&identity.cert_hash],
            |row| row.get(0),
        )?;
        if duplicate > 0 {
            return Err(Error::DuplicateIdentity);
        }
        tx.execute(
            "INSERT INTO identity (label, cert_name, cert_hash, created_at, is_default)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                label,
                identity.cert_name,
                identity.cert_hash,
                identity.added_at,
                existing == 0
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        self.identity(id)?.ok_or(Error::UnknownIdentity(id))
    }

    /// All identities: the default first, then by the date they were added.
    pub fn identities(&self) -> Result<Vec<IdentityRecord>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {IDENTITY_COLUMNS} FROM identity ORDER BY is_default DESC, created_at, id"
        ))?;
        let rows = stmt.query_map([], identity_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn identity(&self, id: IdentityId) -> Result<Option<IdentityRecord>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {IDENTITY_COLUMNS} FROM identity WHERE id = ?1"),
                [id],
                identity_from_row,
            )
            .optional()?)
    }

    pub fn default_identity(&self) -> Result<Option<IdentityRecord>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {IDENTITY_COLUMNS} FROM identity WHERE is_default = 1"),
                [],
                identity_from_row,
            )
            .optional()?)
    }

    /// Changes the local label only; the certificate is untouched.
    pub fn rename_identity(&self, id: IdentityId, label: &str) -> Result<IdentityRecord> {
        let label = checked_label(label)?;
        let changed = self.conn.execute(
            "UPDATE identity SET label = ?2 WHERE id = ?1",
            params![id, label],
        )?;
        if changed == 0 {
            return Err(Error::UnknownIdentity(id));
        }
        self.identity(id)?.ok_or(Error::UnknownIdentity(id))
    }

    /// Makes `id` the only default identity.
    pub fn set_default_identity(&self, id: IdentityId) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if tx
            .query_row("SELECT 1 FROM identity WHERE id = ?1", [id], |_| Ok(()))
            .optional()?
            .is_none()
        {
            return Err(Error::UnknownIdentity(id));
        }
        // The unique index allows only one default, so clear it before setting the new one.
        tx.execute(
            "UPDATE identity SET is_default = 0 WHERE is_default = 1",
            [],
        )?;
        tx.execute("UPDATE identity SET is_default = 1 WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    /// Servers that use this identity directly (not through the default).
    pub fn identity_servers(&self, id: IdentityId) -> Result<Vec<ServerId>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM saved_server WHERE identity_id = ?1 ORDER BY sort_order, id",
        )?;
        let rows = stmt.query_map([id], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Chooses the identity of a saved server; `None` means "use the default identity".
    pub fn set_server_identity(
        &self,
        server: ServerId,
        identity: Option<IdentityId>,
    ) -> Result<()> {
        if let Some(id) = identity
            && self.identity(id)?.is_none()
        {
            return Err(Error::UnknownIdentity(id));
        }
        let changed = self.conn.execute(
            "UPDATE saved_server SET identity_id = ?2 WHERE id = ?1",
            params![server, identity],
        )?;
        if changed == 0 {
            return Err(Error::UnknownServer(server));
        }
        Ok(())
    }

    /// Drops the row of an identity whose key material could not be stored, so a failed
    /// create or import leaves no identity in the list that cannot be used. If it was the
    /// default, the oldest remaining identity takes over.
    pub fn discard_identity(&self, id: IdentityId) -> Result<()> {
        let record = self.identity(id)?.ok_or(Error::UnknownIdentity(id))?;
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM identity WHERE id = ?1", [id])?;
        if record.is_default
            && let Some(next) = tx
                .query_row(
                    &format!(
                        "SELECT {IDENTITY_COLUMNS} FROM identity ORDER BY created_at, id LIMIT 1"
                    ),
                    [],
                    identity_from_row,
                )
                .optional()?
        {
            tx.execute(
                "UPDATE identity SET is_default = 1 WHERE id = ?1",
                [next.id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Removes an identity with its key material. The last identity cannot be removed, and
    /// servers that used it fall back to the default. If it was the default, the oldest
    /// remaining identity becomes the new one.
    pub fn remove_identity(&self, id: IdentityId, secrets: &SecretStore) -> Result<()> {
        let record = self.identity(id)?.ok_or(Error::UnknownIdentity(id))?;
        let all = self.identities()?;
        if all.len() <= 1 {
            return Err(Error::LastIdentity);
        }
        // Key material first: if secure storage fails, the identity stays and the user can
        // retry instead of leaving an orphaned secret behind.
        secrets.delete_identity(id)?;
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM identity WHERE id = ?1", [id])?;
        if record.is_default
            && let Some(next) = all.iter().find(|other| other.id != id)
        {
            tx.execute(
                "UPDATE identity SET is_default = 1 WHERE id = ?1",
                [next.id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_store;

    fn add(store: &Store, label: &str, hash: &str, at: UnixMillis) -> IdentityRecord {
        store
            .add_identity(&NewIdentity::new(label, label, hash, at))
            .expect("add identity")
    }

    #[test]
    fn first_identity_becomes_the_default_and_order_follows() {
        let (_dir, store) = temp_store();
        let first = add(&store, "Main", "hash-main", 10);
        assert!(first.is_default);
        let second = add(&store, "Work", "hash-work", 5);
        assert!(!second.is_default);
        // Default first, then by the date they were added.
        assert_eq!(
            store
                .identities()
                .expect("list")
                .iter()
                .map(|i| i.label.as_str())
                .collect::<Vec<_>>(),
            ["Main", "Work"]
        );
        assert_eq!(
            store.default_identity().expect("default").map(|i| i.id),
            Some(first.id)
        );
    }

    #[test]
    fn labels_are_trimmed_and_validated() {
        let (_dir, store) = temp_store();
        assert_eq!(add(&store, "  Main  ", "h", 1).label, "Main");
        assert!(matches!(
            store.add_identity(&NewIdentity::new("   ", "x", "h2", 2)),
            Err(Error::InvalidIdentityLabel { .. })
        ));
        let long = "x".repeat(MAX_IDENTITY_LABEL + 1);
        assert!(matches!(
            store.add_identity(&NewIdentity::new(&long, "x", "h3", 3)),
            Err(Error::InvalidIdentityLabel { .. })
        ));
        let record = add(&store, "Main", "h4", 4);
        assert!(matches!(
            store.rename_identity(record.id, ""),
            Err(Error::InvalidIdentityLabel { .. })
        ));
        assert_eq!(
            store
                .rename_identity(record.id, " Home ")
                .expect("rename")
                .label,
            "Home"
        );
        assert!(matches!(
            store.rename_identity(999, "x"),
            Err(Error::UnknownIdentity(999))
        ));
    }

    #[test]
    fn duplicate_certificate_hashes_are_refused() {
        let (_dir, store) = temp_store();
        add(&store, "Main", "same", 1);
        assert!(matches!(
            store.add_identity(&NewIdentity::new("Copy", "Main", "same", 2)),
            Err(Error::DuplicateIdentity)
        ));
        assert_eq!(store.identities().expect("list").len(), 1);
    }

    #[test]
    fn exactly_one_identity_is_the_default() {
        let (_dir, store) = temp_store();
        let a = add(&store, "A", "ha", 1);
        let b = add(&store, "B", "hb", 2);
        store.set_default_identity(b.id).expect("default");
        let all = store.identities().expect("list");
        assert_eq!(all.iter().filter(|i| i.is_default).count(), 1);
        assert_eq!(all[0].id, b.id);
        assert!(matches!(
            store.set_default_identity(999),
            Err(Error::UnknownIdentity(999))
        ));
        assert_eq!(
            store.default_identity().expect("default").map(|i| i.id),
            Some(b.id)
        );
        // Setting the current default again is not an error.
        store.set_default_identity(b.id).expect("again");
        assert_eq!(
            store.default_identity().expect("default").map(|i| i.id),
            Some(b.id)
        );
        let _ = a;
    }

    #[test]
    fn a_server_chooses_its_identity_and_falls_back_to_the_default() {
        let (_dir, store) = temp_store();
        let secrets = SecretStore::in_memory();
        let main = add(&store, "Main", "hm", 1);
        let work = add(&store, "Work", "hw", 2);
        let server = store
            .add_server(&crate::NewServer::new("A", "localhost", "Tester"))
            .expect("add server");
        assert_eq!(server.identity_id, None);

        store
            .set_server_identity(server.id, Some(work.id))
            .expect("set");
        assert_eq!(
            store
                .server(server.id)
                .expect("get")
                .expect("some")
                .identity_id,
            Some(work.id)
        );
        assert_eq!(
            store.identity_servers(work.id).expect("servers"),
            [server.id]
        );
        assert!(store.identity_servers(main.id).expect("servers").is_empty());

        store
            .set_server_identity(server.id, Some(999))
            .expect_err("unknown identity");
        assert!(matches!(
            store.set_server_identity(999, None),
            Err(Error::UnknownServer(999))
        ));

        // Removing the identity puts the server back on the default.
        store.remove_identity(work.id, &secrets).expect("remove");
        assert_eq!(
            store
                .server(server.id)
                .expect("get")
                .expect("some")
                .identity_id,
            None
        );
    }

    #[test]
    fn removing_the_default_promotes_the_oldest_remaining_identity() {
        let (_dir, store) = temp_store();
        let secrets = SecretStore::in_memory();
        let main = add(&store, "Main", "hm", 10);
        let work = add(&store, "Work", "hw", 20);
        let stream = add(&store, "Stream", "hs", 30);
        store.remove_identity(main.id, &secrets).expect("remove");
        let all = store.identities().expect("list");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, work.id);
        assert!(all[0].is_default);
        assert!(!all[1].is_default);
        assert_eq!(
            store.default_identity().expect("default").map(|i| i.id),
            Some(work.id)
        );
        let _ = stream;
    }

    #[test]
    fn the_last_identity_cannot_be_removed() {
        let (_dir, store) = temp_store();
        let secrets = SecretStore::in_memory();
        let main = add(&store, "Main", "hm", 1);
        assert!(matches!(
            store.remove_identity(main.id, &secrets),
            Err(Error::LastIdentity)
        ));
        assert_eq!(store.identities().expect("list").len(), 1);
        assert!(matches!(
            store.remove_identity(999, &secrets),
            Err(Error::UnknownIdentity(999))
        ));
    }

    #[test]
    fn removing_an_identity_deletes_its_key_material() {
        let (_dir, store) = temp_store();
        let secrets = SecretStore::in_memory();
        let main = add(&store, "Main", "hm", 1);
        let work = add(&store, "Work", "hw", 2);
        let identity = crate::Identity::generate("Tester").expect("generate");
        secrets
            .save_identity(main.id, &identity)
            .expect("save main");
        secrets
            .save_identity(work.id, &identity)
            .expect("save work");
        store.remove_identity(work.id, &secrets).expect("remove");
        assert_eq!(secrets.load_identity(work.id).expect("gone"), None);
        assert!(secrets.load_identity(main.id).expect("kept").is_some());
    }

    #[test]
    fn editing_a_server_keeps_its_identity() {
        let (_dir, store) = temp_store();
        let work = add(&store, "Work", "hw", 1);
        let mut server = store
            .add_server(&crate::NewServer::new("A", "localhost", "Tester"))
            .expect("add");
        server.identity_id = Some(work.id);
        store.update_server(&server).expect("update");
        assert_eq!(
            store
                .server(server.id)
                .expect("get")
                .expect("some")
                .identity_id,
            Some(work.id)
        );
    }
}
