//! The Server administration window (Epic 4, issue #254): the server's ban list, and the kick and
//! ban of a person (Epic 4, issue #253).
//!
//! The window reads the list when it opens and drops it when it closes or the connection ends
//! (FR-10.63, FR-10.50). Every save re-reads first and sends back the fresh list with only the
//! user's own change applied (FR-10.66). Entries carry the full address; the UI masks it by default
//! and shows it only for one row at a time (FR-10.51, FR-10.65).

use mumcord_engine::{Engine, banlist, registered};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::net::IpAddr;
use tauri::State;

use super::types::{ApiError, ApiResult, ErrorCode, ServerId, SessionId, to_engine_server_id};

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

/// What a kick or ban dialog needs to know about a person before it can be filled (issue #253):
/// the address the server reports only to admins, how long the person is connected, whether the ban
/// list can be read, the certificate hash, and the two rights (FR-10.11, FR-10.12, FR-10.13).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersonTarget {
    pub session: SessionId,
    /// Full address; the UI masks it by default (FR-10.51, FR-10.65).
    pub address: Option<String>,
    /// Seconds the person is connected, `null` when the server did not report it.
    pub online_secs: Option<u32>,
    /// False when the server will not show its list, so only its own default ban length works.
    pub ban_list_readable: bool,
    pub cert_hash: Option<String>,
    /// The rights in the root channel, which decide what the menu offers at all (FR-10.1, FR-10.2).
    pub can_kick: bool,
    pub can_ban: bool,
}

/// What the ban dialog sends (FR-10.12).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct BanInput {
    reason: Option<String>,
    /// Seconds; 0 is permanent. Ignored when `quick` is set.
    duration: u32,
    by_cert: bool,
    by_ip: bool,
    /// Subnet length in the family of the address: 32 for a single IPv4, 128 for a single IPv6.
    mask: u32,
    remove_now: bool,
    /// The dialog's "Save mine anyway" after a conflict (FR-10.66).
    force: bool,
    /// The quick-ban path, which only a server that will not show its list allows (FR-10.13).
    quick: bool,
}

/// What a kick produced: the server's answer, or nothing within 5 s.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum KickResult {
    Confirmed,
    Unconfirmed,
}

/// What a ban produced (FR-10.12, FR-10.13, FR-10.66).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BanResult {
    /// One entry added to the server's list; `removed` says whether the person was taken off too.
    Saved { key: String, removed: bool },
    /// The server's own "remove with ban" message with its default length (FR-10.13).
    QuickBanned,
    /// The entry the dialog wanted to add is different on the server already (FR-10.66).
    Conflict { key: String, fresh: Vec<BanEntry> },
}

/// Reads what the kick or ban dialog needs about a person (issue #253). Fails with
/// `permissionDenied` without the Kick or the Ban right in the root channel.
#[tauri::command]
#[specta::specta]
pub async fn person_target(
    engine: State<'_, Engine>,
    server_id: ServerId,
    session: SessionId,
) -> ApiResult<PersonTarget> {
    let target = engine
        .admin_target(to_engine_server_id(server_id), session)
        .await?;
    Ok(PersonTarget {
        session,
        address: target.address.map(|a| a.to_string()),
        online_secs: target
            .online_secs
            .map(|s| u32::try_from(s).unwrap_or(u32::MAX)),
        ban_list_readable: target.ban_list_readable,
        cert_hash: target.cert_hash,
        can_kick: target.can_kick,
        can_ban: target.can_ban,
    })
}

/// Kicks a person with an optional reason (FR-10.11). A refusal arrives as an error, which the open
/// dialog shows; yourself is never offered, so it is refused here too.
#[tauri::command]
#[specta::specta]
pub async fn person_kick(
    engine: State<'_, Engine>,
    server_id: ServerId,
    session: SessionId,
    reason: Option<String>,
) -> ApiResult<KickResult> {
    let outcome = engine
        .kick_person(to_engine_server_id(server_id), session, reason.as_deref())
        .await?;
    Ok(match outcome {
        mumcord_engine::KickOutcome::Confirmed => KickResult::Confirmed,
        mumcord_engine::KickOutcome::Unconfirmed => KickResult::Unconfirmed,
    })
}

/// Bans a person: one entry added to the server's ban list with the safe save of FR-10.66, then the
/// person is removed when the dialog asked for it. Without a readable ban list the dialog sends the
/// server's own message instead (FR-10.13).
#[tauri::command]
#[specta::specta]
pub async fn person_ban(
    engine: State<'_, Engine>,
    server_id: ServerId,
    session: SessionId,
    input: BanInput,
) -> ApiResult<BanResult> {
    let request = to_ban_request(input)?;
    let outcome = engine
        .ban_person(to_engine_server_id(server_id), session, &request)
        .await?;
    Ok(match outcome {
        mumcord_engine::BanOutcome::Saved { key, removed } => BanResult::Saved { key, removed },
        mumcord_engine::BanOutcome::QuickBanned => BanResult::QuickBanned,
        mumcord_engine::BanOutcome::Conflict { key, fresh } => BanResult::Conflict {
            key,
            fresh: fresh.into_iter().map(Into::into).collect(),
        },
    })
}

/// The dialog closed: what it held is dropped (FR-10.63).
#[tauri::command]
#[specta::specta]
pub fn person_action_close(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<()> {
    engine.admin_close(to_engine_server_id(server_id));
    Ok(())
}

/// One registered account as the window shows it (issue #256). `userId` is the key of the row: the
/// server does not keep names unique in a way a client can rely on, and only the id says which
/// account a change acts on. `certHash` is personal data shown only to a user with the Ban right
/// (FR-10.65).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredUserEntry {
    pub user_id: u32,
    pub name: String,
    /// RFC 3339, or `null` when the server does not say (FR-10.5).
    pub last_seen: Option<String>,
    /// The channel the account was last seen in, named, or `null` when the server does not say.
    pub last_channel: Option<String>,
    /// Whether one of their accounts is connected right now.
    pub online: bool,
    pub cert_hash: Option<String>,
}

impl From<registered::RegisteredView> for RegisteredUserEntry {
    fn from(v: registered::RegisteredView) -> Self {
        Self {
            user_id: v.user_id,
            name: v.name,
            last_seen: v.last_seen,
            last_channel: v.last_channel,
            online: v.online,
            cert_hash: v.cert_hash,
        }
    }
}

/// What a rename or an unregister produced (issue #256). The window shows `entries` either way,
/// because the server's own list is the only account of what stands.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RegisteredSaveResult {
    /// The server's list now carries the change.
    Saved { entries: Vec<RegisteredUserEntry> },
    /// The list still shows the account as it was, which the server does not say in words: the
    /// usual cause is a name another account already holds.
    Kept { entries: Vec<RegisteredUserEntry> },
}

/// Reads the server's registered accounts for the window (issue #256). Fails with
/// `permissionDenied` without the Register right in the root channel, and with `serverUnreachable`
/// when the server does not answer within 5 s. Nothing is stored on disk (FR-10.63).
#[tauri::command]
#[specta::specta]
pub async fn registered_users_open(
    engine: State<'_, Engine>,
    server_id: ServerId,
) -> ApiResult<Vec<RegisteredUserEntry>> {
    Ok(registered_entries(
        engine
            .registered_open(to_engine_server_id(server_id))
            .await?,
    ))
}

/// Renames one account (FR-10.52, AC-10.37). One `UserList` entry carries the new name; the answer
/// is the list the server holds afterwards, which is what tells whether it took the name.
#[tauri::command]
#[specta::specta]
pub async fn registered_user_rename(
    engine: State<'_, Engine>,
    server_id: ServerId,
    user_id: u32,
    name: String,
) -> ApiResult<RegisteredSaveResult> {
    let outcome = engine
        .registered_rename(to_engine_server_id(server_id), user_id, &name)
        .await?;
    Ok(to_save_result(outcome))
}

/// Removes one registration (FR-10.52, AC-10.37). The person can still connect, as a guest, and
/// loses the name and the group memberships that name them.
#[tauri::command]
#[specta::specta]
pub async fn registered_user_unregister(
    engine: State<'_, Engine>,
    server_id: ServerId,
    user_id: u32,
) -> ApiResult<RegisteredSaveResult> {
    let outcome = engine
        .registered_unregister(to_engine_server_id(server_id), user_id)
        .await?;
    Ok(to_save_result(outcome))
}

/// Registers an online guest under their current name (FR-10.20, AC-10.15). Fails with
/// `permissionDenied` without the Register right, and with `invalidInput` when the person is not
/// connected, is registered already, or has no certificate to bind the account to.
#[tauri::command]
#[specta::specta]
pub fn person_register(
    engine: State<'_, Engine>,
    server_id: ServerId,
    session: SessionId,
) -> ApiResult<()> {
    engine
        .registered_register_person(to_engine_server_id(server_id), session)
        .map_err(|error| error.into())
}

/// The groups that name one account in a channel (issue #256). A server that will not show its
/// groups answers with an empty list rather than an error.
#[tauri::command]
#[specta::specta]
pub async fn registered_user_groups(
    engine: State<'_, Engine>,
    server_id: ServerId,
    user_id: u32,
) -> ApiResult<Vec<String>> {
    engine
        .registered_groups(to_engine_server_id(server_id), user_id)
        .await
        .map_err(|error| error.into())
}

/// The window closed: what it held is dropped (FR-10.63).
#[tauri::command]
#[specta::specta]
pub fn registered_users_close(engine: State<'_, Engine>, server_id: ServerId) -> ApiResult<()> {
    engine.registered_close(to_engine_server_id(server_id));
    Ok(())
}

fn to_save_result(outcome: registered::RegisteredOutcome) -> RegisteredSaveResult {
    match outcome {
        registered::RegisteredOutcome::Saved { entries } => RegisteredSaveResult::Saved {
            entries: entries.into_iter().map(Into::into).collect(),
        },
        registered::RegisteredOutcome::Kept { entries } => RegisteredSaveResult::Kept {
            entries: entries.into_iter().map(Into::into).collect(),
        },
    }
}

fn to_ban_request(input: BanInput) -> ApiResult<mumcord_engine::BanRequest> {
    if !input.by_cert && !input.by_ip {
        return Err(invalid("at least one target must stay on"));
    }
    Ok(mumcord_engine::BanRequest {
        reason: input
            .reason
            .filter(|r| !r.trim().is_empty())
            .map(|r| r.trim().to_owned()),
        duration_secs: input.duration,
        by_cert: input.by_cert,
        by_ip: input.by_ip,
        mask: input.mask,
        remove_now: input.remove_now,
        force: input.force,
        quick: input.quick,
    })
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

fn registered_entries(list: Vec<registered::RegisteredView>) -> Vec<RegisteredUserEntry> {
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

/// A server-side moderation action on one person (issue #257).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ModerateInput {
    Mute { on: bool },
    Deafen { on: bool },
    PrioritySpeaker { on: bool },
    Unsuppress,
    ResetComment,
    ResetAvatar,
}

/// Server mute, deafen, priority speaker, un-suppress, reset comment or avatar (FR-10.15,
/// FR-10.19). Acts at once; a refusal arrives as a `serverDenied` notice with the server's reason.
#[tauri::command]
#[specta::specta]
pub fn person_moderate(
    engine: State<'_, Engine>,
    server_id: ServerId,
    session: SessionId,
    action: ModerateInput,
) -> ApiResult<()> {
    use mumcord_engine::ModerateAction as A;
    let action = match action {
        ModerateInput::Mute { on } => A::Mute(on),
        ModerateInput::Deafen { on } => A::Deafen(on),
        ModerateInput::PrioritySpeaker { on } => A::PrioritySpeaker(on),
        ModerateInput::Unsuppress => A::Unsuppress,
        ModerateInput::ResetComment => A::ResetComment,
        ModerateInput::ResetAvatar => A::ResetAvatar,
    };
    Ok(engine.person_moderate(to_engine_server_id(server_id), session, action)?)
}
