//! Local data that needs no connection: identities, saved servers and settings (FR-1.2, 1.3,
//! 1.7, 1.8, §7 settings, issue #148). Secure-storage calls block, so they run on the blocking
//! pool.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use mumcord_audio::dsp::{DspControls, TalkMode as DspTalkMode};
use mumcord_protocol::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use mumcord_protocol::{ClientIdentity, ConnectConfig, ConnectError};
use mumcord_store::{
    Identity, IdentityId, IdentityRecord, NewIdentity, NewServer, SavedServer, SecretStore,
    ServerId, Settings, TalkMode, UnixMillis, VoiceActivation,
};

use super::channel_audio::effective_settings;
use super::machine::{CertCheck, Failure, check_cert, format_fingerprint};
use super::{
    CertificateState, IdentityView, PasswordChange, ServerFields, ServerTestInput, ServerView,
    TestConnectionView,
};
use super::{Engine, EngineError, EngineResult, ErrorKind, lock, store_error};

/// Upper bound for a test connection, from TCP connect to the ping answer.
const TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Applies the DSP switches and talk mode of `settings`.
pub(crate) fn apply_dsp(controls: &DspControls, settings: &Settings) {
    controls.set_echo_cancellation(settings.echo_cancellation);
    controls.set_noise_suppression(settings.noise_suppression);
    controls.set_auto_gain(settings.auto_gain);
    controls.set_input_gain_percent(settings.input_gain_percent);
    controls.set_music_quality(settings.music_quality);
    controls.set_low_latency(settings.low_latency);
    controls.set_bitrate_cap_kbps(settings.bitrate_kbps);
    controls.set_manual_threshold(settings.voice_activation == VoiceActivation::Manual);
    controls.set_threshold_dbfs(settings.voice_threshold_dbfs as f32);
    controls.set_mode(match settings.talk_mode {
        TalkMode::VoiceActivation => DspTalkMode::VoiceActivation,
        TalkMode::PushToTalk => DspTalkMode::PushToTalk,
    });
}

fn identity_view(record: IdentityRecord, server_ids: Vec<ServerId>) -> IdentityView {
    IdentityView {
        id: record.id,
        label: record.label,
        name: record.cert_name,
        cert_hash: record.cert_hash,
        added_at: record.added_at,
        is_default: record.is_default,
        server_ids,
    }
}

/// Maps a failed probe to the same error kinds a normal connect uses.
fn test_failure(err: ConnectError) -> EngineError {
    match super::convert::connect_failure(&err) {
        Failure::Transient(error) | Failure::Fatal(error) => error,
    }
}

fn now_ms() -> UnixMillis {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The identity registry entry `id` with the servers that use it directly.
fn identity_view_by_id(
    store: &mumcord_store::Store,
    id: IdentityId,
) -> mumcord_store::Result<Option<IdentityView>> {
    let Some(record) = store.identity(id)? else {
        return Ok(None);
    };
    let server_ids = store.identity_servers(id)?;
    Ok(Some(identity_view(record, server_ids)))
}

fn server_view(server: SavedServer, has_password: bool) -> ServerView {
    ServerView {
        id: server.id,
        label: server.label,
        address: server.address,
        port: server.port,
        username: server.username,
        has_password,
        save_history: server.save_history,
        always_connect_at_start: server.always_connect_at_start,
        was_voice_server_at_quit: server.was_voice_server_at_quit,
        notifications_muted: server.notifications_muted,
        group_mentions_muted: server.group_mentions_muted,
        theme_id: server.theme_id,
        identity_id: server.identity_id,
    }
}

/// Trims and checks user input for a saved server; an empty label falls back to the address.
fn validated(fields: &ServerFields) -> EngineResult<(String, String, String)> {
    let address = fields.address.trim();
    let username = fields.username.trim();
    if address.is_empty() || address.contains(char::is_whitespace) {
        return Err(EngineError::invalid("server address is empty or invalid"));
    }
    if username.is_empty() {
        return Err(EngineError::invalid("username is empty"));
    }
    if fields.port == 0 {
        return Err(EngineError::invalid("port 0 is not valid"));
    }
    let label = match fields.label.trim() {
        "" => address,
        label => label,
    };
    Ok((label.to_owned(), address.to_owned(), username.to_owned()))
}

impl Engine {
    async fn blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce(&SecretStore) -> mumcord_store::Result<T> + Send + 'static,
    ) -> EngineResult<T> {
        let secrets: Arc<SecretStore> = self.inner.secrets.clone();
        self.inner
            .rt
            .spawn_blocking(move || f(&secrets))
            .await
            .map_err(|e| EngineError::internal(e.to_string()))?
            .map_err(store_error)
    }

    /// The default identity, or `None` before the wizard created one.
    pub async fn identity(&self) -> EngineResult<Option<IdentityView>> {
        let store = self.inner.store.clone();
        self.blocking(move |_| {
            let store = lock(&store);
            match store.default_identity()? {
                Some(record) => Ok(Some(identity_view(
                    record.clone(),
                    store.identity_servers(record.id)?,
                ))),
                None => Ok(None),
            }
        })
        .await
    }

    /// Every identity: the default first, then by the date it was added (issue #148).
    pub async fn identity_list(&self) -> EngineResult<Vec<IdentityView>> {
        let store = self.inner.store.clone();
        self.blocking(move |_| {
            let store = lock(&store);
            store
                .identities()?
                .into_iter()
                .map(|record| {
                    let server_ids = store.identity_servers(record.id)?;
                    Ok(identity_view(record, server_ids))
                })
                .collect()
        })
        .await
    }

    /// Creates a new self-signed identity and stores it in secure storage (FR-1.2). The first
    /// identity becomes the default; the label defaults to the certificate name.
    pub async fn identity_create(
        &self,
        name: String,
        label: Option<String>,
    ) -> EngineResult<IdentityView> {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return Err(EngineError::invalid("name is empty"));
        }
        let store = self.inner.store.clone();
        let record = self
            .blocking(move |secrets| {
                let identity = Identity::generate(&name)?;
                let label = label.as_deref().unwrap_or(&name);
                let record = lock(&store).add_identity(&NewIdentity::new(
                    label,
                    identity.name(),
                    &identity.cert_hash(),
                    now_ms(),
                ))?;
                if let Err(e) = secrets.save_identity(record.id, &identity) {
                    discard(&store, record.id);
                    return Err(e);
                }
                Ok(record)
            })
            .await?;
        tracing::info!(identity = record.id, "identity created");
        Ok(identity_view(record, Vec::new()))
    }

    /// Imports a PKCS#12 file as an additional identity (FR-1.3, issue #148). A certificate
    /// that is already in the list is refused.
    pub async fn identity_import(
        &self,
        path: PathBuf,
        password: Option<String>,
        label: Option<String>,
    ) -> EngineResult<IdentityView> {
        let data = tokio::fs::read(&path).await.map_err(|e| {
            EngineError::new(ErrorKind::Io, format!("reading the file failed: {e}"))
        })?;
        self.identity_import_data(data, password, label).await
    }

    /// Imports PKCS#12 data as an additional identity; shared by the file import and the
    /// import from the official client (issue #66).
    pub(crate) async fn identity_import_data(
        &self,
        data: Vec<u8>,
        password: Option<String>,
        label: Option<String>,
    ) -> EngineResult<IdentityView> {
        let store = self.inner.store.clone();
        let record = self
            .blocking(move |secrets| {
                let identity = Identity::from_pkcs12(&data, password.as_deref().unwrap_or(""))?;
                let label = label.as_deref().unwrap_or_else(|| identity.name());
                let record = lock(&store).add_identity(&NewIdentity::new(
                    label,
                    identity.name(),
                    &identity.cert_hash(),
                    now_ms(),
                ))?;
                if let Err(e) = secrets.save_identity(record.id, &identity) {
                    discard(&store, record.id);
                    return Err(e);
                }
                Ok(record)
            })
            .await?;
        tracing::info!(identity = record.id, "identity imported");
        Ok(identity_view(record, Vec::new()))
    }

    /// Takes back an identity the import from the official client just added (issue #66).
    /// Unlike [`Engine::identity_delete`] this may remove the last identity: the user
    /// undoes the import, e.g. in the first-start wizard before any identity existed.
    pub(crate) async fn identity_undo_import(&self, id: IdentityId) -> EngineResult<()> {
        let store = self.inner.store.clone();
        self.blocking(move |secrets| {
            secrets.delete_identity(id)?;
            lock(&store).discard_identity(id)
        })
        .await
    }

    /// Changes the local label of an identity; the certificate is untouched (issue #148).
    pub async fn identity_rename(
        &self,
        id: IdentityId,
        label: String,
    ) -> EngineResult<IdentityView> {
        let store = self.inner.store.clone();
        self.blocking(move |_| {
            let store = lock(&store);
            let record = store.rename_identity(id, &label)?;
            identity_view_by_id(&store, record.id)?.ok_or(mumcord_store::Error::UnknownIdentity(id))
        })
        .await
    }

    /// Exports the identity as a password-protected PKCS#12 file (FR-1.3).
    pub async fn identity_export(
        &self,
        id: IdentityId,
        path: PathBuf,
        password: String,
    ) -> EngineResult<()> {
        if password.is_empty() {
            return Err(EngineError::invalid("the export password is empty"));
        }
        let store = self.inner.store.clone();
        let data = self
            .blocking(move |secrets| {
                let known = lock(&store).identity(id)?;
                let identity = secrets
                    .load_identity(id)?
                    .ok_or(mumcord_store::Error::UnknownIdentity(id))?;
                if known.is_none() {
                    return Err(mumcord_store::Error::UnknownIdentity(id));
                }
                identity.to_pkcs12(&password)
            })
            .await?;
        tokio::fs::write(&path, data)
            .await
            .map_err(|e| EngineError::new(ErrorKind::Io, format!("writing the file failed: {e}")))
    }

    /// Label of a saved server as the server bar shows it (the address when none was given).
    pub fn server_name(&self, server_id: ServerId) -> EngineResult<Option<String>> {
        let server = lock(&self.inner.store)
            .server(server_id)
            .map_err(store_error)?;
        Ok(server.map(|s| s.label))
    }

    /// Removes an identity with its key material. Servers that used it go back to the default;
    /// a connected one is disconnected first. The last identity cannot be removed.
    pub async fn identity_delete(&self, id: IdentityId) -> EngineResult<()> {
        let users = {
            let store = lock(&self.inner.store);
            store.identity_servers(id).map_err(store_error)?
        };
        for server in users {
            self.disconnect(server);
        }
        let store = self.inner.store.clone();
        self.blocking(move |secrets| lock(&store).remove_identity(id, secrets))
            .await?;
        tracing::info!(identity = id, "identity deleted");
        Ok(())
    }

    /// Makes an identity the default one; servers without their own identity follow it on
    /// their next connect.
    pub async fn identity_set_default(&self, id: IdentityId) -> EngineResult<Vec<IdentityView>> {
        {
            let store = lock(&self.inner.store);
            store.set_default_identity(id).map_err(store_error)?;
        }
        self.identity_list().await
    }

    /// One-shot login probe for the add/edit dialog (issue #151): connects with the values from
    /// the form, reads the server version and one ping, then closes again. Nothing is saved,
    /// no certificate decision is stored and no server connection is created.
    pub async fn server_test_connection(
        &self,
        input: ServerTestInput,
    ) -> EngineResult<TestConnectionView> {
        let address = input.address.trim().to_owned();
        if address.is_empty() || address.contains(char::is_whitespace) {
            return Err(EngineError::invalid("server address is empty or invalid"));
        }
        if input.port == 0 {
            return Err(EngineError::invalid("port 0 is not valid"));
        }
        let username = input.username.trim().to_owned();
        if username.is_empty() {
            return Err(EngineError::invalid("username is empty"));
        }
        let password = self.test_password(input.server_id, input.password).await?;
        let identity = match input.identity_id {
            Some(id) => self.inner.identity_by_id(id).await?,
            None => self.inner.default_identity().await?,
        };
        let identity =
            identity.ok_or_else(|| EngineError::new(ErrorKind::NotFound, "no identity yet"))?;
        let trusted = match input.server_id {
            Some(id) => lock(&self.inner.store)
                .trusted_cert(id)
                .map_err(store_error)?,
            None => None,
        };

        // The probe accepts the certificate to read the version; the trust decision itself is
        // reported, never stored (FR-3.43).
        let presented = Arc::new(std::sync::Mutex::new(None::<String>));
        let seen = presented.clone();
        let config = ConnectConfig {
            host: address.clone(),
            port: input.port,
            username,
            password,
            tokens: Vec::new(),
            identity: Some(ClientIdentity {
                chain: vec![CertificateDer::from(identity.cert_der().to_vec())],
                key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                    identity.key_pkcs8_der().to_vec(),
                )),
            }),
            verify_cert: Arc::new(move |cert| {
                *lock(&seen) = Some(format_fingerprint(&cert.sha256_hex));
                true
            }),
            login_timeout: TEST_TIMEOUT,
        };

        let probe = async {
            let connection = mumcord_protocol::connect(config)
                .await
                .map_err(test_failure)?;
            let info = super::convert::server_info_view(&connection.handle.state());
            let ping = connection.handle.ping().await;
            connection.handle.disconnect();
            Ok::<_, EngineError>((info.version, ping))
        };
        let (version, ping) = tokio::time::timeout(TEST_TIMEOUT, probe)
            .await
            .map_err(|_| {
                EngineError::new(
                    ErrorKind::ServerUnreachable,
                    "the server did not answer in time",
                )
            })??;

        let fingerprint = lock(&presented).clone().unwrap_or_default();
        let certificate = match check_cert(trusted.as_deref(), &fingerprint) {
            CertCheck::Trusted => CertificateState::Trusted,
            CertCheck::Unknown => CertificateState::Unknown,
            CertCheck::Changed { .. } => CertificateState::Changed,
        };
        Ok(TestConnectionView {
            ping_ms: ping.map_or(0, |rtt| u32::try_from(rtt.as_millis()).unwrap_or(u32::MAX)),
            version,
            fingerprint,
            certificate,
        })
    }

    /// Password a test connection uses: the typed one, or the saved one of the server.
    async fn test_password(
        &self,
        server_id: Option<ServerId>,
        change: PasswordChange,
    ) -> EngineResult<Option<String>> {
        match change {
            PasswordChange::Keep => match server_id {
                Some(id) => {
                    self.blocking(move |secrets| secrets.server_password(id))
                        .await
                }
                None => Ok(None),
            },
            PasswordChange::Clear => Ok(None),
            PasswordChange::Set(value) if value.is_empty() => Ok(None),
            PasswordChange::Set(value) => Ok(Some(value)),
        }
    }

    /// Saved servers in server-bar order.
    pub async fn servers(&self) -> EngineResult<Vec<ServerView>> {
        let servers = lock(&self.inner.store).servers().map_err(store_error)?;
        let ids: Vec<ServerId> = servers.iter().map(|s| s.id).collect();
        let has_password = self
            .blocking(move |secrets| {
                // An unreadable password (logged by the store) must not hide the whole server
                // list; the server shows as without password until the next start.
                Ok(ids
                    .iter()
                    .map(|id| secrets.server_password(*id).is_ok_and(|p| p.is_some()))
                    .collect::<Vec<bool>>())
            })
            .await?;
        Ok(servers
            .into_iter()
            .zip(has_password)
            .map(|(s, p)| server_view(s, p))
            .collect())
    }

    async fn apply_password(&self, id: ServerId, change: PasswordChange) -> EngineResult<bool> {
        self.blocking(move |secrets| match change {
            PasswordChange::Keep => secrets.server_password(id).map(|p| p.is_some()),
            PasswordChange::Clear => secrets.delete_server_password(id).map(|()| false),
            PasswordChange::Set(value) if value.is_empty() => {
                secrets.delete_server_password(id).map(|()| false)
            }
            PasswordChange::Set(value) => secrets.set_server_password(id, &value).map(|()| true),
        })
        .await
    }

    /// Adds a server at the end of the server bar (FR-1.7).
    pub async fn server_add(&self, fields: ServerFields) -> EngineResult<ServerView> {
        let (label, address, username) = validated(&fields)?;
        let mut new = NewServer::new(&label, &address, &username);
        new.port = fields.port;
        let saved = lock(&self.inner.store)
            .add_server(&new)
            .map_err(store_error)?;
        let has_password = self.apply_password(saved.id, fields.password).await?;
        Ok(server_view(saved, has_password))
    }

    pub async fn server_update(
        &self,
        id: ServerId,
        fields: ServerFields,
    ) -> EngineResult<ServerView> {
        let (label, address, username) = validated(&fields)?;
        let saved = {
            let store = lock(&self.inner.store);
            let mut saved = store
                .server(id)
                .map_err(store_error)?
                .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown server"))?;
            saved.label = label;
            saved.address = address;
            saved.port = fields.port;
            saved.username = username;
            store.update_server(&saved).map_err(store_error)?;
            saved
        };
        let has_password = self.apply_password(id, fields.password).await?;
        Ok(server_view(saved, has_password))
    }

    /// Marks a server "Always connect at start" (FR-2.7); marking more servers than the
    /// limit of connected servers allows is refused.
    pub async fn server_set_always_connect(
        &self,
        id: ServerId,
        enabled: bool,
    ) -> EngineResult<ServerView> {
        let saved = {
            let store = lock(&self.inner.store);
            let max = store.settings().map_err(store_error)?.max_connected_servers;
            let servers = store.servers().map_err(store_error)?;
            let marked = servers
                .iter()
                .filter(|s| s.always_connect_at_start && s.id != id)
                .count();
            let mut saved = servers
                .into_iter()
                .find(|s| s.id == id)
                .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown server"))?;
            if enabled && marked >= usize::from(max) {
                return Err(super::limit_error(max));
            }
            saved.always_connect_at_start = enabled;
            store.update_server(&saved).map_err(store_error)?;
            saved
        };
        let has_password = self.apply_password(id, PasswordChange::Keep).await?;
        Ok(server_view(saved, has_password))
    }

    /// Chooses the identity a server connects with (issue #148); `None` uses the default. The
    /// caller decides whether to reconnect.
    pub async fn server_set_identity(
        &self,
        id: ServerId,
        identity: Option<IdentityId>,
    ) -> EngineResult<ServerView> {
        let saved = {
            let store = lock(&self.inner.store);
            store
                .set_server_identity(id, identity)
                .map_err(store_error)?;
            store
                .server(id)
                .map_err(store_error)?
                .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown server"))?
        };
        let has_password = self.apply_password(id, PasswordChange::Keep).await?;
        Ok(server_view(saved, has_password))
    }

    /// Sets the theme shown with the server (FR-2.17); `None` or a blank id goes back to the
    /// app theme. The id isn't checked against the themes: an unknown one shows the app theme.
    pub async fn server_set_theme(
        &self,
        id: ServerId,
        theme_id: Option<String>,
    ) -> EngineResult<ServerView> {
        let saved = {
            let store = lock(&self.inner.store);
            let mut saved = store
                .server(id)
                .map_err(store_error)?
                .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown server"))?;
            saved.theme_id = theme_choice(theme_id);
            store.update_server(&saved).map_err(store_error)?;
            saved
        };
        let has_password = self.apply_password(id, PasswordChange::Keep).await?;
        Ok(server_view(saved, has_password))
    }

    /// Mutes or unmutes all notifications of the server, mentions included (FR-2.10).
    pub async fn server_set_notifications_muted(
        &self,
        id: ServerId,
        muted: bool,
    ) -> EngineResult<ServerView> {
        let saved = {
            let store = lock(&self.inner.store);
            let mut saved = store
                .server(id)
                .map_err(store_error)?
                .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown server"))?;
            saved.notifications_muted = muted;
            store.update_server(&saved).map_err(store_error)?;
            saved
        };
        let has_password = self.apply_password(id, PasswordChange::Keep).await?;
        Ok(server_view(saved, has_password))
    }

    /// Turns `@all` and `@channel` mentions on this server off or back on (issue #70).
    pub async fn server_set_group_mentions_muted(
        &self,
        id: ServerId,
        muted: bool,
    ) -> EngineResult<ServerView> {
        let saved = {
            let store = lock(&self.inner.store);
            let mut saved = store
                .server(id)
                .map_err(store_error)?
                .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown server"))?;
            saved.group_mentions_muted = muted;
            store.update_server(&saved).map_err(store_error)?;
            saved
        };
        let has_password = self.apply_password(id, PasswordChange::Keep).await?;
        Ok(server_view(saved, has_password))
    }

    /// Removes a server with password, trust decision, follows and history. Disconnects
    /// it first.
    pub async fn server_remove(&self, id: ServerId) -> EngineResult<()> {
        self.disconnect(id);
        let store = self.inner.store.clone();
        self.blocking(move |secrets| lock(&store).remove_server(id, secrets))
            .await
    }

    /// Sets the server-bar order; `ids` must list every saved server once.
    pub fn servers_reorder(&self, ids: &[ServerId]) -> EngineResult<()> {
        let store = lock(&self.inner.store);
        let mut known: Vec<ServerId> = store
            .servers()
            .map_err(store_error)?
            .iter()
            .map(|s| s.id)
            .collect();
        let mut given = ids.to_vec();
        known.sort_unstable();
        given.sort_unstable();
        if known != given {
            return Err(EngineError::invalid(
                "the order must list every server exactly once",
            ));
        }
        store.reorder_servers(ids).map_err(store_error)
    }

    /// Per-server "Save history" switch (FR-1.33).
    pub fn set_save_history(&self, id: ServerId, enabled: bool) -> EngineResult<()> {
        lock(&self.inner.store)
            .set_save_history(id, enabled)
            .map_err(store_error)
    }

    pub fn settings(&self) -> EngineResult<Settings> {
        self.inner.migrate_audio_devices();
        lock(&self.inner.store).settings().map_err(store_error)
    }

    /// Do Not Disturb is in effect or not (FR-3.111): notification sounds stay silent while it
    /// is. The app shell combines the switch with the followed system state.
    pub fn set_do_not_disturb(&self, on: bool) {
        self.inner.events.sounds().set_do_not_disturb(on);
    }

    /// Stores the settings and applies DSP switches, talk mode and devices immediately.
    /// Log level and the push-to-talk key are applied by the app shell.
    pub async fn update_settings(&self, settings: Settings) -> EngineResult<Settings> {
        self.inner.migrate_audio_devices();
        let previous = {
            let store = lock(&self.inner.store);
            let previous = store.settings().map_err(store_error)?;
            store.save_settings(&settings).map_err(store_error)?;
            previous
        };
        {
            // A channel audio profile in effect keeps overriding its values (issue #171).
            let mut core = lock(&self.inner.core);
            let profile = core.channel_audio.map(|a| a.profile);
            let before = effective_settings(&previous, profile.as_ref());
            let after = effective_settings(&settings, profile.as_ref());
            apply_dsp(&self.inner.controls, &after);
            if before.low_latency != after.low_latency {
                core.set_low_latency(after.low_latency);
            }
        }
        if previous.preview_cache_days != settings.preview_cache_days
            || previous.history_max_age_days != settings.history_max_age_days
        {
            self.run_maintenance_in_background();
        }
        if previous.now_playing != settings.now_playing {
            self.now_playing_set_enabled(settings.now_playing);
        }
        if previous.now_gaming != settings.now_gaming {
            self.now_gaming_set_enabled(settings.now_gaming);
        }
        if previous.allow_direct_connections != settings.allow_direct_connections {
            self.direct()
                .consent_changed(settings.allow_direct_connections);
        }
        if previous.positional != settings.positional {
            self.positional_configure(&settings.positional);
        }
        if settings.talk_mode != TalkMode::PushToTalk {
            self.inner.controls.set_ptt_pressed(false);
        }
        if previous.output_volume_percent != settings.output_volume_percent
            && let Some(audio) = &lock(&self.inner.core).audio
        {
            audio.set_output_volume_percent(settings.output_volume_percent);
        }
        if previous.input_device != settings.input_device
            || previous.output_device != settings.output_device
        {
            let inner = self.inner.clone();
            let (input, output) = (
                settings.input_device.clone(),
                settings.output_device.clone(),
            );
            self.inner
                .rt
                .spawn_blocking(move || {
                    let core = lock(&inner.core);
                    match &core.audio {
                        Some(audio) => audio.set_devices(input, output),
                        None => Ok(()),
                    }
                })
                .await
                .map_err(|e| EngineError::internal(e.to_string()))??;
        }
        Ok(settings)
    }
}

/// Moves the single identity of older versions into the identity list (issue #148): it becomes
/// "Default" and every server keeps connecting with it, because servers without their own
/// identity use the default. Runs at every start and does nothing once the list exists.
///
/// The new keychain items are written before the old ones are removed, so a failure at any
/// point leaves a state the next start can retry from.
pub(crate) fn bootstrap_identities(store: &mumcord_store::Store, secrets: &SecretStore) {
    let existing = match store.identities() {
        Ok(identities) => identities,
        Err(e) => {
            tracing::warn!(error = %e, "reading the identity list failed");
            return;
        }
    };
    let legacy = match secrets.legacy_identity() {
        Ok(legacy) => legacy,
        Err(e) => {
            tracing::warn!(error = %e, "the identity of an older version is not readable");
            return;
        }
    };
    let Some(identity) = legacy else {
        return;
    };
    if let Some(record) = existing
        .iter()
        .find(|record| record.cert_hash == identity.cert_hash())
    {
        // The list was already migrated; an earlier start only failed to remove the old items.
        tracing::info!(
            identity = record.id,
            "removing the identity items of an older version"
        );
        if let Err(e) = secrets.delete_legacy_identity() {
            tracing::warn!(error = %e, "removing the identity items of an older version failed");
        }
        return;
    }
    if !existing.is_empty() {
        tracing::warn!("an identity of an older version does not belong to any entry in the list");
        return;
    }
    let added = store.add_identity(&NewIdentity::new(
        "Default",
        identity.name(),
        &identity.cert_hash(),
        now_ms(),
    ));
    let record = match added {
        Ok(record) => record,
        Err(e) => {
            tracing::warn!(error = %e, "migrating the identity failed");
            return;
        }
    };
    if let Err(e) = secrets.save_identity(record.id, &identity) {
        tracing::warn!(error = %e, "storing the migrated identity failed");
        if let Err(undo) = store.discard_identity(record.id) {
            tracing::warn!(error = %undo, "undoing the migrated identity failed");
        }
        return;
    }
    if let Err(e) = secrets.delete_legacy_identity() {
        tracing::warn!(error = %e, "removing the identity items of an older version failed");
    }
    tracing::info!(
        identity = record.id,
        "the single identity became the default identity"
    );
}

/// Undoes an identity whose key material could not be stored, so the list never shows an
/// identity that cannot connect.
fn discard(store: &Arc<std::sync::Mutex<mumcord_store::Store>>, id: IdentityId) {
    if let Err(e) = lock(store).discard_identity(id) {
        tracing::warn!(error = %e, "undoing an identity without key material failed");
    }
}

fn theme_choice(theme_id: Option<String>) -> Option<String> {
    theme_id
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(label: &str, address: &str, username: &str) -> ServerFields {
        ServerFields {
            label: label.into(),
            address: address.into(),
            port: 64738,
            username: username.into(),
            password: PasswordChange::Keep,
        }
    }

    #[test]
    fn server_input_is_validated() {
        assert_eq!(
            validated(&fields("", " example.org ", " me ")).expect("valid"),
            ("example.org".into(), "example.org".into(), "me".into())
        );
        assert!(validated(&fields("x", "", "me")).is_err());
        assert!(validated(&fields("x", "a b", "me")).is_err());
        assert!(validated(&fields("x", "host", " ")).is_err());
        let mut zero = fields("x", "host", "me");
        zero.port = 0;
        assert!(validated(&zero).is_err());
    }

    async fn engine() -> (tempfile::TempDir, Engine) {
        let dir = tempfile::tempdir().expect("tempdir");
        let (engine, _rx) = Engine::new(
            dir.path(),
            crate::SecretStore::in_memory(),
            tokio::runtime::Handle::current(),
        )
        .expect("engine");
        (dir, engine)
    }

    #[tokio::test]
    async fn identities_are_listed_created_renamed_and_made_default() {
        let (_dir, engine) = engine().await;
        assert!(engine.identity_list().await.expect("empty list").is_empty());
        assert_eq!(engine.identity().await.expect("no default"), None);

        let main = engine
            .identity_create("Zoe".into(), Some("Main".into()))
            .await
            .expect("create main");
        assert!(main.is_default, "the first identity is the default");
        assert_eq!((main.label.as_str(), main.name.as_str()), ("Main", "Zoe"));
        let work = engine
            .identity_create("Zoe (Acme)".into(), None)
            .await
            .expect("create work");
        assert!(!work.is_default);
        assert_eq!(work.label, "Zoe (Acme)", "the label defaults to the name");

        let list = engine.identity_list().await.expect("list");
        assert_eq!(
            list.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
            ["Main", "Zoe (Acme)"]
        );

        let renamed = engine
            .identity_rename(work.id, "Work".into())
            .await
            .expect("rename");
        assert_eq!(renamed.label, "Work");
        assert_eq!(
            renamed.cert_hash, work.cert_hash,
            "rename keeps the certificate"
        );

        let list = engine.identity_set_default(work.id).await.expect("default");
        assert_eq!(list[0].id, work.id);
        assert!(list[0].is_default);
        assert_eq!(list.iter().filter(|i| i.is_default).count(), 1);
        assert_eq!(
            engine.identity().await.expect("default").map(|i| i.id),
            Some(work.id)
        );
    }

    #[tokio::test]
    async fn identities_are_exported_and_imported_and_duplicates_refused() {
        let (dir, engine) = engine().await;
        let main = engine
            .identity_create("Zoe".into(), Some("Main".into()))
            .await
            .expect("create");
        let file = dir.path().join("export.p12");
        assert!(
            engine
                .identity_export(main.id, file.clone(), String::new())
                .await
                .is_err(),
            "an empty export password is refused"
        );
        engine
            .identity_export(main.id, file.clone(), "s3cret".into())
            .await
            .expect("export");

        // A second Mumcord (fresh data folder) imports the file as its first identity.
        let other_dir = tempfile::tempdir().expect("tempdir");
        let (other, _other_rx) = Engine::new(
            other_dir.path(),
            crate::SecretStore::in_memory(),
            tokio::runtime::Handle::current(),
        )
        .expect("second engine");
        let imported = other
            .identity_import(file.clone(), Some("s3cret".into()), Some("Backup".into()))
            .await
            .expect("import");
        assert_eq!(imported.label, "Backup");
        assert_eq!(imported.cert_hash, main.cert_hash);
        assert!(
            imported.is_default,
            "the first identity becomes the default"
        );

        // The same certificate again is refused instead of adding a second entry.
        let again = other
            .identity_import(file, Some("s3cret".into()), None)
            .await
            .expect_err("duplicate");
        assert_eq!(again.kind, ErrorKind::InvalidInput);
        assert_eq!(other.identity_list().await.expect("list").len(), 1);
        let _ = other_dir;
    }

    #[tokio::test]
    async fn deleting_identities_moves_servers_to_the_default_and_keeps_one() {
        let (_dir, engine) = engine().await;
        let main = engine
            .identity_create("Zoe".into(), Some("Main".into()))
            .await
            .expect("create main");
        let work = engine
            .identity_create("Zoe (Acme)".into(), Some("Work".into()))
            .await
            .expect("create work");
        let server = engine
            .server_add(fields("a", "a.example", "me"))
            .await
            .expect("server");
        let chosen = engine
            .server_set_identity(server.id, Some(work.id))
            .await
            .expect("choose");
        assert_eq!(chosen.identity_id, Some(work.id));
        assert_eq!(
            engine.identity_list().await.expect("list")[1].server_ids,
            [server.id]
        );

        engine.identity_delete(work.id).await.expect("delete work");
        let servers = engine.servers().await.expect("servers");
        assert_eq!(servers[0].identity_id, None, "back on the default");
        assert_eq!(engine.identity_list().await.expect("list").len(), 1);

        let last = engine.identity_delete(main.id).await.expect_err("last");
        assert_eq!(last.kind, ErrorKind::InvalidInput);
        assert_eq!(engine.identity_list().await.expect("list").len(), 1);
    }

    #[tokio::test]
    async fn the_single_identity_of_older_versions_becomes_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let secrets = crate::SecretStore::in_memory();
        let old = Identity::generate("Old Tester").expect("generate");
        secrets.seed_legacy_identity(&old).expect("seed legacy");

        let (engine, _rx) = Engine::new(
            dir.path(),
            secrets.reopen(),
            tokio::runtime::Handle::current(),
        )
        .expect("engine");
        let list = engine.identity_list().await.expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].label, "Default");
        assert_eq!(list[0].name, "Old Tester");
        assert_eq!(list[0].cert_hash, old.cert_hash());
        assert!(list[0].is_default);
        // The old items are gone and the certificate is still readable for connecting.
        assert_eq!(secrets.legacy_identity().expect("gone"), None);
        assert_eq!(
            engine
                .identity()
                .await
                .expect("default")
                .map(|i| i.cert_hash),
            Some(old.cert_hash())
        );

        // A second start does not add a second entry.
        let (again, _rx) = Engine::new(
            dir.path(),
            secrets.reopen(),
            tokio::runtime::Handle::current(),
        )
        .expect("engine again");
        assert_eq!(again.identity_list().await.expect("list").len(), 1);
    }

    #[test]
    fn blank_theme_choice_means_app_theme() {
        assert_eq!(theme_choice(None), None);
        assert_eq!(theme_choice(Some("  ".into())), None);
        assert_eq!(
            theme_choice(Some(" preset:ocean ".into())),
            Some("preset:ocean".into())
        );
    }

    #[tokio::test]
    async fn notification_mute_is_saved_per_server() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (engine, _rx) = Engine::new(
            dir.path(),
            crate::SecretStore::in_memory(),
            tokio::runtime::Handle::current(),
        )
        .expect("engine");
        let muted = engine
            .server_add(fields("a", "a.example", "me"))
            .await
            .expect("server");
        let other = engine
            .server_add(fields("b", "b.example", "me"))
            .await
            .expect("server");

        assert_eq!(
            engine.server_name(other.id).expect("name"),
            Some("b".into())
        );
        assert_eq!(engine.server_name(other.id + 100).expect("name"), None);

        let view = engine
            .server_set_notifications_muted(muted.id, true)
            .await
            .expect("mute");
        assert!(view.notifications_muted);
        let servers = engine.servers().await.expect("servers");
        let muted_of = |id| {
            servers
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.notifications_muted)
        };
        assert_eq!(muted_of(muted.id), Some(true));
        assert_eq!(muted_of(other.id), Some(false));

        let view = engine
            .server_set_notifications_muted(muted.id, false)
            .await
            .expect("unmute");
        assert!(!view.notifications_muted);
    }

    #[tokio::test]
    async fn group_mentions_mute_is_saved_per_server() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (engine, _rx) = Engine::new(
            dir.path(),
            crate::SecretStore::in_memory(),
            tokio::runtime::Handle::current(),
        )
        .expect("engine");
        let muted = engine
            .server_add(fields("a", "a.example", "me"))
            .await
            .expect("server");
        let other = engine
            .server_add(fields("b", "b.example", "me"))
            .await
            .expect("server");
        assert!(!muted.group_mentions_muted);

        let view = engine
            .server_set_group_mentions_muted(muted.id, true)
            .await
            .expect("mute");
        assert!(view.group_mentions_muted && !view.notifications_muted);
        let servers = engine.servers().await.expect("servers");
        let muted_of = |id| {
            servers
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.group_mentions_muted)
        };
        assert_eq!(muted_of(muted.id), Some(true));
        assert_eq!(muted_of(other.id), Some(false));

        let view = engine
            .server_set_group_mentions_muted(muted.id, false)
            .await
            .expect("unmute");
        assert!(!view.group_mentions_muted);
    }
}
