//! Local data that needs no connection: identity, saved servers and settings (FR-1.2, 1.3,
//! 1.7, 1.8, §7 settings). Secure-storage calls block, so they run on the blocking pool.

use std::path::PathBuf;
use std::sync::Arc;

use mumcord_audio::dsp::{DspControls, TalkMode as DspTalkMode};
use mumcord_store::{
    Identity, NewServer, SavedServer, SecretStore, ServerId, Settings, TalkMode, VoiceActivation,
};

use super::{Engine, EngineError, EngineResult, ErrorKind, lock, store_error};
use super::{IdentityView, PasswordChange, ServerFields, ServerView};

/// Applies the DSP switches and talk mode of `settings`.
pub(crate) fn apply_dsp(controls: &DspControls, settings: &Settings) {
    controls.set_echo_cancellation(settings.echo_cancellation);
    controls.set_noise_suppression(settings.noise_suppression);
    controls.set_auto_gain(settings.auto_gain);
    controls.set_music_quality(settings.music_quality);
    controls.set_manual_threshold(settings.voice_activation == VoiceActivation::Manual);
    controls.set_threshold_dbfs(settings.voice_threshold_dbfs as f32);
    controls.set_mode(match settings.talk_mode {
        TalkMode::VoiceActivation => DspTalkMode::VoiceActivation,
        TalkMode::PushToTalk => DspTalkMode::PushToTalk,
    });
}

fn identity_view(identity: &Identity) -> IdentityView {
    IdentityView {
        name: identity.name().to_owned(),
        cert_hash: identity.cert_hash(),
    }
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

    /// The identity, or `None` before the wizard created one.
    pub async fn identity(&self) -> EngineResult<Option<IdentityView>> {
        Ok(self.inner.identity().await?.as_ref().map(identity_view))
    }

    /// Creates a new self-signed identity and stores it in secure storage (FR-1.2).
    pub async fn identity_create(&self, name: String) -> EngineResult<IdentityView> {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return Err(EngineError::invalid("name is empty"));
        }
        let identity = self
            .blocking(move |secrets| {
                let identity = Identity::generate(&name)?;
                secrets.save_identity(&identity)?;
                Ok(identity)
            })
            .await?;
        let view = identity_view(&identity);
        *lock(&self.inner.identity) = Some(Some(identity));
        tracing::info!("identity created");
        Ok(view)
    }

    /// Imports a PKCS#12 file, replacing the current identity (FR-1.3).
    pub async fn identity_import(
        &self,
        path: PathBuf,
        password: Option<String>,
    ) -> EngineResult<IdentityView> {
        let data = tokio::fs::read(&path).await.map_err(|e| {
            EngineError::new(ErrorKind::Io, format!("reading the file failed: {e}"))
        })?;
        let identity = self
            .blocking(move |secrets| {
                let identity = Identity::from_pkcs12(&data, password.as_deref().unwrap_or(""))?;
                secrets.save_identity(&identity)?;
                Ok(identity)
            })
            .await?;
        let view = identity_view(&identity);
        *lock(&self.inner.identity) = Some(Some(identity));
        tracing::info!("identity imported");
        Ok(view)
    }

    /// Exports the identity as a password-protected PKCS#12 file.
    pub async fn identity_export(&self, path: PathBuf, password: String) -> EngineResult<()> {
        let identity = self
            .inner
            .identity()
            .await?
            .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "no identity yet"))?;
        let data = self
            .blocking(move |_| identity.to_pkcs12(&password))
            .await?;
        tokio::fs::write(&path, data)
            .await
            .map_err(|e| EngineError::new(ErrorKind::Io, format!("writing the file failed: {e}")))
    }

    /// Saved servers in server-bar order.
    pub async fn servers(&self) -> EngineResult<Vec<ServerView>> {
        let servers = lock(&self.inner.store).servers().map_err(store_error)?;
        let ids: Vec<ServerId> = servers.iter().map(|s| s.id).collect();
        let has_password = self
            .blocking(move |secrets| {
                ids.iter()
                    .map(|id| secrets.server_password(*id).map(|p| p.is_some()))
                    .collect::<mumcord_store::Result<Vec<bool>>>()
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

    /// Removes a server with password, trust decision, follows and history. Disconnects
    /// first if it is the current server.
    pub async fn server_remove(&self, id: ServerId) -> EngineResult<()> {
        if self.connection_state().server_id() == Some(id) {
            self.disconnect();
        }
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
        lock(&self.inner.store).settings().map_err(store_error)
    }

    /// Stores the settings and applies DSP switches, talk mode and devices immediately.
    /// Log level and the push-to-talk key are applied by the app shell.
    pub async fn update_settings(&self, settings: Settings) -> EngineResult<Settings> {
        let previous = {
            let store = lock(&self.inner.store);
            let previous = store.settings().map_err(store_error)?;
            store.save_settings(&settings).map_err(store_error)?;
            previous
        };
        apply_dsp(&self.inner.controls, &settings);
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
        if previous.positional != settings.positional {
            self.positional_configure(&settings.positional);
        }
        if settings.talk_mode != TalkMode::PushToTalk {
            self.inner.controls.set_ptt_pressed(false);
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
}
