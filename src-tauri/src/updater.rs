//! In-app updater (issue #168, FR-3.144–FR-3.148). Checks GitHub on start and every
//! `update_interval_hours`, but only while the user allowed it (`update_check`; "Off" contacts
//! nothing). An offered bundle is downloaded from the release's own download folder, checked
//! against the updater's minisign key and the release's `SHA256SUMS`, and installed through
//! Tauri's updater plugin. Restarting is always the user's decision.

mod install_kind;
mod release;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mumcord_engine::store::Consent;
use mumcord_engine::web::fetch::{AddressPolicy, FetchError, Fetcher, Timeouts};
use mumcord_engine::{Engine, EngineResult};
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;
use tauri_specta::Event;

use crate::commands::updates::{
    InstallKind, UpdateError, UpdateOffer, UpdatePhase, UpdateProgress, UpdateStatus,
    UpdateStatusChanged,
};
use release::Rejected;

const RELEASES_API: &str = "https://api.github.com/repos/Mumcord/mumcord/releases?per_page=30";
const RELEASES: &str = "https://github.com/Mumcord/mumcord/releases";
/// Tests and E2E point the updater at a local stand-in for [`RELEASES`] and the API: it serves
/// `<url>/releases` and `<url>/download/v<version>/…`, and only loopback may be contacted.
const URL_OVERRIDE_ENV: &str = "MUMCORD_UPDATE_URL";
/// E2E builds (`test-backend`) verify with a throwaway key made by the test run.
#[cfg(feature = "test-backend")]
const PUBKEY_OVERRIDE_ENV: &str = "MUMCORD_TEST_UPDATER_PUBKEY";
const HOUR_MS: i64 = 60 * 60 * 1000;
/// How often the background loop looks at the clock; the interval itself is the user's.
const LOOP_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// The release list holds the notes of the last releases; a few hundred KB at most.
const LIST_LIMIT: usize = 4 * 1024 * 1024;
const SMALL_LIMIT: usize = 256 * 1024;
/// Bundles are 100–250 MB today; the plugin installs from memory.
const BUNDLE_LIMIT: usize = 1024 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60 * 60);
/// Progress events are throttled; the bar does not need more.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Whether `hours` passed since the last check. A clock that went back counts as due.
fn is_due(last: Option<i64>, now: i64, hours: u32) -> bool {
    last.is_none_or(|last| now < last || now - last >= i64::from(hours) * HOUR_MS)
}

/// The version compared against releases; `None` for development builds, which never update.
fn current_version() -> Option<&'static str> {
    #[cfg(feature = "test-backend")]
    if overridden_base().is_some() {
        // E2E runs a development build that stands in for the release it is based on.
        return Some(crate::version::PACKAGE);
    }
    crate::version::is_release().then_some(crate::version::DISPLAY)
}

fn overridden_base() -> Option<String> {
    std::env::var(URL_OVERRIDE_ENV)
        .ok()
        .filter(|url| !url.is_empty())
        .map(|url| url.trim_end_matches('/').to_string())
}

/// Where to ask, and which addresses may be contacted.
struct Endpoints {
    list: String,
    /// The download folder of a release, without the version.
    downloads: String,
    policy: AddressPolicy,
}

impl Endpoints {
    fn get() -> Self {
        match overridden_base() {
            Some(base) => Self {
                list: format!("{base}/releases"),
                downloads: format!("{base}/download/"),
                policy: AddressPolicy::Only(Arc::new(vec![
                    IpAddr::V4(Ipv4Addr::LOCALHOST),
                    IpAddr::V6(Ipv6Addr::LOCALHOST),
                ])),
            },
            None => Self {
                list: RELEASES_API.to_string(),
                downloads: format!("{RELEASES}/download/"),
                policy: AddressPolicy::PublicOnly,
            },
        }
    }

    fn release_folder(&self, version: &str) -> String {
        format!("{}v{version}/", self.downloads)
    }
}

/// The updater's public key: `plugins.updater.pubkey` in `tauri.conf.json`. Empty until the
/// maintainer created the key (docs/release-signing.md); without it the app only notifies.
fn pubkey(app: &AppHandle) -> String {
    #[cfg(feature = "test-backend")]
    if let Ok(key) = std::env::var(PUBKEY_OVERRIDE_ENV) {
        return key;
    }
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|c| c.get("pubkey"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// A verified bundle waiting for the restart.
struct Prepared {
    bytes: Vec<u8>,
    manifest: serde_json::Value,
    target: String,
    /// Already written in place (macOS, AppImage); Windows runs the installer on restart.
    installed: bool,
}

struct State {
    phase: UpdatePhase,
    offer: Option<UpdateOffer>,
    progress: Option<UpdateProgress>,
    error: Option<UpdateError>,
    download: Option<tauri::async_runtime::JoinHandle<()>>,
    prepared: Option<Prepared>,
}

struct Inner {
    app: AppHandle,
    engine: Engine,
    install: InstallKind,
    pubkey: String,
    state: Mutex<State>,
}

/// The updater, managed as Tauri state.
#[derive(Clone)]
pub struct Updater(Arc<Inner>);

impl Updater {
    pub fn new(app: AppHandle, engine: Engine) -> Self {
        let pubkey = pubkey(&app);
        if pubkey.is_empty() {
            tracing::info!("no updater key in this build; updates are notify only");
        }
        Self(Arc::new(Inner {
            app,
            engine,
            install: install_kind::detect(),
            pubkey,
            state: Mutex::new(State {
                phase: UpdatePhase::Idle,
                offer: None,
                progress: None,
                error: None,
                download: None,
                prepared: None,
            }),
        }))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // The state holds no invariant a panic could break halfway; keep going.
        self.0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn can_install(&self) -> bool {
        !self.0.pubkey.is_empty() && self.0.install.manifest_target().is_some()
    }

    pub fn status(&self) -> UpdateStatus {
        let last_check = self
            .0
            .engine
            .settings()
            .ok()
            .and_then(|s| s.last_update_check);
        let state = self.state();
        UpdateStatus {
            current_version: crate::version::DISPLAY.to_string(),
            install: self.0.install,
            can_install: self.can_install(),
            dev_build: current_version().is_none(),
            phase: state.phase,
            offer: state.offer.clone(),
            progress: state.progress,
            error: state.error,
            #[expect(clippy::cast_precision_loss, reason = "ms timestamps fit in f64")]
            last_check_ms: last_check.map(|ms| ms as f64),
        }
    }

    fn emit(&self, prompt: bool) {
        let event = UpdateStatusChanged {
            status: self.status(),
            prompt,
        };
        if let Err(e) = event.emit(&self.0.app) {
            tracing::warn!(error = %e, "telling the UI about the updater failed");
        }
    }

    fn set_phase(&self, phase: UpdatePhase, error: Option<UpdateError>) {
        let mut state = self.state();
        state.phase = phase;
        state.error = error;
    }

    /// Runs a check when updates are on and (unless `force`) the interval passed. Offers in
    /// "Notify only" ask the UI to prompt; "Download automatically" starts the download.
    pub async fn check(&self, force: bool) -> EngineResult<UpdateStatus> {
        let Some(current) = current_version() else {
            return Ok(self.status());
        };
        let mut settings = self.0.engine.settings()?;
        if settings.update_check != Consent::Yes {
            return Ok(self.status());
        }
        let now = now_ms();
        if !force
            && !is_due(
                settings.last_update_check,
                now,
                settings.update_interval_hours,
            )
        {
            return Ok(self.status());
        }
        {
            let state = self.state();
            // A running download or a prepared update is not interrupted by a check.
            let settled = matches!(
                state.phase,
                UpdatePhase::Idle | UpdatePhase::UpToDate | UpdatePhase::Available
            ) || (state.phase == UpdatePhase::Failed && state.download.is_none());
            if !settled {
                return Ok(self.status());
            }
        }
        // Stamped before the request so a failing network is not retried every loop.
        settings.last_update_check = Some(now);
        let skipped = settings.update_skipped.clone();
        let auto = settings.update_auto_download;
        self.0.engine.update_settings(settings).await?;
        self.set_phase(UpdatePhase::Checking, None);
        self.emit(false);

        let endpoints = Endpoints::get();
        let found = fetch(
            &endpoints,
            &endpoints.list,
            "application/vnd.github+json",
            LIST_LIMIT,
        )
        .await
        .map(|body| release::pick_offer(&body, current, skipped.as_deref()));
        let prompt = match found {
            Ok(Some(offer)) => {
                let mut state = self.state();
                state.offer = Some(UpdateOffer {
                    url: format!("{RELEASES}/tag/v{}", offer.version),
                    version: offer.version,
                    notes: offer.notes,
                });
                state.phase = UpdatePhase::Available;
                drop(state);
                // "Download automatically" prompts once the update is ready instead.
                !(auto && self.download())
            }
            Ok(None) => {
                let mut state = self.state();
                state.offer = None;
                state.phase = UpdatePhase::UpToDate;
                false
            }
            Err(e) => {
                tracing::warn!(error = %e, "update check failed");
                let mut state = self.state();
                state.phase = if state.offer.is_some() {
                    UpdatePhase::Available
                } else {
                    UpdatePhase::Idle
                };
                state.error = Some(UpdateError::Network);
                false
            }
        };
        self.emit(prompt);
        Ok(self.status())
    }

    /// Starts downloading the offer; `false` when there is none or this install cannot update
    /// itself.
    pub fn download(&self) -> bool {
        let Some(target) = self.0.install.manifest_target() else {
            return false;
        };
        if !self.can_install() {
            return false;
        }
        let mut state = self.state();
        let Some(version) = state.offer.as_ref().map(|o| o.version.clone()) else {
            return false;
        };
        if state.download.is_some() || state.phase == UpdatePhase::Ready {
            return true;
        }
        state.phase = UpdatePhase::Downloading;
        state.error = None;
        state.progress = Some(UpdateProgress {
            received: 0.0,
            total: None,
        });
        let this = self.clone();
        state.download = Some(tauri::async_runtime::spawn(async move {
            let result = this.prepare(&version, &target).await;
            let mut state = this.state();
            state.download = None;
            match result {
                Ok(prepared) => {
                    state.phase = UpdatePhase::Ready;
                    state.prepared = Some(prepared);
                }
                Err(error) => {
                    // The bytes are dropped with the failed attempt; nothing stays on disk.
                    state.phase = UpdatePhase::Failed;
                    state.error = Some(error);
                    state.progress = None;
                }
            }
            drop(state);
            // A finished or failed download always reaches the user.
            this.emit(true);
        }));
        drop(state);
        self.emit(false);
        true
    }

    /// Download, verify and (where the swap does not end the app) install.
    async fn prepare(&self, version: &str, target: &str) -> Result<Prepared, UpdateError> {
        let endpoints = Endpoints::get();
        let folder = endpoints.release_folder(version);
        let manifest = fetch(
            &endpoints,
            &format!("{folder}latest.json"),
            "application/json",
            SMALL_LIMIT,
        )
        .await
        .map_err(|e| network("update manifest", &e))?;
        let (url, signature, manifest) =
            release::manifest_entry(&manifest, version, target, &folder).map_err(rejected)?;

        let fetcher = Fetcher::new(endpoints.policy.clone(), Timeouts::default())
            .map_err(|e| network("bundle", &e))?;
        let mut last = Instant::now();
        let mut on_progress = |received: u64, total: Option<u64>| {
            if last.elapsed() < PROGRESS_EVERY {
                return;
            }
            last = Instant::now();
            #[expect(clippy::cast_precision_loss, reason = "sizes far below 2^53")]
            let progress = UpdateProgress {
                received: received as f64,
                total: total.map(|t| t as f64),
            };
            self.state().progress = Some(progress);
            self.emit(false);
        };
        let bytes = fetcher
            .download(
                &url,
                &user_agent(),
                BUNDLE_LIMIT,
                DOWNLOAD_TIMEOUT,
                &mut on_progress,
            )
            .await
            .map_err(|e| network("bundle", &e))?
            .body;

        self.set_phase(UpdatePhase::Verifying, None);
        self.emit(false);
        release::verify_signature(&bytes, &signature, &self.0.pubkey, version).map_err(rejected)?;
        let sums = fetch(
            &endpoints,
            &format!("{folder}SHA256SUMS"),
            "text/plain",
            SMALL_LIMIT,
        )
        .await
        .map_err(|e| network("SHA256SUMS", &e))?;
        if !release::checksum_matches(&sums, release::file_name(&url), &bytes) {
            return Err(rejected(Rejected::Checksum));
        }

        let mut prepared = Prepared {
            bytes,
            manifest,
            target: target.to_string(),
            installed: false,
        };
        // Windows installs by running the installer, which ends the app: that waits for the
        // restart. macOS and the AppImage swap the files now; the running app is unaffected.
        if !cfg!(target_os = "windows") {
            self.set_phase(UpdatePhase::Installing, None);
            self.emit(false);
            let installer = self.plugin_update(&prepared)?;
            let bytes = std::mem::take(&mut prepared.bytes);
            tauri::async_runtime::spawn_blocking(move || installer.install(bytes))
                .await
                .map_err(|e| install_failed(&e))?
                .map_err(|e| install_failed(&e))?;
            prepared.installed = true;
        }
        Ok(prepared)
    }

    /// The plugin's view of the verified update; it checks the signature once more itself.
    fn plugin_update(
        &self,
        prepared: &Prepared,
    ) -> Result<tauri_plugin_updater::Update, UpdateError> {
        // `build` refuses an updater without endpoints and the config has none, because the
        // plugin never checks or downloads here: it only installs the bundle verified above.
        let endpoint = tauri::Url::parse(RELEASES_API).map_err(|e| install_failed(&e))?;
        let mut builder = self
            .0
            .app
            .updater_builder()
            .endpoints(vec![endpoint])
            .map_err(|e| install_failed(&e))?
            .pubkey(self.0.pubkey.clone())
            .target(prepared.target.clone());
        // The file to replace is the AppImage itself, not the binary inside its mount.
        if self.0.install == InstallKind::AppImage
            && let Some(appimage) = std::env::var_os("APPIMAGE")
        {
            builder = builder.executable_path(appimage);
        }
        builder
            .build()
            .and_then(|u| u.restore_update(prepared.manifest.clone()))
            .map_err(|e| install_failed(&e))?
            .ok_or(UpdateError::Install)
    }

    /// "Cancel": stops the download and keeps the offer.
    pub fn cancel(&self) {
        let mut state = self.state();
        // Installing is not interrupted: a half-written swap is worse than finishing it.
        if state.phase == UpdatePhase::Installing {
            return;
        }
        let Some(task) = state.download.take() else {
            return;
        };
        task.abort();
        state.phase = UpdatePhase::Available;
        state.progress = None;
        state.error = None;
        drop(state);
        self.emit(false);
    }

    /// Restarts into the prepared update; `false` when none is ready.
    pub fn restart(&self, app: &AppHandle) -> bool {
        let Some(prepared) = self.state().prepared.take() else {
            return false;
        };
        if prepared.installed {
            crate::window::restart(app);
            return true;
        }
        let update = match self.plugin_update(&prepared) {
            Ok(update) => update,
            Err(error) => {
                self.set_phase(UpdatePhase::Failed, Some(error));
                self.emit(false);
                return true;
            }
        };
        let this = self.clone();
        // Runs the installer silently for this user; the plugin then ends the app and the
        // installer starts the new version.
        crate::window::after_disconnect(app, move |_| {
            if let Err(e) = update.install(prepared.bytes) {
                this.set_phase(UpdatePhase::Failed, Some(install_failed(&e)));
                this.emit(false);
            }
        });
        true
    }

    /// Checks on start and then whenever the interval passed.
    pub fn start(&self) {
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut first = true;
            loop {
                if let Err(e) = this.check(first).await {
                    tracing::warn!(error = %e, "update check skipped");
                }
                first = false;
                tokio::time::sleep(LOOP_INTERVAL).await;
            }
        });
    }
}

fn user_agent() -> String {
    format!("Mumcord/{}", crate::version::DISPLAY)
}

/// One request; the only thing sent besides the request itself is the `User-Agent`.
async fn fetch(
    endpoints: &Endpoints,
    url: &str,
    accept: &'static str,
    limit: usize,
) -> Result<Vec<u8>, FetchError> {
    let fetcher = Fetcher::new(endpoints.policy.clone(), Timeouts::default())?;
    Ok(fetcher
        .get_as(url, &user_agent(), accept, limit, false)
        .await?
        .body)
}

fn network(what: &str, error: &FetchError) -> UpdateError {
    tracing::warn!(error = %error, "downloading the {what} failed");
    UpdateError::Network
}

fn rejected(why: Rejected) -> UpdateError {
    tracing::warn!(?why, "update rejected and discarded");
    match why {
        Rejected::Manifest | Rejected::Address => UpdateError::Manifest,
        Rejected::Signature => UpdateError::Signature,
        Rejected::Checksum => UpdateError::Checksum,
    }
}

fn install_failed(error: &dyn std::fmt::Display) -> UpdateError {
    tracing::warn!(error = %error, "installing the update failed");
    UpdateError::Install
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn interval_throttles_checks() {
        assert!(is_due(None, 1_000, 12));
        assert!(!is_due(Some(1_000), 1_000 + 12 * HOUR_MS - 1, 12));
        assert!(is_due(Some(1_000), 1_000 + 12 * HOUR_MS, 12));
        assert!(is_due(Some(1_000), 1_000 + HOUR_MS, 1));
        assert!(is_due(Some(5_000), 1_000, 12), "clock went back");
    }

    /// Serves one canned HTTP response; returns the URL and the request text it saw.
    async fn serve(response: String) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("http://{}/releases", listener.local_addr().expect("addr"));
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buf = vec![0u8; 4096];
            let n = socket.read(&mut buf).await.expect("read");
            socket.write_all(response.as_bytes()).await.expect("write");
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        (url, task)
    }

    fn local() -> Endpoints {
        Endpoints {
            list: String::new(),
            downloads: String::new(),
            policy: AddressPolicy::Only(Arc::new(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)])),
        }
    }

    #[tokio::test]
    async fn only_the_user_agent_identifies_the_app() {
        let body = "[]";
        let (url, seen) = serve(format!(
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        ))
        .await;
        fetch(&local(), &url, "application/json", SMALL_LIMIT)
            .await
            .expect("fetch");
        let request = seen.await.expect("server").to_ascii_lowercase();
        assert!(request.contains("user-agent: mumcord/"));
        assert!(!request.contains("cookie") && !request.contains("authorization"));
    }

    #[tokio::test]
    async fn offline_is_a_plain_error() {
        let (url, _) = serve("HTTP/1.1 503 Unavailable\r\ncontent-length: 0\r\n\r\n".into()).await;
        let err = fetch(&local(), &url, "application/json", SMALL_LIMIT).await;
        assert!(matches!(err, Err(FetchError::Status(503))));
        let closed = "http://127.0.0.1:9/releases";
        assert!(
            fetch(&local(), closed, "application/json", SMALL_LIMIT)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn public_only_policy_never_reaches_localhost() {
        let (url, _) = serve("HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n[]".into()).await;
        let endpoints = Endpoints {
            policy: AddressPolicy::PublicOnly,
            ..local()
        };
        let err = fetch(&endpoints, &url, "application/json", SMALL_LIMIT).await;
        assert!(matches!(err, Err(FetchError::Blocked)));
    }

    #[test]
    fn release_folder_is_built_from_the_version() {
        let endpoints = Endpoints {
            downloads: format!("{RELEASES}/download/"),
            ..local()
        };
        assert_eq!(
            endpoints.release_folder("0.4.5"),
            "https://github.com/Mumcord/mumcord/releases/download/v0.4.5/"
        );
    }
}
