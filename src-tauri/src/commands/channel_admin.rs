//! The Channel window (Epic 4, issue #250): create, edit, move, link and delete channels.
//!
//! Each command waits for the server's answer. A refusal is a normal result with the server's
//! reason, so the open window words it (FR-10.59, FR-10.60) and no second notice shows.

use mumcord_engine::{ChannelChange, ChannelCreate, ChannelOutcome, Engine};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::session::{DenialKind, Right};
use super::types::{ApiResult, ChannelId, ServerId, to_engine_server_id};

/// A new channel from the Channel window's General tab (FR-10.21, FR-10.22).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelCreateInput {
    pub parent_id: ChannelId,
    pub name: String,
    /// Clean HTML from the description editor; `None` for none.
    pub description: Option<String>,
    pub position: i32,
    /// 0 is the server's own limit.
    pub max_users: u32,
    pub temporary: bool,
}

/// What the Channel window changed; unset fields stay as they are (FR-10.23, FR-10.24, FR-10.27).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelChangeInput {
    pub name: Option<String>,
    /// An empty string removes the description.
    pub description: Option<String>,
    /// A new parent moves the channel with all its sub-channels.
    pub parent_id: Option<ChannelId>,
    pub position: Option<i32>,
    pub max_users: Option<u32>,
    pub links_add: Vec<ChannelId>,
    pub links_remove: Vec<ChannelId>,
}

/// The server's answer to a channel change.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChannelResult {
    /// Applied; `channelId` is the new channel after a create.
    #[serde(rename_all = "camelCase")]
    Confirmed { channel_id: ChannelId },
    /// No answer within 5 s; the change was written to the server.
    Unconfirmed,
    /// Refused, with the kind, the missing right and the server's own text (untrusted).
    Refused {
        denial: DenialKind,
        permission: Option<Right>,
        reason: Option<String>,
    },
}

impl From<ChannelOutcome> for ChannelResult {
    fn from(outcome: ChannelOutcome) -> Self {
        match outcome {
            ChannelOutcome::Confirmed { channel_id } => Self::Confirmed { channel_id },
            ChannelOutcome::Unconfirmed => Self::Unconfirmed,
            ChannelOutcome::Refused {
                kind,
                permission,
                reason,
            } => Self::Refused {
                denial: kind.into(),
                permission: permission.map(Into::into),
                reason,
            },
        }
    }
}

/// Creates a channel (FR-10.21, FR-10.22). A temporary channel moves you into it.
#[tauri::command]
#[specta::specta]
pub async fn channel_create(
    engine: State<'_, Engine>,
    server_id: ServerId,
    input: ChannelCreateInput,
) -> ApiResult<ChannelResult> {
    let request = ChannelCreate {
        parent: input.parent_id,
        name: input.name,
        description: input.description.filter(|d| !d.is_empty()),
        position: input.position,
        max_users: input.max_users,
        temporary: input.temporary,
    };
    let outcome = engine
        .channel_create(to_engine_server_id(server_id), &request)
        .await?;
    Ok(outcome.into())
}

/// Edits, moves or links a channel (FR-10.23, FR-10.24, FR-10.27); also the menu's quick "Link with
/// my channel" and "Unlink all".
#[tauri::command]
#[specta::specta]
pub async fn channel_edit(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
    change: ChannelChangeInput,
) -> ApiResult<ChannelResult> {
    let change = ChannelChange {
        name: change.name,
        description: change.description,
        parent: change.parent_id,
        position: change.position,
        max_users: change.max_users,
        links_add: change.links_add,
        links_remove: change.links_remove,
    };
    let outcome = engine
        .channel_edit(to_engine_server_id(server_id), channel_id, &change)
        .await?;
    Ok(outcome.into())
}

/// Deletes a channel with its sub-channels; the people in them go to the parent (FR-10.25).
#[tauri::command]
#[specta::specta]
pub async fn channel_remove(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
) -> ApiResult<ChannelResult> {
    let outcome = engine
        .channel_remove(to_engine_server_id(server_id), channel_id)
        .await?;
    Ok(outcome.into())
}

/// One permission rule (FR-10.37). `grant` and `deny` are Mumble permission bits.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct AclRuleView {
    pub inherited: bool,
    pub apply_here: bool,
    pub apply_subs: bool,
    /// One registered user, or else `group`.
    pub user_id: Option<u32>,
    pub group: Option<String>,
    pub grant: u32,
    pub deny: u32,
}

/// One group as this channel sees it (FR-10.38).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct AclGroupView {
    pub name: String,
    pub inherited: bool,
    pub inherit: bool,
    pub inheritable: bool,
    pub add: Vec<u32>,
    pub remove: Vec<u32>,
    pub inherited_members: Vec<u32>,
}

/// A registered user's name, as the server told it (untrusted).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct UserName {
    pub id: u32,
    pub name: String,
}

/// The channel password as the Password tab shows it (FR-10.28).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PasswordView {
    None,
    Set {
        password: String,
    },
    /// The Join rules are more than a password; they are edited under Permissions.
    Custom,
}

/// A channel's rules and groups (FR-10.36).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChannelAclView {
    pub channel_id: ChannelId,
    pub inherit_acls: bool,
    pub groups: Vec<AclGroupView>,
    /// Inherited rules first, then the channel's own ones; later rules win.
    pub rules: Vec<AclRuleView>,
    pub user_names: Vec<UserName>,
    /// Worked out from the rules; ignored when saving.
    pub password: PasswordView,
}

/// What a save produced (FR-10.43).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AclSaveResult {
    Saved {
        acl: ChannelAclView,
    },
    /// Someone changed the rules since they were read; nothing was sent.
    Conflict {
        acl: ChannelAclView,
    },
}

fn acl_view(acl: mumcord_engine::ChannelAcl) -> ChannelAclView {
    let password = match mumcord_engine::password_of(&acl) {
        mumcord_engine::PasswordState::None => PasswordView::None,
        mumcord_engine::PasswordState::Set(password) => PasswordView::Set { password },
        mumcord_engine::PasswordState::Custom => PasswordView::Custom,
    };
    ChannelAclView {
        channel_id: acl.channel_id,
        inherit_acls: acl.inherit_acls,
        groups: acl
            .groups
            .into_iter()
            .map(|g| AclGroupView {
                name: g.name,
                inherited: g.inherited,
                inherit: g.inherit,
                inheritable: g.inheritable,
                add: g.add,
                remove: g.remove,
                inherited_members: g.inherited_members,
            })
            .collect(),
        rules: acl
            .rules
            .into_iter()
            .map(|r| AclRuleView {
                inherited: r.inherited,
                apply_here: r.apply_here,
                apply_subs: r.apply_subs,
                user_id: r.user_id,
                group: r.group,
                grant: r.grant,
                deny: r.deny,
            })
            .collect(),
        user_names: acl
            .user_names
            .into_iter()
            .map(|(id, name)| UserName { id, name })
            .collect(),
        password,
    }
}

fn acl_of(view: ChannelAclView) -> mumcord_engine::ChannelAcl {
    mumcord_engine::ChannelAcl {
        channel_id: view.channel_id,
        inherit_acls: view.inherit_acls,
        groups: view
            .groups
            .into_iter()
            .map(|g| mumcord_engine::AclGroup {
                name: g.name,
                inherited: g.inherited,
                inherit: g.inherit,
                inheritable: g.inheritable,
                add: g.add,
                remove: g.remove,
                inherited_members: g.inherited_members,
            })
            .collect(),
        rules: view
            .rules
            .into_iter()
            .map(|r| mumcord_engine::AclRule {
                inherited: r.inherited,
                apply_here: r.apply_here,
                apply_subs: r.apply_subs,
                user_id: r.user_id,
                group: r.group,
                grant: r.grant,
                deny: r.deny,
            })
            .collect(),
        user_names: view
            .user_names
            .into_iter()
            .map(|u| (u.id, u.name))
            .collect(),
    }
}

fn save_result(outcome: mumcord_engine::AclSaveOutcome) -> AclSaveResult {
    match outcome {
        mumcord_engine::AclSaveOutcome::Saved(acl) => AclSaveResult::Saved { acl: acl_view(acl) },
        mumcord_engine::AclSaveOutcome::Conflict(acl) => {
            AclSaveResult::Conflict { acl: acl_view(acl) }
        }
    }
}

/// Reads a channel's rules and groups (FR-10.36). Fails with `permissionDenied` without the right
/// to edit them, and `serverUnreachable` without an answer within 5 s (FR-10.43).
#[tauri::command]
#[specta::specta]
pub async fn channel_acl_read(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
) -> ApiResult<ChannelAclView> {
    let acl = engine
        .channel_acl_read(to_engine_server_id(server_id), channel_id)
        .await?;
    Ok(acl_view(acl))
}

/// Saves the channel's own rules and groups; `started_from` is what the editor read, so a change by
/// someone else meanwhile comes back as a conflict unless `force` (FR-10.43).
#[tauri::command]
#[specta::specta]
pub async fn channel_acl_save(
    engine: State<'_, Engine>,
    server_id: ServerId,
    acl: ChannelAclView,
    started_from: ChannelAclView,
    force: bool,
) -> ApiResult<AclSaveResult> {
    let outcome = engine
        .channel_acl_save(
            to_engine_server_id(server_id),
            &acl_of(acl),
            &acl_of(started_from),
            force,
        )
        .await?;
    Ok(save_result(outcome))
}

/// Sets or clears the channel password (FR-10.28); `None` or an empty text clears it.
#[tauri::command]
#[specta::specta]
pub async fn channel_password_save(
    engine: State<'_, Engine>,
    server_id: ServerId,
    channel_id: ChannelId,
    password: Option<String>,
    started_from: ChannelAclView,
    force: bool,
) -> ApiResult<AclSaveResult> {
    let outcome = engine
        .channel_password_save(
            to_engine_server_id(server_id),
            channel_id,
            password.as_deref(),
            &acl_of(started_from),
            force,
        )
        .await?;
    Ok(save_result(outcome))
}

/// A row of the "Who can do what" grid (FR-10.40).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AclRowInput {
    Everyone,
    Group {
        name: String,
    },
    /// Someone online.
    Session {
        session: u32,
    },
    /// A registered user, online or not.
    User {
        user_id: u32,
    },
}

/// One cell's answer (FR-10.45–FR-10.47).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum AclVerdict {
    Allowed,
    Denied,
    Depends,
}

/// Why a rule matched the row.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AclMatchReason {
    Everyone,
    Registered,
    InChannel,
    NotInChannel,
    Member { group: String },
    ThisPerson,
}

/// Why a cell depends (FR-10.47).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AclCondition {
    Password { token: String },
    Presence,
    ServerGroup { group: String },
}

/// One cell of the grid with what the "Why?" panel shows (FR-10.46).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct AclCell {
    /// The permission bit.
    pub permission: u32,
    pub verdict: AclVerdict,
    /// Index into the rules of the rule that decided it; `null` for the default rights.
    pub rule: Option<u32>,
    pub overrides: Option<u32>,
    pub reason: Option<AclMatchReason>,
    pub condition: Option<AclCondition>,
    pub via_write: bool,
    pub superuser: bool,
}

/// Works out the grid rows for the editor's (possibly unsaved) rules (FR-10.40, FR-10.45). Runs in
/// the core, on the open channel only.
#[tauri::command]
#[specta::specta]
pub fn channel_acl_evaluate(
    engine: State<'_, Engine>,
    server_id: ServerId,
    acl: ChannelAclView,
    rows: Vec<AclRowInput>,
) -> ApiResult<Vec<Vec<AclCell>>> {
    use mumcord_engine::acl_eval::{Condition, MatchReason, RowSubject, Verdict};
    let rows: Vec<RowSubject> = rows
        .into_iter()
        .map(|row| match row {
            AclRowInput::Everyone => RowSubject::Everyone,
            AclRowInput::Group { name } => RowSubject::Group(name),
            AclRowInput::Session { session } => RowSubject::Session(session),
            AclRowInput::User { user_id } => RowSubject::UserId(user_id),
        })
        .collect();
    let index = |i: usize| u32::try_from(i).unwrap_or(u32::MAX);
    let result =
        engine.channel_acl_evaluate(to_engine_server_id(server_id), &acl_of(acl), &rows)?;
    Ok(result
        .into_iter()
        .map(|cells| {
            cells
                .into_iter()
                .map(|cell| AclCell {
                    permission: cell.permission,
                    verdict: match cell.verdict {
                        Verdict::Allowed => AclVerdict::Allowed,
                        Verdict::Denied => AclVerdict::Denied,
                        Verdict::Depends => AclVerdict::Depends,
                    },
                    rule: cell.rule.map(index),
                    overrides: cell.overrides.map(index),
                    reason: cell.reason.map(|reason| match reason {
                        MatchReason::Everyone => AclMatchReason::Everyone,
                        MatchReason::Registered => AclMatchReason::Registered,
                        MatchReason::InChannel => AclMatchReason::InChannel,
                        MatchReason::NotInChannel => AclMatchReason::NotInChannel,
                        MatchReason::Member(group) => AclMatchReason::Member { group },
                        MatchReason::ThisPerson => AclMatchReason::ThisPerson,
                    }),
                    condition: cell.condition.map(|condition| match condition {
                        Condition::Password(token) => AclCondition::Password { token },
                        Condition::Presence => AclCondition::Presence,
                        Condition::ServerGroup(group) => AclCondition::ServerGroup { group },
                    }),
                    via_write: cell.via_write,
                    superuser: cell.superuser,
                })
                .collect()
        })
        .collect())
}

/// The rights the user would lose here with these rules, as permission bits (FR-10.41); empty for
/// the SuperUser.
#[tauri::command]
#[specta::specta]
pub fn channel_acl_lockout(
    engine: State<'_, Engine>,
    server_id: ServerId,
    acl: ChannelAclView,
) -> ApiResult<Vec<u32>> {
    Ok(engine.channel_acl_lockout(to_engine_server_id(server_id), &acl_of(acl))?)
}
