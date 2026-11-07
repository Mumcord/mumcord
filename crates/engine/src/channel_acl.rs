//! A channel's permission rules and groups (Epic 4, issues #251 and #252).
//!
//! Reading is the standard ACL query; the answer holds the channel's own rules and groups plus the
//! ones it inherits. Saving sends the whole own part back, as the official client does. Before a
//! save Mumcord reads again: if someone else changed the rules meanwhile, the save stops and hands
//! the fresh rules back instead of overwriting them (FR-10.43). After the save it reads once more,
//! so "saved" is the server's own state.
//!
//! The channel password (FR-10.28) is the official client's pattern: "everyone: deny Join" plus
//! "group `#<password>`: allow Join". Mumcord recognises exactly that pattern and only ever touches
//! those two rules.
//!
//! Nothing here goes to disk (FR-10.63); logs never contain rules, names or passwords (FR-10.64).

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::Duration;

use mumcord_protocol::{DenyKind, PermissionDenied, permissions, proto};
use tokio::sync::oneshot;

use crate::ServerId;
use crate::session::{Engine, EngineError, EngineResult, ErrorKind, SessionCtx, convert, lock};

/// How long a read waits for the server, as the other admin dialogs do (FR-10.43).
pub const ANSWER_WAIT: Duration = crate::admin::ANSWER_WAIT;
/// Names for user ids are a nicety; the rules show without them after this.
const NAMES_WAIT: Duration = Duration::from_secs(2);

/// One rule. Later rules win over earlier ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AclRule {
    /// Comes from a parent channel; read-only here.
    pub inherited: bool,
    pub apply_here: bool,
    pub apply_subs: bool,
    /// Who: one registered user, or a group (one of the two is set).
    pub user_id: Option<u32>,
    pub group: Option<String>,
    /// Permission bits, see [`mumcord_protocol::permissions`].
    pub grant: u32,
    pub deny: u32,
}

/// One group as this channel sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AclGroup {
    pub name: String,
    /// Defined in a parent channel; it has no own members here unless `add` says so.
    pub inherited: bool,
    /// Takes the members of the parent's group of the same name.
    pub inherit: bool,
    /// Sub-channels may inherit this group.
    pub inheritable: bool,
    /// Registered user ids added here.
    pub add: Vec<u32>,
    /// Inherited members removed here.
    pub remove: Vec<u32>,
    /// Members that come from the parent.
    pub inherited_members: Vec<u32>,
}

/// A channel's rules and groups, as read from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelAcl {
    pub channel_id: u32,
    /// "Inherit rules from parent".
    pub inherit_acls: bool,
    pub groups: Vec<AclGroup>,
    /// Inherited rules first, then the channel's own ones.
    pub rules: Vec<AclRule>,
    /// Names of the registered users named in rules and groups, where the server told them.
    pub user_names: BTreeMap<u32, String>,
}

/// The channel password as the Password tab shows it (FR-10.28).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasswordState {
    None,
    Set(String),
    /// The Join rules are more than the password pattern; edit them under Permissions.
    Custom,
}

/// What a save produced (FR-10.43).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AclSaveOutcome {
    Saved(ChannelAcl),
    /// Someone changed the rules since they were read; nothing was sent.
    Conflict(ChannelAcl),
}

impl ChannelAcl {
    fn from_message(m: &proto::Acl) -> Self {
        Self {
            channel_id: m.channel_id,
            inherit_acls: m.inherit_acls.unwrap_or(true),
            groups: m
                .groups
                .iter()
                .map(|g| AclGroup {
                    name: g.name.clone(),
                    inherited: g.inherited.unwrap_or(true),
                    inherit: g.inherit.unwrap_or(true),
                    inheritable: g.inheritable.unwrap_or(true),
                    add: g.add.clone(),
                    remove: g.remove.clone(),
                    inherited_members: g.inherited_members.clone(),
                })
                .collect(),
            rules: m
                .acls
                .iter()
                .map(|a| AclRule {
                    inherited: a.inherited.unwrap_or(true),
                    apply_here: a.apply_here.unwrap_or(true),
                    apply_subs: a.apply_subs.unwrap_or(true),
                    user_id: a.user_id,
                    group: a.group.clone(),
                    grant: a.grant.unwrap_or(0),
                    deny: a.deny.unwrap_or(0),
                })
                .collect(),
            user_names: BTreeMap::new(),
        }
    }

    /// The message that writes this channel's own part back. Inherited rules are left out (the
    /// server ignores them); inherited groups are sent only where they carry own changes.
    fn to_message(&self) -> proto::Acl {
        proto::Acl {
            channel_id: self.channel_id,
            inherit_acls: Some(self.inherit_acls),
            groups: self
                .groups
                .iter()
                .filter(|g| {
                    !g.inherited
                        || !g.add.is_empty()
                        || !g.remove.is_empty()
                        || !g.inherit
                        || !g.inheritable
                })
                .map(|g| proto::acl::ChanGroup {
                    name: g.name.clone(),
                    inherited: Some(false),
                    inherit: Some(g.inherit),
                    inheritable: Some(g.inheritable),
                    add: g.add.clone(),
                    remove: g.remove.clone(),
                    inherited_members: Vec::new(),
                })
                .collect(),
            acls: self
                .rules
                .iter()
                .filter(|r| !r.inherited)
                .map(|r| proto::acl::ChanAcl {
                    apply_here: Some(r.apply_here),
                    apply_subs: Some(r.apply_subs),
                    inherited: Some(false),
                    user_id: r.user_id,
                    group: if r.user_id.is_some() {
                        None
                    } else {
                        r.group.clone()
                    },
                    grant: Some(r.grant),
                    deny: Some(r.deny),
                })
                .collect(),
            query: Some(false),
        }
    }

    /// The part a save writes, to compare a fresh read with what the editor started from.
    fn own_part(&self) -> proto::Acl {
        self.to_message()
    }

    /// The registered user ids named in rules and groups.
    fn user_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.rules.iter().filter_map(|r| r.user_id).collect();
        for group in &self.groups {
            ids.extend(&group.add);
            ids.extend(&group.remove);
            ids.extend(&group.inherited_members);
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}

/// The password group's name for `password`: the official client's `#<password>` token group.
fn token_group(rule: &AclRule) -> Option<&str> {
    rule.group.as_deref()?.strip_prefix('#')
}

fn is_deny_all(rule: &AclRule) -> bool {
    rule.user_id.is_none()
        && rule.group.as_deref() == Some("all")
        && rule.grant == 0
        && rule.deny == permissions::ENTER
}

fn is_token_allow(rule: &AclRule) -> bool {
    rule.user_id.is_none()
        && token_group(rule).is_some_and(|t| !t.is_empty())
        && rule.grant == permissions::ENTER
        && rule.deny == 0
}

/// Reads the channel password from the own rules (FR-10.28): exactly one "everyone: deny Join" and
/// one "`#<password>`: allow Join", and no other own rule about Join.
pub fn password_of(acl: &ChannelAcl) -> PasswordState {
    let own: Vec<&AclRule> = acl.rules.iter().filter(|r| !r.inherited).collect();
    let about_join: Vec<&&AclRule> = own
        .iter()
        .filter(|r| (r.grant | r.deny) & permissions::ENTER != 0)
        .collect();
    if about_join.is_empty() {
        return PasswordState::None;
    }
    let deny = about_join.iter().filter(|r| is_deny_all(r)).count();
    let tokens: Vec<&str> = about_join
        .iter()
        .filter(|r| is_token_allow(r))
        .filter_map(|r| token_group(r))
        .collect();
    match (deny, tokens.as_slice()) {
        (1, [token]) if about_join.len() == 2 => PasswordState::Set((*token).to_owned()),
        _ => PasswordState::Custom,
    }
}

/// The rules with the password set or cleared, touching only the two password rules (FR-10.28).
/// A channel with custom Join rules is refused: those are edited under Permissions.
pub fn with_password(acl: &ChannelAcl, password: Option<&str>) -> EngineResult<ChannelAcl> {
    if password_of(acl) == PasswordState::Custom {
        return Err(EngineError::invalid(
            "the Join rules are custom; edit them under Permissions",
        ));
    }
    let password = password.map(str::trim).filter(|p| !p.is_empty());
    if password.is_some_and(|p| p.chars().any(|c| c.is_whitespace() || c == ',')) {
        return Err(EngineError::invalid(
            "a channel password cannot contain spaces or commas",
        ));
    }
    let mut next = acl.clone();
    next.rules
        .retain(|r| r.inherited || !(is_deny_all(r) || is_token_allow(r)));
    if let Some(password) = password {
        // The official client's pattern: here only, so sub-channels keep their own access.
        let rule = |group: String, grant: u32, deny: u32| AclRule {
            inherited: false,
            apply_here: true,
            apply_subs: false,
            user_id: None,
            group: Some(group),
            grant,
            deny,
        };
        next.rules
            .push(rule("all".to_owned(), 0, permissions::ENTER));
        next.rules
            .push(rule(format!("#{password}"), permissions::ENTER, 0));
    }
    Ok(next)
}

/// A read waiting for the rules of one channel.
type ReadWait = oneshot::Sender<Result<ChannelAcl, EngineError>>;

/// Reads waiting for the server, per connection and channel.
#[derive(Default)]
pub struct ChannelAcls {
    reads: HashMap<(ServerId, u32), Vec<ReadWait>>,
    names: HashMap<ServerId, Vec<oneshot::Sender<proto::QueryUsers>>>,
}

impl Engine {
    /// Reads a channel's rules and groups (FR-10.36), with names for the user ids in them.
    pub async fn channel_acl_read(
        &self,
        server_id: ServerId,
        channel_id: u32,
    ) -> EngineResult<ChannelAcl> {
        let ctx = self.session_ctx_for(server_id)?;
        let mut acl = read(&ctx, self.channel_acl_state(), channel_id).await?;
        let ids = acl.user_ids();
        // People online are known already; only the others are asked for.
        let mut missing = Vec::new();
        ctx.handle.with_state(|state| {
            for id in &ids {
                match state.users.values().find(|u| u.user_id == Some(*id)) {
                    Some(user) => {
                        acl.user_names.insert(*id, user.name.clone());
                    }
                    None => missing.push(*id),
                }
            }
        });
        if !missing.is_empty() {
            let answer = {
                let (tx, rx) = oneshot::channel();
                lock(self.channel_acl_state())
                    .names
                    .entry(server_id)
                    .or_default()
                    .push(tx);
                rx
            };
            ctx.handle
                .query_users(missing)
                .map_err(convert::command_error)?;
            if let Ok(Ok(answer)) = tokio::time::timeout(NAMES_WAIT, answer).await {
                for (id, name) in answer.ids.iter().zip(answer.names) {
                    acl.user_names.insert(*id, name);
                }
            }
        }
        Ok(acl)
    }

    /// Saves the channel's own rules and groups (FR-10.37, FR-10.38). `started_from` is what the
    /// editor read; unless `force`, a save stops with the fresh rules when they changed meanwhile
    /// (FR-10.43).
    pub async fn channel_acl_save(
        &self,
        server_id: ServerId,
        acl: &ChannelAcl,
        started_from: &ChannelAcl,
        force: bool,
    ) -> EngineResult<AclSaveOutcome> {
        let ctx = self.session_ctx_for(server_id)?;
        let hub = self.channel_acl_state();
        let fresh = read(&ctx, hub, acl.channel_id).await?;
        if !force && fresh.own_part() != started_from.own_part() {
            tracing::info!(server_id, "acl save: changed meanwhile");
            return Ok(AclSaveOutcome::Conflict(fresh));
        }
        ctx.handle
            .send_acl(acl.to_message())
            .map_err(convert::command_error)?;
        // The server answers a write with nothing; reading again shows what it took. A user who
        // just gave up their own Edit right can no longer read, which also means it was saved.
        let saved = match read(&ctx, hub, acl.channel_id).await {
            Ok(saved) => saved,
            Err(error) if error.kind == ErrorKind::PermissionDenied => acl.clone(),
            Err(error) => return Err(error),
        };
        tracing::info!(server_id, "acl save: done");
        Ok(AclSaveOutcome::Saved(saved))
    }

    /// Sets or clears the channel password (FR-10.28), touching only its two rules.
    pub async fn channel_password_save(
        &self,
        server_id: ServerId,
        channel_id: u32,
        password: Option<&str>,
        started_from: &ChannelAcl,
        force: bool,
    ) -> EngineResult<AclSaveOutcome> {
        if started_from.channel_id != channel_id {
            return Err(EngineError::invalid("the rules are of another channel"));
        }
        let next = with_password(started_from, password)?;
        self.channel_acl_save(server_id, &next, started_from, force)
            .await
    }
}

/// Queries the rules and waits for the answer, a refusal, or [`ANSWER_WAIT`].
async fn read(
    ctx: &SessionCtx,
    hub: &Mutex<ChannelAcls>,
    channel_id: u32,
) -> EngineResult<ChannelAcl> {
    let answer = {
        let (tx, rx) = oneshot::channel();
        lock(hub)
            .reads
            .entry((ctx.server_id, channel_id))
            .or_default()
            .push(tx);
        rx
    };
    ctx.handle
        .query_acl(channel_id)
        .map_err(convert::command_error)?;
    match tokio::time::timeout(ANSWER_WAIT, answer).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(EngineError::not_connected()),
        Err(_) => {
            lock(hub).reads.remove(&(ctx.server_id, channel_id));
            Err(EngineError::new(
                ErrorKind::ServerUnreachable,
                "the server did not answer",
            ))
        }
    }
}

/// The server's answer to an ACL query.
pub(crate) fn on_acl(hub: &Mutex<ChannelAcls>, server_id: ServerId, m: &proto::Acl) {
    let waiting = lock(hub).reads.remove(&(server_id, m.channel_id));
    for tx in waiting.into_iter().flatten() {
        let _ = tx.send(Ok(ChannelAcl::from_message(m)));
    }
}

/// Names the server sent for user ids.
pub(crate) fn on_users(hub: &Mutex<ChannelAcls>, server_id: ServerId, m: &proto::QueryUsers) {
    let waiting = lock(hub).names.remove(&server_id);
    for tx in waiting.into_iter().flatten() {
        let _ = tx.send(m.clone());
    }
}

/// A refused read (the Write right is missing); `true` when a read took it.
pub(crate) fn on_denied(
    hub: &Mutex<ChannelAcls>,
    server_id: ServerId,
    own_session: Option<u32>,
    denied: &PermissionDenied,
) -> bool {
    let own = denied.session.is_none() || denied.session == own_session;
    if !own || denied.kind != DenyKind::Permission || denied.permission != Some(permissions::WRITE)
    {
        return false;
    }
    let Some(channel_id) = denied.channel_id else {
        return false;
    };
    let Some(waiting) = lock(hub).reads.remove(&(server_id, channel_id)) else {
        return false;
    };
    for tx in waiting {
        let _ = tx.send(Err(convert::denial_error(denied)));
    }
    true
}

/// The connection ended: every waiting read is told so.
pub(crate) fn on_disconnect(hub: &Mutex<ChannelAcls>, server_id: ServerId) {
    let mut hub = lock(hub);
    hub.reads.retain(|(id, _), _| *id != server_id);
    hub.names.remove(&server_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(group: &str, grant: u32, deny: u32) -> AclRule {
        AclRule {
            inherited: false,
            apply_here: true,
            apply_subs: false,
            user_id: None,
            group: Some(group.to_owned()),
            grant,
            deny,
        }
    }

    fn acl(rules: Vec<AclRule>) -> ChannelAcl {
        ChannelAcl {
            channel_id: 4,
            inherit_acls: true,
            groups: Vec::new(),
            rules,
            user_names: BTreeMap::new(),
        }
    }

    const ENTER: u32 = permissions::ENTER;

    #[test]
    fn the_official_password_pattern_is_recognised() {
        let speak = rule("all", 0, permissions::SPEAK);
        let set = acl(vec![
            speak.clone(),
            rule("all", 0, ENTER),
            rule("#secret", ENTER, 0),
        ]);
        assert_eq!(password_of(&set), PasswordState::Set("secret".into()));
        assert_eq!(password_of(&acl(vec![speak])), PasswordState::None);
    }

    #[test]
    fn more_join_rules_are_custom() {
        let two_tokens = acl(vec![
            rule("all", 0, ENTER),
            rule("#a", ENTER, 0),
            rule("#b", ENTER, 0),
        ]);
        assert_eq!(password_of(&two_tokens), PasswordState::Custom);
        let group_join = acl(vec![rule("all", 0, ENTER), rule("mods", ENTER, 0)]);
        assert_eq!(password_of(&group_join), PasswordState::Custom);
        assert!(with_password(&group_join, Some("x")).is_err());
    }

    #[test]
    fn inherited_join_rules_do_not_count() {
        let mut inherited = rule("all", 0, ENTER);
        inherited.inherited = true;
        assert_eq!(password_of(&acl(vec![inherited])), PasswordState::None);
    }

    #[test]
    fn changing_the_password_touches_only_its_two_rules() {
        let speak = rule("out", 0, permissions::SPEAK);
        let start = acl(vec![
            rule("all", 0, ENTER),
            speak.clone(),
            rule("#old", ENTER, 0),
        ]);
        let next = with_password(&start, Some("new")).expect("set");
        assert_eq!(next.rules[0], speak);
        assert_eq!(password_of(&next), PasswordState::Set("new".into()));
        let cleared = with_password(&next, None).expect("clear");
        assert_eq!(cleared.rules, vec![speak]);
        assert!(with_password(&start, Some("two words")).is_err());
    }

    #[test]
    fn a_save_sends_only_the_own_part() {
        let mut inherited = rule("all", permissions::SPEAK, 0);
        inherited.inherited = true;
        let mut group = AclGroup {
            name: "admin".into(),
            inherited: true,
            inherit: true,
            inheritable: true,
            add: Vec::new(),
            remove: Vec::new(),
            inherited_members: vec![3],
        };
        let mut channel = acl(vec![inherited, rule("mods", ENTER, 0)]);
        channel.groups.push(group.clone());
        let message = channel.to_message();
        assert_eq!(message.acls.len(), 1);
        assert!(
            message.groups.is_empty(),
            "an untouched inherited group stays out"
        );
        group.add.push(9);
        channel.groups[0] = group;
        let message = channel.to_message();
        assert_eq!(message.groups[0].add, vec![9]);
        assert!(message.groups[0].inherited_members.is_empty());
    }
}
