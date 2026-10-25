//! Full-size chat images between Mumcord clients (FR-1.60c): the thumbnail goes through the
//! chat, the full image is fetched from the sender on demand for the image viewer.

use mumcord_engine::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::chat::decode_image_data_url;
use super::types::{ApiResult, SessionId};

/// Progress of a full image being fetched: `received` of `total` bytes of the image with this
/// reference (the `data-mumcord-full` value of the thumbnail).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct FullImageProgress {
    pub reference: String,
    pub received: u32,
    pub total: u32,
}

/// Keeps the full image of a picture about to be sent (the original file as a `data:` URL;
/// re-encoded when larger than 512 KiB or 3840 px) and returns the reference to put into the
/// thumbnail's `data-mumcord-full` attribute. Errors: `invalidInput` for anything that is not
/// a usable PNG, JPEG, GIF or WebP image.
#[tauri::command]
#[specta::specta]
pub async fn full_image_keep(engine: State<'_, Engine>, data_url: String) -> ApiResult<String> {
    let bytes = decode_image_data_url(&data_url)?;
    Ok(engine.full_image_keep(bytes).await?)
}

/// The full image behind a thumbnail as a `data:` URL: from the cache, else fetched from the
/// message's sender while they are online (progress as `fullImageProgress`). `null` when it
/// cannot be had; keep showing the thumbnail then.
#[tauri::command]
#[specta::specta]
pub async fn full_image_fetch(
    engine: State<'_, Engine>,
    reference: String,
    sender_session: Option<SessionId>,
    sender_cert_hash: Option<String>,
) -> ApiResult<Option<String>> {
    Ok(engine
        .full_image_fetch(&reference, sender_session, sender_cert_hash)
        .await)
}
