//! The receiving side (FR-7.19 – FR-7.22, FR-7.25 – FR-7.29, FR-7.34, section 7.2): file cards,
//! availability, and the one download a recipient runs at a time across all servers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mumcord_p2p::RelayUse;
use mumcord_store::FileCardRecord;
use tokio::sync::{mpsc, oneshot};

use super::card::{
    CardState, DownloadBlock, DownloadContext, FailReason, FileCardView, FilesUnavailable,
    Unavailable, availability, download_block, shown_state,
};
use super::offer::FileOffer;
use super::receive::{IncomingFile, ReceiveError};
use super::transfer::{ANSWER_TIMEOUT, TransferError};
use super::unix_now;
use super::wire::{self, FileMessage};
use crate::ServerId;
use crate::p2p::DirectError;
use crate::session::{Engine, EngineError, EngineEvent, EngineResult, ErrorKind, lock};

/// Read size of the file stream.
const CHUNK: usize = 256 * 1024;
/// Progress updates of a running download at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// One offer seen in a chat on one server.
#[derive(Debug, Clone)]
struct Card {
    offer: FileOffer,
    sender_name: String,
    sender_cert: Option<String>,
    /// The session that posted the offer, while it is known on this connection.
    sender_session: Option<u32>,
    message_id: Option<i64>,
    own: bool,
    state: CardState,
}

/// Why a running download was stopped from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// The user pressed Cancel.
    User,
    /// The sender left before the transfer began.
    SenderLeft,
    /// The own connection ended.
    Disconnected,
}

struct Download {
    server_id: ServerId,
    share_id: String,
    session: u32,
    answers: mpsc::UnboundedSender<FileMessage>,
    stop: Option<oneshot::Sender<Stop>>,
}

#[derive(Default)]
pub(crate) struct Recipient {
    cards: HashMap<(ServerId, String), Card>,
    download: Option<Download>,
}

/// What a received offer is known by.
pub(crate) struct OfferOrigin {
    pub sender_name: String,
    pub sender_cert: Option<String>,
    pub sender_session: Option<u32>,
    pub message_id: Option<i64>,
    pub own: bool,
}

fn failed(e: TransferError) -> CardState {
    CardState::Failed(match e {
        TransferError::Connect(DirectError::Relay(reason)) => FailReason::Connect(Some(reason)),
        TransferError::Connect(_) => FailReason::Connect(None),
        TransferError::Stalled => FailReason::Stalled,
        TransferError::ConnectionLost | TransferError::Refused => FailReason::ConnectionLost,
        e @ (TransferError::Read(_) | TransferError::BadShareId) => FailReason::Io(e.to_string()),
    })
}

fn receive_failed(e: ReceiveError) -> CardState {
    match e {
        ReceiveError::NoSpace { folder } => CardState::Failed(FailReason::NoSpace(folder)),
        ReceiveError::Mismatch => CardState::Mismatch,
        ReceiveError::Io(e) => CardState::Failed(FailReason::Io(e.to_string())),
    }
}

fn record_of(state: &CardState) -> Option<FileCardRecord> {
    match state {
        CardState::Done { path } => Some(FileCardRecord::Done {
            path: path.to_string_lossy().into_owned(),
        }),
        CardState::Mismatch => Some(FileCardRecord::Mismatch),
        _ => None,
    }
}

fn state_of(record: FileCardRecord) -> CardState {
    match record {
        FileCardRecord::Done { path } => CardState::Done {
            path: PathBuf::from(path),
        },
        FileCardRecord::Mismatch => CardState::Mismatch,
    }
}

impl Engine {
    fn recipient(&self) -> std::sync::MutexGuard<'_, Recipient> {
        lock(&self.files().recipient)
    }

    /// Whether file sharing works on `server_id` (FR-7.9): the paperclip button's state.
    pub fn files_availability(&self, server_id: ServerId) -> Result<(), FilesUnavailable> {
        let relay = self.relays().availability_for(server_id, RelayUse::Files);
        let has_certificate = self.session_ctx_for(server_id).is_ok_and(|ctx| {
            ctx.handle
                .with_state(|s| s.own_user().is_some_and(|u| u.cert_hash.is_some()))
        });
        let mode = self.settings().map(|s| s.p2p_mode).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "reading settings failed; using the default");
            Default::default()
        });
        availability(relay, has_certificate, mode)
    }

    /// The folder downloads are saved to (FR-7.25): the setting, else `downloads` (the system
    /// Downloads folder, which only the app layer knows).
    pub fn files_save_folder(&self, downloads: &Path) -> PathBuf {
        self.settings()
            .ok()
            .and_then(|s| s.files_save_folder)
            .map_or_else(|| downloads.to_path_buf(), PathBuf::from)
    }

    /// The session that posted the card's offer, if it is online and not this client.
    fn card_sender(&self, server_id: ServerId, card: &Card) -> Option<u32> {
        let ctx = self.session_ctx_for(server_id).ok()?;
        ctx.handle.with_state(|s| {
            let matches = |session: u32| {
                s.users
                    .get(&session)
                    .is_some_and(|u| match &card.sender_cert {
                        Some(cert) => u.cert_hash.as_ref() == Some(cert),
                        None => true,
                    })
            };
            let found = card.sender_session.filter(|s| matches(*s)).or_else(|| {
                let cert = card.sender_cert.as_ref()?;
                s.users
                    .values()
                    .find(|u| u.cert_hash.as_ref() == Some(cert))
                    .map(|u| u.session)
            })?;
            (s.session != Some(found)).then_some(found)
        })
    }

    fn card_view(
        &self,
        server_id: ServerId,
        card: &Card,
        busy: Option<(ServerId, &str)>,
    ) -> FileCardView {
        let mut view = FileCardView::new(&card.offer, &card.sender_name, card.own);
        if card.own {
            view.state = CardState::Available;
            view.download_block = Some(DownloadBlock::Own);
            view.share = Some(self.own_share_view(&card.offer.id));
            return view;
        }
        let online = self.card_sender(server_id, card).is_some();
        let expired = card.offer.expires <= unix_now();
        view.state = shown_state(&card.state, expired, online);
        if view.state.can_start() {
            let settings = self.settings().ok();
            let cx = DownloadContext {
                own: false,
                availability: self.files_availability(server_id),
                receive: settings.as_ref().is_none_or(|s| s.files_receive),
                limit: settings.map(|s| s.files_max_size).unwrap_or_default(),
                other_download: busy.is_some_and(|b| b != (server_id, card.offer.id.as_str())),
            };
            view.download_block = download_block(card.offer.size, &cx);
        }
        view
    }

    /// The current card of share `share_id` on `server_id`.
    pub fn file_card(&self, server_id: ServerId, share_id: &str) -> Option<FileCardView> {
        let (card, busy) = {
            let recipient = self.recipient();
            let card = recipient
                .cards
                .get(&(server_id, share_id.to_owned()))?
                .clone();
            let busy = recipient
                .download
                .as_ref()
                .map(|d| (d.server_id, d.share_id.clone()));
            (card, busy)
        };
        let busy = busy.as_ref().map(|(s, id)| (*s, id.as_str()));
        Some(self.card_view(server_id, &card, busy))
    }

    /// Remembers an offer shown in the chat and returns its card. `stored` is the state saved
    /// with the chat history. An offer seen again keeps its current state.
    pub(crate) fn files_register_offer(
        &self,
        server_id: ServerId,
        offer: FileOffer,
        origin: OfferOrigin,
        stored: Option<FileCardRecord>,
    ) -> FileCardView {
        let share_id = offer.id.clone();
        let key = (server_id, share_id.clone());
        {
            let mut recipient = self.recipient();
            let card = recipient.cards.entry(key).or_insert_with(|| Card {
                offer,
                sender_name: origin.sender_name,
                sender_cert: origin.sender_cert,
                sender_session: origin.sender_session,
                message_id: origin.message_id,
                own: origin.own,
                state: stored.map_or(CardState::Available, state_of),
            });
            if card.message_id.is_none() {
                card.message_id = origin.message_id;
            }
            if card.sender_session.is_none() {
                card.sender_session = origin.sender_session;
            }
        }
        self.file_card(server_id, &share_id)
            .expect("the card was just registered")
    }

    /// The stored message that carries the offer, once known.
    pub(crate) fn files_set_message(&self, server_id: ServerId, share_id: &str, message_id: &str) {
        let Ok(id) = message_id.parse::<i64>() else {
            return;
        };
        if let Some(card) = self
            .recipient()
            .cards
            .get_mut(&(server_id, share_id.to_owned()))
        {
            card.message_id.get_or_insert(id);
        }
    }

    fn emit_card(&self, server_id: ServerId, share_id: &str) {
        if let Some(view) = self.file_card(server_id, share_id) {
            self.events()
                .for_server(server_id)
                .emit(EngineEvent::FileCardChanged(Box::new(view)));
        }
    }

    /// Re-emits every card that could be downloaded, e.g. when "Another download is running"
    /// starts or stops applying, or a sender came or went.
    fn emit_cards(&self, filter: impl Fn(ServerId, &Card) -> bool) {
        let keys: Vec<(ServerId, String)> = self
            .recipient()
            .cards
            .iter()
            .filter(|((server_id, _), card)| filter(*server_id, card))
            .map(|(key, _)| key.clone())
            .collect();
        for (server_id, share_id) in keys {
            self.emit_card(server_id, &share_id);
        }
    }

    fn set_card_state(&self, server_id: ServerId, share_id: &str, state: CardState) {
        if let Some(card) = self
            .recipient()
            .cards
            .get_mut(&(server_id, share_id.to_owned()))
        {
            card.state = state;
        }
        self.emit_card(server_id, share_id);
    }

    fn send_file_message(&self, server_id: ServerId, session: u32, message: &FileMessage) -> bool {
        let (Some(data), Ok(ctx)) = (wire::encode(message), self.session_ctx_for(server_id)) else {
            return false;
        };
        ctx.handle
            .send_plugin_data(&[session], wire::DATA_ID, data)
            .inspect_err(|e| tracing::debug!(error = %e, "file message not sent"))
            .is_ok()
    }

    /// Starts the download of a card after the user confirmed it (FR-7.22): saves into the
    /// save folder (`downloads` is the system Downloads folder). Returns once the request is
    /// on its way; the card reports the progress.
    pub fn files_download(
        &self,
        server_id: ServerId,
        share_id: &str,
        downloads: &Path,
    ) -> EngineResult<()> {
        let view = self
            .file_card(server_id, share_id)
            .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown file"))?;
        if let Some(block) = view.download_block {
            return Err(EngineError::invalid(format!(
                "download not possible: {block:?}"
            )));
        }
        if !view.state.can_start() {
            return Err(EngineError::invalid("the file can't be downloaded now"));
        }
        let key = (server_id, share_id.to_owned());
        let card = self
            .recipient()
            .cards
            .get(&key)
            .cloned()
            .ok_or_else(|| EngineError::new(ErrorKind::NotFound, "unknown file"))?;
        let session = self
            .card_sender(server_id, &card)
            .ok_or_else(|| EngineError::invalid("the sender is offline"))?;
        let folder = self.files_save_folder(downloads);
        let store = self.store_handle();
        let host = lock(&store)
            .server(server_id)
            .ok()
            .flatten()
            .map(|s| s.address)
            .unwrap_or_default();
        let (answers, answers_rx) = mpsc::unbounded_channel();
        let (stop, stop_rx) = oneshot::channel();
        {
            let mut recipient = self.recipient();
            if recipient.download.is_some() {
                return Err(EngineError::invalid("another download is running"));
            }
            recipient.download = Some(Download {
                server_id,
                share_id: share_id.to_owned(),
                session,
                answers,
                stop: Some(stop),
            });
            if let Some(card) = recipient.cards.get_mut(&key) {
                card.state = CardState::Requested;
                card.sender_session = Some(session);
            }
        }
        self.emit_cards(|_, card| !card.own);
        let engine = self.clone();
        let offer = card.offer;
        self.runtime().spawn(async move {
            let run = engine.run_download(server_id, session, &offer, folder, host, answers_rx);
            let outcome = tokio::select! {
                state = run => state,
                stop = stop_rx => match stop {
                    Ok(Stop::User) => CardState::Cancelled,
                    Ok(Stop::SenderLeft) => CardState::Available,
                    Ok(Stop::Disconnected) | Err(_) => CardState::Failed(FailReason::ConnectionLost),
                },
            };
            engine.download_finished(server_id, &offer.id, session, outcome);
        });
        Ok(())
    }

    async fn run_download(
        &self,
        server_id: ServerId,
        session: u32,
        offer: &FileOffer,
        folder: PathBuf,
        host: String,
        mut answers: mpsc::UnboundedReceiver<FileMessage>,
    ) -> CardState {
        let (id, name, size, sha256) = (
            offer.id.clone(),
            offer.name.clone(),
            offer.size,
            offer.sha256.clone(),
        );
        // Free space and the partial file come first: without them nothing is requested.
        let created = tokio::task::spawn_blocking(move || {
            IncomingFile::create(&folder, &id, &name, size, &sha256)
        })
        .await;
        let mut file = match created {
            Ok(Ok(file)) => file,
            Ok(Err(e)) => return receive_failed(e),
            Err(e) => return CardState::Failed(FailReason::Io(e.to_string())),
        };
        let expected = match self.file_expect(server_id, session, &offer.id, size) {
            Ok(expected) => expected,
            Err(e) => return failed(e),
        };
        let request = FileMessage::Request {
            id: offer.id.clone(),
        };
        if !self.send_file_message(server_id, session, &request) {
            return CardState::Failed(FailReason::ConnectionLost);
        }
        let mut queued = false;
        loop {
            let answer = if queued {
                answers.recv().await
            } else {
                match tokio::time::timeout(ANSWER_TIMEOUT, answers.recv()).await {
                    Ok(answer) => answer,
                    Err(_) => return CardState::Failed(FailReason::NoAnswer),
                }
            };
            match answer {
                Some(FileMessage::Queued { position, .. }) => {
                    queued = true;
                    self.set_card_state(server_id, &offer.id, CardState::Queued { position });
                }
                Some(FileMessage::Go { .. }) => break,
                Some(FileMessage::Busy { .. }) => return CardState::Failed(FailReason::Busy),
                Some(FileMessage::Gone { .. }) => {
                    return CardState::Unavailable(Unavailable::Ended);
                }
                Some(_) => {}
                None => return CardState::Failed(FailReason::ConnectionLost),
            }
        }
        self.set_card_state(
            server_id,
            &offer.id,
            CardState::Downloading {
                received: 0,
                bytes_per_sec: 0,
            },
        );
        let mut stream = match expected.stream().await {
            Ok(stream) => stream,
            Err(e) => return failed(e),
        };
        let started = Instant::now();
        let mut shown = Instant::now();
        let mut buf = vec![0u8; CHUNK];
        loop {
            let read = match stream.read(&mut buf).await {
                Ok(0) => break,
                Ok(read) => read,
                Err(e) => return failed(e),
            };
            let written = tokio::task::spawn_blocking(move || {
                let result = file.write(&buf[..read]);
                (file, buf, result)
            })
            .await;
            let result;
            (file, buf, result) = match written {
                Ok(written) => written,
                Err(e) => return CardState::Failed(FailReason::Io(e.to_string())),
            };
            if let Err(e) = result {
                return receive_failed(e);
            }
            if shown.elapsed() >= PROGRESS_EVERY {
                shown = Instant::now();
                let received = stream.received();
                let millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                let bytes_per_sec = received
                    .saturating_mul(1000)
                    .checked_div(millis)
                    .unwrap_or(0);
                self.set_card_state(
                    server_id,
                    &offer.id,
                    CardState::Downloading {
                        received,
                        bytes_per_sec,
                    },
                );
            }
        }
        self.set_card_state(server_id, &offer.id, CardState::Checking);
        match tokio::task::spawn_blocking(move || file.finish(&host)).await {
            Ok(Ok(path)) => CardState::Done { path },
            Ok(Err(e)) => receive_failed(e),
            Err(e) => CardState::Failed(FailReason::Io(e.to_string())),
        }
    }

    fn download_finished(
        &self,
        server_id: ServerId,
        share_id: &str,
        session: u32,
        outcome: CardState,
    ) {
        // A failed or cancelled download frees the sender's slot (FR-7.28).
        if matches!(outcome, CardState::Failed(_) | CardState::Cancelled) {
            self.send_file_message(
                server_id,
                session,
                &FileMessage::Cancel {
                    id: share_id.to_owned(),
                },
            );
        }
        let message_id = {
            let mut recipient = self.recipient();
            if recipient
                .download
                .as_ref()
                .is_some_and(|d| d.server_id == server_id && d.share_id == share_id)
            {
                recipient.download = None;
            }
            let card = recipient.cards.get_mut(&(server_id, share_id.to_owned()));
            card.and_then(|card| {
                card.state = outcome.clone();
                card.message_id
            })
        };
        tracing::info!(outcome = outcome_name(&outcome), "file download ended");
        if let (Some(message_id), Some(record)) = (message_id, record_of(&outcome))
            && let Err(e) = lock(&self.store_handle()).save_file_card(message_id, &record)
        {
            tracing::warn!(error = %e, "saving a file card failed");
        }
        self.emit_cards(|_, card| !card.own);
    }

    /// Cancel on the card (FR-7.28): stops the download and deletes the partial file.
    pub fn files_cancel(&self, server_id: ServerId, share_id: &str) {
        self.stop_download(
            |d| d.server_id == server_id && d.share_id == share_id,
            Stop::User,
        );
    }

    fn stop_download(&self, which: impl Fn(&Download) -> bool, why: Stop) {
        let stop = self
            .recipient()
            .download
            .as_mut()
            .filter(|d| which(d))
            .and_then(|d| d.stop.take());
        if let Some(stop) = stop {
            let _ = stop.send(why);
        }
    }

    /// Where a downloaded file is, for Open and Open folder (FR-7.29). A moved or deleted file
    /// gives the error "The file is no longer in {folder}".
    pub fn files_saved_path(&self, server_id: ServerId, share_id: &str) -> EngineResult<PathBuf> {
        let state = self
            .recipient()
            .cards
            .get(&(server_id, share_id.to_owned()))
            .map(|c| c.state.clone());
        let Some(CardState::Done { path }) = state else {
            return Err(EngineError::new(ErrorKind::NotFound, "not downloaded"));
        };
        if path.is_file() {
            return Ok(path);
        }
        let folder = path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        Err(EngineError::new(
            ErrorKind::NotFound,
            format!("The file is no longer in {folder}"),
        ))
    }

    /// The sender's answer to a request (`queued`, `go`, `busy`, `gone`).
    pub(crate) fn files_on_answer(&self, server_id: ServerId, session: u32, msg: FileMessage) {
        let recipient = self.recipient();
        if let Some(d) = &recipient.download
            && d.server_id == server_id
            && d.session == session
            && d.share_id == msg.id()
        {
            let _ = d.answers.send(msg);
        }
    }

    /// A person left `server_id`: a request still waiting for them stops, and their cards show
    /// "{sender} is offline" (FR-7.20).
    pub(crate) fn files_sender_left(&self, server_id: ServerId, session: u32) {
        let waiting = {
            let recipient = self.recipient();
            recipient.download.as_ref().is_some_and(|d| {
                d.server_id == server_id
                    && d.session == session
                    && recipient
                        .cards
                        .get(&(server_id, d.share_id.clone()))
                        .is_some_and(|c| {
                            matches!(c.state, CardState::Requested | CardState::Queued { .. })
                        })
            })
        };
        if waiting {
            self.stop_download(
                |d| d.server_id == server_id && d.session == session,
                Stop::SenderLeft,
            );
        }
        for card in self.recipient().cards.iter_mut() {
            if card.0.0 == server_id && card.1.sender_session == Some(session) {
                card.1.sender_session = None;
            }
        }
        self.emit_cards(|id, card| id == server_id && !card.own && card.sender_session.is_none());
    }

    /// A person joined `server_id`: their cards may be Available again (FR-7.20).
    pub(crate) fn files_sender_joined(&self, server_id: ServerId, cert_hash: Option<&str>) {
        let Some(cert) = cert_hash else {
            return;
        };
        self.emit_cards(|id, card| {
            id == server_id && !card.own && card.sender_cert.as_deref() == Some(cert)
        });
    }

    /// The own connection to `server_id` ended or was re-established: sessions are no longer
    /// valid and a running download from there fails.
    pub(crate) fn files_connection_reset(&self, server_id: ServerId) {
        self.stop_download(|d| d.server_id == server_id, Stop::Disconnected);
        for ((id, _), card) in self.recipient().cards.iter_mut() {
            if *id == server_id {
                card.sender_session = None;
            }
        }
    }
}

fn outcome_name(state: &CardState) -> &'static str {
    match state {
        CardState::Done { .. } => "done",
        CardState::Mismatch => "mismatch",
        CardState::Failed(_) => "failed",
        CardState::Cancelled => "cancelled",
        CardState::Unavailable(_) => "unavailable",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn offer(expires: u64) -> FileOffer {
        FileOffer {
            id: "0123456789abcdef0123456789abcdef".into(),
            name: "report.pdf.exe".into(),
            size: 10,
            mime: "application/pdf".into(),
            sha256: "b".repeat(64),
            expires,
        }
    }

    fn origin(own: bool) -> OfferOrigin {
        OfferOrigin {
            sender_name: "Alice".into(),
            sender_cert: Some("aa".into()),
            sender_session: Some(5),
            message_id: None,
            own,
        }
    }

    #[tokio::test]
    async fn a_card_of_an_offline_sender_is_unavailable() {
        let (_dir, engine) = engine().await;
        let card = engine.files_register_offer(1, offer(unix_now() + 60), origin(false), None);
        assert_eq!(card.name, "report.pdf.exe");
        assert!(card.risky);
        assert_eq!(
            card.state,
            CardState::Unavailable(Unavailable::SenderOffline)
        );
        assert_eq!(card.download_block, None);
        let expired = engine.files_register_offer(
            2,
            offer(unix_now().saturating_sub(1)),
            origin(false),
            None,
        );
        assert_eq!(expired.state, CardState::Unavailable(Unavailable::Expired));
        let refused = engine.files_download(1, &card.share_id, Path::new("/tmp"));
        assert!(refused.is_err(), "no download while the sender is offline");
    }

    #[tokio::test]
    async fn own_cards_have_no_download_and_show_the_share() {
        let (_dir, engine) = engine().await;
        let card = engine.files_register_offer(1, offer(unix_now() + 60), origin(true), None);
        assert_eq!(card.download_block, Some(DownloadBlock::Own));
        assert_eq!(
            card.share.expect("own share").ended,
            Some(None),
            "a share from before this run has ended"
        );
    }

    #[tokio::test]
    async fn stored_results_come_back_and_a_moved_file_is_reported() {
        let (dir, engine) = engine().await;
        let path = dir.path().join("x.txt");
        let record = FileCardRecord::Done {
            path: path.to_string_lossy().into_owned(),
        };
        let card = engine.files_register_offer(1, offer(0), origin(false), Some(record));
        assert_eq!(card.state, CardState::Done { path: path.clone() });
        let missing = engine
            .files_saved_path(1, &card.share_id)
            .expect_err("not there");
        assert_eq!(
            missing.message,
            format!("The file is no longer in {}", dir.path().display())
        );
        std::fs::write(&path, b"x").expect("file");
        assert_eq!(
            engine.files_saved_path(1, &card.share_id).expect("found"),
            path
        );
    }

    #[tokio::test]
    async fn an_offer_seen_again_keeps_its_state() {
        let (dir, engine) = engine().await;
        let record = FileCardRecord::Mismatch;
        engine.files_register_offer(1, offer(0), origin(false), Some(record));
        let again = engine.files_register_offer(1, offer(0), origin(false), None);
        assert_eq!(again.state, CardState::Mismatch);
        drop(dir);
    }

    #[test]
    fn transfer_errors_map_to_card_texts() {
        assert_eq!(
            failed(TransferError::Stalled),
            CardState::Failed(FailReason::Stalled)
        );
        assert_eq!(
            failed(TransferError::Refused),
            CardState::Failed(FailReason::ConnectionLost)
        );
        assert_eq!(
            failed(TransferError::Connect(DirectError::NoAnswer)),
            CardState::Failed(FailReason::Connect(None))
        );
        assert_eq!(receive_failed(ReceiveError::Mismatch), CardState::Mismatch);
        assert_eq!(
            receive_failed(ReceiveError::NoSpace {
                folder: PathBuf::from("/d")
            }),
            CardState::Failed(FailReason::NoSpace(PathBuf::from("/d")))
        );
    }

    #[test]
    fn only_finished_states_are_stored() {
        let done = CardState::Done {
            path: PathBuf::from("/d/a.txt"),
        };
        assert_eq!(state_of(record_of(&done).expect("stored")), done);
        assert_eq!(
            state_of(record_of(&CardState::Mismatch).expect("stored")),
            CardState::Mismatch
        );
        assert!(record_of(&CardState::Cancelled).is_none());
        assert!(record_of(&CardState::Queued { position: 1 }).is_none());
    }
}
