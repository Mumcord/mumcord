//! Third-party plugin repositories (spec 06, FR-6.74 – FR-6.76): add, list, switch on or off,
//! remove and review a key change. The UI never fetches anything itself; every address is
//! checked and fetched here.

use mumcord_engine::Engine;
use mumcord_engine::store::{Error as StoreError, NewPluginRepository, PluginRepository};
use mumcord_plugins::registry::{
    RepoOutcome, RepositoryInfo, fetch_repository, key_fingerprint, preview_repository,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use super::plugins::{plugins_of, registry_client, registry_error};
use super::types::{ApiError, ApiResult, ErrorCode};
use crate::plugins::suggestions::ensure_index;

/// `paused_reason` the update check stores after a key change (FR-6.75).
const KEY_CHANGED: &str = "key-changed";

/// One added repository as the Repositories tab shows it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryView {
    pub id: u32,
    pub url: String,
    pub name: String,
    /// The pinned key's id, e.g. `1CB49831E26E61AD`.
    pub fingerprint: String,
    pub enabled: bool,
    /// Paused after the repository started signing with another key.
    pub key_changed: bool,
    /// Milliseconds since the epoch of the last successful check.
    pub last_checked: Option<f64>,
    /// Plugins installed from this repository.
    pub installed_plugins: u32,
}

/// What the Add dialog shows before the user confirms (FR-6.74).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryPreview {
    pub url: String,
    pub name: String,
    pub homepage: Option<String>,
    pub contact: Option<String>,
    pub fingerprint: String,
    pub plugin_count: u32,
    /// Entries left out of the list, as plain-text notes (FR-6.76).
    pub notes: Vec<String>,
}

/// The key a paused repository signs with now (FR-6.75).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct KeyChangeReview {
    pub name: String,
    pub old_fingerprint: String,
    pub new_fingerprint: String,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::InvalidInput, message)
}

fn store_error(error: StoreError) -> ApiError {
    match error {
        StoreError::TooManyPluginRepositories { .. } | StoreError::DuplicatePluginRepository => {
            invalid(error.to_string())
        }
        other => ApiError::new(ErrorCode::Internal, other.to_string()),
    }
}

fn repository_id(id: u32) -> i64 {
    i64::from(id)
}

fn source_of(id: i64) -> String {
    format!("repo:{id}")
}

fn find(engine: &Engine, id: u32) -> ApiResult<PluginRepository> {
    let store = engine.store_handle();
    let store = store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store
        .plugin_repositories()
        .map_err(store_error)?
        .into_iter()
        .find(|r| r.id == repository_id(id))
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "no such repository"))
}

fn view(repository: &PluginRepository, installed_plugins: usize) -> RepositoryView {
    RepositoryView {
        id: u32::try_from(repository.id).unwrap_or(u32::MAX),
        url: repository.url.clone(),
        name: repository.name.clone(),
        fingerprint: key_fingerprint(&repository.public_key).unwrap_or_default(),
        enabled: repository.enabled,
        key_changed: repository.paused_reason.as_deref() == Some(KEY_CHANGED),
        #[allow(clippy::cast_precision_loss)]
        last_checked: repository.last_checked.map(|t| t as f64),
        installed_plugins: count(installed_plugins),
    }
}

fn preview_of(preview: mumcord_plugins::registry::RepoPreview) -> RepositoryPreview {
    let RepositoryInfo {
        name,
        homepage,
        contact,
        ..
    } = preview.info;
    RepositoryPreview {
        url: preview.url,
        name,
        homepage,
        contact,
        fingerprint: preview.fingerprint,
        plugin_count: count(preview.plugin_count),
        notes: preview.ignored.iter().map(ToString::to_string).collect(),
    }
}

/// Fetches the repository and checks it against the official list (FR-6.76).
async fn preview(
    app: &AppHandle,
    engine: &Engine,
    url: &str,
    expected_key: Option<&str>,
) -> ApiResult<mumcord_plugins::registry::RepoPreview> {
    let plugins = plugins_of(app)?;
    ensure_index(&plugins, engine).await;
    let official: Vec<String> = plugins
        .index()
        .as_ref()
        .map(|i| i.plugins.iter().map(|p| p.id.clone()).collect())
        .unwrap_or_default();
    let client = registry_client(engine.settings()?.developer_mode)?;
    preview_repository(&client, url, expected_key, &|id| {
        official.iter().any(|o| o == id)
    })
    .await
    .map_err(registry_error)
}

fn installed_per_repository(engine: &Engine) -> ApiResult<Vec<String>> {
    let store = engine.store_handle();
    let store = store
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(store
        .plugins()
        .map_err(store_error)?
        .into_iter()
        .filter(|p| p.removed_at.is_none())
        .map(|p| p.source)
        .collect())
}

/// Turns off the plugins that came from `repository` (FR-6.75). A plugin that cannot be turned
/// off is logged, not fatal: the repository change itself has already happened.
async fn turn_off_plugins(app: &AppHandle, engine: &Engine, repository: i64) -> ApiResult<()> {
    let source = source_of(repository);
    let ids: Vec<String> = {
        let store = engine.store_handle();
        let store = store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store
            .plugins()
            .map_err(store_error)?
            .into_iter()
            .filter(|p| p.removed_at.is_none() && p.enabled && p.source == source)
            .map(|p| p.id)
            .collect()
    };
    let plugins = plugins_of(app)?;
    for id in ids {
        let target = id.clone();
        if let Err(error) = plugins
            .call(move |service| service.set_enabled(&target, false))
            .await
        {
            tracing::warn!(plugin = %id, %error, "turning off a plugin of a repository failed");
        }
    }
    Ok(())
}

fn announce_change(app: &AppHandle) {
    if let Err(error) = (crate::plugins::PluginsChanged {}).emit(app) {
        tracing::warn!(%error, "announcing the repository change failed");
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

/// Every added repository, oldest first.
#[tauri::command]
#[specta::specta]
pub async fn plugin_repos_list(engine: State<'_, Engine>) -> ApiResult<Vec<RepositoryView>> {
    let sources = installed_per_repository(&engine)?;
    let repositories = {
        let store = engine.store_handle();
        let store = store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store.plugin_repositories().map_err(store_error)?
    };
    Ok(repositories
        .iter()
        .map(|r| {
            let source = source_of(r.id);
            view(r, sources.iter().filter(|s| **s == source).count())
        })
        .collect())
}

/// Fetches a repository for the Add dialog without adding it. With an "Expected key" the
/// repository must be signed with exactly that key.
#[tauri::command]
#[specta::specta]
pub async fn plugin_repo_preview(
    app: AppHandle,
    engine: State<'_, Engine>,
    url: String,
    expected_key: Option<String>,
) -> ApiResult<RepositoryPreview> {
    preview(&app, &engine, &url, expected_key.as_deref())
        .await
        .map(preview_of)
}

/// Adds the repository and pins the key it is signed with (trust on first use). The repository
/// is fetched again, so what is pinned is what was verified just now.
#[tauri::command]
#[specta::specta]
pub async fn plugin_repo_add(
    app: AppHandle,
    engine: State<'_, Engine>,
    url: String,
    expected_key: Option<String>,
) -> ApiResult<RepositoryView> {
    let checked = preview(&app, &engine, &url, expected_key.as_deref()).await?;
    let new = NewPluginRepository {
        url: checked.url.clone(),
        name: checked.info.name.clone(),
        public_key: checked.info.public_key.clone(),
    };
    let now = now_millis();
    let (id, repository) = {
        let store = engine.store_handle();
        let store = store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = store
            .add_plugin_repository(&new, now)
            .map_err(store_error)?;
        let repository = store
            .plugin_repositories()
            .map_err(store_error)?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| ApiError::new(ErrorCode::Internal, "the repository vanished"))?;
        (id, repository)
    };
    tracing::info!(repository = id, "added a plugin repository");
    announce_change(&app);
    Ok(view(&repository, 0))
}

/// Switches a repository on or off. Off turns its plugins off too (FR-6.75). Switching on a
/// repository that was paused for a key change goes through `plugin_repo_accept_key`.
#[tauri::command]
#[specta::specta]
pub async fn plugin_repo_set_enabled(
    app: AppHandle,
    engine: State<'_, Engine>,
    id: u32,
    enabled: bool,
) -> ApiResult<()> {
    let repository = find(&engine, id)?;
    if enabled && repository.paused_reason.as_deref() == Some(KEY_CHANGED) {
        return Err(invalid("review the key change first"));
    }
    {
        let store = engine.store_handle();
        let store = store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store
            .set_plugin_repository_enabled(repository.id, enabled)
            .map_err(store_error)?;
    }
    if !enabled {
        turn_off_plugins(&app, &engine, repository.id).await?;
    }
    announce_change(&app);
    Ok(())
}

/// Removes a repository. Its plugins are turned off and stay installed, so the user can still
/// export or delete their data; they get no updates any more.
#[tauri::command]
#[specta::specta]
pub async fn plugin_repo_remove(
    app: AppHandle,
    engine: State<'_, Engine>,
    id: u32,
) -> ApiResult<()> {
    let repository = find(&engine, id)?;
    turn_off_plugins(&app, &engine, repository.id).await?;
    {
        let store = engine.store_handle();
        let store = store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store
            .remove_plugin_repository(repository.id)
            .map_err(store_error)?;
    }
    announce_change(&app);
    Ok(())
}

/// Looks at the key a paused repository signs with now (key-change dialog, FR-6.75).
#[tauri::command]
#[specta::specta]
pub async fn plugin_repo_review_key(
    app: AppHandle,
    engine: State<'_, Engine>,
    id: u32,
) -> ApiResult<KeyChangeReview> {
    let repository = find(&engine, id)?;
    let plugins = plugins_of(&app)?;
    ensure_index(&plugins, &engine).await;
    let official: Vec<String> = plugins
        .index()
        .as_ref()
        .map(|i| i.plugins.iter().map(|p| p.id.clone()).collect())
        .unwrap_or_default();
    let client = registry_client(engine.settings()?.developer_mode)?;
    let dir = plugins
        .registry_cache()
        .join(format!("repo-{}", repository.id));
    let outcome = fetch_repository(&client, &dir, &repository, &|id| {
        official.iter().any(|o| o == id)
    })
    .await
    .map_err(registry_error)?;
    let new_key = match outcome {
        RepoOutcome::KeyChanged { new_key, .. } | RepoOutcome::Rotated { new_key, .. } => new_key,
        RepoOutcome::Loaded(_) => return Err(invalid("the repository key did not change")),
    };
    Ok(KeyChangeReview {
        name: repository.name,
        old_fingerprint: key_fingerprint(&repository.public_key).unwrap_or_default(),
        new_fingerprint: key_fingerprint(&new_key).unwrap_or_default(),
    })
}

/// The user trusts the new key: pins it and switches the repository back on. The key is read
/// again from the repository, not taken from the UI.
#[tauri::command]
#[specta::specta]
pub async fn plugin_repo_accept_key(
    app: AppHandle,
    engine: State<'_, Engine>,
    id: u32,
) -> ApiResult<()> {
    let repository = find(&engine, id)?;
    let checked = preview(&app, &engine, &repository.url, None).await?;
    let now = now_millis();
    {
        let store = engine.store_handle();
        let store = store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store
            .replace_plugin_repository_key(repository.id, &checked.info.public_key, now)
            .map_err(store_error)?;
    }
    tracing::info!(repository = repository.id, "pinned a new repository key");
    announce_change(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository(paused: Option<&str>) -> PluginRepository {
        PluginRepository {
            id: 3,
            url: "https://example.org/index.json".into(),
            name: "Example".into(),
            public_key: String::new(),
            pinned_at: 0,
            enabled: paused.is_none(),
            paused_reason: paused.map(str::to_owned),
            last_sequence: 0,
            last_checked: None,
        }
    }

    #[test]
    fn only_a_key_change_counts_as_key_changed() {
        assert!(view(&repository(Some(KEY_CHANGED)), 0).key_changed);
        assert!(!view(&repository(Some("other")), 0).key_changed);
        assert!(!view(&repository(None), 2).key_changed);
        assert_eq!(view(&repository(None), 2).installed_plugins, 2);
    }
}
