//! "Send to your phone" (spec 05, FR-5.64, FR-5.67): the identity settings show identities and
//! optionally the server list as a loop of QR codes plus a code the user types on the phone.
//!
//! The transfer lives here, in app state, for as long as the dialog shows it. The webview gets
//! what it has to draw (the modules of each QR code and the typed code) and nothing else: the
//! text inside the QR codes stays on this side. Nothing is logged, written to disk or sent
//! anywhere. Starting again, cancelling and expiry drop the transfer, which wipes its parts and
//! its code.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use mumcord_engine::store::transfer::{
    Transfer, TransferError, TransferPayload, VALIDITY_SECS, unix_now,
};
use mumcord_engine::{Engine, test_hooks};
use qrcodegen::{QrCode, QrCodeEcc, QrSegment, Version};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use zeroize::Zeroize;

use super::types::{ApiError, ApiResult, ErrorCode, to_engine_identity_id};

/// Whether this build offers "Send to your phone": only with the `phone-transfer` Cargo feature,
/// which release builds leave off until the phone apps that read the codes are released
/// (CONTRIBUTING.md, "Build flags"). The UI learns it from `app_info` and shows the button only
/// then; the invoke handler refuses the commands of this module otherwise.
pub const ENABLED: bool = cfg!(feature = "phone-transfer");

/// The commands of this module, as the webview names them.
const COMMANDS: [&str; 2] = ["phone_transfer_start", "phone_transfer_cancel"];

/// Whether the webview may call `command` in this build: everything but the commands of this
/// module, and those only when the feature is [`ENABLED`].
pub fn command_allowed(command: &str) -> bool {
    command_allowed_when(ENABLED, command)
}

fn command_allowed_when(enabled: bool, command: &str) -> bool {
    enabled || !COMMANDS.contains(&command)
}

/// One QR code as the UI draws it: `size` rows of `size` characters, `1` for a dark module.
/// The quiet zone around it is not included.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PhoneTransferQr {
    pub size: u32,
    pub rows: Vec<String>,
}

/// A transfer on show.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PhoneTransfer {
    /// The QR codes in the order they are shown, all of the same size.
    pub codes: Vec<PhoneTransferQr>,
    /// The digits the user types on the phone; the UI groups them.
    pub code: String,
    pub total_parts: u32,
    /// Seconds until the codes must leave the screen.
    pub seconds_left: u32,
}

impl Drop for PhoneTransfer {
    fn drop(&mut self) {
        self.code.zeroize();
        for qr in &mut self.codes {
            for row in &mut qr.rows {
                row.zeroize();
            }
        }
    }
}

/// The transfer the dialog shows right now, if any.
#[derive(Default)]
pub struct PhoneTransferState(Mutex<Slot>);

#[derive(Default)]
struct Slot {
    /// Counts starts and cancels, so a start that was overtaken does not put its transfer back.
    generation: u64,
    shown: Option<Shown>,
}

struct Shown {
    transfer: Transfer,
    /// Seconds the desktop shows it; the format's two minutes unless a test shortens it.
    validity: u64,
}

impl Shown {
    fn seconds_left(&self, now: u64) -> u64 {
        let shown_for = now.saturating_sub(self.transfer.created_at());
        self.validity
            .saturating_sub(shown_for)
            .min(self.transfer.seconds_left(now))
    }
}

impl PhoneTransferState {
    fn slot(&self) -> MutexGuard<'_, Slot> {
        // The slot holds no invariant a panic could break: take it back and carry on.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes the current transfer off and returns the ticket of the start that replaces it.
    fn begin(&self) -> u64 {
        let mut slot = self.slot();
        slot.generation += 1;
        slot.shown = None;
        slot.generation
    }

    /// Puts the finished transfer on show, unless a newer start or a cancel came in between.
    fn show(
        &self,
        ticket: u64,
        transfer: Transfer,
        validity: u64,
        now: u64,
    ) -> ApiResult<PhoneTransfer> {
        let mut slot = self.slot();
        if slot.generation != ticket {
            return Err(ApiError::new(
                ErrorCode::InvalidInput,
                "the transfer was replaced before it was ready",
            ));
        }
        let codes = qr_codes(transfer.parts())?;
        let shown = Shown { transfer, validity };
        let view = PhoneTransfer {
            total_parts: count(codes.len() as u64),
            codes,
            code: shown.transfer.code().to_owned(),
            seconds_left: count(shown.seconds_left(now)),
        };
        slot.shown = Some(shown);
        Ok(view)
    }

    /// Seconds the transfer of this start has left; `None` once it is no longer shown. An
    /// expired one is dropped here.
    fn seconds_left(&self, ticket: u64, now: u64) -> Option<u64> {
        let mut slot = self.slot();
        if slot.generation != ticket {
            return None;
        }
        let left = slot.shown.as_ref()?.seconds_left(now);
        if left == 0 {
            slot.shown = None;
            return None;
        }
        Some(left)
    }

    /// Forgets the transfer, also one that is still being built.
    pub fn clear(&self) {
        let mut slot = self.slot();
        slot.generation += 1;
        slot.shown = None;
    }
}

fn count(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Seconds a transfer stays on the desktop screen.
fn validity() -> u64 {
    test_hooks::transfer_validity().map_or(VALIDITY_SECS, |d| d.as_secs().min(VALIDITY_SECS))
}

fn transfer_error(err: TransferError) -> ApiError {
    let code = match err {
        TransferError::PayloadTooLarge | TransferError::InvalidPayload(_) => {
            ErrorCode::InvalidInput
        }
        _ => ErrorCode::Internal,
    };
    // The texts of `TransferError` never contain content of the transfer.
    ApiError::new(code, err.to_string())
}

fn qr_error() -> ApiError {
    ApiError::new(ErrorCode::Internal, "a QR code could not be built")
}

/// One QR code per part: alphanumeric mode, error correction M, and all codes at the version
/// of the largest one, so the picture does not change size while the codes cycle (the last
/// part is usually shorter).
fn qr_codes(parts: &[String]) -> ApiResult<Vec<PhoneTransferQr>> {
    let encode = |part: &str, min: Version| {
        if !QrSegment::is_alphanumeric(part) {
            return Err(qr_error());
        }
        let segments = [QrSegment::make_alphanumeric(part)];
        // No automatic raise of the error correction: the phone side is tested against M.
        QrCode::encode_segments_advanced(
            &segments,
            QrCodeEcc::Medium,
            min,
            Version::MAX,
            None,
            false,
        )
        .map_err(|_| qr_error())
    };
    let mut largest = Version::MIN;
    for part in parts {
        let version = encode(part, Version::MIN)?.version();
        if version.value() > largest.value() {
            largest = version;
        }
    }
    parts
        .iter()
        .map(|part| {
            let qr = encode(part, largest)?;
            let size = qr.size();
            let rows = (0..size)
                .map(|y| {
                    (0..size)
                        .map(|x| if qr.get_module(x, y) { '1' } else { '0' })
                        .collect()
                })
                .collect();
            Ok(PhoneTransferQr {
                size: u32::try_from(size).map_err(|_| qr_error())?,
                rows,
            })
        })
        .collect()
}

/// Builds a new transfer of the chosen identities and, if wanted, the server list and puts it
/// on show. A transfer that was shown before is dropped first, so calling this again is how
/// the dialog gets new codes after a change of the selection or after the expiry. Saved server
/// passwords and channel access tokens are never part of it (FR-5.66).
#[tauri::command]
#[specta::specta]
pub async fn phone_transfer_start(
    app: AppHandle,
    engine: State<'_, Engine>,
    state: State<'_, PhoneTransferState>,
    identity_ids: Vec<u32>,
    include_servers: bool,
) -> ApiResult<PhoneTransfer> {
    let ticket = state.begin();
    let ids = identity_ids
        .into_iter()
        .map(to_engine_identity_id)
        .collect();
    let payload = engine.transfer_export_payload(ids, include_servers).await?;
    let transfer = build(payload).await?;
    let view = state.show(ticket, transfer, validity(), unix_now())?;
    tauri::async_runtime::spawn(drop_when_expired(app, ticket));
    Ok(view)
}

/// Takes the transfer out of memory when its time is up, also if the dialog is gone by then
/// without having cancelled (a reloaded or crashed webview).
async fn drop_when_expired(app: AppHandle, ticket: u64) {
    let state = app.state::<PhoneTransferState>();
    while let Some(left) = state.seconds_left(ticket, unix_now()) {
        tokio::time::sleep(Duration::from_secs(left)).await;
    }
}

/// The key derivation fills 128 MiB several times: off the async threads. The payload with the
/// private keys ends inside the closure.
async fn build(payload: TransferPayload) -> ApiResult<Transfer> {
    tauri::async_runtime::spawn_blocking(move || Transfer::create(&payload, unix_now()))
        .await
        .map_err(|_| ApiError::new(ErrorCode::Internal, "building the transfer was aborted"))?
        .map_err(transfer_error)
}

/// Drops the transfer; the dialog calls it when it closes.
#[tauri::command]
#[specta::specta]
pub fn phone_transfer_cancel(state: State<'_, PhoneTransferState>) {
    state.clear();
}

#[cfg(test)]
mod tests {
    use mumcord_engine::store::transfer::{TransferReader, TransferServer};

    use super::*;

    const NOW: u64 = 1_800_000_000;

    #[test]
    fn the_commands_are_refused_unless_the_feature_is_on() {
        for command in COMMANDS {
            assert!(!command_allowed_when(false, command));
            assert!(command_allowed_when(true, command));
            assert_eq!(command_allowed(command), ENABLED);
        }
        assert!(command_allowed_when(false, "identity_list"));
    }

    /// Release builds are built without the feature: they must not offer the transfer.
    #[cfg(not(feature = "phone-transfer"))]
    #[test]
    fn a_build_without_the_feature_does_not_offer_the_transfer() {
        assert!(!command_allowed("phone_transfer_start"));
        assert!(!command_allowed("phone_transfer_cancel"));
        let info = crate::commands::diagnostics::app_info().expect("app info");
        assert!(!info.phone_transfer);
    }

    #[cfg(feature = "phone-transfer")]
    #[test]
    fn a_build_with_the_feature_offers_the_transfer() {
        assert!(command_allowed("phone_transfer_start"));
        let info = crate::commands::diagnostics::app_info().expect("app info");
        assert!(info.phone_transfer);
    }

    fn servers(count: usize) -> TransferPayload {
        TransferPayload {
            identities: Vec::new(),
            servers: Some(
                (0..count)
                    .map(|n| TransferServer {
                        name: format!("Server {n}"),
                        address: format!("voice-{n}.example.org"),
                        port: 64738,
                        username: format!("user-{n}"),
                        identity_fingerprint: None,
                    })
                    .collect(),
            ),
        }
    }

    fn transfer(count: usize) -> Transfer {
        Transfer::create(&servers(count), NOW).expect("transfer")
    }

    #[test]
    fn a_started_transfer_is_shown_with_its_code_and_time() {
        let state = PhoneTransferState::default();
        let ticket = state.begin();
        let view = state
            .show(ticket, transfer(3), VALIDITY_SECS, NOW)
            .expect("shown");
        assert_eq!(view.code.len(), 8);
        assert!(view.code.bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(view.total_parts as usize, view.codes.len());
        assert_eq!(view.seconds_left, 120);
        assert_eq!(state.seconds_left(ticket, NOW + 20), Some(100));
    }

    #[test]
    fn starting_again_replaces_the_transfer_with_a_new_code() {
        let state = PhoneTransferState::default();
        let old = state.begin();
        let first = state
            .show(old, transfer(3), VALIDITY_SECS, NOW)
            .expect("first");
        let ticket = state.begin();
        assert!(
            state.slot().shown.is_none(),
            "the old codes are off at once"
        );
        assert_eq!(state.seconds_left(old, NOW), None);
        let second = state
            .show(ticket, transfer(3), VALIDITY_SECS, NOW)
            .expect("second");
        assert_ne!(first.code, second.code);
        assert_ne!(first.codes, second.codes);
        assert_eq!(
            state
                .slot()
                .shown
                .as_ref()
                .map(|s| s.transfer.code().to_owned()),
            { Some(second.code.clone()) }
        );
    }

    #[test]
    fn an_overtaken_start_does_not_come_back() {
        let state = PhoneTransferState::default();
        let slow = state.begin();
        let fast = state.begin();
        state
            .show(fast, transfer(1), VALIDITY_SECS, NOW)
            .expect("the newer start is shown");
        assert!(state.show(slow, transfer(1), VALIDITY_SECS, NOW).is_err());
        assert_eq!(state.seconds_left(fast, NOW), Some(120));
        assert_eq!(state.seconds_left(slow, NOW), None);

        let late = state.begin();
        state.clear();
        assert!(state.show(late, transfer(1), VALIDITY_SECS, NOW).is_err());
        assert!(state.slot().shown.is_none(), "closed stays closed");
    }

    #[test]
    fn an_expired_transfer_is_dropped() {
        let state = PhoneTransferState::default();
        let ticket = state.begin();
        state
            .show(ticket, transfer(1), VALIDITY_SECS, NOW)
            .expect("shown");
        assert_eq!(state.seconds_left(ticket, NOW + 119), Some(1));
        assert_eq!(state.seconds_left(ticket, NOW + 120), None);
        assert!(state.slot().shown.is_none());
    }

    #[test]
    fn a_shortened_validity_expires_earlier() {
        let state = PhoneTransferState::default();
        let ticket = state.begin();
        let view = state.show(ticket, transfer(1), 5, NOW).expect("shown");
        assert_eq!(view.seconds_left, 5);
        assert_eq!(state.seconds_left(ticket, NOW + 4), Some(1));
        assert_eq!(state.seconds_left(ticket, NOW + 5), None);
        assert!(state.slot().shown.is_none());
    }

    #[test]
    fn cancelling_forgets_the_transfer() {
        let state = PhoneTransferState::default();
        let ticket = state.begin();
        state
            .show(ticket, transfer(1), VALIDITY_SECS, NOW)
            .expect("shown");
        state.clear();
        assert!(state.slot().shown.is_none());
        assert_eq!(state.seconds_left(ticket, NOW), None);
    }

    #[test]
    fn qr_codes_are_version_15_at_most_and_all_the_same_size() {
        // Enough servers that the compressed list does not fit into one part.
        let transfer = transfer(400);
        assert!(transfer.parts().len() > 1, "several parts");
        let codes = qr_codes(transfer.parts()).expect("codes");
        assert_eq!(codes.len(), transfer.parts().len());
        // 600 characters in alphanumeric mode with error correction M: version 15, 77 modules.
        assert_eq!(codes[0].size, 77);
        for qr in &codes {
            assert_eq!(qr.size, 77);
            assert_eq!(qr.rows.len(), 77);
            assert!(qr.rows.iter().all(|row| row.len() == 77));
        }
    }

    #[test]
    fn the_shown_parts_open_with_the_shown_code() {
        let state = PhoneTransferState::default();
        let view = state
            .show(state.begin(), transfer(3), VALIDITY_SECS, NOW)
            .expect("shown");
        let mut reader = TransferReader::new();
        let parts: Vec<String> = state
            .slot()
            .shown
            .as_ref()
            .expect("shown")
            .transfer
            .parts()
            .to_vec();
        for part in &parts {
            reader.add_part(part, NOW).expect("part");
        }
        let payload = reader.open(&view.code, NOW).expect("opens");
        assert_eq!(payload, servers(3));
    }

    #[test]
    fn text_outside_the_qr_alphabet_is_refused() {
        assert!(qr_codes(&["lower case".to_owned()]).is_err());
    }
}
