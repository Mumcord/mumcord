//! The Server administration window (Epic 4, issue #254): the server's ban list.
//!
//! The window reads the list when it opens and drops it when it closes or the connection ends
//! (FR-10.63, FR-10.50). Every save re-reads first and sends back the fresh list with only the
//! user's own change applied (FR-10.66). Entries carry the full address; the UI masks it by default
//! and shows it only for one row at a time (FR-10.51, FR-10.65).

use mumcord_engine::{Engine, banlist};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::net::IpAddr;
use tauri::State;

use super::types::{ApiError, ApiResult, ErrorCode, ServerId, to_engine_server_id};

/// One ban entry as the window shows it. `key` is what identifies the entry across a re-read, so
/// an edit or a removal can still be matched (FR-10.66). Name, reason and fingerprint are server
/// text: untrusted, and personal data.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct BanEntry {
    pub key: String,
    pub name: Option<String>,
    /// Hex certificate hash; empty when the entry bans by address only.
    pub cert_hash: Option<String>,
    /// Full address, IPv4 in IPv4 form. The UI masks it unless the row was revealed (FR-10.51).
    pub address: String,
    /// Subnet length in the family the UI shows: 32 for a single IPv4, 128 for a single IPv6.
    pub mask: u32,
    pub reason: Option<String>,
    /// RFC 3339, or `null` when the server sent a time Mumcord cannot read.
    pub start: Option<String>,
    /// Seconds; 0 is permanent.
    pub duration_secs: u32,
    pub expired: bool,
}

impl From<banlist::BanView> for BanEntry {
    fn from(v: banlist::BanView) -> Self {
        Self {
            key: v.key,
            name: v.name,
            cert_hash: v.cert_hash,
            address: v.address,
            mask: v.mask,
            reason: v.reason,
            start: v.start,
            duration_secs: v.duration_secs,
            expired: v.expired,
        }
    }
}

/// One change the window collected; a save applies all of them at once.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BanChangeInput {
    Add {
        name: Option<String>,
        hash: Option<String>,
        address: String,
        mask: u32,
        reason: Option<String>,
        duration: u32,
    },
    Edit {
        key: String,
        reason: Option<String>,
        duration: u32,
    },
    Remove {
        key: String,
    },
}

/// What a save produced: the new list, or the conflict the UI must ask about (FR-10.66).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BanSaveResult {
    Saved {
        entries: Vec<BanEntry>,
    },
    /// The entry was changed or removed on the server while the window was open.
    Conflict {
        key: String,
        fresh: Vec<BanEntry>,
    },
}

/// Reads the server's ban list for the window (FR-10.51). Fails with `permissionDenied` without
/// the Ban right, and with `serverUnreachable` when the server does not answer within 5 s.
#[tauri::command]
#[specta::specta]
pub async fn ban_list_open(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> ApiResult<Vec<BanEntry>> {
    Ok(entries(
        engine.ban_list_open(to_engine_server_id(server_id)).await?,
    ))
}

/// Saves the window's changes together. Before sending, the engine re-reads the list; if an entry
/// the user touched changed meanwhile it answers with `conflict` instead of saving (FR-10.66).
/// `force` is the UI's "Save mine anyway".
#[tauri::command]
#[specta::specta]
pub async fn ban_list_save(
    engine: State<'_, Engine>,
    server_id: ServerId,
    changes: Vec<BanChangeInput>,
    force: bool,
) -> ApiResult<BanSaveResult> {
    let mut prepared = Vec::new();
    for change in changes {
        prepared.push(to_engine_change(change)?);
    }
    let outcome = engine
        .ban_list_save(to_engine_server_id(server_id), &prepared, force)
        .await?;
    Ok(match outcome {
        banlist::BanSaveOutcome::Saved(entries) => BanSaveResult::Saved {
            entries: entries.into_iter().map(Into::into).collect(),
        },
        banlist::BanSaveOutcome::Conflict { key, fresh } => BanSaveResult::Conflict {
            key,
            fresh: fresh.into_iter().map(Into::into).collect(),
        },
    })
}

/// The window closed: what it held is dropped (FR-10.63).
#[tauri::command]
#[specta::specta]
pub fn ban_list_close(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<()> {
    engine.ban_list_close(to_engine_server_id(server_id));
    Ok(())
}

fn to_engine_change(change: BanChangeInput) -> ApiResult<banlist::BanChange> {
    match change {
        BanChangeInput::Add {
            name,
            hash,
            address,
            mask,
            reason,
            duration,
        } => {
            let parsed: IpAddr = address
                .parse()
                .map_err(|_| invalid("the address is not an IPv4 or IPv6 address"))?;
            let family = if parsed.is_ipv4() { 32 } else { 128 };
            if mask > family {
                return Err(invalid("the subnet mask cannot be longer than the address"));
            }
            Ok(banlist::BanChange::Add {
                name,
                cert_hash: hash,
                address: parsed,
                mask,
                reason,
                duration_secs: duration,
            })
        }
        BanChangeInput::Edit {
            key,
            reason,
            duration,
        } => Ok(banlist::BanChange::Edit {
            key,
            reason,
            duration_secs: duration,
        }),
        BanChangeInput::Remove { key } => Ok(banlist::BanChange::Remove { key }),
    }
}

fn invalid(message: &str) -> ApiError {
    ApiError::new(ErrorCode::InvalidInput, message)
}

fn entries(list: Vec<banlist::BanView>) -> Vec<BanEntry> {
    list.into_iter().map(Into::into).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(address: &str, mask: u32) -> BanChangeInput {
        BanChangeInput::Add {
            name: Some("Mallory".into()),
            hash: Some("a3f9c21e".into()),
            address: address.into(),
            mask,
            reason: Some("spam links".into()),
            duration: 3600,
        }
    }

    fn added(change: banlist::BanChange) -> u32 {
        match change {
            banlist::BanChange::Add { mask, .. } => mask,
            other => panic!("expected an add change, got {other:?}"),
        }
    }

    #[test]
    fn an_added_entry_needs_a_readable_address() {
        assert_eq!(
            added(to_engine_change(add("203.0.113.1", 32)).expect("v4")),
            32
        );
        let refused = to_engine_change(add("mallory.example", 32));
        assert_eq!(refused.err().map(|e| e.code), Some(ErrorCode::InvalidInput));
    }

    #[test]
    fn a_mask_cannot_be_longer_than_the_address_family() {
        assert_eq!(
            added(to_engine_change(add("2001:db8::1", 64)).expect("v6")),
            64
        );
        let refused = to_engine_change(add("203.0.113.1", 40));
        assert_eq!(refused.err().map(|e| e.code), Some(ErrorCode::InvalidInput));
    }
}
