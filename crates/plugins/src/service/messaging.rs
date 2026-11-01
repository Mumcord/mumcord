//! Engine events in, plugin events and messages out (FR-6.16, FR-6.33 … FR-6.36, spec §6.2).
//!
//! The app turns each engine plugin event into one call here. The service keeps the messaging
//! state per connection in [`crate::api::peers::Peers`] (shared with the `peers` host calls),
//! forwards events of the focused server to the plugins with each plugin's own peer keys, and
//! filters incoming plugin messages. Plugins see only the server the user looks at.

use std::collections::BTreeMap;
use std::time::Instant;

use mumcord_store::{PluginVisibility, ServerId};

use super::plugin_io::{
    API_MAJOR, BotInfo, Connection, MessagingMeta, PeerInfo, PluginMeta, SelfInfo, wit_peer,
};
use super::{Origin, PluginService};
use crate::manifest::Visibility;
use crate::messaging::announce::{self, ANNOUNCE_DATA_ID, AnnounceEntry, SourceTag};
use crate::messaging::envelope::plugin_id_of;
use crate::messaging::{Counters, Member, PluginRef};
use crate::registry::key_fingerprint_prefix;
use crate::wit::events::{Channel, Event, PeerMessage, SelfState};

/// An announce (`mumcord.plugins`) to send on one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announce {
    pub server: ServerId,
    /// Users whose hello lists `plugins`.
    pub receivers: Vec<u32>,
    pub bytes: Vec<u8>,
}

impl PluginService {
    /// The server the user looks at changed (`None`: none). Plugins get `self-changed`.
    pub fn set_focused_server(&mut self, server: Option<ServerId>) {
        let changed = std::mem::replace(&mut self.io.peers().focused, server) != server;
        if changed {
            self.io.menu_forget_sessions(None);
            self.io.refresh_menus();
        }
        self.broadcast_self();
    }

    /// Whether the server passes plugin data (engine `SharedState`).
    pub fn shared_state(&mut self, server: ServerId, supported: bool) {
        let mut state = self.io.peers();
        let connection = connection(&mut state.connections, server);
        if supported && !connection.supported {
            connection.throttle.reset();
        }
        connection.supported = supported;
    }

    /// The connection ended: its messaging state goes; plugins get `self-changed`.
    pub fn disconnected(&mut self, server: ServerId) {
        let focused = {
            let mut state = self.io.peers();
            state.connections.remove(&server);
            state.focused == Some(server)
        };
        if focused {
            self.io.menu_forget_sessions(None);
            self.io.refresh_menus();
            self.broadcast_self();
        }
    }

    /// A user's hello does (or no longer does) list `plugins`. A new one gets our announce.
    pub fn peer_capable(&mut self, server: ServerId, session: u32, capable: bool) {
        let mut state = self.io.peers();
        let connection = connection(&mut state.connections, server);
        if capable {
            if connection.capable.insert(session) {
                connection.throttle.mark_changed();
            }
        } else {
            connection.capable.remove(&session);
            connection.announced.remove(&session);
        }
    }

    /// Someone joined the user's channel (or was there when the user arrived).
    pub fn peer_joined(&mut self, server: ServerId, peer: PeerInfo) {
        connection(&mut self.io.peers().connections, server)
            .members
            .insert(peer.session, peer.clone());
        self.io.refresh_menus();
        self.dispatch(server, |keys, id| Event::Joined(wit_peer(keys, id, &peer)));
    }

    /// Someone left the user's channel or the server, or the user left their channel.
    pub fn peer_left(&mut self, server: ServerId, peer: &PeerInfo) {
        {
            let mut state = self.io.peers();
            let connection = connection(&mut state.connections, server);
            connection.members.remove(&peer.session);
            connection.messenger.forget_session(peer.session);
            if state.focused == Some(server) {
                drop(state);
                self.io.menu_forget_sessions(Some(peer.session));
            }
        }
        self.io.refresh_menus();
        self.dispatch(server, |keys, id| Event::Left(wit_peer(keys, id, peer)));
    }

    /// A person's hello says they are a bot (`None`: no longer, or they left the server).
    pub fn peer_bot(&mut self, server: ServerId, session: u32, bot: Option<BotInfo>) {
        {
            let mut state = self.io.peers();
            let bots = &mut connection(&mut state.connections, server).bots;
            match bot {
                Some(bot) => bots.insert(session, bot),
                None => bots.remove(&session),
            };
        }
        self.io.refresh_menus();
    }

    pub fn peer_talking(&mut self, server: ServerId, peer: &PeerInfo, talking: bool) {
        self.dispatch(server, |keys, id| {
            Event::Talking((wit_peer(keys, id, peer), talking))
        });
    }

    pub fn screen_share_started(&mut self, server: ServerId, peer: &PeerInfo) {
        self.dispatch(server, |keys, id| {
            Event::ScreenShareStarted(wit_peer(keys, id, peer))
        });
    }

    /// Someone wrote in a channel; never the text.
    pub fn chat_activity(&mut self, server: ServerId, sender: &PeerInfo, channel: (u32, String)) {
        self.dispatch(server, |keys, id| {
            Event::ChatActivity((
                wit_peer(keys, id, sender),
                Channel {
                    id: channel.0,
                    name: channel.1.clone(),
                },
            ))
        });
    }

    /// Someone mentioned the user; never the text.
    pub fn mentioned(&mut self, server: ServerId, sender: &PeerInfo) {
        self.dispatch(server, |keys, id| {
            Event::Mentioned(wit_peer(keys, id, sender))
        });
    }

    /// The user's own state on `server` changed.
    pub fn self_changed(&mut self, server: ServerId, me: SelfInfo) {
        let focused = {
            let mut state = self.io.peers();
            connection(&mut state.connections, server).me = Some(me);
            state.focused == Some(server)
        };
        if focused {
            self.broadcast_self();
        }
    }

    /// Raw plugin data from the engine: an announce, or a message for one of our plugins.
    /// Messages pass the B12 checks (same id and source announced, sender in the channel,
    /// visibility, flood guard, envelope) and reach the plugin as `peer-message`.
    pub fn peer_data(&mut self, server: ServerId, sender: &PeerInfo, data_id: &str, data: &[u8]) {
        let mut state = self.io.peers();
        if data_id == ANNOUNCE_DATA_ID {
            connection(&mut state.connections, server)
                .announced
                .insert(sender.session, announce::parse(data));
            return;
        }
        let Some(plugin) = plugin_id_of(data_id) else {
            return;
        };
        if state.focused != Some(server) {
            return;
        }
        let Some(meta) = state.plugins.get(plugin).and_then(|m| m.messaging.clone()) else {
            return;
        };
        let Some(connection) = state.connections.get_mut(&server) else {
            return;
        };
        let announced = connection
            .announced
            .get(&sender.session)
            .cloned()
            .unwrap_or_default();
        let member = connection.members.get(&sender.session).map(|peer| Member {
            session: peer.session,
            is_friend: peer.is_friend,
            plugins: &announced,
        });
        let delivery = connection.messenger.receive(
            &PluginRef {
                id: plugin,
                source: meta.source,
                visibility: meta.visibility,
            },
            sender.session,
            member.as_ref(),
            data,
            Instant::now(),
        );
        drop(state);
        if let Some(delivery) = delivery {
            let peer = wit_peer(self.ctx.peer_keys.as_ref(), plugin, sender);
            self.host.send(
                plugin,
                Event::PeerMessage(PeerMessage {
                    sender: peer,
                    kind: delivery.kind,
                    payload: delivery.payload,
                    direct: delivery.direct,
                }),
            );
        }
    }

    /// The engine dropped one of `plugin`'s messages after 10 s (B5 `LowOutcome::Expired`).
    pub fn message_expired(&mut self, server: ServerId, plugin: &str) {
        if let Some(connection) = self.io.peers().connections.get_mut(&server) {
            connection.messenger.record_expired(plugin);
        }
    }

    /// A plugin's messaging counters over all connections, for Diagnostics.
    pub fn messaging_counters(&self, plugin: &str) -> Counters {
        let state = self.io.peers();
        state
            .connections
            .values()
            .map(|c| c.messenger.counters(plugin))
            .fold(Counters::default(), |sum, c| Counters {
                sent: sum.sent + c.sent,
                rate_limited: sum.rate_limited + c.rate_limited,
                expired: sum.expired + c.expired,
                received: sum.received + c.received,
                incoming_rate_dropped: sum.incoming_rate_dropped + c.incoming_rate_dropped,
                incoming_invalid: sum.incoming_invalid + c.incoming_invalid,
                incoming_refused: sum.incoming_refused + c.incoming_refused,
            })
    }

    /// The announces due now (at most one per connection and 10 s). The app sends each with
    /// data id `mumcord.plugins` and calls this again at [`Self::next_announce_at`].
    pub fn poll_announces(&mut self) -> Vec<Announce> {
        let now = Instant::now();
        let mut state = self.io.peers();
        let entries: BTreeMap<&str, (AnnounceEntry, Visibility)> = state
            .plugins
            .iter()
            .filter_map(|(id, meta)| meta.messaging.as_ref().map(|meta| (id, meta)))
            .map(|(id, meta)| {
                let entry = AnnounceEntry {
                    id: id.clone(),
                    api_major: u32::from(API_MAJOR),
                    source: meta.source,
                };
                (id.as_str(), (entry, meta.visibility))
            })
            .collect();
        let bytes = announce::build(entries.values().map(|(entry, v)| (entry, *v)));
        drop(entries);
        let mut due = Vec::new();
        for (server, connection) in &mut state.connections {
            if connection.supported
                && !connection.capable.is_empty()
                && connection.throttle.poll(now)
            {
                due.push(Announce {
                    server: *server,
                    receivers: connection.capable.iter().copied().collect(),
                    bytes: bytes.clone(),
                });
            }
        }
        due
    }

    /// When [`Self::poll_announces`] has something to send next, if anything is pending.
    pub fn next_announce_at(&self) -> Option<Instant> {
        let now = Instant::now();
        let state = self.io.peers();
        state
            .connections
            .values()
            .filter(|c| c.supported && !c.capable.is_empty())
            .filter_map(|c| c.throttle.due_at(now))
            .min()
    }

    /// Do Not Disturb or quiet time: notifications are held and plugins see it in
    /// `self-changed`.
    pub fn set_quiet(&mut self, quiet: bool) {
        self.quiet = quiet;
        self.ctx.notify.set_quiet(quiet);
        self.broadcast_self();
    }

    /// Brings the I/O view of the switched-on plugins (name for the chat prefix; source,
    /// visibility and payload limit for those holding `peers.messages`) in line with the
    /// service; a messaging change is announced.
    pub(super) fn sync_peers(&self) {
        let records = match self.store().plugins() {
            Ok(records) => records,
            Err(error) => {
                tracing::warn!(%error, "could not read the plugins for messaging");
                return;
            }
        };
        let repositories = self.store().plugin_repositories().unwrap_or_default();
        let mut metas = std::collections::HashMap::new();
        if !self.host.all_off() {
            for record in records.iter().filter(|r| r.enabled) {
                let Some(manifest) = self.host.manifest(&record.id) else {
                    continue;
                };
                let messaging = if self.ctx.grants.allows(&record.id, "peers.messages") {
                    self.source_tag(&record.source, &repositories)
                        .map(|source| MessagingMeta {
                            source,
                            visibility: match record.visibility {
                                PluginVisibility::Everyone => Visibility::Everyone,
                                PluginVisibility::Friends => Visibility::Friends,
                                PluginVisibility::Nobody => Visibility::Nobody,
                            },
                            max_payload: manifest.messaging.max_payload,
                        })
                } else {
                    None
                };
                metas.insert(
                    record.id.clone(),
                    PluginMeta {
                        name: manifest.name.clone(),
                        messaging,
                    },
                );
            }
        }
        let mut state = self.io.peers();
        if state.plugins == metas {
            return;
        }
        let now = Instant::now();
        let gone: Vec<String> = state
            .plugins
            .iter()
            .filter(|(id, meta)| {
                meta.messaging.is_some() && metas.get(*id).is_none_or(|m| m.messaging.is_none())
            })
            .map(|(id, _)| id.clone())
            .collect();
        for connection in state.connections.values_mut() {
            for id in &gone {
                connection.messenger.forget_plugin(id, now);
            }
            connection.throttle.mark_changed();
        }
        state.plugins = metas;
    }

    /// How peers tell this plugin's source apart (FR-6.35); `None` for a repository that is
    /// gone or has an unreadable key.
    fn source_tag(
        &self,
        source: &str,
        repositories: &[mumcord_store::PluginRepository],
    ) -> Option<SourceTag> {
        match Origin::parse(source)? {
            Origin::Official => Some(SourceTag::Official),
            Origin::Dev(_) => Some(SourceTag::Dev),
            Origin::Repo(repository) => repositories
                .iter()
                .find(|r| r.id == repository)
                .and_then(|r| key_fingerprint_prefix(&r.public_key))
                .map(SourceTag::Repo),
        }
    }

    /// `self-changed` for one plugin: the focused server's state with this plugin's keys.
    pub(super) fn self_event(&self, plugin: &str) -> Event {
        let keys = self.ctx.peer_keys.as_ref();
        let state = self.io.peers();
        let focused = state
            .focused
            .and_then(|server| state.connections.get(&server).map(|c| (server, c)));
        let me = focused.and_then(|(_, c)| c.me.as_ref());
        Event::SelfChanged(SelfState {
            me: me.map(|me| wit_peer(keys, plugin, &me.me)),
            server: focused.filter(|_| me.is_some()).and_then(|(server, _)| {
                keys.and_then(|k| k.key(plugin, Some(&format!("server:{server}"))))
            }),
            channel: me
                .and_then(|me| me.channel.clone())
                .map(|(id, name)| Channel { id, name }),
            muted: me.is_some_and(|me| me.muted),
            deafened: me.is_some_and(|me| me.deafened),
            do_not_disturb: self.quiet,
        })
    }

    fn broadcast_self(&self) {
        for id in self.loaded_ids() {
            self.host.send(&id, self.self_event(&id));
        }
    }

    /// Sends an event of `server` to every plugin, built per plugin (each has its own peer
    /// keys). Events of other servers are not for plugins. The host drops it for plugins
    /// without the event's permission.
    fn dispatch(
        &self,
        server: ServerId,
        event: impl Fn(Option<&crate::peer_key::PeerKeys>, &str) -> Event,
    ) {
        if self.io.peers().focused != Some(server) {
            return;
        }
        let keys = self.ctx.peer_keys.as_ref();
        for id in self.loaded_ids() {
            self.host.send(&id, event(keys, &id));
        }
    }

    /// The user clicked in `plugin`'s UI (a `ui-action`): it may post one chat line within 2 s.
    pub fn gesture(&self, plugin: &str) {
        self.io.gesture(plugin);
    }

    /// The user typed one of `plugin`'s slash commands: one chat line within 8 s.
    pub fn command_gesture(&self, plugin: &str) {
        self.io.command_gesture(plugin);
    }

    /// An incoming channel text message (its plain text as other clients see it). Returns
    /// `Replace` when a plugin card supersedes it (FR-6.45); only a hash is kept.
    pub fn chat_text(&self, message: u64, sender: u32, line: &str) -> crate::chat::Supersede {
        self.io.chat_text(message, sender, line)
    }

    fn loaded_ids(&self) -> Vec<String> {
        self.host.ids().map(str::to_owned).collect()
    }
}

fn connection(
    connections: &mut std::collections::HashMap<ServerId, Connection>,
    server: ServerId,
) -> &mut Connection {
    connections
        .entry(server)
        .or_insert_with(|| Connection::new(Instant::now()))
}
