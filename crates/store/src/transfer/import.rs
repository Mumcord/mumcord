//! The store side of a transfer: collecting what the desktop sends and importing what the
//! phone received, with the rules of spec 03 (duplicate identities are refused) and FR-5.66
//! (existing entries are kept, exact duplicates are skipped, nothing is synchronised).

use std::collections::HashSet;

use super::payload::{TransferIdentity, TransferPayload, TransferServer};
use crate::error::{Error, Result};
use crate::{
    Identity, IdentityId, IdentityRecord, MAX_IDENTITY_LABEL, NewIdentity, NewServer, SavedServer,
    SecretStore, Store, UnixMillis,
};

/// Why an entry of a transfer was not imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// It is already there: an identity with the same certificate, or a server with the same
    /// name, address, port and user name.
    Duplicate,
    /// It cannot be used: a key that does not belong to its certificate, a server without an
    /// address, and the like.
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedIdentity {
    pub label: String,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedServer {
    pub name: String,
    pub address: String,
    pub reason: SkipReason,
}

/// What an import did, for the summary the app shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReport {
    pub identities_imported: Vec<IdentityRecord>,
    pub identities_skipped: Vec<SkippedIdentity>,
    pub servers_imported: Vec<SavedServer>,
    pub servers_skipped: Vec<SkippedServer>,
}

impl Store {
    /// Collects the chosen identities and, if wanted, the server list for a transfer to the
    /// phone (FR-5.64). Servers carry name, address, port and user name; saved passwords and
    /// channel access tokens are not read at all (FR-5.66).
    pub fn export_transfer_payload(
        &self,
        secrets: &SecretStore,
        identity_ids: &[IdentityId],
        include_servers: bool,
    ) -> Result<TransferPayload> {
        let mut identities = Vec::with_capacity(identity_ids.len());
        let mut seen = HashSet::new();
        for &id in identity_ids {
            if !seen.insert(id) {
                continue;
            }
            let record = self.identity(id)?.ok_or(Error::UnknownIdentity(id))?;
            let identity = secrets
                .load_identity(id)?
                .ok_or(Error::CorruptStoredIdentity)?;
            identities.push(TransferIdentity::new(
                &record.label,
                identity.cert_der(),
                identity.key_pkcs8_der(),
                record.is_default,
            ));
        }
        let servers = if include_servers {
            let known = self.identities()?;
            let fingerprint = |id: IdentityId| {
                known
                    .iter()
                    .find(|record| record.id == id)
                    .map(|record| record.cert_hash.clone())
            };
            Some(
                self.servers()?
                    .into_iter()
                    .map(|server| TransferServer {
                        name: server.label,
                        address: server.address,
                        port: server.port,
                        username: server.username,
                        identity_fingerprint: server.identity_id.and_then(fingerprint),
                    })
                    .collect(),
            )
        } else {
            None
        };
        Ok(TransferPayload {
            identities,
            servers,
        })
    }

    /// Imports a received transfer (FR-5.65, FR-5.66) and reports what happened to each entry.
    ///
    /// Identities are added to the list; one whose certificate is already there is skipped
    /// (spec 03, FR-3.6). If the list was empty, the sender's default identity becomes the
    /// default here too; an existing default is never changed. Servers are added after the
    /// existing ones; a server that uses a specific identity on the sender keeps it when that
    /// identity is on this device after the import, and uses the default otherwise.
    ///
    /// The payload is untrusted. An entry that cannot be used is skipped and the rest is
    /// imported; only a failure of the database or of the secure storage ends the import with
    /// an error, and what was imported up to then stays.
    pub fn import_transfer_payload(
        &self,
        secrets: &SecretStore,
        payload: &TransferPayload,
        now: UnixMillis,
    ) -> Result<ImportReport> {
        let mut report = ImportReport::default();
        let was_empty = self.identities()?.is_empty();
        let mut imported_ids = Vec::new();
        let mut new_default = None;
        for entry in &payload.identities {
            let skipped = |reason| SkippedIdentity {
                label: entry.label.clone(),
                reason,
            };
            let Ok(identity) = Identity::from_der(&entry.cert_der, entry.key_pkcs8_der()) else {
                report.identities_skipped.push(skipped(SkipReason::Invalid));
                continue;
            };
            let Some(label) = import_label(&entry.label, identity.name()) else {
                report.identities_skipped.push(skipped(SkipReason::Invalid));
                continue;
            };
            let record = match self.add_identity(&NewIdentity::new(
                &label,
                identity.name(),
                &identity.cert_hash(),
                now,
            )) {
                Ok(record) => record,
                Err(Error::DuplicateIdentity) => {
                    report
                        .identities_skipped
                        .push(skipped(SkipReason::Duplicate));
                    continue;
                }
                Err(e) => return Err(e),
            };
            if let Err(e) = secrets.save_identity(record.id, &identity) {
                // No list entry without its key.
                self.discard_identity(record.id)?;
                return Err(e);
            }
            if entry.is_default {
                new_default = Some(record.id);
            }
            imported_ids.push(record.id);
        }
        if was_empty && let Some(id) = new_default {
            self.set_default_identity(id)?;
        }
        let known = self.identities()?;
        report.identities_imported = imported_ids
            .iter()
            .filter_map(|id| known.iter().find(|record| record.id == *id).cloned())
            .collect();

        let Some(servers) = &payload.servers else {
            return Ok(report);
        };
        let mut existing: HashSet<(String, String, u16, String)> = self
            .servers()?
            .into_iter()
            .map(|server| (server.label, server.address, server.port, server.username))
            .collect();
        for entry in servers {
            let skipped = |reason| SkippedServer {
                name: entry.name.clone(),
                address: entry.address.clone(),
                reason,
            };
            let Some(mut new) = checked_server(entry) else {
                report.servers_skipped.push(skipped(SkipReason::Invalid));
                continue;
            };
            let key = (
                new.label.clone(),
                new.address.clone(),
                new.port,
                new.username.clone(),
            );
            if !existing.insert(key) {
                report.servers_skipped.push(skipped(SkipReason::Duplicate));
                continue;
            }
            new.identity_id = entry.identity_fingerprint.as_ref().and_then(|fingerprint| {
                known
                    .iter()
                    .find(|record| record.cert_hash.eq_ignore_ascii_case(fingerprint))
                    .map(|record| record.id)
            });
            report.servers_imported.push(self.add_server(&new)?);
        }
        Ok(report)
    }
}

/// The sender's label if it is one this app accepts, otherwise the certificate's common name
/// cut to the allowed length; `None` if neither gives a label.
fn import_label(label: &str, cert_name: &str) -> Option<String> {
    let label = label.trim();
    if !label.is_empty() && label.chars().count() <= MAX_IDENTITY_LABEL {
        return Some(label.to_owned());
    }
    let fallback: String = cert_name.trim().chars().take(MAX_IDENTITY_LABEL).collect();
    let fallback = fallback.trim();
    (!fallback.is_empty()).then(|| fallback.to_owned())
}

/// The same rules the app applies to a server typed by hand; an empty name falls back to the
/// address.
fn checked_server(entry: &TransferServer) -> Option<NewServer> {
    let address = entry.address.trim();
    let username = entry.username.trim();
    if address.is_empty()
        || address.contains(char::is_whitespace)
        || username.is_empty()
        || entry.port == 0
    {
        return None;
    }
    let name = match entry.name.trim() {
        "" => address,
        name => name,
    };
    let mut server = NewServer::new(name, address, username);
    server.port = entry.port;
    Some(server)
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;
    use crate::db::tests::temp_store;
    use crate::transfer::{Transfer, TransferReader};

    // RSA key generation is slow; the tests share three identities.
    static IDENTITIES: LazyLock<[Identity; 3]> = LazyLock::new(|| {
        ["Zoe", "Work", "Stream"].map(|name| Identity::generate(name).expect("generate"))
    });

    /// A desktop profile: `identities` identities (the second one is the default if there are
    /// two or more) and two servers, the first with a saved password, a channel access token
    /// and the first identity.
    fn desktop(identities: usize) -> (tempfile::TempDir, Store, SecretStore, Vec<IdentityId>) {
        let (dir, store) = temp_store();
        let secrets = SecretStore::in_memory();
        let mut ids = Vec::new();
        for (index, identity) in IDENTITIES.iter().take(identities).enumerate() {
            let record = store
                .add_identity(&NewIdentity::new(
                    &format!("Label {index}"),
                    identity.name(),
                    &identity.cert_hash(),
                    1,
                ))
                .expect("add identity");
            secrets.save_identity(record.id, identity).expect("save");
            ids.push(record.id);
        }
        if let Some(second) = ids.get(1) {
            store.set_default_identity(*second).expect("default");
        }
        let mut home = NewServer::new("Home", "voice.example.org", "Zoe");
        home.identity_id = ids.first().copied();
        let home = store.add_server(&home).expect("add server");
        secrets
            .set_server_password(home.id, &sample_password())
            .expect("password");
        secrets
            .set_access_tokens(home.id, &["token-of-a-channel".to_owned()])
            .expect("token");
        let mut other = NewServer::new("Other", "10.0.0.5", "Zoe");
        other.port = 50000;
        store.add_server(&other).expect("add server");
        (dir, store, secrets, ids)
    }

    /// A throwaway password built at runtime, so no credential-looking literal sits in the test.
    fn sample_password() -> String {
        format!("server-pw-{}", std::process::id())
    }

    fn phone() -> (tempfile::TempDir, Store, SecretStore) {
        let (dir, store) = temp_store();
        (dir, store, SecretStore::in_memory())
    }

    #[test]
    fn export_carries_identities_and_servers_but_never_passwords_or_tokens() {
        let (_dir, store, secrets, ids) = desktop(2);
        let payload = store
            .export_transfer_payload(&secrets, &[ids[0], ids[1], ids[0]], true)
            .expect("export");
        assert_eq!(payload.identities.len(), 2);
        assert_eq!(payload.identities[0].label, "Label 0");
        assert_eq!(payload.identities[0].cert_der, IDENTITIES[0].cert_der());
        assert_eq!(
            payload.identities[0].key_pkcs8_der(),
            IDENTITIES[0].key_pkcs8_der()
        );
        assert!(!payload.identities[0].is_default);
        assert!(payload.identities[1].is_default);

        let servers = payload.servers.as_ref().expect("servers");
        assert_eq!(servers.len(), 2);
        assert_eq!(
            servers[0],
            TransferServer {
                name: "Home".to_owned(),
                address: "voice.example.org".to_owned(),
                port: 64738,
                username: "Zoe".to_owned(),
                identity_fingerprint: Some(IDENTITIES[0].cert_hash()),
            }
        );
        assert_eq!(servers[1].port, 50000);
        assert_eq!(servers[1].identity_fingerprint, None);

        // Neither the password nor the token is anywhere in what gets encrypted.
        let encoded = payload.encode().expect("encode");
        let contains = |needle: &[u8]| encoded.windows(needle.len()).any(|w| w == needle);
        assert!(contains(b"voice.example.org"));
        assert!(!contains(sample_password().as_bytes()));
        assert!(!contains(b"token-of-a-channel"));
    }

    #[test]
    fn export_without_servers_and_with_unknown_identities() {
        let (_dir, store, secrets, ids) = desktop(1);
        let payload = store
            .export_transfer_payload(&secrets, &ids, false)
            .expect("export");
        assert_eq!(payload.servers, None);
        assert!(matches!(
            store.export_transfer_payload(&secrets, &[4711], false),
            Err(Error::UnknownIdentity(4711))
        ));
    }

    #[test]
    fn a_fresh_phone_gets_identities_default_and_servers() {
        let (_dir, store, secrets, ids) = desktop(2);
        let payload = store
            .export_transfer_payload(&secrets, &ids, true)
            .expect("export");
        let (_phone_dir, phone, phone_secrets) = phone();
        let report = phone
            .import_transfer_payload(&phone_secrets, &payload, 99)
            .expect("import");

        assert_eq!(report.identities_imported.len(), 2);
        assert!(report.identities_skipped.is_empty());
        assert_eq!(report.servers_imported.len(), 2);
        assert!(report.servers_skipped.is_empty());

        // The sender's default (the second identity) is the default here too.
        let default = phone.default_identity().expect("query").expect("default");
        assert_eq!(default.label, "Label 1");
        assert_eq!(default.cert_hash, IDENTITIES[1].cert_hash());
        assert!(report.identities_imported[1].is_default);
        assert!(!report.identities_imported[0].is_default);
        assert_eq!(report.identities_imported[0].added_at, 99);

        // Keys are in secure storage and usable.
        for record in &report.identities_imported {
            let loaded = phone_secrets
                .load_identity(record.id)
                .expect("load")
                .expect("stored");
            assert_eq!(loaded.cert_hash(), record.cert_hash);
        }

        // The first server keeps its identity; no password came along.
        let servers = phone.servers().expect("servers");
        assert_eq!(servers[0].label, "Home");
        assert_eq!(
            servers[0].identity_id,
            Some(report.identities_imported[0].id)
        );
        assert_eq!(servers[1].port, 50000);
        assert_eq!(servers[1].identity_id, None);
        for server in &servers {
            assert_eq!(
                phone_secrets.server_password(server.id).expect("password"),
                None
            );
            assert!(
                phone_secrets
                    .access_tokens(server.id)
                    .expect("tokens")
                    .is_empty()
            );
        }
    }

    #[test]
    fn importing_twice_skips_everything_as_duplicates() {
        let (_dir, store, secrets, ids) = desktop(2);
        let payload = store
            .export_transfer_payload(&secrets, &ids, true)
            .expect("export");
        let (_phone_dir, phone, phone_secrets) = phone();
        phone
            .import_transfer_payload(&phone_secrets, &payload, 1)
            .expect("import");
        let again = phone
            .import_transfer_payload(&phone_secrets, &payload, 2)
            .expect("import");
        assert!(again.identities_imported.is_empty());
        assert!(again.servers_imported.is_empty());
        assert_eq!(
            again.identities_skipped,
            vec![
                SkippedIdentity {
                    label: "Label 0".to_owned(),
                    reason: SkipReason::Duplicate
                },
                SkippedIdentity {
                    label: "Label 1".to_owned(),
                    reason: SkipReason::Duplicate
                },
            ]
        );
        assert_eq!(again.servers_skipped.len(), 2);
        assert!(
            again
                .servers_skipped
                .iter()
                .all(|server| server.reason == SkipReason::Duplicate)
        );
        assert_eq!(phone.identities().expect("identities").len(), 2);
        assert_eq!(phone.servers().expect("servers").len(), 2);
    }

    #[test]
    fn existing_entries_and_the_existing_default_are_kept() {
        let (_dir, store, secrets, ids) = desktop(2);
        let payload = store
            .export_transfer_payload(&secrets, &ids, true)
            .expect("export");

        let (_phone_dir, phone, phone_secrets) = phone();
        let own = phone
            .add_identity(&NewIdentity::new(
                "Phone",
                IDENTITIES[2].name(),
                &IDENTITIES[2].cert_hash(),
                1,
            ))
            .expect("add identity");
        phone_secrets
            .save_identity(own.id, &IDENTITIES[2])
            .expect("save");
        // Same address as a transferred server but another user name: not an exact duplicate.
        phone
            .add_server(&NewServer::new("Home", "voice.example.org", "Someone"))
            .expect("add server");

        let report = phone
            .import_transfer_payload(&phone_secrets, &payload, 5)
            .expect("import");
        assert_eq!(report.identities_imported.len(), 2);
        assert_eq!(report.servers_imported.len(), 2);
        assert_eq!(
            phone
                .default_identity()
                .expect("query")
                .expect("default")
                .id,
            own.id
        );
        assert!(report.identities_imported.iter().all(|r| !r.is_default));
        let servers = phone.servers().expect("servers");
        assert_eq!(servers.len(), 3);
        assert_eq!(servers[0].username, "Someone");
    }

    #[test]
    fn a_server_whose_identity_is_not_on_the_phone_uses_the_default() {
        let (_dir, store, secrets, ids) = desktop(2);
        // Only the second identity is sent; the first server refers to the first one.
        let payload = store
            .export_transfer_payload(&secrets, &ids[1..], true)
            .expect("export");
        let (_phone_dir, phone, phone_secrets) = phone();
        let report = phone
            .import_transfer_payload(&phone_secrets, &payload, 5)
            .expect("import");
        assert_eq!(report.identities_imported.len(), 1);
        assert_eq!(report.servers_imported[0].identity_id, None);
    }

    #[test]
    fn unusable_entries_are_skipped_and_the_rest_is_imported() {
        let good = &IDENTITIES[0];
        let payload = TransferPayload {
            identities: vec![
                // The key of another certificate.
                TransferIdentity::new(
                    "Swapped",
                    good.cert_der(),
                    IDENTITIES[1].key_pkcs8_der(),
                    false,
                ),
                TransferIdentity::new("Garbage", b"not a certificate", b"not a key", false),
                // A label this app would not accept falls back to the certificate's name.
                TransferIdentity::new(
                    &"x".repeat(60),
                    good.cert_der(),
                    good.key_pkcs8_der(),
                    false,
                ),
            ],
            servers: Some(
                [
                    ("No address", "", 64738, "Zoe"),
                    ("Space", "bad host", 64738, "Zoe"),
                    ("No port", "example.org", 0, "Zoe"),
                    ("No user", "example.org", 64738, " "),
                    ("", " example.org ", 64738, "Zoe"),
                ]
                .into_iter()
                .map(|(name, address, port, username)| TransferServer {
                    name: name.to_owned(),
                    address: address.to_owned(),
                    port,
                    username: username.to_owned(),
                    identity_fingerprint: Some("zz".to_owned()),
                })
                .collect(),
            ),
        };
        let (_phone_dir, phone, phone_secrets) = phone();
        let report = phone
            .import_transfer_payload(&phone_secrets, &payload, 5)
            .expect("import");
        assert_eq!(
            report
                .identities_skipped
                .iter()
                .map(|skipped| (skipped.label.as_str(), skipped.reason))
                .collect::<Vec<_>>(),
            vec![
                ("Swapped", SkipReason::Invalid),
                ("Garbage", SkipReason::Invalid)
            ]
        );
        assert_eq!(report.identities_imported.len(), 1);
        assert_eq!(report.identities_imported[0].label, "Zoe");
        assert!(report.identities_imported[0].is_default);
        assert_eq!(report.servers_skipped.len(), 4);
        assert!(
            report
                .servers_skipped
                .iter()
                .all(|server| server.reason == SkipReason::Invalid)
        );
        assert_eq!(report.servers_imported.len(), 1);
        assert_eq!(report.servers_imported[0].label, "example.org");
        assert_eq!(report.servers_imported[0].address, "example.org");
        assert_eq!(report.servers_imported[0].identity_id, None);
        // Nothing of the skipped identities is left behind.
        assert_eq!(phone.identities().expect("identities").len(), 1);
    }

    /// How many QR codes typical selections need with real RSA-2048 identities and the
    /// default part size.
    #[test]
    fn typical_selections_need_few_codes() {
        let (_dir, store, secrets, ids) = desktop(3);
        // Two servers are in the profile already.
        for index in 0..18 {
            let name = format!("Community server {index}");
            let address = format!("mumble{index}.example-community.org");
            store
                .add_server(&NewServer::new(&name, &address, "Zoe"))
                .expect("add server");
        }
        let parts = |ids: &[IdentityId], servers: bool| {
            let payload = store
                .export_transfer_payload(&secrets, ids, servers)
                .expect("export");
            Transfer::create(&payload, 1_800_000_000)
                .expect("create")
                .parts()
                .len()
        };
        let counts = [
            parts(&ids[..1], false),
            parts(&ids, false),
            parts(&ids[..1], true),
        ];

        // 1 identity, 3 identities, 1 identity with 20 servers. Certificates differ in size by
        // a few bytes, so the counts may be one lower.
        assert!((4..=5).contains(&counts[0]), "{counts:?}");
        assert!((12..=13).contains(&counts[1]), "{counts:?}");
        assert!((5..=6).contains(&counts[2]), "{counts:?}");
    }

    /// AC-5.21 without the two apps: desktop profile → QR parts and code → phone profile.
    #[test]
    fn end_to_end_from_the_desktop_profile_to_the_phone_profile() {
        let (_dir, store, secrets, ids) = desktop(1);
        let payload = store
            .export_transfer_payload(&secrets, &ids, true)
            .expect("export");
        let now = 1_800_000_000;
        let transfer = Transfer::create(&payload, now).expect("create");

        let mut reader = TransferReader::new();
        for part in transfer.parts().iter().rev() {
            reader.add_part(part, now + 20).expect("add part");
        }
        let received = reader.open(transfer.code(), now + 40).expect("open");
        assert_eq!(received, payload);

        let (_phone_dir, phone, phone_secrets) = phone();
        let report = phone
            .import_transfer_payload(&phone_secrets, &received, 7)
            .expect("import");
        assert_eq!(report.identities_imported.len(), 1);
        assert_eq!(
            report.identities_imported[0].cert_hash,
            IDENTITIES[0].cert_hash()
        );
        assert_eq!(report.servers_imported.len(), 2);
        let home = &phone.servers().expect("servers")[0];
        assert_eq!(
            phone_secrets.server_password(home.id).expect("password"),
            None
        );
    }
}
