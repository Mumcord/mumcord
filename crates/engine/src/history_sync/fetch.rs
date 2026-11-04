//! Fetching the referenced images of synced messages (FR-1.60f).
//!
//! A message that arrived with image references is stored and shown with placeholders right
//! away. Each missing image is then loaded through the full-image path from the holder that
//! sent the message (direct connection first, plugin data otherwise), checked against its
//! reference and put back into every stored message of the channel that waits for it. Images
//! already in the local image cache are not fetched again. With "Load full-size images from
//! others" off the placeholder offers a manual load instead; a failed fetch is retried when a
//! later sync brings the message again.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use mumcord_store::{Conversation, ServerId};

use super::core::ImageFetch;
use super::images::{self, ImageRef};
use crate::full_image::check;
use crate::session::{
    ConversationRef, Engine, EngineError, EngineEvent, EngineResult, ErrorKind, HistoryImageState,
    WeakEngine, lock,
};
use crate::web::fetch::ImageKind;

/// Most stored messages updated for one arrived image.
const MAX_RESTORED: u32 = 500;
/// Time between two image requests: a holder answers a requester one image at a time, pauses
/// after each upload, and takes at most `MAX_REQUESTS` per `REQUEST_WINDOW` from it.
const SPACING: Duration = Duration::from_millis(10_500);

/// An image waiting for a manual load: where it can be fetched and which channel needs it.
#[derive(Clone)]
struct Waiting {
    holder: u32,
    channel: u32,
    key: String,
    image: ImageRef,
}

/// The receiving side of image references, for every connected server.
#[derive(Default)]
pub(crate) struct HistoryImages {
    engine: OnceLock<WeakEngine>,
    /// One fetch at a time, [`SPACING`] apart: when the last one started.
    turn: tokio::sync::Mutex<Option<tokio::time::Instant>>,
    /// Images that wait for a manual load, by server and image hash.
    waiting: Mutex<HashMap<(ServerId, [u8; 32]), Waiting>>,
}

impl HistoryImages {
    pub(crate) fn bind(&self, engine: &Engine) {
        let _ = self.engine.set(engine.downgrade());
    }

    fn engine(&self) -> Option<Engine> {
        self.engine.get().and_then(WeakEngine::upgrade)
    }

    /// Loads the images of messages that just arrived, in the background.
    pub(crate) fn fetch(&self, server_id: ServerId, fetches: Vec<ImageFetch>) {
        if fetches.is_empty() {
            return;
        }
        let Some(engine) = self.engine() else {
            return;
        };
        let rt = engine.runtime().clone();
        rt.spawn(async move {
            for fetch in fetches {
                for image in fetch.refs {
                    let waiting = Waiting {
                        holder: fetch.holder,
                        channel: fetch.channel,
                        key: fetch.key.clone(),
                        image,
                    };
                    load(&engine, server_id, waiting, false).await;
                }
            }
        });
    }

    /// The connection to `server_id` ended: its holders' sessions mean nothing any more.
    pub(crate) fn reset(&self, server_id: ServerId) {
        lock(&self.waiting).retain(|(id, _), _| *id != server_id);
    }
}

/// Fetches one image (unless cached) and restores the messages that wait for it.
async fn load(engine: &Engine, server_id: ServerId, waiting: Waiting, manual: bool) {
    let images = engine.history_images();
    let image = waiting.image;
    let bytes = match cached(engine, &image).await {
        Some(bytes) => Some(bytes),
        None if !manual && !load_setting(engine) => {
            lock(&images.waiting).insert((server_id, image.full.hash), waiting.clone());
            status(engine, server_id, &image, HistoryImageState::Manual);
            return;
        }
        None => {
            let mut last = images.turn.lock().await;
            if let Some(at) = *last {
                tokio::time::sleep_until(at + SPACING).await;
            }
            *last = Some(tokio::time::Instant::now());
            engine
                .full_image_download(server_id, image.full, waiting.holder)
                .await
                .and_then(|url| decoded(&url))
                .filter(|bytes| fits(&image, bytes))
        }
    };
    let Some(bytes) = bytes else {
        tracing::info!("history image not loaded; keeping the placeholder");
        status(engine, server_id, &image, HistoryImageState::Unavailable);
        return;
    };
    lock(&images.waiting).remove(&(server_id, image.full.hash));
    restore(
        engine,
        server_id,
        &waiting.key,
        waiting.channel,
        image,
        bytes,
    )
    .await;
}

/// The image from the local image cache, if it is there and matches.
async fn cached(engine: &Engine, image: &ImageRef) -> Option<Vec<u8>> {
    let url = engine.full_image_cached(&image.full).await?;
    decoded(&url).filter(|bytes| fits(image, bytes))
}

fn decoded(data_url: &str) -> Option<Vec<u8>> {
    let (_, payload) = data_url.split_once(";base64,")?;
    crate::people::base64_decode(payload)
}

/// Whether `bytes` are exactly the referenced image: hash, size, type and pixel size, so the
/// restored body is the holder's.
fn fits(image: &ImageRef, bytes: &[u8]) -> bool {
    image.full.matches(bytes)
        && ImageKind::sniff(bytes) == Some(image.kind)
        && check::dimensions(bytes, image.kind) == Some((image.width, image.height))
}

fn load_setting(engine: &Engine) -> bool {
    engine.settings().map_or_else(
        |e| {
            tracing::warn!(error = %e, "reading settings failed; using the default");
            true
        },
        |s| s.load_full_images,
    )
}

fn status(engine: &Engine, server_id: ServerId, image: &ImageRef, state: HistoryImageState) {
    if let Ok(ctx) = engine.session_ctx_for(server_id) {
        ctx.events.emit(EngineEvent::HistoryImageStatus {
            reference: image.to_string(),
            state,
        });
    }
}

/// Puts the image back into every stored message of the channel that waits for it and shows
/// the updated messages.
async fn restore(
    engine: &Engine,
    server_id: ServerId,
    key: &str,
    channel: u32,
    image: ImageRef,
    bytes: Vec<u8>,
) {
    let Ok(ctx) = engine.session_ctx_for(server_id) else {
        return;
    };
    let key = key.to_owned();
    let worker = ctx.clone();
    let views = tokio::task::spawn_blocking(move || {
        let own = worker
            .handle
            .with_state(|s| s.own_user().and_then(|u| u.cert_hash.clone()));
        let store = worker.store();
        let needle = format!("{}=\"{image}\"", images::ATTRIBUTE);
        let rows = store
            .messages_containing(
                worker.server_id,
                &Conversation::Channel(key),
                &needle,
                MAX_RESTORED,
            )
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "reading messages waiting for an image failed");
                Vec::new()
            });
        let ids: Vec<i64> = rows.iter().map(|m| m.id).collect();
        let mut origins = store.synced_origins(&ids).unwrap_or_default();
        let mut views = Vec::new();
        for mut message in rows {
            let body = images::restore(&message.body, |r| {
                (r.full.hash == image.full.hash).then(|| bytes.clone())
            })
            .into_owned();
            if let Err(e) = store.set_message_body(message.id, &body) {
                tracing::warn!(error = %e, "storing a restored image failed");
                continue;
            }
            message.body = body;
            let origin = origins.remove(&message.id);
            views.push(crate::chat::history_view(
                message,
                &ConversationRef::Channel {
                    channel_id: channel,
                },
                own.as_deref(),
                Vec::new(),
                origin,
            ));
        }
        views
    })
    .await
    .unwrap_or_default();
    if !views.is_empty() {
        ctx.events.emit(EngineEvent::HistorySynced {
            channel_id: channel,
            messages: views,
        });
    }
}

impl Engine {
    /// "Load image" on the placeholder of a synced message (FR-1.60f): fetches the referenced
    /// image from the holder that sent the message, also while "Load full-size images from
    /// others" is off. `reference` is the placeholder's reference. The outcome arrives as an
    /// updated message (`HistorySynced`) or a `HistoryImageStatus` event.
    pub fn history_image_load(&self, server_id: ServerId, reference: &str) -> EngineResult<()> {
        let image = ImageRef::parse(reference)
            .ok_or_else(|| EngineError::invalid("not an image reference"))?;
        let waiting = lock(&self.history_images().waiting)
            .get(&(server_id, image.full.hash))
            .cloned()
            .ok_or_else(|| {
                EngineError::new(ErrorKind::NotFound, "nobody offered this image any more")
            })?;
        let engine = self.clone();
        self.runtime().spawn(async move {
            load(&engine, server_id, waiting, true).await;
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetches_stay_within_the_holders_request_limit() {
        use crate::full_image::transfer::{MAX_REQUESTS, REQUEST_WINDOW};
        let requests = u32::try_from(MAX_REQUESTS).expect("small");
        assert!(SPACING * requests > REQUEST_WINDOW);
    }

    #[test]
    fn only_the_exact_image_fits_its_reference() {
        let png = crate::people::base64_decode(images::TEST_PNG).expect("png");
        let body = format!(
            "<img src=\"data:image/png;base64,{}\">",
            crate::people::base64(&png)
        );
        let image = images::references(&images::to_references(&body, |_, _| true).0)[0];
        assert!(fits(&image, &png));
        let mut other = png.clone();
        other.push(0);
        assert!(!fits(&image, &other), "different bytes");
        let wrong_size = ImageRef { width: 2, ..image };
        assert!(!fits(&wrong_size, &png), "announced size must match");
        assert_eq!(
            decoded(&format!(
                "data:image/png;base64,{}",
                crate::people::base64(&png)
            )),
            Some(png)
        );
    }
}
