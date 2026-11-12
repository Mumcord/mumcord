//! File sharing core (spec 07, FR-7.12 – FR-7.24, section 6.4): pure functions and types for the
//! offer message, the `mumcord.file` plugin data, file-name sanitizing and the risky-type list.
//! Everything here comes from or goes to other clients, so decoding validates every field.
//!
//! [`FileSharing`] ties the parts into the engine: [`sender`] posts offers and serves requests
//! through the global queue, [`recipient`] shows cards and runs the one download at a time. The
//! offer and `mumcord.file` encodings stay inside [`offer`] and [`wire`].

pub mod card;
mod motw;
pub mod offer;
pub mod prepare;
pub mod queue;
pub mod receive;
pub mod recipient;
pub mod risky;
pub mod sanitize;
pub mod sender;
pub mod share;
pub mod transfer;
pub mod wire;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use mumcord_protocol::PluginData;

use crate::ServerId;
use crate::session::{Engine, SessionCtx, WeakEngine, lock};
use wire::FileMessage;

/// Hello feature a client announces when it understands file offers (section 6.4).
pub const FEATURE: &str = "files";

/// Largest single file or zipped folder: 2 GiB (FR-7.12).
pub const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024 * 1024;

/// Whether `id` is a share id: 32 lowercase hex characters.
pub(crate) fn is_share_id(id: &str) -> bool {
    id.len() == 32 && is_lower_hex(id)
}

pub(crate) fn is_lower_hex(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Current time in Unix seconds.
pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// File sharing state of the engine: own shares and chips, and the cards of received offers.
#[derive(Default)]
pub(crate) struct FileSharing {
    engine: OnceLock<WeakEngine>,
    sender: Mutex<sender::Sender>,
    recipient: Mutex<recipient::Recipient>,
    /// The start-up removal of old zips; whoever writes into the same folder waits for it first.
    cleanup: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl FileSharing {
    /// Connects the state to its engine and removes zips a previous run left behind (nothing
    /// about shares survives a restart, FR-7.15).
    pub(crate) fn bind(&self, engine: &Engine) {
        let _ = self.engine.set(engine.downgrade());
        let leftovers = zip_cache(engine.data_dir()).join("files");
        let handle = std::thread::spawn(move || {
            if let Err(e) = std::fs::remove_dir_all(&leftovers)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(error = %e, "removing old shared-folder zips failed");
            }
        });
        *lock(&self.cleanup) = Some(handle);
    }

    /// Waits until the start-up removal is done, so it cannot delete a copy written meanwhile.
    pub(crate) fn finish_cleanup(&self) {
        let handle = lock(&self.cleanup).take();
        if let Some(handle) = handle
            && handle.join().is_err()
        {
            tracing::warn!("removing old shared-folder zips panicked");
        }
    }

    fn engine(&self) -> Option<Engine> {
        self.engine.get().and_then(WeakEngine::upgrade)
    }

    /// `mumcord.file` from another person (hook from the session; blocked people are filtered
    /// out before, see [`FileSharing::on_blocked_data`]).
    pub(crate) fn on_plugin_data(&self, ctx: &SessionCtx, data: &PluginData) {
        let Some((sender, msg)) = decode(ctx, data) else {
            return;
        };
        let Some(engine) = self.engine() else {
            return;
        };
        match msg {
            FileMessage::Request { .. } | FileMessage::Cancel { .. } => {
                engine.files_on_request(ctx.server_id, sender, &msg);
            }
            answer => engine.files_on_answer(ctx.server_id, sender, answer),
        }
    }

    /// A received offer, shown in the chat as a card (FR-7.19).
    pub(crate) fn on_offer(
        &self,
        server_id: ServerId,
        offer: offer::FileOffer,
        message: &crate::chat::ChatMessageView,
    ) -> Option<card::FileCardView> {
        let origin = recipient::OfferOrigin {
            sender_name: message.sender_name.clone(),
            sender_cert: message.sender_cert_hash.clone(),
            sender_session: message.sender_session,
            message_id: None,
            own: message.own,
        };
        Some(
            self.engine()?
                .files_register_offer(server_id, offer, origin, None),
        )
    }

    /// The offer's message was stored with id `message_id`.
    pub(crate) fn set_message(&self, server_id: ServerId, share_id: &str, message_id: &str) {
        if let Some(engine) = self.engine() {
            engine.files_set_message(server_id, share_id, message_id);
        }
    }

    /// A blocked person's request is refused silently with `gone` (FR-7.18).
    pub(crate) fn on_blocked_data(&self, ctx: &SessionCtx, data: &PluginData) {
        if let Some((sender, FileMessage::Request { id, .. })) = decode(ctx, data)
            && let Some(engine) = self.engine()
        {
            engine.files_refuse_blocked(ctx.server_id, sender, &id);
        }
    }

    /// A person joined `server_id`.
    pub(crate) fn on_user_joined(&self, server_id: ServerId, cert_hash: Option<&str>) {
        if let Some(engine) = self.engine() {
            engine.files_sender_joined(server_id, cert_hash);
        }
    }

    /// A person left `server_id`: as recipient of own shares and as sender of offers.
    pub(crate) fn on_user_removed(&self, server_id: ServerId, session: u32) {
        if let Some(engine) = self.engine() {
            engine.files_requester_left(server_id, session);
            engine.files_sender_left(server_id, session);
        }
    }

    /// The own connection to `server_id` ended or started afresh: own shares there end and a
    /// download from there fails (FR-7.15, FR-7.28). Runs as a task: the session calls this
    /// while it holds its connection state.
    pub(crate) fn on_connection_reset(&self, server_id: ServerId) {
        if let Some(engine) = self.engine() {
            let runtime = engine.runtime().clone();
            runtime.spawn(async move {
                engine.files_left_server(server_id);
                engine.files_connection_reset(server_id);
            });
        }
    }
}

/// The sender and message of `mumcord.file` data from someone else.
fn decode(ctx: &SessionCtx, data: &PluginData) -> Option<(u32, FileMessage)> {
    if data.data_id != wire::DATA_ID {
        return None;
    }
    let sender = data.sender?;
    let (own, known) = ctx
        .handle
        .with_state(|s| (s.session, s.users.contains_key(&sender)));
    if own == Some(sender) || !known {
        return None;
    }
    let msg = wire::decode(&data.data);
    if msg.is_none() {
        tracing::debug!("ignoring a malformed file message");
    }
    Some((sender, msg?))
}

/// Where folders are zipped before they are shared (FR-7.13).
pub(crate) fn zip_cache(data_dir: &Path) -> PathBuf {
    data_dir.join("file-shares")
}
