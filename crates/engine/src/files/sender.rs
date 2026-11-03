//! The sending side (FR-7.11 – FR-7.18, section 7.3): composer chips, posting one offer message
//! per file, answering requests through the global queue, sending the file, and ending shares.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::task::AbortHandle;

use super::card::OwnShareView;
use super::offer::{FileOffer, encode_offer_html};
use super::prepare::{self, Cancel, FileStamp, PrepareError};
use super::queue::{Request, Requester, SenderQueue, Step};
use super::share::{EndReason, Ended, Share, ShareRegistry, lifetime, new_share_id};
use super::wire::{self, FileMessage};
use super::{FileSharing, unix_now};
use crate::ServerId;
use crate::chat::ChatMessageView;
use crate::session::{Engine, EngineError, EngineEvent, EngineResult, ErrorKind, lock};

/// At most this many files per message (FR-7.11).
pub const MAX_CHIPS: usize = 10;

/// A file or folder in the composer's attachment tray (mockup 1b).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChipView {
    pub id: u64,
    /// File or folder name as picked; a folder gets `.zip` once zipped.
    pub name: String,
    /// Known once the file is ready.
    pub size: Option<u64>,
    pub state: ChipState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChipState {
    /// "Zipping folder… {percent} %" (FR-7.13).
    Zipping {
        done: u64,
        total: u64,
    },
    Ready,
    /// "Checking… {percent} %" while the SHA-256 is computed on send (FR-7.14).
    Checking {
        done: u64,
        total: u64,
    },
    Failed(ChipError),
    /// The offer message was posted; the chip leaves the tray.
    Sent,
}

/// Why a chip could not be shared; the UI shows the matching `files.composer` text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChipError {
    /// "{name} is larger than 2 GB and can't be shared"
    TooLarge(String),
    /// "Couldn't read {path}"
    Unreadable(PathBuf),
    /// "The server refused the message: {reason}"
    ServerRefused(String),
    /// Anything else, with the system's text.
    Other(String),
}

impl From<PrepareError> for ChipError {
    fn from(e: PrepareError) -> Self {
        match e {
            PrepareError::TooLarge(name) => Self::TooLarge(name),
            PrepareError::Unreadable(path) => Self::Unreadable(path),
            other => Self::Other(other.to_string()),
        }
    }
}

struct Chip {
    view: ChipView,
    /// The file to send once ready: the picked file or the zip in the cache.
    path: Option<PathBuf>,
    owns_zip: bool,
    cancel: Cancel,
}

/// The transfer that runs now (one per sender, FR-7.17).
struct Running {
    request: Request,
    task: AbortHandle,
    recipient_name: String,
    percent: u8,
}

#[derive(Default)]
pub(crate) struct Sender {
    chips: HashMap<u64, Chip>,
    next_chip: u64,
    shares: ShareRegistry,
    queue: SenderQueue,
    running: Option<Running>,
    /// Shares that ended during this run, for the "Sharing ended" note on the own card.
    ended: HashMap<String, EndReason>,
    sweeping: bool,
}

impl Sender {
    pub(crate) fn own_share_view(&self, share_id: &str) -> OwnShareView {
        let Some(share) = self.shares.get(share_id) else {
            return OwnShareView {
                ended: Some(self.ended.get(share_id).copied()),
                downloaded_by: Vec::new(),
                sending_to: None,
                waiting: 0,
            };
        };
        OwnShareView {
            ended: None,
            downloaded_by: share.downloaded_by.clone(),
            sending_to: self
                .running
                .as_ref()
                .filter(|r| r.request.share_id == share_id)
                .map(|r| (r.recipient_name.clone(), r.percent)),
            waiting: self.queue.waiting_for(share_id),
        }
    }
}

/// How often expired shares are looked for.
const SWEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// A guess of the MIME type from the extension; display only (section 6.4).
fn mime_of(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "ogg" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "txt" | "log" | "md" => "text/plain",
        _ => "application/octet-stream",
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl Engine {
    fn file_sharing(&self) -> &FileSharing {
        self.files()
    }

    fn emit_chip(&self, view: ChipView) {
        self.events().emit(EngineEvent::FileChipChanged(view));
    }

    fn update_chip(&self, id: u64, state: ChipState) {
        let view = {
            let mut sender = lock(&self.file_sharing().sender);
            let Some(chip) = sender.chips.get_mut(&id) else {
                return;
            };
            chip.view.state = state;
            chip.view.clone()
        };
        self.emit_chip(view);
    }

    /// Adds a file or folder to the attachment tray (FR-7.11 – FR-7.13). A folder is zipped into
    /// Mumcord's cache right away; the chip shows the progress. A file over 2 GB or one that
    /// can't be read gives a failed chip.
    pub fn files_chip_add(&self, path: PathBuf) -> ChipView {
        let is_dir = path.is_dir();
        let (id, cancel) = {
            let mut sender = lock(&self.file_sharing().sender);
            sender.next_chip += 1;
            (sender.next_chip, Cancel::default())
        };
        let mut chip = Chip {
            view: ChipView {
                id,
                name: file_name(&path),
                size: None,
                state: ChipState::Ready,
            },
            path: None,
            owns_zip: is_dir,
            cancel: cancel.clone(),
        };
        if is_dir {
            chip.view.name.push_str(".zip");
            chip.view.state = ChipState::Zipping { done: 0, total: 0 };
            self.spawn_zip(id, path, cancel);
        } else {
            match prepare::check_file(&path) {
                Ok(stamp) => {
                    chip.view.size = Some(stamp.size);
                    chip.path = Some(path);
                }
                Err(e) => chip.view.state = ChipState::Failed(e.into()),
            }
        }
        let view = chip.view.clone();
        lock(&self.file_sharing().sender).chips.insert(id, chip);
        view
    }

    fn spawn_zip(&self, id: u64, src: PathBuf, cancel: Cancel) {
        let engine = self.downgrade();
        let progress_engine = self.downgrade();
        let cache = super::zip_cache(self.data_dir());
        let mut last_percent = None;
        let progress: prepare::Progress = Box::new(move |done, total| {
            let percent = done.saturating_mul(100).checked_div(total).unwrap_or(0);
            if last_percent == Some(percent) {
                return;
            }
            last_percent = Some(percent);
            if let Some(engine) = progress_engine.upgrade() {
                engine.update_chip(id, ChipState::Zipping { done, total });
            }
        });
        self.runtime().spawn(async move {
            let result = prepare::zip_folder_async(src, cache, cancel, progress).await;
            let Some(engine) = engine.upgrade() else {
                return;
            };
            let view = {
                let mut sender = lock(&engine.file_sharing().sender);
                let Some(chip) = sender.chips.get_mut(&id) else {
                    // Removed while zipping finished: nobody wants the zip any more.
                    if let Ok(zip) = &result {
                        prepare::remove_zip(zip);
                    }
                    return;
                };
                match result {
                    Ok(zip) => {
                        chip.view.size = FileStamp::of(&zip).ok().map(|s| s.size);
                        chip.view.state = ChipState::Ready;
                        chip.path = Some(zip);
                    }
                    Err(PrepareError::Cancelled) => return,
                    Err(e) => chip.view.state = ChipState::Failed(e.into()),
                }
                chip.view.clone()
            };
            engine.emit_chip(view);
        });
    }

    /// Removes a chip; a running zip stops and its zip is deleted (FR-7.13).
    pub fn files_chip_remove(&self, id: u64) {
        let Some(chip) = lock(&self.file_sharing().sender).chips.remove(&id) else {
            return;
        };
        chip.cancel.cancel();
        if chip.owns_zip
            && let Some(zip) = chip.path
        {
            self.runtime()
                .spawn_blocking(move || prepare::remove_zip(&zip));
        }
    }

    /// Posts one offer message per chip into `channel_id` (FR-7.14): computes each file's
    /// SHA-256 ("Checking…"), creates the share id and sends the message. A refused message
    /// fails its chip with the server's reason and creates no share. Returns the posted own
    /// messages; failed chips stay in the tray.
    pub async fn files_send(
        &self,
        server_id: ServerId,
        channel_id: u32,
        chip_ids: Vec<u64>,
    ) -> EngineResult<Vec<ChatMessageView>> {
        if chip_ids.len() > MAX_CHIPS {
            return Err(EngineError::invalid("You can share up to 10 files at once"));
        }
        if let Err(reason) = self.files_availability(server_id) {
            return Err(EngineError::new(
                ErrorKind::PermissionDenied,
                format!("file sharing is not available: {reason:?}"),
            ));
        }
        let mut posted = Vec::new();
        for id in chip_ids {
            if let Some(message) = self.send_chip(server_id, channel_id, id).await {
                posted.push(message);
            }
        }
        tracing::info!(files = posted.len(), "file offers posted");
        Ok(posted)
    }

    async fn send_chip(
        &self,
        server_id: ServerId,
        channel_id: u32,
        id: u64,
    ) -> Option<ChatMessageView> {
        let (path, name, owns_zip, cancel) = {
            let sender = lock(&self.file_sharing().sender);
            let chip = sender.chips.get(&id)?;
            if chip.view.state != ChipState::Ready {
                return None;
            }
            (
                chip.path.clone()?,
                chip.view.name.clone(),
                chip.owns_zip,
                chip.cancel.clone(),
            )
        };
        self.update_chip(id, ChipState::Checking { done: 0, total: 0 });
        let progress_engine = self.downgrade();
        let mut last_percent = None;
        let progress: prepare::Progress = Box::new(move |done, total| {
            let percent = done.saturating_mul(100).checked_div(total).unwrap_or(0);
            if last_percent != Some(percent) {
                last_percent = Some(percent);
                if let Some(engine) = progress_engine.upgrade() {
                    engine.update_chip(id, ChipState::Checking { done, total });
                }
            }
        });
        let sha256 = match prepare::hash_file_async(path.clone(), cancel, progress).await {
            Ok(hash) => hash,
            Err(PrepareError::Cancelled) => return None,
            Err(e) => {
                self.update_chip(id, ChipState::Failed(e.into()));
                return None;
            }
        };
        // The stamp after hashing: a later change ends the share before a transfer (FR-7.15).
        let stamp = match FileStamp::of(&path) {
            Ok(stamp) => stamp,
            Err(_) => {
                self.update_chip(id, ChipState::Failed(ChipError::Unreadable(path)));
                return None;
            }
        };
        let offer = FileOffer {
            id: new_share_id(),
            mime: mime_of(&name).to_owned(),
            name,
            size: stamp.size,
            sha256,
            expires: unix_now() + lifetime().as_secs(),
        };
        let own_name = self
            .session_ctx_for(server_id)
            .ok()?
            .handle
            .with_state(|s| s.own_user().map(|u| u.name.clone()))
            .unwrap_or_default();
        let html = match encode_offer_html(&own_name, &offer) {
            Ok(html) => html,
            Err(e) => {
                self.update_chip(id, ChipState::Failed(ChipError::Other(e.to_string())));
                return None;
            }
        };
        // Registered before posting, so a request that races the own echo finds the share.
        lock(&self.file_sharing().sender).shares.insert(Share {
            id: offer.id.clone(),
            server_id,
            path: path.clone(),
            name: offer.name.clone(),
            size: offer.size,
            mime: offer.mime.clone(),
            sha256: offer.sha256.clone(),
            stamp,
            expires: offer.expires,
            owns_zip,
            downloaded_by: Vec::new(),
        });
        match self
            .send_channel_message(server_id, channel_id, &html)
            .await
        {
            Ok(mut message) => {
                let origin = super::recipient::OfferOrigin {
                    sender_name: message.sender_name.clone(),
                    sender_cert: message.sender_cert_hash.clone(),
                    sender_session: message.sender_session,
                    message_id: message.id.parse().ok(),
                    own: true,
                };
                message.file = Some(self.files_register_offer(server_id, offer, origin, None));
                let removed = lock(&self.file_sharing().sender).chips.remove(&id);
                if let Some(chip) = removed {
                    self.emit_chip(ChipView {
                        state: ChipState::Sent,
                        ..chip.view
                    });
                }
                self.ensure_sweeping();
                Some(message)
            }
            Err(e) => {
                // No share without its message; the zip stays with the chip for another try.
                lock(&self.file_sharing().sender).shares.withdraw(&offer.id);
                self.update_chip(id, ChipState::Failed(ChipError::ServerRefused(e.message)));
                None
            }
        }
    }

    /// Stop sharing (FR-7.15); the card keeps showing "Sharing ended".
    pub fn files_stop(&self, share_id: &str) {
        let ended = lock(&self.file_sharing().sender).shares.stop(share_id);
        self.shares_ended(ended.into_iter().collect());
    }

    /// Ends shares: waiting and running recipients get `gone`, the card shows the reason.
    pub(crate) fn shares_ended(&self, ended: Vec<Ended>) {
        for ended in ended {
            let step = {
                let mut sender = lock(&self.file_sharing().sender);
                sender.ended.insert(ended.share.id.clone(), ended.reason);
                sender.queue.end_share(&ended.share.id)
            };
            self.apply_step(step);
            self.emit_own_card(ended.share.server_id, &ended.share.id);
        }
    }

    fn ensure_sweeping(&self) {
        {
            let mut sender = lock(&self.file_sharing().sender);
            if sender.sweeping {
                return;
            }
            sender.sweeping = true;
        }
        let engine = self.downgrade();
        self.runtime().spawn(async move {
            loop {
                tokio::time::sleep(SWEEP_EVERY).await;
                let Some(engine) = engine.upgrade() else {
                    return;
                };
                let ended = {
                    let mut sender = lock(&engine.file_sharing().sender);
                    let ended = sender.shares.sweep_expired(unix_now());
                    if sender.shares.is_empty() {
                        sender.sweeping = false;
                    }
                    ended
                };
                let done = !lock(&engine.file_sharing().sender).sweeping;
                engine.shares_ended(ended);
                if done {
                    return;
                }
            }
        });
    }

    /// A `req` or `cancel` from `session` (FR-7.17, FR-7.18).
    pub(crate) fn files_on_request(&self, server_id: ServerId, session: u32, msg: &FileMessage) {
        let requester = Requester { server_id, session };
        let step = {
            let mut sender = lock(&self.file_sharing().sender);
            match msg {
                FileMessage::Request { id } => {
                    let active = sender
                        .shares
                        .get(id)
                        .is_some_and(|s| s.server_id == server_id);
                    sender.queue.request(id, requester, false, active)
                }
                FileMessage::Cancel { id } => sender.queue.cancel(id, requester),
                _ => return,
            }
        };
        self.apply_step(step);
    }

    /// A blocked person asked for a file: refused silently with `gone` (FR-7.18).
    pub(crate) fn files_refuse_blocked(&self, server_id: ServerId, session: u32, id: &str) {
        let step = lock(&self.file_sharing().sender).queue.request(
            id,
            Requester { server_id, session },
            true,
            false,
        );
        self.apply_step(step);
    }

    /// A recipient left the server: their requests are dropped (FR-7.17).
    pub(crate) fn files_requester_left(&self, server_id: ServerId, session: u32) {
        let step = lock(&self.file_sharing().sender)
            .queue
            .drop_requester(Requester { server_id, session });
        self.apply_step(step);
    }

    /// The own connection to `server_id` ended: its shares end (FR-7.15).
    pub(crate) fn files_left_server(&self, server_id: ServerId) {
        let (ended, step) = {
            let mut sender = lock(&self.file_sharing().sender);
            let ended = sender.shares.end_server(server_id);
            (ended, sender.queue.end_server(server_id))
        };
        self.apply_step(step);
        self.shares_ended(ended);
    }

    /// Mumcord quits: every share ends and the zips (also of unsent chips) are deleted.
    pub(crate) fn files_quit(&self) {
        let mut sender = lock(&self.file_sharing().sender);
        if let Some(running) = sender.running.take() {
            running.task.abort();
        }
        let ended = sender.shares.end_all();
        for chip in sender.chips.drain().map(|(_, c)| c) {
            chip.cancel.cancel();
            if chip.owns_zip
                && let Some(zip) = chip.path
            {
                prepare::remove_zip(&zip);
            }
        }
        tracing::info!(shares = ended.len(), "file shares ended on quit");
    }

    /// Carries out a queue change: aborts, starts (after the file check of FR-7.15) and sends.
    fn apply_step(&self, mut step: Step) {
        let mut ended = Vec::new();
        let mut aborted = Vec::new();
        let mut started = None;
        loop {
            let mut sender = lock(&self.file_sharing().sender);
            if let Some(abort) = step.abort.take() {
                if let Some(running) = sender.running.take_if(|r| r.request == abort) {
                    running.task.abort();
                }
                aborted.push(abort);
            }
            let Some(start) = step.start.take() else {
                break;
            };
            match sender.shares.verify_unchanged(&start.share_id) {
                Some(Ok(share)) => {
                    let share = share.clone();
                    // Set before the lock is released, so the task's first update finds it.
                    let task = self.spawn_transfer(&start, &share);
                    sender.running = Some(Running {
                        request: start.clone(),
                        task,
                        recipient_name: String::new(),
                        percent: 0,
                    });
                    started = Some(start);
                    break;
                }
                changed => {
                    // No `go` for a share that just ended; ending it answers `gone` instead.
                    step.send.retain(|o| {
                        !(o.to == start.requester
                            && matches!(&o.message, FileMessage::Go { id } if *id == start.share_id))
                    });
                    if let Some(Err(e)) = changed {
                        ended.push(e);
                    }
                    let more = sender.queue.end_share(&start.share_id);
                    step.send.extend(more.send);
                    // `more.abort` is `start` itself, which never ran.
                    step.start = more.start;
                }
            }
        }
        if let Some(start) = &started {
            let name = self
                .session_ctx_for(start.requester.server_id)
                .ok()
                .and_then(|ctx| {
                    ctx.handle.with_state(|s| {
                        s.users
                            .get(&start.requester.session)
                            .map(|u| u.name.clone())
                    })
                })
                .unwrap_or_default();
            if let Some(running) = lock(&self.file_sharing().sender)
                .running
                .as_mut()
                .filter(|r| &r.request == start)
            {
                running.recipient_name = name;
            }
        }
        self.send_outgoing(step.send);
        for request in aborted {
            self.emit_own_card(request.requester.server_id, &request.share_id);
        }
        self.shares_ended(ended);
    }

    fn send_outgoing(&self, send: Vec<super::queue::Outgoing>) {
        for out in send {
            let Some(data) = wire::encode(&out.message) else {
                continue;
            };
            let Ok(ctx) = self.session_ctx_for(out.to.server_id) else {
                continue;
            };
            if let Err(e) = ctx
                .handle
                .send_plugin_data(&[out.to.session], wire::DATA_ID, data)
            {
                tracing::debug!(error = %e, "file message not sent");
            }
            if let FileMessage::Queued { id, .. } | FileMessage::Go { id } = &out.message {
                self.emit_own_card(out.to.server_id, id);
            }
        }
    }

    fn spawn_transfer(&self, request: &Request, share: &Share) -> AbortHandle {
        let engine = self.downgrade();
        let request = request.clone();
        let share = share.clone();
        self.runtime()
            .spawn(async move {
                let Some(engine) = engine.upgrade() else {
                    return;
                };
                let progress_engine = engine.clone();
                let progress_request = request.clone();
                let size = share.size;
                let result = engine
                    .file_send(
                        request.requester.server_id,
                        request.requester.session,
                        &share.id,
                        &share.path,
                        size,
                        move |sent| {
                            progress_engine.transfer_progress(&progress_request, sent, size)
                        },
                    )
                    .await;
                engine.transfer_finished(&request, &share, result.is_ok());
                if let Err(e) = result {
                    tracing::info!(error = %e, "file transfer failed");
                }
            })
            .abort_handle()
    }

    fn transfer_progress(&self, request: &Request, sent: u64, size: u64) {
        let percent =
            u8::try_from(sent.saturating_mul(100).checked_div(size).unwrap_or(100)).unwrap_or(100);
        {
            let mut sender = lock(&self.file_sharing().sender);
            let Some(running) = sender.running.as_mut().filter(|r| &r.request == request) else {
                return;
            };
            if running.percent == percent {
                return;
            }
            running.percent = percent;
        }
        self.emit_own_card(request.requester.server_id, &request.share_id);
    }

    fn transfer_finished(&self, request: &Request, share: &Share, ok: bool) {
        let step = {
            let mut sender = lock(&self.file_sharing().sender);
            if sender.running.as_ref().map(|r| &r.request) != Some(request) {
                return;
            }
            let running = sender.running.take();
            if ok && let Some(running) = running {
                sender
                    .shares
                    .record_download(&share.id, &running.recipient_name);
            }
            sender.queue.finish()
        };
        self.apply_step(step);
        self.emit_own_card(share.server_id, &share.id);
    }

    /// The own card's sharing details changed.
    fn emit_own_card(&self, server_id: ServerId, share_id: &str) {
        if let Some(view) = self.file_card(server_id, share_id) {
            self.events()
                .for_server(server_id)
                .emit(EngineEvent::FileCardChanged(Box::new(view)));
        }
    }

    /// The sharing details of an own card.
    pub(crate) fn own_share_view(&self, share_id: &str) -> OwnShareView {
        lock(&self.file_sharing().sender).own_share_view(share_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_types_are_guessed_from_the_extension() {
        assert_eq!(mime_of("Photo.JPG"), "image/jpeg");
        assert_eq!(mime_of("notes.txt"), "text/plain");
        assert_eq!(mime_of("folder.zip"), "application/zip");
        assert_eq!(mime_of("README"), "application/octet-stream");
    }

    #[test]
    fn prepare_errors_map_to_chip_errors() {
        assert_eq!(
            ChipError::from(PrepareError::TooLarge("big.iso".into())),
            ChipError::TooLarge("big.iso".into())
        );
        assert_eq!(
            ChipError::from(PrepareError::Unreadable(PathBuf::from("/x"))),
            ChipError::Unreadable(PathBuf::from("/x"))
        );
    }

    async fn engine() -> (tempfile::TempDir, Engine) {
        let dir = tempfile::tempdir().expect("tempdir");
        let (engine, _rx) = Engine::new(
            dir.path(),
            crate::SecretStore::in_memory(),
            tokio::runtime::Handle::current(),
        )
        .expect("engine");
        (dir, engine)
    }

    fn chip_state(engine: &Engine, id: u64) -> Option<ChipState> {
        lock(&engine.files().sender)
            .chips
            .get(&id)
            .map(|c| c.view.state.clone())
    }

    fn share_of(path: &Path, owns_zip: bool) -> Share {
        Share {
            id: new_share_id(),
            server_id: 1,
            path: path.to_path_buf(),
            name: file_name(path),
            size: 3,
            mime: "text/plain".into(),
            sha256: "a".repeat(64),
            stamp: FileStamp::of(path).expect("stamp"),
            expires: unix_now() + 60,
            owns_zip,
            downloaded_by: Vec::new(),
        }
    }

    #[tokio::test]
    async fn files_are_ready_at_once_and_big_ones_are_refused() {
        let (dir, engine) = engine().await;
        let small = dir.path().join("notes.txt");
        std::fs::write(&small, b"abc").expect("file");
        let chip = engine.files_chip_add(small);
        assert_eq!((chip.state, chip.size), (ChipState::Ready, Some(3)));
        let big = dir.path().join("big.iso");
        std::fs::File::create(&big)
            .and_then(|f| f.set_len(super::super::MAX_FILE_SIZE + 1))
            .expect("sparse file");
        let chip = engine.files_chip_add(big);
        assert_eq!(
            chip.state,
            ChipState::Failed(ChipError::TooLarge("big.iso".into()))
        );
    }

    #[tokio::test]
    async fn folders_are_zipped_and_removing_the_chip_deletes_the_zip() {
        let (dir, engine) = engine().await;
        let folder = dir.path().join("Photos");
        std::fs::create_dir(&folder).expect("folder");
        std::fs::write(folder.join("a.txt"), b"hello").expect("file");
        let chip = engine.files_chip_add(folder);
        assert_eq!(chip.name, "Photos.zip");
        assert!(matches!(chip.state, ChipState::Zipping { .. }));
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        while chip_state(&engine, chip.id) != Some(ChipState::Ready) {
            assert!(tokio::time::Instant::now() < deadline, "zip did not finish");
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let zip = lock(&engine.files().sender).chips[&chip.id]
            .path
            .clone()
            .expect("zip path");
        assert!(zip.is_file());
        engine.files_chip_remove(chip.id);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        while zip.exists() {
            assert!(tokio::time::Instant::now() < deadline, "zip not removed");
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(chip_state(&engine, chip.id), None);
    }

    #[tokio::test]
    async fn sending_needs_file_sharing_and_at_most_ten_files() {
        let (_dir, engine) = engine().await;
        let too_many = engine.files_send(1, 0, (0..11).collect()).await;
        assert_eq!(too_many.expect_err("refused").kind, ErrorKind::InvalidInput);
        let unavailable = engine.files_send(1, 0, vec![1]).await;
        assert_eq!(
            unavailable.expect_err("refused").kind,
            ErrorKind::PermissionDenied
        );
    }

    #[tokio::test]
    async fn a_file_changed_on_disk_ends_the_share_before_the_transfer() {
        let (dir, engine) = engine().await;
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"abc").expect("file");
        let share = share_of(&path, false);
        let id = share.id.clone();
        lock(&engine.files().sender).shares.insert(share);
        std::fs::write(&path, b"abcd").expect("changed");
        engine.files_on_request(1, 7, &FileMessage::Request { id: id.clone() });
        let view = engine.own_share_view(&id);
        assert_eq!(view.ended, Some(Some(EndReason::FileChanged)));
        assert!(lock(&engine.files().sender).queue.running().is_none());
    }

    #[tokio::test]
    async fn requests_for_shares_of_another_server_are_not_served() {
        let (dir, engine) = engine().await;
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"abc").expect("file");
        let share = share_of(&path, false);
        let id = share.id.clone();
        lock(&engine.files().sender).shares.insert(share);
        engine.files_on_request(2, 7, &FileMessage::Request { id: id.clone() });
        assert!(lock(&engine.files().sender).queue.running().is_none());
        engine.files_on_request(1, 7, &FileMessage::Request { id: id.clone() });
        assert!(
            lock(&engine.files().sender).queue.running().is_some(),
            "the transfer starts"
        );
        assert_eq!(engine.own_share_view(&id).ended, None);
    }

    #[tokio::test]
    async fn stopping_and_leaving_end_shares_and_delete_own_zips() {
        let (dir, engine) = engine().await;
        let zip_dir = dir.path().join("z");
        std::fs::create_dir(&zip_dir).expect("dir");
        let zip = zip_dir.join("f.zip");
        std::fs::write(&zip, b"zip").expect("zip");
        let stopped = share_of(&zip, true);
        let stopped_id = stopped.id.clone();
        let other = dir.path().join("b.txt");
        std::fs::write(&other, b"abc").expect("file");
        let left = share_of(&other, false);
        let left_id = left.id.clone();
        {
            let mut sender = lock(&engine.files().sender);
            sender.shares.insert(stopped);
            sender.shares.insert(left);
        }
        engine.files_stop(&stopped_id);
        assert_eq!(
            engine.own_share_view(&stopped_id).ended,
            Some(Some(EndReason::Stopped))
        );
        assert!(!zip.exists());
        engine.files_left_server(1);
        assert_eq!(
            engine.own_share_view(&left_id).ended,
            Some(Some(EndReason::LeftServer))
        );
        assert!(other.exists(), "the user's own file stays");
    }

    #[test]
    fn an_unknown_share_shows_as_ended_before_this_run() {
        let sender = Sender::default();
        let view = sender.own_share_view("0123456789abcdef0123456789abcdef");
        assert_eq!(view.ended, Some(None));
    }
}
