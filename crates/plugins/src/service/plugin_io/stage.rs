//! The stage engine ([`crate::stage::Stages`]) wired to the service (FR-9.44 … FR-9.52).
//!
//! The room (focused server, the user's channel, its members and their announces) comes from
//! [`PeersState`]; plugins take part when they are switched on and hold `ui.stage`. Effects run
//! after the locks are released: host frames go out on the plugin lane as
//! [`ServiceEvent::PeerSend`], plugin events go straight to the plugin when the service has the
//! host at hand and otherwise round-trip as [`ServiceEvent::StageEvent`] (a plugin's own call
//! runs on its worker), and every visible change ends with [`ServiceEvent::StageChanged`].

use std::collections::{BTreeSet, HashMap};
use std::sync::{MutexGuard, PoisonError};
use std::time::Instant;

use serde::Serialize;

use super::{PeerInfo, PeersState, PluginMeta, ServiceIo, UiUpdate, wit_peer};
use crate::api::io::{PluginIo, StageContent};
use crate::api::v1_1::Up;
use crate::api::webview::WebviewPlace;
use crate::manifest::Visibility;
use crate::messaging::announce::SourceTag;
use crate::messaging::envelope::{self, HOST_KIND_STAGE};
use crate::messaging::{Member, Outgoing, PluginRef};
use crate::runtime::PluginEvent;
use crate::service::{PluginService, ServiceEvent};
use crate::stage::{
    ActivitySnapshot, Effect, JoinError, Place, Refusal, Room, StageEvent, StagePlugin, Stages,
    StartError, announce,
};
use crate::wit::types::Error;
use crate::wit::ui::Node;

/// One activity (or Get-plugin notice) as the app shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StageActivity {
    #[serde(flatten)]
    pub activity: ActivitySnapshot,
    /// The starter's name (untrusted server text); empty if they are gone.
    pub starter_name: String,
}

/// The stage's view of the peers state while both locks are held.
struct StageCtx<'a> {
    room: Option<Room<'a>>,
    plugins: &'a HashMap<String, PluginMeta>,
}

impl StageCtx<'_> {
    fn room(&self) -> Option<&Room<'_>> {
        self.room.as_ref()
    }

    /// How `plugin`'s activities reach others; `None` if it is not announced.
    fn plugin_ref(&self, plugin: &str) -> Option<PluginRef<'_>> {
        let (id, meta) = self.plugins.get_key_value(plugin)?;
        let messaging = meta.messaging.as_ref()?;
        Some(PluginRef {
            id,
            source: messaging.source,
            visibility: messaging.visibility,
        })
    }

    /// `plugin` if it may run activities (switched on, announced, holding `ui.stage`).
    fn local(&self, plugin: &str) -> Option<StagePlugin<'_>> {
        let meta = self.plugins.get(plugin)?;
        Some(StagePlugin {
            plugin: self.plugin_ref(plugin)?,
            name: &meta.name,
            max_participants: meta.stage?,
        })
    }

    fn locals(&self) -> Vec<StagePlugin<'_>> {
        self.plugins
            .keys()
            .filter_map(|id| self.local(id))
            .collect()
    }
}

/// A gone plugin's activities are still left; nobody new sees them (visibility Nobody).
fn gone_plugin(id: &str) -> PluginRef<'_> {
    PluginRef {
        id,
        source: SourceTag::Dev,
        visibility: Visibility::Nobody,
    }
}

/// The user's voice channel and session on the focused server.
fn place(peers: &PeersState) -> Option<(Place, u32)> {
    let server = peers.focused?;
    let me = peers.connections.get(&server)?.me.as_ref()?;
    let (channel, _) = me.channel.as_ref()?;
    Some((
        Place {
            server,
            channel: *channel,
        },
        me.me.session,
    ))
}

/// Someone on the focused server as the stage knows them: the user or a channel member.
fn find_peer(peers: &PeersState, session: u32) -> Option<PeerInfo> {
    let connection = peers.connections.get(&peers.focused?)?;
    match connection.me.as_ref().filter(|me| me.me.session == session) {
        Some(me) => Some(me.me.clone()),
        None => connection.members.get(&session).cloned(),
    }
}

impl ServiceIo {
    fn stages(&self) -> MutexGuard<'_, Stages> {
        self.stages.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `f` on the stages with the current room (lock order: peers, then stages).
    fn with_stage<R>(&self, f: impl FnOnce(&mut Stages, &StageCtx<'_>) -> R) -> R {
        let peers = self.peers();
        let mut stages = self.stages();
        let connection = peers.focused.and_then(|s| peers.connections.get(&s));
        let members: Vec<Member<'_>> = connection.map_or_else(Vec::new, |c| {
            c.members
                .values()
                .map(|peer| Member {
                    session: peer.session,
                    is_friend: peer.is_friend,
                    plugins: c.announced.get(&peer.session).map_or(&[], Vec::as_slice),
                })
                .collect()
        });
        let no_one = BTreeSet::new();
        let ctx = StageCtx {
            room: place(&peers).map(|(place, me)| Room {
                place,
                me,
                members: &members,
                capable: connection.map_or(&no_one, |c| &c.capable),
            }),
            plugins: &peers.plugins,
        };
        f(&mut stages, &ctx)
    }

    /// Carries out stage effects; `deliver` hands a plugin its event.
    pub(in crate::service) fn run_stage(
        &self,
        effects: Vec<Effect>,
        deliver: &dyn Fn(&str, PluginEvent),
    ) {
        for effect in effects {
            match effect {
                Effect::Send {
                    server,
                    plugin,
                    receivers,
                    frame,
                } => match announce::encode(&frame) {
                    Some(bytes) => self.emit(ServiceEvent::PeerSend {
                        server,
                        outgoing: Outgoing {
                            data_id: envelope::data_id(&plugin),
                            receivers,
                            bytes,
                            direct: false,
                        },
                        plugin,
                    }),
                    None => tracing::warn!(plugin, "stage frame could not be encoded"),
                },
                Effect::Deliver { plugin, event } => {
                    let event = self.stage_event(&plugin, event);
                    deliver(&plugin, event);
                }
                Effect::ShowWeb { plugin, entry } => {
                    if let Err(error) = self.stage_web_show(&plugin, entry) {
                        tracing::info!(plugin, ?error, "stage web content not opened");
                    }
                }
                Effect::CloseStage { plugin } => {
                    if self.webviews.stage_closed(&plugin) {
                        let place = WebviewPlace::Stage;
                        self.ui(&plugin, UiUpdate::WebviewHide { place });
                    }
                    self.ui(&plugin, UiUpdate::StageTree { nodes: Vec::new() });
                }
            }
        }
        if self.stages().take_changed() {
            self.emit(ServiceEvent::StageChanged);
        }
    }

    /// Effects of a plugin's own call: its events come back through the app.
    fn run_stage_from_plugin(&self, effects: Vec<Effect>) {
        self.run_stage(effects, &|plugin, event| {
            self.emit(ServiceEvent::StageEvent {
                plugin: plugin.to_owned(),
                event,
            });
        });
    }

    /// A stage event as `plugin` sees it, with its own peer keys.
    fn stage_event(&self, plugin: &str, event: StageEvent) -> PluginEvent {
        let peer = |session| {
            let info = find_peer(&self.peers(), session).unwrap_or(PeerInfo {
                session,
                name: String::new(),
                cert_hash: None,
                is_friend: false,
            });
            wit_peer(self.keys(), plugin, &info).up()
        };
        match event {
            StageEvent::Started { starter, activity } => {
                PluginEvent::StageActivityStarted((peer(starter), activity))
            }
            StageEvent::Ended(activity) => PluginEvent::StageActivityEnded(activity),
            StageEvent::Joined(session) => PluginEvent::StageJoined(peer(session)),
            StageEvent::Left(session) => PluginEvent::StageLeft(peer(session)),
        }
    }

    /// `stage.start`: gesture → channel and limits → content.
    pub(super) fn start_activity(
        &self,
        plugin: &str,
        title: String,
        content: StageContent,
    ) -> Result<String, Error> {
        if !self.chat().gate.recent(plugin, Instant::now()) {
            return Err(Error::NotPermitted);
        }
        let id = format!("{:016x}", rand::random::<u64>());
        let tree = match &content {
            StageContent::Tree(nodes) => Some(nodes.clone()),
            StageContent::Web(_) => None,
        };
        let started = self.with_stage(|stages, ctx| {
            let local = ctx.local(plugin).ok_or(Error::NotPermitted)?;
            match stages.can_start(ctx.room(), plugin) {
                Ok(()) => {}
                Err(StartError::NoChannel) => return Err(Error::NotPermitted),
                Err(StartError::TooMany) => return Err(Error::QuotaExceeded),
            }
            let room = ctx.room().ok_or(Error::NotPermitted)?;
            let web = match content {
                StageContent::Web(entry) => {
                    self.stage_web_show(plugin, entry.clone())?;
                    Some(entry)
                }
                StageContent::Tree(_) => None,
            };
            stages
                .start(room, &local, id.clone(), title, web)
                .map_err(|_| Error::QuotaExceeded)
        });
        let effects = match started {
            Ok(effects) => effects,
            Err(error) => {
                if matches!(error, Error::QuotaExceeded) {
                    self.refused(plugin, Refusal::TooMany);
                }
                return Err(error);
            }
        };
        if let Some(nodes) = tree {
            self.ui(plugin, UiUpdate::StageTree { nodes });
        }
        self.run_stage_from_plugin(effects);
        Ok(id)
    }

    /// `stage.join` from the plugin's own button: needs a fresh gesture (FR-9.45).
    pub(super) fn join_activity_from_plugin(&self, plugin: &str, id: &str) -> Result<(), Error> {
        if !self.chat().gate.recent(plugin, Instant::now()) {
            return Err(Error::NotPermitted);
        }
        match self.join_activity(plugin, id) {
            Ok(effects) => {
                self.run_stage_from_plugin(effects);
                Ok(())
            }
            Err(JoinError::Full) => {
                self.refused(plugin, Refusal::Full);
                Err(Error::QuotaExceeded)
            }
            Err(JoinError::Gone) => Err(Error::Invalid),
            Err(JoinError::NotPermitted) => Err(Error::NotPermitted),
        }
    }

    fn join_activity(&self, plugin: &str, id: &str) -> Result<Vec<Effect>, JoinError> {
        self.with_stage(|stages, ctx| {
            let local = ctx.local(plugin).ok_or(JoinError::NotPermitted)?;
            stages.join(ctx.room(), &local, id)
        })
    }

    fn leave_effects(&self, plugin: &str) -> Vec<Effect> {
        self.with_stage(|stages, ctx| {
            let plugin = ctx
                .plugin_ref(plugin)
                .unwrap_or_else(|| gone_plugin(plugin));
            stages.leave(ctx.room(), &plugin)
        })
    }

    /// `stage.leave`.
    pub(super) fn leave_activity(&self, plugin: &str) {
        let effects = self.leave_effects(plugin);
        self.run_stage_from_plugin(effects);
    }

    /// `stage.end`.
    pub(super) fn end_activity(&self, plugin: &str) {
        let effects = self.with_stage(|stages, ctx| {
            let plugin = ctx
                .plugin_ref(plugin)
                .unwrap_or_else(|| gone_plugin(plugin));
            stages.end(ctx.room(), &plugin)
        });
        self.run_stage_from_plugin(effects);
    }

    /// `stage.set-tree`: only while the user is in the plugin's tree activity.
    pub(super) fn set_stage_tree(&self, plugin: &str, nodes: Vec<Node>) -> Result<(), Error> {
        let shows = self.with_stage(|stages, ctx| {
            ctx.room()
                .is_some_and(|room| stages.shows_tree(plugin, room.me))
        });
        if !shows {
            return Err(Error::Invalid);
        }
        self.ui(plugin, UiUpdate::StageTree { nodes });
        Ok(())
    }

    /// `plugin` is switched off or unloaded: its activities are left, without events.
    pub(super) fn forget_stage(&self, plugin: &str) {
        let effects = self.with_stage(|stages, ctx| {
            let plugin = ctx
                .plugin_ref(plugin)
                .unwrap_or_else(|| gone_plugin(plugin));
            stages.forget_plugin(ctx.room(), &plugin)
        });
        self.run_stage(effects, &|_, _| {});
    }

    fn refused(&self, plugin: &str, reason: Refusal) {
        self.emit(ServiceEvent::StageRefused {
            plugin: plugin.to_owned(),
            reason,
        });
    }
}

impl PluginService {
    fn run_stage(&self, effects: Vec<Effect>) {
        self.io.run_stage(effects, &|plugin, event| {
            self.host.send_stage(plugin, event);
        });
    }

    /// The activities and Get-plugin notices in the user's channel on the focused server (the
    /// activity lines, stage tabs and headers). Refetch on [`ServiceEvent::StageChanged`].
    pub fn stage_activities(&self) -> Vec<StageActivity> {
        let snapshot = self.io.stages().snapshot();
        let peers = self.io.peers();
        snapshot
            .into_iter()
            .map(|activity| StageActivity {
                starter_name: find_peer(&peers, activity.starter)
                    .map(|p| p.name)
                    .unwrap_or_default(),
                activity,
            })
            .collect()
    }

    /// The user clicked Join on an activity line or stage (FR-9.45): Mumcord joins it and the
    /// plugin gets `stage-joined` for the user. `Full` shows "This activity is full".
    pub fn stage_join(&self, activity: &str) -> Result<(), JoinError> {
        let plugin = self
            .io
            .stages()
            .plugin_of(activity)
            .map(str::to_owned)
            .ok_or(JoinError::Gone)?;
        let effects = self.io.join_activity(&plugin, activity)?;
        self.run_stage(effects);
        Ok(())
    }

    /// The user clicked Leave in the stage header.
    pub fn stage_leave(&self, activity: &str) {
        let plugin = self.io.stages().plugin_of(activity).map(str::to_owned);
        if let Some(plugin) = plugin {
            let effects = self.io.leave_effects(&plugin);
            self.run_stage(effects);
        }
    }

    /// Passes back an event from [`ServiceEvent::StageEvent`].
    pub fn stage_event(&self, plugin: &str, event: PluginEvent) {
        self.host.send_stage(plugin, event);
    }

    /// The user's place may have changed (focused server, channel, session or connection):
    /// leaving it leaves every activity there (FR-9.51). `tell_old` sends `leave` frames to the
    /// old channel (a server switch, where the old connection stays).
    pub(in crate::service) fn stage_sync_place(&self, tell_old: bool) {
        let (place, me) = place(&self.io.peers()).unzip();
        let effects = self.io.stages().moved(place, me, tell_old);
        self.run_stage(effects);
    }

    /// `session` is leaving the user's channel; call before forgetting the member.
    pub(in crate::service) fn stage_peer_left(&self, session: u32) {
        let effects = self.io.with_stage(|stages, ctx| {
            ctx.room()
                .map(|room| stages.peer_left(room, session))
                .unwrap_or_default()
        });
        self.run_stage(effects);
    }

    /// A member's announce arrived: they may need to hear of running activities.
    pub(in crate::service) fn stage_member_announced(&self, session: u32) {
        let effects = self.io.with_stage(|stages, ctx| {
            let locals = ctx.locals();
            ctx.room()
                .map(|room| stages.member_announced(room, session, &locals))
                .unwrap_or_default()
        });
        self.run_stage(effects);
    }

    /// Plugin data that may be a host frame. Returns true if it was one (and is handled).
    pub(in crate::service) fn stage_frame(&self, sender: u32, plugin: &str, data: &[u8]) -> bool {
        let Some((kind, body)) = envelope::decode_host(data) else {
            return false;
        };
        if kind != HOST_KIND_STAGE {
            return true;
        }
        let Some(frame) = announce::decode(body) else {
            tracing::debug!(size = data.len(), "stage frame refused");
            return true;
        };
        let effects = self.io.with_stage(|stages, ctx| {
            let local = ctx.local(plugin);
            ctx.room()
                .map(|room| stages.receive(room, plugin, local.as_ref(), sender, frame))
                .unwrap_or_default()
        });
        self.run_stage(effects);
        true
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    use super::super::{Connection, MessagingMeta, SelfInfo};
    use super::*;
    use crate::messaging::announce::AnnounceEntry;
    use crate::wit::peers;

    const SERVER: mumcord_store::ServerId = 1;

    fn peer(session: u32) -> PeerInfo {
        PeerInfo {
            session,
            name: format!("user{session}"),
            cert_hash: None,
            is_friend: false,
        }
    }

    /// The user (session 1) in channel 5 with session 2 who runs the same official `brawl`;
    /// `brawl` holds `ui.stage` but not `peers.messages`.
    fn io() -> (ServiceIo, UnboundedReceiver<ServiceEvent>) {
        let (events, rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        {
            let mut peers = io.peers();
            peers.focused = Some(SERVER);
            let mut connection = Connection::new(Instant::now());
            connection.supported = true;
            connection.me = Some(SelfInfo {
                me: peer(1),
                channel: Some((5, "Lobby".into())),
                muted: false,
                deafened: false,
            });
            connection.members.insert(2, peer(2));
            connection.announced.insert(
                2,
                vec![AnnounceEntry {
                    id: "brawl".into(),
                    api_major: 1,
                    source: SourceTag::Official,
                }],
            );
            peers.connections.insert(SERVER, connection);
            peers.plugins.insert(
                "brawl".into(),
                PluginMeta {
                    name: "Board Brawl".into(),
                    messaging: Some(MessagingMeta {
                        source: SourceTag::Official,
                        visibility: Visibility::Everyone,
                        max_payload: 997,
                        messages: false,
                    }),
                    stage: Some(4),
                },
            );
        }
        let place = place(&io.peers()).unzip();
        io.stages().moved(place.0, place.1, false);
        (io, rx)
    }

    fn drain(rx: &mut UnboundedReceiver<ServiceEvent>) -> Vec<ServiceEvent> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    fn tree() -> StageContent {
        StageContent::Tree(Vec::new())
    }

    #[test]
    fn start_needs_a_gesture_then_announces_and_joins_the_starter() {
        let (io, mut rx) = io();
        assert_eq!(
            io.stage_start("brawl", "Game".into(), tree()),
            Err(Error::NotPermitted)
        );
        io.gesture("brawl");
        let id = io
            .stage_start("brawl", "Game".into(), tree())
            .expect("started");
        let events = drain(&mut rx);
        let frame = events.iter().find_map(|e| match e {
            ServiceEvent::PeerSend {
                server: SERVER,
                plugin,
                outgoing,
            } if plugin == "brawl" => Some(outgoing.clone()),
            _ => None,
        });
        let frame = frame.expect("start frame");
        assert_eq!(frame.data_id, "mumcord.p.brawl");
        assert_eq!(frame.receivers, vec![2]);
        let (_, body) = envelope::decode_host(&frame.bytes).expect("host frame");
        assert!(matches!(
            announce::decode(body),
            Some(announce::Frame::Start { id: ref sent, .. }) if *sent == id
        ));
        assert!(events.iter().any(|e| matches!(
            e,
            ServiceEvent::StageEvent { event: PluginEvent::StageJoined(p), .. } if p.session == 1
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            ServiceEvent::Ui {
                update: UiUpdate::StageTree { .. },
                ..
            }
        )));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ServiceEvent::StageChanged))
        );
        // Stage frames need no `peers.messages`, plugin messages still do.
        assert_eq!(
            io.peer_send("brawl", peers::Target::Channel, 1, vec![]),
            Err(Error::NotPermitted)
        );
    }

    #[test]
    fn a_second_activity_of_the_plugin_is_refused_with_the_limit_text() {
        let (io, mut rx) = io();
        io.gesture("brawl");
        io.stage_start("brawl", "Game".into(), tree())
            .expect("started");
        drain(&mut rx);
        assert_eq!(
            io.stage_start("brawl", "Again".into(), tree()),
            Err(Error::QuotaExceeded)
        );
        assert!(drain(&mut rx).iter().any(|e| matches!(
            e,
            ServiceEvent::StageRefused {
                reason: Refusal::TooMany,
                ..
            }
        )));
    }

    #[test]
    fn set_tree_needs_a_tree_activity_and_leave_closes_the_stage() {
        let (io, mut rx) = io();
        assert_eq!(io.stage_set_tree("brawl", Vec::new()), Err(Error::Invalid));
        io.gesture("brawl");
        io.stage_start("brawl", "Game".into(), tree())
            .expect("started");
        assert_eq!(io.stage_set_tree("brawl", Vec::new()), Ok(()));
        drain(&mut rx);
        io.stage_leave("brawl");
        let events = drain(&mut rx);
        assert!(events.iter().any(|e| matches!(
            e,
            ServiceEvent::StageEvent { event: PluginEvent::StageLeft(p), .. } if p.session == 1
        )));
        // The user was the only one: it ended.
        assert!(events.iter().any(|e| matches!(
            e,
            ServiceEvent::StageEvent {
                event: PluginEvent::StageActivityEnded(_),
                ..
            }
        )));
        assert_eq!(io.stage_set_tree("brawl", Vec::new()), Err(Error::Invalid));
    }

    #[test]
    fn web_content_opens_the_stage_frame() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("index.html"), "<p>hi</p>").expect("page written");
        let (io, mut rx) = io();
        io.webviews().set_root(
            "brawl",
            Some(dir.path().to_path_buf()),
            vec![WebviewPlace::Stage],
        );
        io.gesture("brawl");
        assert_eq!(
            io.stage_start(
                "brawl",
                "Game".into(),
                StageContent::Web("missing.html".into())
            ),
            Err(Error::Invalid)
        );
        io.stage_start(
            "brawl",
            "Game".into(),
            StageContent::Web("index.html".into()),
        )
        .expect("started");
        assert!(drain(&mut rx).iter().any(|e| matches!(
            e,
            ServiceEvent::Ui {
                update: UiUpdate::WebviewShow {
                    place: WebviewPlace::Stage,
                    ..
                },
                ..
            }
        )));
        assert_eq!(io.stage_set_tree("brawl", Vec::new()), Err(Error::Invalid));
    }

    #[test]
    fn without_a_channel_start_is_not_permitted() {
        let (io, _rx) = io();
        io.stages().moved(None, None, false);
        io.gesture("brawl");
        assert_eq!(
            io.stage_start("brawl", "Game".into(), tree()),
            Err(Error::NotPermitted)
        );
    }
}
