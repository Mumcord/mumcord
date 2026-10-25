//! Full-size images between Mumcord clients (FR-1.60c, QA.57).
//!
//! A chat message can carry only what fits the server's image limit, so the UI sends a
//! thumbnail inside the message (what official clients see) and hands the original to
//! [`Engine::full_image_keep`]. That keeps a full image (the original, or a re-encoding within
//! [`check::MAX_BYTES`]) in the local image cache and returns a [`FullRef`] (content hash and
//! size), which the UI puts into the thumbnail's `<img>` as [`wire::ATTRIBUTE`].
//!
//! When a Mumcord user opens such a thumbnail in the image viewer, [`Engine::full_image_fetch`]
//! asks the original sender for the full image over plugin data ([`transfer`]); the received
//! bytes must match the announced hash and pass [`check::check`] before they are cached and
//! shown. If the sender is offline, not a Mumcord client, no longer has the image, or the
//! transfer fails, the viewer keeps the thumbnail. Both sent and received full images live in
//! the link-preview image cache and follow its clean-up rules (QA.42).

pub mod check;
pub(crate) mod transfer;
pub mod wire;

use std::sync::Arc;

use mumcord_protocol::{Handle, PluginData};

use crate::session::{Engine, EngineError, EngineEvent, EngineResult, ErrorKind, SessionCtx};
use crate::web::{FullImage, LinkPreviews};
pub use transfer::FetchError;
use transfer::{Link, Originals, Transfers};
pub use wire::FullRef;

/// Sends transfer messages as plugin data on the current connection.
struct HandleLink(Handle);

impl Link for HandleLink {
    fn send(&self, to: u32, msg: &wire::Msg) -> bool {
        match self.0.send_plugin_data(&[to], wire::DATA_ID, msg.encode()) {
            Ok(()) => true,
            Err(e) => {
                tracing::debug!(error = %e, "full image: plugin data not sent");
                false
            }
        }
    }
}

/// Own originals by content hash, for answering requests.
fn originals(previews: Arc<LinkPreviews>) -> Originals {
    Arc::new(move |hash: &[u8; 32]| {
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        let image = previews.full_image(&hex).filter(|image| image.own)?;
        let (_, payload) = image.data_url.split_once(";base64,")?;
        crate::people::base64_decode(payload)
    })
}

pub(crate) fn new_transfers() -> Transfers {
    Transfers::new(transfer::Timing::default())
}

/// Plugin data from another client (session task).
pub(crate) fn on_plugin_data(
    transfers: &Arc<Transfers>,
    previews: &Arc<LinkPreviews>,
    ctx: &SessionCtx,
    data: &PluginData,
) {
    if data.data_id != wire::DATA_ID {
        return;
    }
    let Some(sender) = data.sender else {
        return;
    };
    let (own, known) = ctx
        .handle
        .with_state(|s| (s.session, s.users.contains_key(&sender)));
    if own == Some(sender) || !known {
        return;
    }
    let Some(msg) = wire::Msg::decode(&data.data) else {
        tracing::debug!("ignoring a malformed full-image message");
        return;
    };
    transfers.on_message(
        sender,
        msg,
        Arc::new(HandleLink(ctx.handle.clone())),
        originals(previews.clone()),
    );
}

/// The online session that sent a message: by certificate (survives reconnects), else by the
/// session the message came from. Never the own session.
fn online_sender(handle: &Handle, session: Option<u32>, cert_hash: Option<&str>) -> Option<u32> {
    handle.with_state(|s| {
        let by_cert = cert_hash.and_then(|cert| {
            s.users
                .values()
                .find(|u| u.cert_hash.as_deref() == Some(cert))
                .map(|u| u.session)
        });
        let found = by_cert.or_else(|| session.filter(|id| s.users.contains_key(id)))?;
        (s.session != Some(found)).then_some(found)
    })
}

impl Engine {
    /// Keeps the full image for a picture the user is about to send (the original file's
    /// bytes) and returns its reference for the message's `<img>` (see [`wire::ATTRIBUTE`]).
    /// Errors: `invalidInput` when it is not a supported image or cannot be made small enough.
    pub async fn full_image_keep(&self, source: Vec<u8>) -> EngineResult<String> {
        let previews = self.link_previews_arc();
        tokio::task::spawn_blocking(move || {
            let bytes = check::prepare(&source)
                .map_err(|e| EngineError::new(ErrorKind::InvalidInput, e.to_string()))?;
            let checked = check::check(&bytes)
                .map_err(|e| EngineError::new(ErrorKind::Internal, e.to_string()))?;
            let full = FullRef::of(&bytes);
            previews.keep_full_image(
                &full.hash_hex(),
                FullImage {
                    data_url: check::data_url(&bytes, &checked),
                    own: true,
                },
            );
            tracing::debug!(bytes = full.size, "full image kept for sending");
            Ok(full.to_string())
        })
        .await
        .map_err(|e| EngineError::new(ErrorKind::Internal, e.to_string()))?
    }

    /// The full image behind a thumbnail as a `data:` URL: from the cache, else fetched from the
    /// message's sender (identified by certificate hash or session) while they are online.
    /// Emits [`EngineEvent::FullImageProgress`] while loading. `None` when it cannot be had;
    /// the UI keeps showing the thumbnail then.
    pub async fn full_image_fetch(
        &self,
        reference: &str,
        sender_session: Option<u32>,
        sender_cert_hash: Option<String>,
    ) -> Option<String> {
        let full = FullRef::parse(reference, check::MAX_BYTES)?;
        let previews = self.link_previews_arc();
        let hex = full.hash_hex();
        let cached = {
            let previews = previews.clone();
            let hex = hex.clone();
            tokio::task::spawn_blocking(move || previews.full_image(&hex))
                .await
                .ok()
                .flatten()
        };
        if let Some(image) = cached {
            return Some(image.data_url);
        }
        let ctx = self.session_ctx().ok()?;
        let from = online_sender(&ctx.handle, sender_session, sender_cert_hash.as_deref())?;
        let events = ctx.events.clone();
        let progress_ref = reference.to_owned();
        let bytes = self
            .full_image_transfers()
            .fetch(
                Arc::new(HandleLink(ctx.handle.clone())),
                from,
                full,
                move |received, total| {
                    events.emit(EngineEvent::FullImageProgress {
                        reference: progress_ref.clone(),
                        received,
                        total,
                    });
                },
            )
            .await;
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::info!(error = %e, "full image not loaded; keeping the thumbnail");
                return None;
            }
        };
        tokio::task::spawn_blocking(move || {
            let checked = match check::check(&bytes) {
                Ok(checked) => checked,
                Err(e) => {
                    tracing::info!(error = %e, "received full image rejected");
                    return None;
                }
            };
            let data_url = check::data_url(&bytes, &checked);
            previews.keep_full_image(
                &hex,
                FullImage {
                    data_url: data_url.clone(),
                    own: false,
                },
            );
            Some(data_url)
        })
        .await
        .ok()
        .flatten()
    }
}
