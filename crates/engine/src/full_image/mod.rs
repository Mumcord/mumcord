//! Full-size images between Mumcord clients (FR-1.60c, QA.57).
//!
//! A chat message can carry only what fits the server's image limit, so the UI sends a
//! thumbnail inside the message (what official clients see) and hands the original to
//! [`Engine::full_image_keep`]. That keeps a full image (the original, or a re-encoding within
//! [`check::MAX_BYTES`]) in the local image cache and returns a [`FullRef`] (content hash and
//! size), which the UI puts into the thumbnail's `<img>` as [`wire::ATTRIBUTE`].
//!
//! When a Mumcord user opens such a thumbnail in the image viewer, [`Engine::full_image_fetch`]
//! asks the original sender for the full image over a direct connection ([`direct`], FR-2.37)
//! and, when that is not possible, over plugin data ([`transfer`]); the received
//! bytes must match the announced hash and pass [`check::check`] before they are cached and
//! shown. If the sender is offline, did not announce [`FEATURE`] in the Mumcord hello, no longer has the image, or the
//! transfer fails, the viewer keeps the thumbnail. Both sent and received full images live in
//! the link-preview image cache and follow its clean-up rules (QA.42).

pub mod check;
mod direct;
pub(crate) mod transfer;
pub mod wire;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use mumcord_p2p::quinn::{RecvStream, SendStream};
use tokio::sync::Notify;

use mumcord_protocol::{Handle, PluginData};

use crate::ServerId;
use crate::session::{Engine, EngineError, EngineEvent, EngineResult, ErrorKind, SessionCtx, lock};
use crate::web::{FullImage, LinkPreviews};
pub use transfer::FetchError;
use transfer::{Link, Originals, Transfers};
pub use wire::FullRef;

/// Feature announced in the Mumcord hello; full images are only requested from peers that
/// announced it, so older Mumcord clients and official clients are never asked.
pub const FEATURE: &str = "full-image";

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
/// With "Offer my full-size images" off nothing is served, so every request is declined.
fn originals(previews: Arc<LinkPreviews>, offer: bool) -> Originals {
    Arc::new(move |hash: &[u8; 32]| {
        if !offer {
            return None;
        }
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        let image = previews.full_image(&hex).filter(|image| image.own)?;
        let (_, payload) = image.data_url.split_once(";base64,")?;
        crate::people::base64_decode(payload)
    })
}

/// Stops one running fetch while it goes direct (the plugin-data part has its own cancel).
#[derive(Default)]
struct Cancel {
    cancelled: AtomicBool,
    wake: Notify,
}

impl Cancel {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        // `notify_one` keeps a permit for a fetch that is not waiting yet.
        self.wake.notify_one();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

/// The transfers of every connected server; sessions are only unique per server.
#[derive(Default)]
pub(crate) struct ServerTransfers {
    servers: Mutex<HashMap<ServerId, Arc<Transfers>>>,
    /// Running fetches by id.
    fetches: Mutex<HashMap<u64, RunningFetch>>,
    next_fetch: AtomicU64,
}

/// The server and image a running fetch is for.
type RunningFetch = (ServerId, [u8; 32], Arc<Cancel>);

/// Unregisters a fetch from [`ServerTransfers::fetches`] when it ends.
struct FetchGuard<'a> {
    transfers: &'a ServerTransfers,
    id: u64,
    cancel: Arc<Cancel>,
}

impl Drop for FetchGuard<'_> {
    fn drop(&mut self) {
        lock(&self.transfers.fetches).remove(&self.id);
    }
}

impl ServerTransfers {
    fn start_fetch(&self, server_id: ServerId, hash: [u8; 32]) -> FetchGuard<'_> {
        let id = self.next_fetch.fetch_add(1, Ordering::Relaxed);
        let cancel = Arc::new(Cancel::default());
        lock(&self.fetches).insert(id, (server_id, hash, cancel.clone()));
        FetchGuard {
            transfers: self,
            id,
            cancel,
        }
    }

    pub(crate) fn of(&self, server_id: ServerId) -> Arc<Transfers> {
        lock(&self.servers)
            .entry(server_id)
            .or_insert_with(|| Arc::new(Transfers::new(transfer::Timing::default())))
            .clone()
    }

    /// The connection to `server_id` (re)synced or ended: drops its transfers.
    pub(crate) fn reset(&self, server_id: ServerId) {
        if let Some(transfers) = lock(&self.servers).remove(&server_id) {
            transfers.reset();
        }
    }

    /// A person on `server_id` left.
    pub(crate) fn forget(&self, server_id: ServerId, session: u32) {
        if let Some(transfers) = lock(&self.servers).get(&server_id) {
            transfers.forget(session);
        }
    }

    fn cancel(&self, server_id: ServerId, hash: &[u8; 32]) {
        for (id, image, cancel) in lock(&self.fetches).values() {
            if *id == server_id && image == hash {
                cancel.cancel();
            }
        }
        if let Some(transfers) = lock(&self.servers).get(&server_id) {
            transfers.cancel(hash);
        }
    }
}

/// Plugin data from another client (session task).
pub(crate) fn on_plugin_data(
    transfers: &ServerTransfers,
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
    transfers.of(ctx.server_id).on_message(
        sender,
        msg,
        Arc::new(HandleLink(ctx.handle.clone())),
        originals(
            previews.clone(),
            setting(ctx.store().settings(), |s| s.offer_full_images),
        ),
    );
}

/// A request on a direct connection (the stream kind is already read).
pub(crate) async fn serve_direct(engine: &Engine, send: SendStream, recv: RecvStream) {
    direct::serve(engine, send, recv).await;
}

/// How the direct attempt of a fetch ended.
enum DirectOutcome {
    Loaded(Vec<u8>),
    /// Cancelled, or the sender said it does not have the image: plugin data would not help.
    Stop,
    /// No direct connection or it broke: try plugin data.
    Fallback,
}

/// A full-image switch from the settings; on if they cannot be read, as that is the default.
fn setting<E: std::fmt::Display>(
    settings: Result<mumcord_store::Settings, E>,
    pick: fn(&mumcord_store::Settings) -> bool,
) -> bool {
    settings.map_or_else(
        |e| {
            tracing::warn!(error = %e, "reading settings failed; using the default");
            true
        },
        |s| pick(&s),
    )
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
        if !setting(self.settings(), |s| s.offer_full_images) {
            return Err(EngineError::new(
                ErrorKind::InvalidInput,
                "offering full-size images is switched off",
            ));
        }
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

    /// Stops a running [`Self::full_image_fetch`] of the reference on `server_id` (the viewer
    /// closed); the fetch then returns `None`. Does nothing when no such fetch runs.
    pub fn full_image_cancel(&self, server_id: ServerId, reference: &str) {
        if let Some(full) = FullRef::parse(reference, check::MAX_BYTES) {
            self.full_image_transfers().cancel(server_id, &full.hash);
        }
    }

    /// The full image behind a thumbnail as a `data:` URL: from the cache, else fetched from the
    /// message's sender on `server_id` (identified by certificate hash or session) while they
    /// are online.
    /// Emits [`EngineEvent::FullImageProgress`] while loading. `None` when it cannot be had;
    /// the UI keeps showing the thumbnail then.
    pub async fn full_image_fetch(
        &self,
        server_id: ServerId,
        reference: &str,
        sender_session: Option<u32>,
        sender_cert_hash: Option<String>,
    ) -> Option<String> {
        if !setting(self.settings(), |s| s.load_full_images) {
            return None;
        }
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
        let ctx = self.session_ctx_for(server_id).ok()?;
        let from = online_sender(&ctx.handle, sender_session, sender_cert_hash.as_deref())?;
        if !self.mumcord_user_has_feature(server_id, from, FEATURE) {
            return None;
        }
        let progress = {
            let events = ctx.events.clone();
            let reference = reference.to_owned();
            move |received, total| {
                events.emit(EngineEvent::FullImageProgress {
                    reference: reference.clone(),
                    received,
                    total,
                });
            }
        };
        let fetch = self
            .full_image_transfers()
            .start_fetch(server_id, full.hash);
        let bytes = match self
            .full_image_direct(server_id, from, full, &fetch.cancel, progress.clone())
            .await
        {
            DirectOutcome::Loaded(bytes) => Ok(bytes),
            DirectOutcome::Stop => return None,
            DirectOutcome::Fallback if fetch.cancel.is_cancelled() => return None,
            DirectOutcome::Fallback => {
                self.full_image_transfers()
                    .of(server_id)
                    .fetch(
                        Arc::new(HandleLink(ctx.handle.clone())),
                        from,
                        full,
                        progress,
                    )
                    .await
            }
        };
        drop(fetch);
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

    /// Tries to fetch `full` from `from` over a direct connection (FR-2.37).
    async fn full_image_direct(
        &self,
        server_id: ServerId,
        from: u32,
        full: FullRef,
        cancel: &Cancel,
        progress: impl Fn(u32, u32),
    ) -> DirectOutcome {
        let attempt = async {
            let (send, recv) = match self
                .p2p_open_stream(server_id, from, crate::p2p::STREAM_FULL_IMAGE)
                .await
            {
                Ok(stream) => stream,
                Err(e) => {
                    tracing::info!(error = %e, "no direct connection; full image over plugin data");
                    return DirectOutcome::Fallback;
                }
            };
            match direct::fetch(send, recv, full, progress).await {
                Ok(bytes) => {
                    tracing::debug!(bytes = bytes.len(), "full image loaded directly");
                    DirectOutcome::Loaded(bytes)
                }
                Err(direct::DirectFetchError::Unavailable) => DirectOutcome::Stop,
                Err(e) => {
                    tracing::info!(error = %e, "direct full image failed; trying plugin data");
                    DirectOutcome::Fallback
                }
            }
        };
        tokio::select! {
            outcome = attempt => outcome,
            () = cancel.wake.notified() => DirectOutcome::Stop,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_images_are_only_served_while_offered() {
        let previews = Arc::new(LinkPreviews::new(
            None,
            LinkPreviews::app_policy(),
            Default::default(),
        ));
        let bytes = b"original".to_vec();
        let full = FullRef::of(&bytes);
        let hex = full.hash_hex();
        previews.keep_full_image(
            &hex,
            FullImage {
                data_url: format!("data:image/png;base64,{}", crate::people::base64(&bytes)),
                own: true,
            },
        );
        let mut key = [0u8; 32];
        for (i, b) in key.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("hex");
        }
        assert_eq!(originals(previews.clone(), true)(&key), Some(bytes));
        assert_eq!(
            originals(previews, false)(&key),
            None,
            "nothing served while off"
        );
    }
}
