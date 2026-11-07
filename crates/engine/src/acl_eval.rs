//! Who may do what in a channel, worked out from its rules (Epic 4, issue #252, FR-10.45–FR-10.48).
//!
//! The same steps as the standard server: start from the default rights, apply the rules from Root
//! down to the channel in order, the last matching rule wins for each permission, and "Edit
//! permissions" (Write) brings most other rights with it. Mumcord only sees the open channel's
//! answer, so where the result hangs on something it cannot know (a channel password, being in
//! another channel, a group the server fills itself) the answer is "depends", with the condition.
//! The server has the final say; nothing here is stored (FR-10.63).

use mumcord_protocol::permissions as p;

use crate::channel_acl::{AclGroup, AclRule, ChannelAcl};

/// What every user may do before any rule applies.
const DEFAULT: u32 = p::TRAVERSE | p::ENTER | p::SPEAK | p::WHISPER | p::TEXT_MESSAGE | p::LISTEN;
/// What "Edit permissions" brings with it in any channel.
const WRITE_IMPLIES: u32 = p::TRAVERSE
    | p::ENTER
    | p::MUTE_DEAFEN
    | p::MOVE
    | p::MAKE_CHANNEL
    | p::LINK_CHANNEL
    | p::TEXT_MESSAGE
    | p::MAKE_TEMP_CHANNEL
    | p::LISTEN;
/// What it brings with it in the Root channel on top.
const WRITE_IMPLIES_ROOT: u32 =
    p::KICK | p::BAN | p::REGISTER | p::SELF_REGISTER | p::RESET_USER_CONTENT;

/// Who a row of the grid is (FR-10.40).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// Someone who is in no group: matches only "all".
    Everyone,
    /// A member of this group, and of no other named group.
    Group(String),
    /// One person.
    Person(Person),
}

/// What Mumcord knows about a person.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Person {
    pub user_id: Option<u32>,
    /// `Some(true)` while they are in the open channel; `None` when offline or unknown.
    pub in_channel: Option<bool>,
    pub superuser: bool,
}

/// The answer for one permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    Denied,
    /// Hangs on something Mumcord cannot know; see the condition.
    Depends,
}

/// Why a cell is "depends" (FR-10.47).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// A password (access token) group: allowed if they entered that password.
    Password(String),
    /// Being in a channel: "in" or "out" for someone offline or in an inherited rule.
    Presence,
    /// A group the server works out itself (`sub`, `$hash`, `strong`, …).
    ServerGroup(String),
}

/// Why a rule matched the row (FR-10.46).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchReason {
    Everyone,
    Registered,
    InChannel,
    NotInChannel,
    Member(String),
    ThisPerson,
}

/// One cell of the grid with what the "Why?" panel shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub permission: u32,
    pub verdict: Verdict,
    /// Index into `ChannelAcl::rules` of the rule that decided it; `None` for the default rights.
    pub rule: Option<usize>,
    /// The earlier rule it overrode.
    pub overrides: Option<usize>,
    pub reason: Option<MatchReason>,
    pub condition: Option<Condition>,
    /// Allowed because "Edit permissions" brings it with it.
    pub via_write: bool,
    /// The SuperUser ignores all rules.
    pub superuser: bool,
}

/// Every permission the grid shows, in its order (FR-10.40). Root-only ones only for Root.
pub fn grid_permissions(root: bool) -> Vec<u32> {
    let mut all = vec![
        p::ENTER,
        p::TRAVERSE,
        p::SPEAK,
        p::WHISPER,
        p::TEXT_MESSAGE,
        p::LISTEN,
        p::MUTE_DEAFEN,
        p::MOVE,
        p::WRITE,
        p::MAKE_CHANNEL,
        p::MAKE_TEMP_CHANNEL,
        p::LINK_CHANNEL,
    ];
    if root {
        all.extend([
            p::KICK,
            p::BAN,
            p::REGISTER,
            p::SELF_REGISTER,
            p::RESET_USER_CONTENT,
        ]);
    }
    all
}

enum Match {
    Yes(MatchReason),
    No,
    Depends(Condition),
}

/// Whether `rule` applies to `subject` in the open channel.
fn matches(rule: &AclRule, subject: &Subject, groups: &[AclGroup]) -> Match {
    if let Some(user_id) = rule.user_id {
        return match subject {
            Subject::Person(person) if person.user_id == Some(user_id) => {
                Match::Yes(MatchReason::ThisPerson)
            }
            _ => Match::No,
        };
    }
    let Some(group) = rule.group.as_deref() else {
        return Match::No;
    };
    let (invert, name) = match group.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, group),
    };
    // `~` names the group in the channel being checked, which is the open channel here.
    let name = name.strip_prefix('~').unwrap_or(name);
    let result = match_group(name, rule.inherited, subject, groups);
    match (invert, result) {
        (false, result) => result,
        (true, Match::Yes(_)) => Match::No,
        (true, Match::No) => Match::Yes(MatchReason::Everyone),
        (true, depends) => depends,
    }
}

fn match_group(name: &str, inherited: bool, subject: &Subject, groups: &[AclGroup]) -> Match {
    let person = match subject {
        Subject::Person(person) => Some(person),
        _ => None,
    };
    match name {
        "all" => Match::Yes(MatchReason::Everyone),
        "none" => Match::No,
        "auth" => match subject {
            Subject::Everyone => Match::No,
            // Group members are registered users.
            Subject::Group(_) => Match::Yes(MatchReason::Registered),
            Subject::Person(person) => {
                if person.user_id.is_some() {
                    Match::Yes(MatchReason::Registered)
                } else {
                    Match::No
                }
            }
        },
        "in" | "out" => {
            // An inherited rule's "in" means its own channel, which the answer does not name.
            let known = person.and_then(|p| p.in_channel).filter(|_| !inherited);
            match known {
                Some(inside) if inside == (name == "in") => Match::Yes(if inside {
                    MatchReason::InChannel
                } else {
                    MatchReason::NotInChannel
                }),
                Some(_) => Match::No,
                None => Match::Depends(Condition::Presence),
            }
        }
        _ if name.starts_with('#') => Match::Depends(Condition::Password(name[1..].to_owned())),
        _ if name.starts_with('$') || name.starts_with("sub") || name == "strong" => {
            Match::Depends(Condition::ServerGroup(name.to_owned()))
        }
        _ => match subject {
            Subject::Everyone => Match::No,
            Subject::Group(own) => {
                if own == name {
                    Match::Yes(MatchReason::Member(name.to_owned()))
                } else {
                    Match::No
                }
            }
            Subject::Person(person) => match person.user_id {
                Some(id) if is_member(groups, name, id) => {
                    Match::Yes(MatchReason::Member(name.to_owned()))
                }
                _ => Match::No,
            },
        },
    }
}

/// Membership as this channel sees it: inherited members unless removed here, plus those added.
fn is_member(groups: &[AclGroup], name: &str, user_id: u32) -> bool {
    groups.iter().any(|g| {
        g.name == name
            && (g.add.contains(&user_id)
                || (g.inherit
                    && g.inherited_members.contains(&user_id)
                    && !g.remove.contains(&user_id)))
    })
}

/// Works out every grid permission for one row (FR-10.45, FR-10.46, FR-10.47).
pub fn evaluate(acl: &ChannelAcl, subject: &Subject, root: bool) -> Vec<Cell> {
    let permissions = grid_permissions(root);
    if let Subject::Person(person) = subject
        && person.superuser
    {
        return permissions
            .into_iter()
            .map(|permission| Cell {
                permission,
                verdict: Verdict::Allowed,
                rule: None,
                overrides: None,
                reason: None,
                condition: None,
                via_write: false,
                superuser: true,
            })
            .collect();
    }
    let mut cells: Vec<Cell> = permissions
        .iter()
        .map(|&permission| Cell {
            permission,
            verdict: if DEFAULT & permission != 0 {
                Verdict::Allowed
            } else {
                Verdict::Denied
            },
            rule: None,
            overrides: None,
            reason: None,
            condition: None,
            via_write: false,
            superuser: false,
        })
        .collect();
    for (index, rule) in acl.rules.iter().enumerate() {
        // The server sends inherited rules only where they reach this channel.
        if !rule.inherited && !rule.apply_here {
            continue;
        }
        if !acl.inherit_acls && rule.inherited {
            continue;
        }
        let outcome = matches(rule, subject, &acl.groups);
        for cell in &mut cells {
            let touched = (rule.grant | rule.deny) & cell.permission != 0;
            if !touched {
                continue;
            }
            let verdict = if rule.deny & cell.permission != 0 {
                Verdict::Denied
            } else {
                Verdict::Allowed
            };
            match &outcome {
                Match::Yes(reason) => {
                    cell.overrides = cell.rule;
                    cell.verdict = verdict;
                    cell.rule = Some(index);
                    cell.reason = Some(reason.clone());
                    cell.condition = None;
                }
                Match::Depends(condition)
                    if verdict != cell.verdict || cell.condition.is_some() =>
                {
                    cell.overrides = cell.rule;
                    cell.verdict = Verdict::Depends;
                    cell.rule = Some(index);
                    cell.reason = None;
                    cell.condition = Some(condition.clone());
                }
                _ => {}
            }
        }
    }
    let write = cells
        .iter()
        .find(|c| c.permission == p::WRITE)
        .is_some_and(|c| c.verdict == Verdict::Allowed);
    if write {
        let implied = WRITE_IMPLIES | if root { WRITE_IMPLIES_ROOT } else { 0 };
        for cell in &mut cells {
            if implied & cell.permission != 0 && cell.verdict != Verdict::Allowed {
                cell.verdict = Verdict::Allowed;
                cell.via_write = true;
                cell.condition = None;
            }
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn rule(group: &str, grant: u32, deny: u32, inherited: bool) -> AclRule {
        AclRule {
            inherited,
            apply_here: true,
            apply_subs: true,
            user_id: None,
            group: Some(group.to_owned()),
            grant,
            deny,
        }
    }

    fn acl(rules: Vec<AclRule>, groups: Vec<AclGroup>) -> ChannelAcl {
        ChannelAcl {
            channel_id: 5,
            inherit_acls: true,
            groups,
            rules,
            user_names: BTreeMap::new(),
        }
    }

    fn cell(cells: &[Cell], permission: u32) -> &Cell {
        cells
            .iter()
            .find(|c| c.permission == permission)
            .expect("cell")
    }

    fn mods(members: Vec<u32>) -> AclGroup {
        AclGroup {
            name: "mods".into(),
            inherited: false,
            inherit: true,
            inheritable: true,
            add: members,
            remove: Vec::new(),
            inherited_members: Vec::new(),
        }
    }

    #[test]
    fn defaults_apply_without_rules() {
        let cells = evaluate(&acl(vec![], vec![]), &Subject::Everyone, false);
        assert_eq!(cell(&cells, p::SPEAK).verdict, Verdict::Allowed);
        assert_eq!(cell(&cells, p::MOVE).verdict, Verdict::Denied);
        assert_eq!(cell(&cells, p::SPEAK).rule, None);
    }

    #[test]
    fn the_last_matching_rule_wins_and_names_what_it_overrode() {
        let rules = vec![
            rule("all", p::SPEAK, 0, true),
            rule("out", 0, p::SPEAK, false),
        ];
        let freya = Subject::Person(Person {
            user_id: Some(7),
            in_channel: Some(false),
            superuser: false,
        });
        let cells = evaluate(&acl(rules, vec![]), &freya, false);
        let speak = cell(&cells, p::SPEAK);
        assert_eq!(speak.verdict, Verdict::Denied);
        assert_eq!(speak.rule, Some(1));
        assert_eq!(speak.overrides, Some(0));
        assert_eq!(speak.reason, Some(MatchReason::NotInChannel));
    }

    #[test]
    fn group_rows_and_members_match_their_group() {
        let rules = vec![rule("mods", p::MOVE, 0, false)];
        let groups = vec![mods(vec![7])];
        let channel = acl(rules, groups);
        let row = evaluate(&channel, &Subject::Group("mods".into()), false);
        assert_eq!(cell(&row, p::MOVE).verdict, Verdict::Allowed);
        let member = Subject::Person(Person {
            user_id: Some(7),
            ..Person::default()
        });
        assert_eq!(
            cell(&evaluate(&channel, &member, false), p::MOVE).verdict,
            Verdict::Allowed
        );
        let other = Subject::Person(Person {
            user_id: Some(8),
            ..Person::default()
        });
        assert_eq!(
            cell(&evaluate(&channel, &other, false), p::MOVE).verdict,
            Verdict::Denied
        );
        assert_eq!(
            cell(&evaluate(&channel, &Subject::Everyone, false), p::MOVE).verdict,
            Verdict::Denied
        );
    }

    #[test]
    fn a_password_group_makes_the_cell_depend() {
        let rules = vec![
            rule("all", 0, p::ENTER, false),
            rule("#secret", p::ENTER, 0, false),
        ];
        let cells = evaluate(&acl(rules, vec![]), &Subject::Everyone, false);
        let enter = cell(&cells, p::ENTER);
        assert_eq!(enter.verdict, Verdict::Depends);
        assert_eq!(enter.condition, Some(Condition::Password("secret".into())));
        assert_eq!(enter.overrides, Some(0));
    }

    #[test]
    fn edit_permissions_brings_other_rights_and_root_ones_only_in_root() {
        let rules = vec![rule("admin", p::WRITE, 0, true)];
        let channel = acl(rules, vec![]);
        let admin = Subject::Group("admin".into());
        let here = evaluate(&channel, &admin, false);
        assert_eq!(cell(&here, p::MOVE).verdict, Verdict::Allowed);
        assert!(cell(&here, p::MOVE).via_write);
        assert!(here.iter().all(|c| c.permission != p::KICK));
        let root = evaluate(&channel, &admin, true);
        assert_eq!(cell(&root, p::KICK).verdict, Verdict::Allowed);
    }

    #[test]
    fn negated_groups_and_the_superuser() {
        let rules = vec![rule("!auth", 0, p::SPEAK, false)];
        let channel = acl(rules, vec![]);
        assert_eq!(
            cell(&evaluate(&channel, &Subject::Everyone, false), p::SPEAK).verdict,
            Verdict::Denied
        );
        let registered = Subject::Person(Person {
            user_id: Some(3),
            ..Person::default()
        });
        assert_eq!(
            cell(&evaluate(&channel, &registered, false), p::SPEAK).verdict,
            Verdict::Allowed
        );
        let superuser = Subject::Person(Person {
            user_id: Some(0),
            superuser: true,
            ..Person::default()
        });
        let cells = evaluate(&channel, &superuser, false);
        assert!(
            cells
                .iter()
                .all(|c| c.verdict == Verdict::Allowed && c.superuser)
        );
    }

    #[test]
    fn in_and_out_depend_when_unknown_or_inherited() {
        let rules = vec![rule("in", p::MOVE, 0, true)];
        let inside = Subject::Person(Person {
            user_id: Some(3),
            in_channel: Some(true),
            superuser: false,
        });
        let cells = evaluate(&acl(rules, vec![]), &inside, false);
        assert_eq!(cell(&cells, p::MOVE).condition, Some(Condition::Presence));
    }
}

/// A row the UI asks about; people are resolved from the session (FR-10.40, FR-10.49).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowSubject {
    Everyone,
    Group(String),
    /// Someone online.
    Session(u32),
    /// A registered user, online or not.
    UserId(u32),
}

impl crate::session::Engine {
    /// The grid rows for the editor's (possibly unsaved) rules (FR-10.40, FR-10.45).
    pub fn channel_acl_evaluate(
        &self,
        server_id: crate::ServerId,
        acl: &ChannelAcl,
        rows: &[RowSubject],
    ) -> crate::session::EngineResult<Vec<Vec<Cell>>> {
        let ctx = self.session_ctx_for(server_id)?;
        let root = acl.channel_id == 0;
        Ok(ctx.handle.with_state(|state| {
            let person_of = |user: &mumcord_protocol::User| Person {
                user_id: user.user_id,
                in_channel: Some(user.channel_id == acl.channel_id),
                superuser: user.user_id == Some(0),
            };
            rows.iter()
                .map(|row| {
                    let subject = match row {
                        RowSubject::Everyone => Subject::Everyone,
                        RowSubject::Group(name) => Subject::Group(name.clone()),
                        RowSubject::Session(session) => Subject::Person(
                            state.users.get(session).map(person_of).unwrap_or_default(),
                        ),
                        RowSubject::UserId(id) => Subject::Person(
                            state
                                .users
                                .values()
                                .find(|u| u.user_id == Some(*id))
                                .map(person_of)
                                .unwrap_or(Person {
                                    user_id: Some(*id),
                                    in_channel: None,
                                    superuser: *id == 0,
                                }),
                        ),
                    };
                    evaluate(acl, &subject, root)
                })
                .collect()
        }))
    }

    /// The rights the user holds here now and would lose with these rules (FR-10.41), as
    /// permission bits. The standard server gives a registered user back "Edit permissions" on the
    /// channel they edit, so that one is never lost by them. Empty for the SuperUser.
    pub fn channel_acl_lockout(
        &self,
        server_id: crate::ServerId,
        acl: &ChannelAcl,
    ) -> crate::session::EngineResult<Vec<u32>> {
        let ctx = self.session_ctx_for(server_id)?;
        let root = acl.channel_id == 0;
        Ok(ctx.handle.with_state(|state| {
            let Some(me) = state.session.and_then(|s| state.users.get(&s)) else {
                return Vec::new();
            };
            if me.user_id == Some(0) {
                return Vec::new();
            }
            let held = state.permissions.get(&acl.channel_id).copied().unwrap_or(0);
            let person = Person {
                user_id: me.user_id,
                in_channel: Some(me.channel_id == acl.channel_id),
                superuser: false,
            };
            evaluate(acl, &Subject::Person(person), root)
                .into_iter()
                .filter(|cell| held & cell.permission != 0 && cell.verdict == Verdict::Denied)
                .filter(|cell| !(cell.permission == p::WRITE && me.user_id.is_some()))
                .map(|cell| cell.permission)
                .collect()
        }))
    }
}
