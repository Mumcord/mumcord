//! Full-size chat images between Mumcord clients (FR-1.60c): the thumbnail goes through the
//! chat, the full image is fetched from the sender on demand for the image viewer.

use mumcord_engine::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use super::chat::decode_image_data_url;
use super::types::{ApiResult, ServerId, SessionId, to_engine_server_id};

/// Progress of a full image being fetched: `received` of `total` bytes of the image with this
/// reference (the `data-mumcord-full` value of the thumbnail).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct FullImageProgress {
    pub server_id: ServerId,
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
    server_id: ServerId,
    reference: String,
    sender_session: Option<SessionId>,
    sender_cert_hash: Option<String>,
) -> ApiResult<Option<String>> {
    Ok(engine
        .full_image_fetch(
            to_engine_server_id(server_id),
            &reference,
            sender_session,
            sender_cert_hash,
        )
        .await)
}

/// Cancels a running `full_image_fetch` of the reference, e.g. when the image viewer closes
/// (FR-2.38); the fetch then resolves with `null`. Does nothing when none runs.
#[tauri::command]
#[specta::specta]
pub fn full_image_cancel(
    engine: State<'_, Engine>,
    server_id: ServerId,
    reference: String,
) -> ApiResult<()> {
    engine.full_image_cancel(to_engine_server_id(server_id), &reference);
    Ok(())
}
