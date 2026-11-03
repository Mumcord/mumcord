//! File cards in the chat (spec 07, section 7.2, FR-7.9, FR-7.16, FR-7.20, FR-7.21): what a
//! card shows and whether its Download button works. Pure functions; the engine feeds them the
//! current relay, settings and presence.

use std::path::PathBuf;

use mumcord_store::{FilesMaxSize, P2pMode};

use super::offer::FileOffer;
use super::risky::is_risky_file_name;
use super::share::EndReason;
use crate::peer::RelayUnavailable;

/// Why file sharing is not available on a server, in the order of FR-7.9 (the first that applies
/// is shown).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesUnavailable {
    /// "This server has no relay, so file sharing isn't available"
    NoRelay,
    /// "This server's relay doesn't allow file sharing"
    RelayNoFiles,
    /// "File sharing needs a client certificate"
    NoCertificate,
    /// "File sharing needs the relay; your peer-to-peer setting is {Off / Direct only}"
    P2pMode(P2pMode),
}

/// FR-7.9: a relay that allows files, a client certificate, and a peer-to-peer mode that lets
/// files use the relay. Files never go direct, so every mode but `off` and `direct_only` is fine.
pub fn availability(
    relay: Result<(), RelayUnavailable>,
    has_certificate: bool,
    mode: P2pMode,
) -> Result<(), FilesUnavailable> {
    match relay {
        Ok(()) => {}
        Err(RelayUnavailable::UseNotAllowed(_)) => return Err(FilesUnavailable::RelayNoFiles),
        Err(_) => return Err(FilesUnavailable::NoRelay),
    }
    if !has_certificate {
        return Err(FilesUnavailable::NoCertificate);
    }
    match mode {
        P2pMode::Off | P2pMode::DirectOnly => Err(FilesUnavailable::P2pMode(mode)),
        P2pMode::DirectRelay | P2pMode::RelayFirst | P2pMode::RelayOnly => Ok(()),
    }
}

/// Why a card's Download button is disabled (FR-7.21); the UI shows it as tooltip and accessible
/// description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadBlock {
    /// Own offer: no Download at all.
    Own,
    Unavailable(FilesUnavailable),
    /// "You turned off receiving files"
    ReceiveOff,
    /// "Larger than your limit of {limit}"
    TooLarge(FilesMaxSize),
    /// "Another download is running" (one download at a time per recipient, all servers).
    AnotherDownload,
}

/// What the Download decision depends on.
#[derive(Debug, Clone, Copy)]
pub struct DownloadContext {
    pub own: bool,
    pub availability: Result<(), FilesUnavailable>,
    pub receive: bool,
    pub limit: FilesMaxSize,
    /// A download of another card is running.
    pub other_download: bool,
}

/// The first reason in FR-7.21 order that disables Download for a file of `size` bytes.
pub fn download_block(size: u64, cx: &DownloadContext) -> Option<DownloadBlock> {
    if cx.own {
        return Some(DownloadBlock::Own);
    }
    if let Err(reason) = cx.availability {
        return Some(DownloadBlock::Unavailable(reason));
    }
    if !cx.receive {
        return Some(DownloadBlock::ReceiveOff);
    }
    if size > cx.limit.bytes() {
        return Some(DownloadBlock::TooLarge(cx.limit));
    }
    cx.other_download.then_some(DownloadBlock::AnotherDownload)
}

/// Why a card is unavailable (FR-7.20, mockup 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// "{sender} is offline"
    SenderOffline,
    /// "Sharing ended" (the sender answered `gone`).
    Ended,
    /// "Expired"
    Expired,
}

/// Why a download failed (FR-7.20 "Download failed: {reason}", FR-7.34).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailReason {
    /// "{sender} did not answer": no `go`/`queued` within 10 s.
    NoAnswer,
    /// "{sender} is busy, try again later": the sender's queue is full.
    Busy,
    /// "Couldn't connect through the relay: {reason}"; `None` when the relay gave no reason.
    Connect(Option<RelayUnavailable>),
    /// "The transfer stalled"
    Stalled,
    /// The connection dropped during the transfer.
    ConnectionLost,
    /// "Not enough disk space in {folder}"; nothing was requested.
    NoSpace(PathBuf),
    /// Writing the file failed; the text is the system's error.
    Io(String),
}

/// A card's state as the recipient sees it (section 7.2). Confirming is only a dialog in the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardState {
    Available,
    Unavailable(Unavailable),
    /// `req` sent, waiting for the sender's answer.
    Requested,
    /// Waiting in the sender's queue; 1 = next.
    Queued {
        position: u16,
    },
    Downloading {
        received: u64,
        /// Average over the last seconds, for the speed and time-left display.
        bytes_per_sec: u64,
    },
    /// All bytes arrived; the SHA-256 is being compared.
    Checking,
    /// "✓ Downloaded and checked"; `path` is where the file was saved.
    Done {
        path: PathBuf,
    },
    /// The file did not match the offer and was deleted (mockup 12).
    Mismatch,
    Failed(FailReason),
    Cancelled,
}

impl CardState {
    /// Whether no download of this card is in progress.
    pub fn is_idle(&self) -> bool {
        !matches!(
            self,
            Self::Requested | Self::Queued { .. } | Self::Downloading { .. } | Self::Checking
        )
    }

    /// States from which Download (or Retry) starts a download.
    pub fn can_start(&self) -> bool {
        matches!(self, Self::Available | Self::Failed(_) | Self::Cancelled)
    }
}

/// The state shown, given the stored one and what is true right now: a card that could be
/// downloaded becomes Unavailable while the share expired, ended or its sender is offline, and
/// Available again when the sender is back with the share still active (FR-7.20).
pub fn shown_state(state: &CardState, expired: bool, sender_online: bool) -> CardState {
    if !state.can_start() && !matches!(state, CardState::Unavailable(Unavailable::SenderOffline)) {
        return state.clone();
    }
    if expired {
        CardState::Unavailable(Unavailable::Expired)
    } else if !sender_online {
        CardState::Unavailable(Unavailable::SenderOffline)
    } else if matches!(state, CardState::Unavailable(_)) {
        CardState::Available
    } else {
        state.clone()
    }
}

/// The sender's view of an own share (FR-7.16, mockup 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnShareView {
    /// `None` while the share is active; after the end the card shows "Sharing ended" with the
    /// reason. `Some(None)`: ended before this run of Mumcord (nothing about shares is stored).
    pub ended: Option<Option<EndReason>>,
    /// Names of users who finished a download, in order.
    pub downloaded_by: Vec<String>,
    /// The running transfer of this share: recipient name and percent.
    pub sending_to: Option<(String, u8)>,
    /// "{n} waiting"
    pub waiting: usize,
}

/// A file card as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCardView {
    pub share_id: String,
    /// Sanitized name (FR-7.23).
    pub name: String,
    pub size: u64,
    /// Display only; untrusted.
    pub mime: String,
    /// Full SHA-256, 64 lowercase hex characters.
    pub sha256: String,
    /// Unix seconds.
    pub expires: u64,
    pub sender_name: String,
    pub own: bool,
    /// A program or script (FR-7.24): the risky warning replaces the confirmation.
    pub risky: bool,
    pub state: CardState,
    pub download_block: Option<DownloadBlock>,
    /// Own cards only.
    pub share: Option<OwnShareView>,
}

impl FileCardView {
    pub(crate) fn new(offer: &FileOffer, sender_name: &str, own: bool) -> Self {
        let name = offer.display_name();
        Self {
            share_id: offer.id.clone(),
            risky: is_risky_file_name(&name),
            name,
            size: offer.size,
            mime: offer.mime.clone(),
            sha256: offer.sha256.clone(),
            expires: offer.expires,
            sender_name: sender_name.to_owned(),
            own,
            state: CardState::Available,
            download_block: None,
            share: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_p2p::RelayUse;

    #[test]
    fn availability_reasons_follow_the_spec_order() {
        let no_relay = Err(RelayUnavailable::NoRelay);
        assert_eq!(
            availability(no_relay, false, P2pMode::Off),
            Err(FilesUnavailable::NoRelay)
        );
        let no_files = Err(RelayUnavailable::UseNotAllowed(RelayUse::Files));
        assert_eq!(
            availability(no_files, false, P2pMode::Off),
            Err(FilesUnavailable::RelayNoFiles)
        );
        assert_eq!(
            availability(Ok(()), false, P2pMode::Off),
            Err(FilesUnavailable::NoCertificate)
        );
        for mode in [P2pMode::Off, P2pMode::DirectOnly] {
            assert_eq!(
                availability(Ok(()), true, mode),
                Err(FilesUnavailable::P2pMode(mode))
            );
        }
        for mode in [
            P2pMode::DirectRelay,
            P2pMode::RelayFirst,
            P2pMode::RelayOnly,
        ] {
            assert_eq!(availability(Ok(()), true, mode), Ok(()));
        }
    }

    #[test]
    fn a_relay_that_is_down_counts_as_no_relay() {
        assert_eq!(
            availability(Err(RelayUnavailable::BotLeft), true, P2pMode::RelayOnly),
            Err(FilesUnavailable::NoRelay)
        );
    }

    fn ready() -> DownloadContext {
        DownloadContext {
            own: false,
            availability: Ok(()),
            receive: true,
            limit: FilesMaxSize::M100,
            other_download: false,
        }
    }

    #[test]
    fn download_blocks_follow_the_spec_order() {
        let small = 1024;
        let big = FilesMaxSize::M100.bytes() + 1;
        assert_eq!(download_block(small, &ready()), None);
        assert_eq!(
            download_block(FilesMaxSize::M100.bytes(), &ready()),
            None,
            "exactly the limit is allowed"
        );
        let busy = DownloadContext {
            other_download: true,
            ..ready()
        };
        assert_eq!(
            download_block(small, &busy),
            Some(DownloadBlock::AnotherDownload)
        );
        assert_eq!(
            download_block(big, &busy),
            Some(DownloadBlock::TooLarge(FilesMaxSize::M100))
        );
        let off = DownloadContext {
            receive: false,
            ..busy
        };
        assert_eq!(download_block(big, &off), Some(DownloadBlock::ReceiveOff));
        let no_cert = DownloadContext {
            availability: Err(FilesUnavailable::NoCertificate),
            ..off
        };
        assert_eq!(
            download_block(big, &no_cert),
            Some(DownloadBlock::Unavailable(FilesUnavailable::NoCertificate))
        );
        let own = DownloadContext {
            own: true,
            ..no_cert
        };
        assert_eq!(download_block(big, &own), Some(DownloadBlock::Own));
    }

    #[test]
    fn offline_expired_and_back_online() {
        let available = CardState::Available;
        assert_eq!(
            shown_state(&available, false, false),
            CardState::Unavailable(Unavailable::SenderOffline)
        );
        assert_eq!(
            shown_state(&available, true, true),
            CardState::Unavailable(Unavailable::Expired)
        );
        let offline = CardState::Unavailable(Unavailable::SenderOffline);
        assert_eq!(shown_state(&offline, false, true), CardState::Available);
        let failed = CardState::Failed(FailReason::Stalled);
        assert_eq!(shown_state(&failed, false, true), failed);
        assert_eq!(
            shown_state(&failed, false, false),
            CardState::Unavailable(Unavailable::SenderOffline)
        );
    }

    #[test]
    fn finished_and_running_states_are_kept() {
        let done = CardState::Done {
            path: PathBuf::from("/tmp/a"),
        };
        assert_eq!(shown_state(&done, true, false), done);
        assert_eq!(
            shown_state(&CardState::Mismatch, true, false),
            CardState::Mismatch
        );
        let queued = CardState::Queued { position: 2 };
        assert_eq!(shown_state(&queued, false, true), queued);
        let ended = CardState::Unavailable(Unavailable::Ended);
        assert_eq!(
            shown_state(&ended, false, true),
            ended,
            "a share the sender ended stays ended"
        );
    }

    #[test]
    fn idle_and_startable_states() {
        assert!(CardState::Available.can_start() && CardState::Available.is_idle());
        assert!(CardState::Cancelled.can_start());
        assert!(CardState::Failed(FailReason::Busy).can_start());
        assert!(!CardState::Requested.is_idle() && !CardState::Requested.can_start());
        assert!(!CardState::Checking.is_idle());
        assert!(!CardState::Mismatch.can_start() && CardState::Mismatch.is_idle());
    }
}
