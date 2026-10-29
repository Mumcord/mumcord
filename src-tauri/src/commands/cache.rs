//! Settings > Privacy: the size of the caches and the "Clear cache" action.

use mumcord_engine::Engine;
use mumcord_engine::cache::{self, CacheKind};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};

use super::types::{ApiError, ApiResult, ErrorCode};
use crate::cache_dirs;

/// Space used per category in KiB (capped at about 4 TB).
#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CacheSizes {
    pub previews_kib: u32,
    pub web_kib: u32,
    pub temp_kib: u32,
}

/// Result of "Clear cache".
#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CacheCleared {
    /// Space freed, including the web cache that the next start deletes.
    pub freed_kib: u32,
    pub web_pending: bool,
}

fn kib(bytes: u64) -> u32 {
    u32::try_from(bytes.div_ceil(1024)).unwrap_or(u32::MAX)
}

fn internal(message: impl ToString) -> ApiError {
    ApiError::new(ErrorCode::Internal, message.to_string())
}

/// Measures the caches off the UI thread.
#[tauri::command]
#[specta::specta]
pub async fn cache_sizes(app: AppHandle) -> ApiResult<CacheSizes> {
    let locations = cache_dirs::locations(&app).map_err(internal)?;
    let sizes = tauri::async_runtime::spawn_blocking(move || cache::sizes(&locations))
        .await
        .map_err(internal)?;
    Ok(CacheSizes {
        previews_kib: kib(sizes.previews),
        web_kib: kib(sizes.web),
        temp_kib: kib(sizes.temp),
    })
}

/// Deletes the selected caches, and only caches (never history, settings, servers,
/// certificates, friends or notes).
#[tauri::command]
#[specta::specta]
pub async fn cache_clear(
    app: AppHandle,
    engine: State<'_, Engine>,
    kinds: Vec<CacheKind>,
) -> ApiResult<CacheCleared> {
    let locations = cache_dirs::locations(&app).map_err(internal)?;
    let mut freed = 0;
    if kinds.contains(&CacheKind::Previews) {
        // Also empties the in-memory cache, which would otherwise serve deleted entries.
        freed += engine.preview_cache_clear().await?;
    }
    let cleared =
        tauri::async_runtime::spawn_blocking(move || cache::clear_files(&locations, &kinds))
            .await
            .map_err(internal)?;
    Ok(CacheCleared {
        freed_kib: kib(freed + cleared.freed),
        web_pending: cleared.web_pending,
    })
}
