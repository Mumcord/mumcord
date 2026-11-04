//! Plugin activities in the media area (FR-9.44 … FR-9.52), free of I/O.
//!
//! [`Stages`] knows the activities in the user's current voice channel on the server plugins
//! see: the ones its plugins started or joined, the ones other hosts announced (only for plugins
//! the user has, from the same source) and, for official plugins the user lacks, Get-plugin
//! notices. Every operation returns [`Effect`]s for the caller to carry out: host frames to send,
//! events for plugins and stage frames to close. Time and randomness stay with the caller.
//!
//! Rules: one activity per plugin and at most [`MAX_PER_CHANNEL`] per channel; at most
//! `max_participants` (≤ 16) people per activity; an activity ends when its last participant
//! leaves or its starter ends it. A channel change, server switch or disconnect leaves every
//! activity (FR-9.51); everything known about the old channel is dropped.

pub mod announce;

use std::collections::{BTreeMap, BTreeSet};

use mumcord_store::ServerId;
use serde::Serialize;

use crate::manifest::Visibility;
use crate::messaging::announce::SourceTag;
use crate::messaging::{Member, PluginRef};
pub use announce::{Announced, Frame, MAX_PARTICIPANTS};

/// Plugin activities that may run in one channel (FR-9.46).
pub const MAX_PER_CHANNEL: usize = 3;

/// Where the user is: the voice channel on the server plugins see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub server: ServerId,
    pub channel: u32,
}

/// The user's channel right now. `members` are the others in it (blocked users already
/// removed), with what their last announce listed; `capable` are the sessions whose hello lists
/// `plugins`.
#[derive(Debug, Clone, Copy)]
pub struct Room<'a> {
    pub place: Place,
    pub me: u32,
    pub members: &'a [Member<'a>],
    pub capable: &'a BTreeSet<u32>,
}

impl Room<'_> {
    fn member(&self, session: u32) -> Option<&Member<'_>> {
        self.members.iter().find(|m| m.session == session)
    }

    /// Members who see `plugin`'s activities: same id and source, visibility allows them.
    fn audience(&self, plugin: &PluginRef<'_>) -> Vec<u32> {
        self.members
            .iter()
            .filter(|m| plugin.reaches(m))
            .map(|m| m.session)
            .collect()
    }
}

/// A local plugin that may run activities: switched on, holding `ui.stage`.
#[derive(Debug, Clone, Copy)]
pub struct StagePlugin<'a> {
    pub plugin: PluginRef<'a>,
    pub name: &'a str,
    /// The manifest's `stage.max_participants`, at most 16.
    pub max_participants: u32,
}

/// A stage event for one plugin; sessions become peers with the plugin's own key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageEvent {
    Started { starter: u32, activity: String },
    Ended(String),
    Joined(u32),
    Left(u32),
}

/// Something the caller carries out, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// A host frame on `mumcord.p.<plugin>`.
    Send {
        server: ServerId,
        plugin: String,
        receivers: Vec<u32>,
        frame: Frame,
    },
    Deliver {
        plugin: String,
        event: StageEvent,
    },
    /// The user joined an activity with web content: open its stage frame on `entry`.
    ShowWeb {
        plugin: String,
        entry: String,
    },
    /// The user is no longer in the plugin's activity: close its stage frame and tree.
    CloseStage {
        plugin: String,
    },
}

/// Why `start` was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StartError {
    /// Not in a voice channel on the server plugins see.
    #[error("not in a voice channel")]
    NoChannel,
    /// "Too many activities in this channel": this plugin already runs one here, or 3 run.
    #[error("too many activities in this channel")]
    TooMany,
}

/// A limit text the app shows when a plugin's own call hit it (FR-9.46).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Refusal {
    /// "Too many activities in this channel".
    TooMany,
    /// "This activity is full".
    Full,
}

/// Why joining was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum JoinError {
    /// The activity ended or is unknown, or the user is not in a voice channel.
    #[error("the activity is gone")]
    Gone,
    /// "This activity is full".
    #[error("this activity is full")]
    Full,
    /// The plugin is off or lacks `ui.stage` (or, for `stage.join`, had no recent gesture).
    #[error("not permitted")]
    NotPermitted,
}

#[derive(Debug, Clone)]
struct Activity {
    plugin: String,
    plugin_name: String,
    info: Announced,
    starter: u32,
    participants: BTreeSet<u32>,
    /// Who this host told about the activity (start or state), so newcomers get it once.
    informed: BTreeSet<u32>,
    /// Who got our Get-plugin notice (only on the starter's host).
    noticed: BTreeSet<u32>,
    official: bool,
}

#[derive(Debug, Clone)]
struct Notice {
    plugin: String,
    plugin_name: String,
    title: String,
    starter: u32,
}

/// What the app shows for one activity (activity line, stage tab and header).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActivitySnapshot {
    pub id: String,
    pub plugin: String,
    pub plugin_name: String,
    /// Plugin text; the UI renders it as plain text.
    pub title: String,
    pub starter: u32,
    /// Sessions in the activity (empty for a Get-plugin notice).
    pub participants: Vec<u32>,
    pub max: u32,
    /// The user is in it.
    pub joined: bool,
    /// The user started it.
    pub started_by_me: bool,
    /// Its content is the plugin's web page rather than a tree.
    pub web: bool,
    pub kind: ActivityKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityKind {
    /// "<name> started <plugin> · Join".
    Activity,
    /// "<name> is playing <plugin> · Get plugin" (official plugins the user lacks).
    GetPlugin,
}

/// See the module docs.
#[derive(Debug, Default)]
pub struct Stages {
    place: Option<Place>,
    /// The user's session at `place`.
    me: Option<u32>,
    activities: BTreeMap<String, Activity>,
    notices: BTreeMap<String, Notice>,
    changed: bool,
}

impl Stages {
    /// Whether anything the app shows changed since the last call.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// The user is now at `place` as session `me` (`None`: no channel, or not connected). A new
    /// place or session leaves
    /// every activity there: participating plugins get `stage-left(me)`, every plugin that knew
    /// an activity gets `stage-activity-ended`. With `tell_old`, the old channel's members get
    /// `leave` frames (a server switch; after a channel change or disconnect they see the user
    /// leave anyway).
    pub fn moved(&mut self, place: Option<Place>, me: Option<u32>, tell_old: bool) -> Vec<Effect> {
        if self.place == place && self.me == me {
            return Vec::new();
        }
        let old = std::mem::replace(&mut self.place, place);
        let me = std::mem::replace(&mut self.me, me);
        let mut effects = Vec::new();
        for (id, activity) in std::mem::take(&mut self.activities) {
            let joined = me.is_some_and(|me| activity.participants.contains(&me));
            if let (Some(old), Some(me), true) = (old, me, tell_old) {
                let receivers = receivers(&[], &activity, me);
                send_goodbye(
                    &mut effects,
                    old.server,
                    &id,
                    &activity,
                    me,
                    joined,
                    receivers,
                );
            }
            if let (Some(me), true) = (me, joined) {
                effects.push(Effect::CloseStage {
                    plugin: activity.plugin.clone(),
                });
                deliver(&mut effects, &activity.plugin, StageEvent::Left(me));
            }
            deliver(&mut effects, &activity.plugin, StageEvent::Ended(id));
        }
        self.notices.clear();
        self.changed = true;
        effects
    }

    fn here(&self, room: &Room<'_>) -> bool {
        self.place == Some(room.place) && self.me == Some(room.me)
    }

    /// The plugin an activity or notice belongs to.
    pub fn plugin_of(&self, id: &str) -> Option<&str> {
        self.activities
            .get(id)
            .map(|a| a.plugin.as_str())
            .or_else(|| self.notices.get(id).map(|n| n.plugin.as_str()))
    }

    fn count(&self) -> usize {
        self.activities.len() + self.notices.len()
    }

    /// Whether `plugin` may start an activity in `room` now.
    pub fn can_start(&self, room: Option<&Room<'_>>, plugin: &str) -> Result<(), StartError> {
        if !room.is_some_and(|r| self.here(r)) {
            return Err(StartError::NoChannel);
        }
        if self.activities.values().any(|a| a.plugin == plugin) || self.count() >= MAX_PER_CHANNEL {
            return Err(StartError::TooMany);
        }
        Ok(())
    }

    /// Starts activity `id` of `local` with the user as its only participant and announces it.
    /// The caller checked the permission, gesture, title and content, and opened web content.
    pub fn start(
        &mut self,
        room: &Room<'_>,
        local: &StagePlugin<'_>,
        id: String,
        title: String,
        web: Option<String>,
    ) -> Result<Vec<Effect>, StartError> {
        self.can_start(Some(room), local.plugin.id)?;
        let info = Announced {
            title,
            max: local.max_participants.clamp(1, MAX_PARTICIPANTS),
            web,
        };
        let audience = room.audience(&local.plugin);
        let official = local.plugin.source == SourceTag::Official;
        let noticed: BTreeSet<u32> = if official {
            room.members
                .iter()
                .filter(|m| needs_notice(room, &local.plugin, m))
                .map(|m| m.session)
                .collect()
        } else {
            BTreeSet::new()
        };
        let mut effects = Vec::new();
        send(
            &mut effects,
            room.place.server,
            local.plugin.id,
            audience.clone(),
            Frame::Start {
                id: id.clone(),
                info: info.clone(),
            },
        );
        send(
            &mut effects,
            room.place.server,
            local.plugin.id,
            noticed.iter().copied().collect(),
            Frame::Notice {
                id: id.clone(),
                title: info.title.clone(),
                name: local.name.to_owned(),
            },
        );
        deliver(&mut effects, local.plugin.id, StageEvent::Joined(room.me));
        self.activities.insert(
            id,
            Activity {
                plugin: local.plugin.id.to_owned(),
                plugin_name: local.name.to_owned(),
                info,
                starter: room.me,
                participants: BTreeSet::from([room.me]),
                informed: audience.into_iter().collect(),
                noticed,
                official,
            },
        );
        self.changed = true;
        Ok(effects)
    }

    /// The user joins activity `id` of `local` (their Join click, or `stage.join`).
    pub fn join(
        &mut self,
        room: Option<&Room<'_>>,
        local: &StagePlugin<'_>,
        id: &str,
    ) -> Result<Vec<Effect>, JoinError> {
        let room = room.filter(|r| self.here(r)).ok_or(JoinError::Gone)?;
        let activity = self
            .activities
            .get_mut(id)
            .filter(|a| a.plugin == local.plugin.id)
            .ok_or(JoinError::Gone)?;
        if activity.participants.contains(&room.me) {
            return Ok(Vec::new());
        }
        if activity.participants.len() >= activity.info.max as usize {
            return Err(JoinError::Full);
        }
        activity.participants.insert(room.me);
        let mut effects = Vec::new();
        send(
            &mut effects,
            room.place.server,
            local.plugin.id,
            room.audience(&local.plugin),
            Frame::Join { id: id.to_owned() },
        );
        if let Some(entry) = &activity.info.web {
            effects.push(Effect::ShowWeb {
                plugin: local.plugin.id.to_owned(),
                entry: entry.clone(),
            });
        }
        deliver(&mut effects, local.plugin.id, StageEvent::Joined(room.me));
        self.changed = true;
        Ok(effects)
    }

    /// The activity of `plugin` the user is in, if any.
    pub fn current(&self, plugin: &str, me: u32) -> Option<&str> {
        self.activities
            .iter()
            .find(|(_, a)| a.plugin == plugin && a.participants.contains(&me))
            .map(|(id, _)| id.as_str())
    }

    /// Whether the user is in an activity of `plugin` that shows a tree (for `stage.set-tree`).
    pub fn shows_tree(&self, plugin: &str, me: u32) -> bool {
        self.current(plugin, me)
            .and_then(|id| self.activities.get(id))
            .is_some_and(|a| a.info.web.is_none())
    }

    /// The user leaves the activity of `plugin`; it keeps running for the others. The news goes
    /// to everyone who sees it.
    pub fn leave(&mut self, room: Option<&Room<'_>>, plugin: &PluginRef<'_>) -> Vec<Effect> {
        let Some(room) = room.filter(|r| self.here(r)) else {
            return Vec::new();
        };
        let Some(id) = self.current(plugin.id, room.me).map(str::to_owned) else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        if let Some(activity) = self.activities.get(&id) {
            let receivers = receivers(&room.audience(plugin), activity, room.me);
            let server = room.place.server;
            send_goodbye(
                &mut effects,
                server,
                &id,
                activity,
                room.me,
                true,
                receivers,
            );
        }
        effects.push(Effect::CloseStage {
            plugin: plugin.id.to_owned(),
        });
        deliver(&mut effects, plugin.id, StageEvent::Left(room.me));
        self.remove_participant(&id, room.me, room.me, &mut effects);
        effects
    }

    /// The starter's plugin ends its activity for everyone.
    pub fn end(&mut self, room: Option<&Room<'_>>, plugin: &PluginRef<'_>) -> Vec<Effect> {
        let audience = room.map(|r| r.audience(plugin)).unwrap_or_default();
        let plugin = plugin.id;
        let Some(room) = room.filter(|r| self.here(r)) else {
            return Vec::new();
        };
        let Some(id) = self
            .activities
            .iter()
            .find(|(_, a)| a.plugin == plugin && a.starter == room.me)
            .map(|(id, _)| id.clone())
        else {
            return Vec::new();
        };
        let Some(activity) = self.activities.remove(&id) else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        send(
            &mut effects,
            room.place.server,
            plugin,
            receivers(&audience, &activity, room.me),
            Frame::End { id: id.clone() },
        );
        send(
            &mut effects,
            room.place.server,
            plugin,
            activity.noticed.iter().copied().collect(),
            Frame::NoticeEnd { id: id.clone() },
        );
        if activity.participants.contains(&room.me) {
            effects.push(Effect::CloseStage {
                plugin: plugin.to_owned(),
            });
        }
        deliver(&mut effects, plugin, StageEvent::Ended(id));
        self.changed = true;
        effects
    }

    /// `plugin` was switched off or unloaded: its activities are left and forgotten; nothing is
    /// delivered to it any more.
    pub fn forget_plugin(
        &mut self,
        room: Option<&Room<'_>>,
        plugin: &PluginRef<'_>,
    ) -> Vec<Effect> {
        let mut effects = self.leave(room, plugin);
        effects.retain(|e| !matches!(e, Effect::Deliver { .. }));
        let before = self.activities.len();
        self.activities.retain(|_, a| a.plugin != plugin.id);
        self.changed |= self.activities.len() != before;
        effects
    }

    /// A host frame from `sender` on `plugin`'s data id. `local` is the user's copy of the plugin
    /// when it may run activities; frames for plugins the user lacks only count as notices.
    pub fn receive(
        &mut self,
        room: &Room<'_>,
        plugin: &str,
        local: Option<&StagePlugin<'_>>,
        sender: u32,
        frame: Frame,
    ) -> Vec<Effect> {
        let mut effects = Vec::new();
        if !self.here(room) || sender == room.me {
            return effects;
        }
        let Some(member) = room.member(sender) else {
            return effects;
        };
        match frame {
            Frame::Notice { id, title, name } => {
                let fresh = !self.notices.contains_key(&id) && !self.activities.contains_key(&id);
                if local.is_none() && fresh && self.count() < MAX_PER_CHANNEL {
                    self.notices.insert(
                        id,
                        Notice {
                            plugin: plugin.to_owned(),
                            plugin_name: name,
                            title,
                            starter: sender,
                        },
                    );
                    self.changed = true;
                }
            }
            Frame::NoticeEnd { id } => {
                if self
                    .notices
                    .get(&id)
                    .is_some_and(|n| n.starter == sender && n.plugin == plugin)
                {
                    self.notices.remove(&id);
                    self.changed = true;
                }
            }
            frame => {
                // Activity frames only count from someone who runs the same plugin from the same
                // source and whom the plugin's visibility allows (FR-9.47).
                if let Some(local) = local.filter(|l| l.plugin.reaches(member)) {
                    self.activity_frame(room, local, sender, frame, &mut effects);
                }
            }
        }
        effects
    }

    fn activity_frame(
        &mut self,
        room: &Room<'_>,
        local: &StagePlugin<'_>,
        sender: u32,
        frame: Frame,
        effects: &mut Vec<Effect>,
    ) {
        let plugin = local.plugin.id;
        let me = room.me;
        match frame {
            Frame::Start { id, info } => {
                self.announced(local, id, info, sender, BTreeSet::from([sender]), effects);
            }
            Frame::State {
                id,
                info,
                starter,
                participants,
            } => {
                let present: BTreeSet<u32> = participants
                    .into_iter()
                    .filter(|s| *s != me && (*s == sender || room.member(*s).is_some()))
                    .take(info.max as usize)
                    .collect();
                if present.contains(&sender) {
                    let starter = if present.contains(&starter) {
                        starter
                    } else {
                        sender
                    };
                    self.announced(local, id, info, starter, present, effects);
                }
            }
            Frame::Join { id } => {
                let Some(activity) = self.activities.get_mut(&id).filter(|a| a.plugin == plugin)
                else {
                    return;
                };
                // Two people joining the last place at once: the later news is dropped.
                if activity.participants.len() < activity.info.max as usize
                    && activity.participants.insert(sender)
                {
                    if activity.participants.contains(&me) {
                        deliver(effects, plugin, StageEvent::Joined(sender));
                    }
                    self.changed = true;
                }
            }
            Frame::Leave { id } => {
                if self
                    .activities
                    .get(&id)
                    .is_some_and(|a| a.plugin == plugin && a.participants.contains(&sender))
                {
                    self.remove_participant(&id, sender, me, effects);
                }
            }
            Frame::End { id } => {
                let ends = self
                    .activities
                    .get(&id)
                    .is_some_and(|a| a.plugin == plugin && a.starter == sender);
                if let Some(activity) = ends.then(|| self.activities.remove(&id)).flatten() {
                    if activity.participants.contains(&me) {
                        effects.push(Effect::CloseStage {
                            plugin: plugin.to_owned(),
                        });
                    }
                    deliver(effects, plugin, StageEvent::Ended(id));
                    self.changed = true;
                }
            }
            Frame::Notice { .. } | Frame::NoticeEnd { .. } => {}
        }
    }

    /// Someone else's activity became known (a start, or the state for a latecomer).
    fn announced(
        &mut self,
        local: &StagePlugin<'_>,
        id: String,
        info: Announced,
        starter: u32,
        participants: BTreeSet<u32>,
        effects: &mut Vec<Effect>,
    ) {
        let plugin = local.plugin.id;
        // A known id, a second activity of the plugin or a fourth one here is not taken on.
        if self.activities.contains_key(&id)
            || self.activities.values().any(|a| a.plugin == plugin)
            || self.count() >= MAX_PER_CHANNEL
        {
            return;
        }
        deliver(
            effects,
            plugin,
            StageEvent::Started {
                starter,
                activity: id.clone(),
            },
        );
        self.activities.insert(
            id,
            Activity {
                plugin: plugin.to_owned(),
                plugin_name: local.name.to_owned(),
                info,
                starter,
                participants,
                informed: BTreeSet::new(),
                noticed: BTreeSet::new(),
                official: local.plugin.source == SourceTag::Official,
            },
        );
        self.changed = true;
    }

    /// `session` left the activity `id`. The user's plugin hears of it when the user is still
    /// in it (the user's own leave is delivered by the caller); an activity nobody is left in
    /// ends.
    fn remove_participant(&mut self, id: &str, session: u32, me: u32, effects: &mut Vec<Effect>) {
        let Some(activity) = self.activities.get_mut(id) else {
            return;
        };
        if !activity.participants.remove(&session) {
            return;
        }
        let plugin = activity.plugin.clone();
        if session != me && activity.participants.contains(&me) {
            deliver(effects, &plugin, StageEvent::Left(session));
        }
        if activity.participants.is_empty() {
            self.activities.remove(id);
            deliver(effects, &plugin, StageEvent::Ended(id.to_owned()));
        }
        self.changed = true;
    }

    /// `session` left the channel or the server.
    pub fn peer_left(&mut self, room: &Room<'_>, session: u32) -> Vec<Effect> {
        let mut effects = Vec::new();
        if !self.here(room) || session == room.me {
            return effects;
        }
        let ids: Vec<String> = self
            .activities
            .iter()
            .filter(|(_, a)| a.participants.contains(&session))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.remove_participant(&id, session, room.me, &mut effects);
        }
        for activity in self.activities.values_mut() {
            activity.informed.remove(&session);
            activity.noticed.remove(&session);
        }
        let before = self.notices.len();
        self.notices.retain(|_, n| n.starter != session);
        self.changed |= before != self.notices.len();
        effects
    }

    /// A member's announce arrived: whoever is responsible tells them about running activities
    /// they did not hear of (the lowest session in an activity sends its state; the starter
    /// sends Get-plugin notices). `locals` are the user's plugins that may run activities.
    pub fn member_announced(
        &mut self,
        room: &Room<'_>,
        session: u32,
        locals: &[StagePlugin<'_>],
    ) -> Vec<Effect> {
        let mut effects = Vec::new();
        let Some(member) = room.member(session).filter(|_| self.here(room)) else {
            return effects;
        };
        for (id, activity) in &mut self.activities {
            let Some(local) = locals.iter().find(|l| l.plugin.id == activity.plugin) else {
                continue;
            };
            let responsible = activity.participants.first() == Some(&room.me);
            if responsible
                && local.plugin.reaches(member)
                && !activity.participants.contains(&session)
                && activity.informed.insert(session)
            {
                send(
                    &mut effects,
                    room.place.server,
                    &activity.plugin,
                    vec![session],
                    Frame::State {
                        id: id.clone(),
                        info: activity.info.clone(),
                        starter: activity.starter,
                        participants: activity.participants.iter().copied().collect(),
                    },
                );
            }
            if activity.official
                && activity.starter == room.me
                && activity.participants.contains(&room.me)
                && needs_notice(room, &local.plugin, member)
                && activity.noticed.insert(session)
            {
                send(
                    &mut effects,
                    room.place.server,
                    &activity.plugin,
                    vec![session],
                    Frame::Notice {
                        id: id.clone(),
                        title: activity.info.title.clone(),
                        name: activity.plugin_name.clone(),
                    },
                );
            }
        }
        effects
    }

    /// What the app shows: the activities and notices in the user's channel.
    pub fn snapshot(&self) -> Vec<ActivitySnapshot> {
        let me = self.me;
        let activities = self.activities.iter().map(|(id, a)| ActivitySnapshot {
            id: id.clone(),
            plugin: a.plugin.clone(),
            plugin_name: a.plugin_name.clone(),
            title: a.info.title.clone(),
            starter: a.starter,
            participants: a.participants.iter().copied().collect(),
            max: a.info.max,
            joined: me.is_some_and(|me| a.participants.contains(&me)),
            started_by_me: me == Some(a.starter),
            web: a.info.web.is_some(),
            kind: ActivityKind::Activity,
        });
        let notices = self.notices.iter().map(|(id, n)| ActivitySnapshot {
            id: id.clone(),
            plugin: n.plugin.clone(),
            plugin_name: n.plugin_name.clone(),
            title: n.title.clone(),
            starter: n.starter,
            participants: Vec::new(),
            max: 0,
            joined: false,
            started_by_me: false,
            web: false,
            kind: ActivityKind::GetPlugin,
        });
        activities.chain(notices).collect()
    }
}

/// Whether `member` should get a Get-plugin notice for official `plugin`: their hello lists
/// `plugins`, their announce lacks the plugin (from any source) and the visibility allows them.
fn needs_notice(room: &Room<'_>, plugin: &PluginRef<'_>, member: &Member<'_>) -> bool {
    let allowed = match plugin.visibility {
        Visibility::Everyone => true,
        Visibility::Friends => member.is_friend,
        Visibility::Nobody => false,
    };
    allowed
        && room.capable.contains(&member.session)
        && !member.plugins.iter().any(|e| e.id == plugin.id)
}

/// The `leave` (if the user was in it) and, from the starter, `notice-end` frames for an
/// activity the user leaves.
fn send_goodbye(
    effects: &mut Vec<Effect>,
    server: ServerId,
    id: &str,
    activity: &Activity,
    me: u32,
    joined: bool,
    receivers: Vec<u32>,
) {
    if joined {
        send(
            effects,
            server,
            &activity.plugin,
            receivers,
            Frame::Leave { id: id.to_owned() },
        );
    }
    if activity.starter == me {
        send(
            effects,
            server,
            &activity.plugin,
            activity.noticed.iter().copied().collect(),
            Frame::NoticeEnd { id: id.to_owned() },
        );
    }
}

/// Everyone who should hear about a change: the plugin's audience in the channel, whoever this
/// host told about the activity and its participants, without the user.
fn receivers(audience: &[u32], activity: &Activity, me: u32) -> Vec<u32> {
    let mut all: BTreeSet<u32> = audience.iter().copied().collect();
    all.extend(&activity.informed);
    all.extend(&activity.participants);
    all.remove(&me);
    all.into_iter().collect()
}

fn send(
    effects: &mut Vec<Effect>,
    server: ServerId,
    plugin: &str,
    receivers: Vec<u32>,
    frame: Frame,
) {
    if !receivers.is_empty() {
        effects.push(Effect::Send {
            server,
            plugin: plugin.to_owned(),
            receivers,
            frame,
        });
    }
}

fn deliver(effects: &mut Vec<Effect>, plugin: &str, event: StageEvent) {
    effects.push(Effect::Deliver {
        plugin: plugin.to_owned(),
        event,
    });
}

#[cfg(test)]
mod tests;
