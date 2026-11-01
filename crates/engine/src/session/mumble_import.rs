//! Import from the official Mumble client (FR-3.93, FR-3.94, issue #66): a preview of what
//! was found, then the import of the chosen parts, and an undo of the last import.
//!
//! Server passwords and the certificate go straight from Mumble's files into the OS secure
//! storage; they never reach the UI or a log. Settings (devices, push-to-talk key, volume)
//! are only proposed here and saved by the UI through the normal settings update.

use std::collections::HashSet;

use mumcord_store::{Identity, IdentityId, ServerId, TalkMode};

use super::{
    Engine, EngineError, EngineResult, ErrorKind, IdentityView, PasswordChange, ServerFields,
    ServerView, lock,
};
use crate::mumble_import::{
    self, Favourite, KeyIssue, MumbleData, MumbleEnv, MumbleVersion, Platform, Transmit,
};

/// A favourite as the preview shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportFavouriteView {
    pub label: String,
    pub host: String,
    pub port: u16,
    /// Empty when Mumble has none; the import then uses the fallback name.
    pub username: String,
    pub has_password: bool,
    /// The server is already in Mumcord and is skipped.
    pub duplicate: bool,
}

/// The certificate found in Mumble's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportIdentityView {
    /// Common name; empty while the certificate is locked by a password.
    pub name: String,
    /// End of validity, Unix milliseconds; `None` while locked or unreadable.
    pub valid_until: Option<i64>,
    /// The certificate is protected by a password the user has to type.
    pub needs_password: bool,
    /// The same certificate is already one of Mumcord's identities.
    pub already_imported: bool,
}

/// A device Mumble used that exists on this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDevice {
    pub id: String,
    pub name: String,
}

/// What an installed official client offers for import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MumbleImportPreview {
    pub version: MumbleVersion,
    pub favourites: Vec<ImportFavouriteView>,
    /// The favourites database exists but could not be read.
    pub favourites_failed: bool,
    pub identity: Option<ImportIdentityView>,
    pub input_device: Option<ImportDevice>,
    pub output_device: Option<ImportDevice>,
    /// Mumble had a device set that this computer does not have (now).
    pub devices_missing: bool,
    pub ptt_key: Option<String>,
    /// Why Mumble's push-to-talk shortcut cannot be used, when it cannot.
    pub ptt_issue: Option<KeyIssue>,
    /// Mumble's way of talking, when Mumcord has the same.
    pub talk_mode: Option<TalkMode>,
    pub output_volume_percent: Option<u16>,
}

/// What the user chose in the preview. Favourites are named by host and port, so a change
/// in Mumble's list between preview and import cannot import a different server.
#[derive(Clone, Default)]
pub struct MumbleImportChoice {
    pub favourites: Vec<(String, u16)>,
    pub passwords: bool,
    pub identity: bool,
    /// Only for a certificate that needs one. Never logged.
    pub certificate_password: Option<String>,
}

impl std::fmt::Debug for MumbleImportChoice {
    /// Leaves out the certificate password.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MumbleImportChoice")
            .field("favourites", &self.favourites)
            .field("passwords", &self.passwords)
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

/// What the import did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MumbleImportResult {
    pub added: Vec<ServerView>,
    /// Labels of chosen favourites that were already in Mumcord.
    pub skipped: Vec<String>,
    pub passwords_saved: u32,
    pub identity: Option<IdentityView>,
    /// The certificate was chosen but is already one of Mumcord's identities.
    pub identity_skipped: bool,
    /// End of validity of the imported certificate, Unix milliseconds.
    pub identity_valid_until: Option<i64>,
}

/// What the last import added, so it can be undone.
#[derive(Debug, Default)]
pub(crate) struct LastImport {
    servers: Vec<ServerId>,
    identity: Option<IdentityId>,
}

fn read_data(env: MumbleEnv) -> Option<MumbleData> {
    mumble_import::read(&env)
}

/// End of validity of a DER certificate, Unix milliseconds.
fn valid_until(cert_der: &[u8]) -> Option<i64> {
    let (_, cert) = x509_parser::parse_x509_certificate(cert_der).ok()?;
    cert.validity().not_after.timestamp().checked_mul(1000)
}

fn talk_mode(transmit: Option<Transmit>) -> Option<TalkMode> {
    match transmit? {
        Transmit::PushToTalk => Some(TalkMode::PushToTalk),
        Transmit::VoiceActivation => Some(TalkMode::VoiceActivation),
        // Mumcord has no "always on"; voice activation stays the default.
        Transmit::Continuous => None,
    }
}

fn volume_percent(volume: Option<f64>) -> Option<u16> {
    let v = volume?;
    if !v.is_finite() || v < 0.0 {
        return None;
    }
    // Mumble goes above 100 % (amplification); Mumcord's master volume stops at 100 %.
    let percent = (v * 100.0).round().min(100.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "0..=100"
    )]
    Some(percent as u16)
}

/// The user name for an imported server: Mumble's, else its last used, else the
/// certificate's name.
fn username_for(favourite: &Favourite, fallback: Option<&str>) -> Option<String> {
    Some(favourite.username.clone())
        .filter(|u| !u.is_empty())
        .or_else(|| fallback.map(str::to_owned))
        .filter(|u| !u.trim().is_empty())
}

impl Engine {
    async fn existing_servers(&self) -> EngineResult<HashSet<(String, u16)>> {
        Ok(self
            .servers()
            .await?
            .iter()
            .map(|s| mumble_import::server_key(&s.address, s.port))
            .collect())
    }

    async fn read_mumble(&self, env: MumbleEnv) -> EngineResult<Option<MumbleData>> {
        self.inner
            .rt
            .spawn_blocking(move || read_data(env))
            .await
            .map_err(|e| EngineError::internal(e.to_string()))
    }

    /// Looks for an installed official client; `None` when there is none (FR-3.93).
    pub async fn mumble_import_preview(
        &self,
        env: MumbleEnv,
    ) -> EngineResult<Option<MumbleImportPreview>> {
        let platform = env.platform.unwrap_or_else(Platform::current);
        let Some(data) = self.read_mumble(env).await? else {
            return Ok(None);
        };
        let existing = self.existing_servers().await?;
        let favourites = mumble_import::unique_favourites(data.favourites)
            .into_iter()
            .map(|f| ImportFavouriteView {
                duplicate: existing.contains(&mumble_import::server_key(&f.host, f.port)),
                label: if f.name.is_empty() {
                    f.host.clone()
                } else {
                    f.name.clone()
                },
                has_password: f.password.is_some(),
                host: f.host,
                port: f.port,
                username: f.username,
            })
            .collect();

        let identity = match &data.settings.certificate {
            Some(p12) => {
                let hashes: HashSet<String> = self
                    .identity_list()
                    .await?
                    .into_iter()
                    .map(|i| i.cert_hash)
                    .collect();
                Some(match Identity::from_pkcs12(p12, "") {
                    Ok(identity) => ImportIdentityView {
                        name: identity.name().to_owned(),
                        valid_until: valid_until(identity.cert_der()),
                        needs_password: false,
                        already_imported: hashes.contains(&identity.cert_hash()),
                    },
                    Err(_) => ImportIdentityView {
                        name: String::new(),
                        valid_until: None,
                        needs_password: true,
                        already_imported: false,
                    },
                })
            }
            None => None,
        };

        let devices = self.audio_devices().await.unwrap_or_default();
        let pick = |stored: &[String], list: &[super::AudioDeviceView]| {
            mumble_import::match_device(stored, list, |d| &d.id, |d| &d.name).map(|d| {
                ImportDevice {
                    id: d.id.clone(),
                    name: d.name.clone(),
                }
            })
        };
        let input_device = pick(&data.settings.input_devices, &devices.inputs);
        let output_device = pick(&data.settings.output_devices, &devices.outputs);
        let devices_missing = (input_device.is_none() && !data.settings.input_devices.is_empty())
            || (output_device.is_none() && !data.settings.output_devices.is_empty());

        let (ptt_key, ptt_issue) = match &data.settings.ptt_buttons {
            Some(buttons) => match mumble_import::ptt_key(platform, buttons) {
                Ok(key) => (Some(key), None),
                Err(issue) => (None, Some(issue)),
            },
            None => (None, None),
        };

        tracing::info!(
            version = ?data.version,
            favourites_failed = data.favourites_failed,
            "found the official Mumble client"
        );
        Ok(Some(MumbleImportPreview {
            version: data.version,
            favourites,
            favourites_failed: data.favourites_failed,
            identity,
            input_device,
            output_device,
            devices_missing,
            ptt_key,
            ptt_issue,
            talk_mode: talk_mode(data.settings.transmit),
            output_volume_percent: volume_percent(data.settings.volume),
        }))
    }

    /// Imports the chosen favourites, their passwords and the certificate (FR-3.93).
    /// Servers already in Mumcord are skipped, never overwritten (FR-3.94). A wrong
    /// certificate password fails before anything is written.
    pub async fn mumble_import_apply(
        &self,
        env: MumbleEnv,
        choice: MumbleImportChoice,
    ) -> EngineResult<MumbleImportResult> {
        let data = self
            .read_mumble(env)
            .await?
            .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "Mumble data not found"))?;

        let certificate = match (&data.settings.certificate, choice.identity) {
            (Some(p12), true) => {
                let password = choice.certificate_password.clone().unwrap_or_default();
                let identity = Identity::from_pkcs12(p12, &password).map_err(|_| {
                    EngineError::new(
                        ErrorKind::WrongPassword,
                        "the certificate could not be opened",
                    )
                })?;
                Some((p12.clone(), identity))
            }
            _ => None,
        };

        let mut result = MumbleImportResult {
            added: Vec::new(),
            skipped: Vec::new(),
            passwords_saved: 0,
            identity: None,
            identity_skipped: false,
            identity_valid_until: None,
        };
        let mut record = LastImport::default();

        if let Some((p12, identity)) = certificate {
            let known = self
                .identity_list()
                .await?
                .into_iter()
                .any(|i| i.cert_hash == identity.cert_hash());
            if known {
                result.identity_skipped = true;
            } else {
                let view = self
                    .identity_import_data(p12, choice.certificate_password.clone(), None, "")
                    .await?;
                result.identity_valid_until = valid_until(identity.cert_der());
                record.identity = Some(view.id);
                result.identity = Some(view);
            }
        }

        let fallback_name = data
            .settings
            .username
            .clone()
            .or_else(|| result.identity.as_ref().map(|i| i.name.clone()));
        let wanted: HashSet<(String, u16)> = choice
            .favourites
            .iter()
            .map(|(host, port)| mumble_import::server_key(host, *port))
            .collect();
        let mut existing = self.existing_servers().await?;
        for favourite in mumble_import::unique_favourites(data.favourites) {
            let key = mumble_import::server_key(&favourite.host, favourite.port);
            if !wanted.contains(&key) {
                continue;
            }
            let label = if favourite.name.is_empty() {
                favourite.host.clone()
            } else {
                favourite.name.clone()
            };
            if existing.contains(&key) {
                result.skipped.push(label);
                continue;
            }
            let Some(username) = username_for(&favourite, fallback_name.as_deref()) else {
                result.skipped.push(label);
                continue;
            };
            let password = match (&favourite.password, choice.passwords) {
                (Some(p), true) => PasswordChange::Set(p.clone()),
                _ => PasswordChange::Keep,
            };
            let mut server = self
                .server_add(ServerFields {
                    label,
                    address: favourite.host.clone(),
                    port: favourite.port,
                    username,
                    password,
                })
                .await?;
            record.servers.push(server.id);
            existing.insert(key);
            if server.has_password {
                result.passwords_saved += 1;
            }
            // The servers the certificate is registered on are most likely Mumble's
            // favourites, so they connect with it even when it is not the default.
            if let Some(identity) = &result.identity
                && !identity.is_default
            {
                server = self
                    .server_set_identity(server.id, Some(identity.id))
                    .await?;
            }
            result.added.push(server);
        }

        tracing::info!(
            servers = result.added.len(),
            skipped = result.skipped.len(),
            passwords = result.passwords_saved,
            identity = result.identity.is_some(),
            "imported from the official Mumble client"
        );
        *lock(&self.inner.mumble_import) = Some(record);
        Ok(result)
    }

    /// Removes the servers and the identity the last import added. Settings the UI saved
    /// are restored by the UI.
    pub async fn mumble_import_undo(&self) -> EngineResult<()> {
        let Some(record) = lock(&self.inner.mumble_import).take() else {
            return Err(EngineError::new(ErrorKind::NotFound, "nothing to undo"));
        };
        for id in record.servers {
            self.server_remove(id).await?;
        }
        if let Some(id) = record.identity {
            self.identity_undo_import(id).await?;
        }
        tracing::info!("undid the import from the official Mumble client");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_and_talk_mode_mapping() {
        assert_eq!(volume_percent(Some(0.8)), Some(80));
        assert_eq!(volume_percent(Some(1.6)), Some(100));
        assert_eq!(volume_percent(Some(f64::NAN)), None);
        assert_eq!(volume_percent(None), None);
        assert_eq!(
            talk_mode(Some(Transmit::PushToTalk)),
            Some(TalkMode::PushToTalk)
        );
        assert_eq!(talk_mode(Some(Transmit::Continuous)), None);
    }

    #[test]
    fn username_falls_back() {
        let mut favourite = Favourite {
            name: "A".into(),
            host: "a.example".into(),
            port: 1,
            username: String::new(),
            password: None,
        };
        assert_eq!(username_for(&favourite, Some("Zoe")), Some("Zoe".into()));
        assert_eq!(username_for(&favourite, None), None);
        favourite.username = "max".into();
        assert_eq!(username_for(&favourite, Some("Zoe")), Some("max".into()));
    }

    fn fixture(root: &std::path::Path, p12: &[u8]) -> MumbleEnv {
        use base64::Engine as _;
        let env = crate::mumble_import::tests::linux_env(root);
        let config = root.join("config/Mumble/Mumble");
        let data = root.join("data/Mumble/Mumble");
        std::fs::create_dir_all(&config).expect("mkdir");
        std::fs::create_dir_all(&data).expect("mkdir");
        let settings = serde_json::json!({
            "certificate": base64::engine::general_purpose::STANDARD.encode(p12),
            "audio": {"transmit_mode": "PTT", "volume": 0.5},
        });
        std::fs::write(config.join("mumble_settings.json"), settings.to_string()).expect("write");
        crate::mumble_import::favourites_fixture(
            &data.join("mumble.sqlite"),
            &[
                (
                    "Nordic Gamers",
                    "nordic.example",
                    64738,
                    "zoe",
                    Some("hunter2"),
                ),
                ("Quiet Library", "library.example", 64738, "zoe", None),
                ("Retro LAN", "retro.example", 64739, "", None),
            ],
        );
        env
    }

    async fn engine(dir: &std::path::Path) -> Engine {
        let (engine, _rx) = Engine::new(
            dir,
            crate::SecretStore::in_memory(),
            tokio::runtime::Handle::current(),
        )
        .expect("engine");
        engine
    }

    #[tokio::test]
    async fn preview_import_and_undo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p12 = Identity::generate("Zoe")
            .expect("identity")
            .to_pkcs12("")
            .expect("p12");
        let env = fixture(dir.path(), &p12);
        let engine = engine(&dir.path().join("mumcord")).await;
        engine
            .server_add(ServerFields {
                label: "Library".into(),
                address: "LIBRARY.example".into(),
                port: 64738,
                username: "me".into(),
                password: PasswordChange::Keep,
            })
            .await
            .expect("server");

        let preview = engine
            .mumble_import_preview(env.clone())
            .await
            .expect("preview")
            .expect("found");
        assert_eq!(preview.version, MumbleVersion::Json);
        assert_eq!(preview.favourites.len(), 3);
        let library = preview
            .favourites
            .iter()
            .find(|f| f.host == "library.example")
            .expect("library");
        assert!(library.duplicate);
        let identity = preview.identity.clone().expect("identity");
        assert_eq!(identity.name, "Zoe");
        assert!(!identity.needs_password && !identity.already_imported);
        assert!(identity.valid_until.is_some());
        assert_eq!(preview.talk_mode, Some(TalkMode::PushToTalk));
        assert_eq!(preview.output_volume_percent, Some(50));

        let choice = MumbleImportChoice {
            favourites: preview
                .favourites
                .iter()
                .map(|f| (f.host.clone(), f.port))
                .collect(),
            passwords: true,
            identity: true,
            certificate_password: None,
        };
        let result = engine
            .mumble_import_apply(env.clone(), choice.clone())
            .await
            .expect("import");
        assert_eq!(result.added.len(), 2);
        assert_eq!(result.skipped, vec!["Quiet Library".to_owned()]);
        assert_eq!(result.passwords_saved, 1);
        let imported = result.identity.clone().expect("identity imported");
        // Mumble's last user name is empty, so the certificate's name fills in.
        let retro = result
            .added
            .iter()
            .find(|s| s.address == "retro.example")
            .expect("retro");
        assert_eq!(retro.username, "Zoe");
        // The first identity is the default, so the servers need no explicit choice.
        assert!(imported.is_default);
        assert_eq!(retro.identity_id, None);
        let nordic = result
            .added
            .iter()
            .find(|s| s.address == "nordic.example")
            .expect("nordic");
        assert_eq!(
            engine
                .inner
                .secrets
                .server_password(nordic.id)
                .expect("secret"),
            Some("hunter2".into())
        );

        // A second import skips everything that is already there.
        let again = engine
            .mumble_import_apply(env.clone(), choice)
            .await
            .expect("again");
        assert!(again.added.is_empty());
        assert!(again.identity_skipped);

        engine
            .mumble_import_undo()
            .await
            .expect("undo of the second import");
        assert_eq!(engine.servers().await.expect("servers").len(), 3);
        assert!(engine.mumble_import_undo().await.is_err());
    }

    #[tokio::test]
    async fn undo_removes_servers_and_the_only_identity() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p12 = Identity::generate("Zoe")
            .expect("identity")
            .to_pkcs12("")
            .expect("p12");
        let env = fixture(dir.path(), &p12);
        let engine = engine(&dir.path().join("mumcord")).await;
        let result = engine
            .mumble_import_apply(
                env,
                MumbleImportChoice {
                    favourites: vec![("nordic.example".into(), 64738)],
                    passwords: true,
                    identity: true,
                    certificate_password: None,
                },
            )
            .await
            .expect("import");
        let identity = result.identity.expect("identity");
        assert!(identity.is_default);
        engine.mumble_import_undo().await.expect("undo");
        assert!(engine.servers().await.expect("servers").is_empty());
        assert!(engine.identity_list().await.expect("identities").is_empty());
        assert_eq!(
            engine
                .inner
                .secrets
                .load_identity(identity.id)
                .expect("load"),
            None
        );
    }

    #[tokio::test]
    async fn a_locked_certificate_needs_its_password() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p12 = Identity::generate("Zoe")
            .expect("identity")
            .to_pkcs12("pw")
            .expect("p12");
        let env = fixture(dir.path(), &p12);
        let engine = engine(&dir.path().join("mumcord")).await;
        let preview = engine
            .mumble_import_preview(env.clone())
            .await
            .expect("preview")
            .expect("found");
        assert!(preview.identity.expect("identity").needs_password);
        engine
            .identity_create("Max".into(), None)
            .await
            .expect("existing identity");
        let choice = |password: &str| MumbleImportChoice {
            favourites: vec![("nordic.example".into(), 64738)],
            passwords: false,
            identity: true,
            certificate_password: Some(password.into()),
        };
        let err = engine
            .mumble_import_apply(env.clone(), choice("wrong"))
            .await
            .expect_err("wrong password");
        assert_eq!(err.kind, ErrorKind::WrongPassword);
        // Nothing was written.
        assert!(engine.servers().await.expect("servers").is_empty());
        let result = engine
            .mumble_import_apply(env, choice("pw"))
            .await
            .expect("import");
        let imported = result.identity.expect("identity");
        assert_eq!(imported.name, "Zoe");
        assert!(!imported.is_default);
        assert_eq!(result.passwords_saved, 0);
        assert!(!result.added[0].has_password);
        // Not the default, so the imported server is set to connect with it.
        assert_eq!(result.added[0].identity_id, Some(imported.id));
    }
}
