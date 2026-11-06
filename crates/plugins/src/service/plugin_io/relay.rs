//! The service side of `relay` (FR-9.57 … FR-9.63): status per plugin, reach and rate of a
//! send, the Mumble fallback, incoming relay messages and the user's switches.
//!
//! The engine reports per server whether its relay can carry plugin data
//! ([`PluginService::set_relay_availability`]); the app passes the global switch
//! ([`PluginService::set_relay_data_enabled`]). A 1:1 message through the relay leaves as
//! [`ServiceEvent::RelaySend`] for the engine's pair session and comes back on the other side
//! through [`PluginService::relay_received`]. A channel message through the relay leaves as
//! [`ServiceEvent::RelayBroadcast`] for the sender's relay broadcast; the members that have not
//! joined it ([`PluginService::set_relay_broadcast_joined`]) get it as fallback when it fits
//! (FR-9.61). Fallback messages are host frames
//! ([`HOST_KIND_RELAY`]) on the plugin's Mumble plugin-data id. Payloads are opaque: never
//! parsed or logged (FR-9.94).

use std::collections::HashMap;
use std::sync::{MutexGuard, PoisonError};
use std::time::Instant;

use mumcord_store::ServerId;

use super::{PeersState, ServiceIo, wit_peer};
use crate::api::relay::{
    self, Availability, Broadcast, BroadcastState, Bucket, ENVELOPE_API, MAX_FALLBACK, Path,
    RelayStatus, SendError,
};
use crate::api::v1_1::{Up, relay_status};
use crate::messaging::envelope::{self, HOST_KIND_RELAY};
use crate::messaging::{Member, PluginRef, Target};
use crate::runtime::PluginEvent;
use crate::service::{PluginService, ServiceEvent};
use crate::wit::types::Error;

#[derive(Debug)]
pub(in crate::service) struct RelayState {
    /// "Let plugins use a server's relay" (FR-9.64), on by default.
    pub(in crate::service) switch_on: bool,
    availability: HashMap<ServerId, Availability>,
    buckets: HashMap<String, Bucket>,
    /// Bytes each plugin sent and received through the relay in this app session (FR-9.64).
    usage: HashMap<String, u64>,
    /// The status each plugin was last told about.
    told: HashMap<String, RelayStatus>,
    /// Receivers in the own relay broadcast per server, as the engine reports them.
    joined: HashMap<ServerId, Vec<u32>>,
    /// The own relay broadcast per server, as the engine reports it.
    broadcast: HashMap<ServerId, Broadcast>,
}

impl Default for RelayState {
    fn default() -> Self {
        Self {
            switch_on: true,
            availability: HashMap::new(),
            buckets: HashMap::new(),
            usage: HashMap::new(),
            told: HashMap::new(),
            joined: HashMap::new(),
            broadcast: HashMap::new(),
        }
    }
}

impl RelayState {
    /// The status on the focused server for a plugin that holds `peers.relay`.
    fn status(&self, focused: Option<ServerId>) -> RelayStatus {
        let availability = focused
            .and_then(|server| self.availability.get(&server).copied())
            .unwrap_or_default();
        relay::status(availability, self.switch_on, true)
    }
}

impl ServiceIo {
    /// The connection to `server` ended: no relay there any more.
    pub(in crate::service) fn relay_forget_server(&self, server: ServerId) {
        let mut relay = self.relay();
        relay.availability.remove(&server);
        relay.joined.remove(&server);
        relay.broadcast.remove(&server);
    }

    pub(in crate::service) fn relay(&self) -> MutexGuard<'_, RelayState> {
        self.relay.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn relay_status_of(&self) -> RelayStatus {
        let focused = self.peers().focused;
        self.relay().status(focused)
    }

    pub(super) fn relay_send_to(
        &self,
        plugin: &str,
        target: Target,
        data: Vec<u8>,
        fallback: bool,
        permitted: bool,
    ) -> Result<(), SendError> {
        let mut peers = self.peers();
        let PeersState {
            focused,
            plugins,
            connections,
            ..
        } = &mut *peers;
        let meta = plugins
            .get(plugin)
            .and_then(|meta| meta.messaging.clone())
            .ok_or(Error::NotPermitted)?;
        let server = focused.ok_or(Error::NoRecipients)?;
        let connection = connections
            .get_mut(&server)
            .filter(|c| c.supported)
            .ok_or(Error::NoRecipients)?;
        let plugin_ref = PluginRef {
            id: plugin,
            source: meta.source,
            visibility: meta.visibility,
        };
        let members: Vec<Member<'_>> = connection
            .members
            .values()
            .map(|peer| Member {
                session: peer.session,
                is_friend: peer.is_friend,
                plugins: connection
                    .announced
                    .get(&peer.session)
                    .map_or(&[], Vec::as_slice),
            })
            .collect();
        // Reach first (FR-9.58): the same rules and errors as `peers.send`.
        let receivers = crate::messaging::recipients::select(&plugin_ref, target, &members)
            .map_err(super::wit_error)?;
        // Without `peers.relay` the plugin's sends behave as with the relay switched off
        // (FR-9.63): small ones take the Mumble fallback, the rest are `relay-unavailable`.
        let status = if permitted {
            self.relay().status(Some(server))
        } else {
            RelayStatus::SwitchedOff
        };
        let path =
            relay::path(status, target, data.len(), fallback).ok_or(SendError::Unavailable)?;
        let now = Instant::now();
        let take_rate = |relay: &mut RelayState| {
            if !relay
                .buckets
                .entry(plugin.to_owned())
                .or_insert_with(|| Bucket::new(now))
                .take(data.len(), now)
            {
                return Err(SendError::Error(Error::RateLimited));
            }
            *relay.usage.entry(plugin.to_owned()).or_default() += data.len() as u64;
            Ok(())
        };
        match path {
            Path::Relay => {
                take_rate(&mut self.relay())?;
                drop(peers);
                for session in receivers {
                    self.emit(ServiceEvent::RelaySend {
                        server,
                        plugin: plugin.to_owned(),
                        session,
                        api: ENVELOPE_API,
                        data: data.clone(),
                    });
                }
            }
            Path::Broadcast => {
                let joined = self
                    .relay()
                    .joined
                    .get(&server)
                    .cloned()
                    .unwrap_or_default();
                let outside: Vec<Member<'_>> = members
                    .iter()
                    .filter(|m| receivers.contains(&m.session) && !joined.contains(&m.session))
                    .copied()
                    .collect();
                let fell_back = fallback && data.len() <= MAX_FALLBACK && !outside.is_empty();
                // Whether it can arrive while the broadcast starts or is down, before anything
                // leaves; it counts only once it is sent.
                let mut broadcast = self
                    .relay()
                    .broadcast
                    .get(&server)
                    .copied()
                    .unwrap_or_default();
                broadcast.accept(data.len(), fell_back, now)?;
                // The members outside the broadcast first, so a refused fallback (its rate)
                // sends nothing at all.
                let outgoing = if fell_back {
                    let frame = envelope::encode_host(HOST_KIND_RELAY, &data);
                    Some(
                        connection
                            .messenger
                            .send_host(&plugin_ref, target, &outside, frame, now)
                            .map_err(super::wit_error)?,
                    )
                } else {
                    None
                };
                {
                    let mut relay = self.relay();
                    take_rate(&mut relay)?;
                    relay.broadcast.insert(server, broadcast);
                }
                drop(peers);
                if let Some(outgoing) = outgoing {
                    self.emit(ServiceEvent::PeerSend {
                        server,
                        plugin: plugin.to_owned(),
                        outgoing,
                    });
                }
                self.emit(ServiceEvent::RelayBroadcast {
                    server,
                    plugin: plugin.to_owned(),
                    api: ENVELOPE_API,
                    data,
                    receivers,
                    fell_back,
                });
            }
            Path::Fallback => {
                let frame = envelope::encode_host(HOST_KIND_RELAY, &data);
                let outgoing = connection
                    .messenger
                    .send_host(&plugin_ref, target, &members, frame, Instant::now())
                    .map_err(super::wit_error)?;
                drop(peers);
                self.emit(ServiceEvent::PeerSend {
                    server,
                    plugin: plugin.to_owned(),
                    outgoing,
                });
            }
        }
        Ok(())
    }
}

impl PluginService {
    /// What the engine reports for `server`'s relay; plugins whose status changes get
    /// `relay-status-changed`.
    pub fn set_relay_availability(&mut self, server: ServerId, availability: Availability) {
        self.io.relay().availability.insert(server, availability);
        self.relay_refresh();
    }

    /// The receivers in the own relay broadcast on `server` (engine `PluginRelayJoined`).
    pub fn set_relay_broadcast_joined(&mut self, server: ServerId, joined: Vec<u32>) {
        self.io.relay().joined.insert(server, joined);
    }

    /// The own relay broadcast on `server` went live, down or ended (engine
    /// `PluginRelayBroadcast`).
    pub fn set_relay_broadcast_state(&mut self, server: ServerId, state: BroadcastState) {
        let broadcast = Broadcast::reported(state, Instant::now());
        self.io.relay().broadcast.insert(server, broadcast);
    }

    /// The global switch (FR-9.63). The app ends the engine's relay streams itself.
    pub fn set_relay_data_enabled(&mut self, enabled: bool) {
        self.io.relay().switch_on = enabled;
        self.relay_refresh();
    }

    /// `relay.status()` of the focused server as the UI shows it for plugins that may use it.
    pub fn relay_availability(&self, server: ServerId) -> Availability {
        self.io
            .relay()
            .availability
            .get(&server)
            .copied()
            .unwrap_or_default()
    }

    /// Bytes `plugin` sent and received through the relay in this app session (FR-9.64).
    pub fn relay_usage(&self, plugin: &str) -> u64 {
        self.io.relay().usage.get(plugin).copied().unwrap_or(0)
    }

    /// Tells every running plugin with `peers.relay` its new status when it changed. Call after
    /// anything that moves it: availability, switch, focused server, permissions.
    pub(in crate::service) fn relay_refresh(&self) {
        let focused = self.io.peers().focused;
        let ids: Vec<String> = self.io.peers().plugins.keys().cloned().collect();
        let mut changed = Vec::new();
        {
            let mut relay = self.io.relay();
            let base = relay.status(focused);
            relay.told.retain(|id, _| ids.contains(id));
            for id in ids {
                let status = if self.ctx.grants.allows(&id, relay::PERMISSION) {
                    base
                } else {
                    RelayStatus::SwitchedOff
                };
                // A plugin that just started asks `relay.status()` itself.
                if let Some(old) = relay.told.insert(id.clone(), status)
                    && old != status
                {
                    changed.push((id, status));
                }
            }
        }
        for (id, status) in changed {
            self.host
                .send_relay(&id, PluginEvent::RelayStatusChanged(relay_status(status)));
        }
    }

    /// A relay message for `plugin` from `session` on `server` (engine `PluginRelayData`).
    pub fn relay_received(&mut self, server: ServerId, session: u32, plugin: &str, data: Vec<u8>) {
        let len = data.len() as u64;
        if self.relay_deliver(server, session, plugin, data, false) {
            *self.io.relay().usage.entry(plugin.to_owned()).or_default() += len;
        }
    }

    /// A [`HOST_KIND_RELAY`] frame on `plugin`'s Mumble plugin-data id.
    pub(in crate::service) fn relay_fallback_frame(
        &mut self,
        server: ServerId,
        session: u32,
        plugin: &str,
        body: &[u8],
    ) {
        self.relay_deliver(server, session, plugin, body.to_vec(), true);
    }

    /// Delivers `relay-message` if the plugin may take it: it holds `peers.relay`, the sender
    /// is in the channel and passes the visibility rules, and, for relay messages, the switch
    /// is on. Fallback frames also pass the Mumble flood guard (FR-6.36).
    fn relay_deliver(
        &mut self,
        server: ServerId,
        session: u32,
        plugin: &str,
        data: Vec<u8>,
        fallback: bool,
    ) -> bool {
        if !self.ctx.grants.allows(plugin, relay::PERMISSION)
            || (!fallback && !self.io.relay().switch_on)
        {
            return false;
        }
        let mut peers = self.io.peers();
        if peers.focused != Some(server) {
            return false;
        }
        let Some(meta) = peers.plugins.get(plugin).and_then(|m| m.messaging.clone()) else {
            return false;
        };
        let Some(connection) = peers.connections.get_mut(&server) else {
            return false;
        };
        let announced = connection
            .announced
            .get(&session)
            .cloned()
            .unwrap_or_default();
        let sender = connection.members.get(&session).cloned();
        let member = sender.as_ref().map(|peer| Member {
            session,
            is_friend: peer.is_friend,
            plugins: &announced,
        });
        let plugin_ref = PluginRef {
            id: plugin,
            source: meta.source,
            visibility: meta.visibility,
        };
        let admitted = if fallback {
            connection
                .messenger
                .admit_host(&plugin_ref, session, member.as_ref(), Instant::now())
        } else {
            crate::messaging::recipients::accept(&plugin_ref, member.as_ref()).is_ok()
        };
        drop(peers);
        let Some(sender) = sender.filter(|_| admitted) else {
            return false;
        };
        let peer = wit_peer(self.ctx.peer_keys.as_ref(), plugin, &sender);
        self.host
            .send_relay(plugin, PluginEvent::RelayMessage((peer.up(), data)))
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    use super::super::{Connection, MessagingMeta, PeerInfo, PluginMeta};
    use super::*;
    use crate::api::relay::MAX_MESSAGE;
    use crate::manifest::Visibility;
    use crate::messaging::announce::{AnnounceEntry, SourceTag};

    const SERVER: ServerId = 1;
    const AVAILABLE: Availability = Availability::Available { kbps: Some(512) };

    /// The user with sessions 2 and 4 (run `dice`) and session 3 (does not) in the channel.
    fn io() -> (ServiceIo, UnboundedReceiver<ServiceEvent>) {
        let (events, rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        {
            let mut peers = io.peers();
            peers.focused = Some(SERVER);
            let mut connection = Connection::new(Instant::now());
            connection.supported = true;
            for session in [2, 3, 4] {
                connection.members.insert(
                    session,
                    PeerInfo {
                        session,
                        name: format!("user{session}"),
                        cert_hash: None,
                        is_friend: false,
                    },
                );
            }
            for session in [2, 4] {
                connection.announced.insert(
                    session,
                    vec![AnnounceEntry {
                        id: "dice".into(),
                        api_major: 1,
                        source: SourceTag::Official,
                    }],
                );
            }
            peers.connections.insert(SERVER, connection);
            peers.plugins.insert(
                "dice".into(),
                PluginMeta {
                    name: "Dice".into(),
                    messaging: Some(MessagingMeta {
                        source: SourceTag::Official,
                        visibility: Visibility::Everyone,
                        max_payload: 997,
                        messages: false,
                    }),
                    stage: None,
                },
            );
        }
        (io, rx)
    }

    fn available(io: &ServiceIo) {
        io.relay().availability.insert(SERVER, AVAILABLE);
    }

    fn drain(rx: &mut UnboundedReceiver<ServiceEvent>) -> Vec<ServiceEvent> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    #[test]
    fn status_reports_no_relay_until_the_engine_says_otherwise() {
        let (io, _rx) = io();
        assert_eq!(io.relay_status_of(), RelayStatus::NoRelay);
        available(&io);
        assert_eq!(io.relay_status_of(), RelayStatus::Available);
        io.relay().switch_on = false;
        assert_eq!(io.relay_status_of(), RelayStatus::SwitchedOff);
        io.relay()
            .availability
            .insert(SERVER, Availability::UseNotAllowed);
        assert_eq!(io.relay_status_of(), RelayStatus::NotAllowed);
    }

    #[test]
    fn a_direct_message_goes_to_the_engine_unchanged() {
        let (io, mut rx) = io();
        available(&io);
        let data: Vec<u8> = (0..=255u8).cycle().take(MAX_MESSAGE).collect();
        io.relay_send_to("dice", Target::Direct(2), data.clone(), false, true)
            .expect("sent");
        let events = drain(&mut rx);
        assert!(
            matches!(
                events.as_slice(),
                [ServiceEvent::RelaySend { server: SERVER, plugin, session: 2, api: (1, 1), data: sent }]
                    if plugin == "dice" && *sent == data
            ),
            "{events:?}"
        );
        assert_eq!(io.relay().usage.get("dice"), Some(&(MAX_MESSAGE as u64)));
    }

    #[test]
    fn reach_follows_peers_messages() {
        let (io, _rx) = io();
        available(&io);
        // Session 3 does not run the plugin; 9 is not in the channel.
        for session in [3, 9] {
            assert_eq!(
                io.relay_send_to("dice", Target::Direct(session), vec![1], true, true),
                Err(SendError::Error(Error::NoRecipients))
            );
        }
        io.peers()
            .plugins
            .get_mut("dice")
            .and_then(|m| m.messaging.as_mut())
            .expect("meta")
            .visibility = Visibility::Nobody;
        assert_eq!(
            io.relay_send_to("dice", Target::Direct(2), vec![1], true, true),
            Err(SendError::Error(Error::Hidden))
        );
    }

    #[test]
    fn the_rate_allows_a_1_mib_burst_per_plugin() {
        let (io, _rx) = io();
        available(&io);
        for _ in 0..16 {
            io.relay_send_to("dice", Target::Direct(2), vec![0; MAX_MESSAGE], false, true)
                .expect("within the burst");
        }
        assert_eq!(
            io.relay_send_to("dice", Target::Direct(2), vec![0; MAX_MESSAGE], false, true),
            Err(SendError::Error(Error::RateLimited))
        );
    }

    #[test]
    fn without_the_relay_small_messages_fall_back_to_mumble_plugin_data() {
        let (io, mut rx) = io();
        let data = vec![7; MAX_FALLBACK];
        io.relay_send_to("dice", Target::Direct(2), data.clone(), true, true)
            .expect("fallback");
        let events = drain(&mut rx);
        let [ServiceEvent::PeerSend { outgoing, .. }] = events.as_slice() else {
            panic!("{events:?}");
        };
        assert_eq!(outgoing.data_id, "mumcord.p.dice");
        assert_eq!(outgoing.receivers, [2]);
        assert_eq!(
            envelope::decode_host(&outgoing.bytes),
            Some((HOST_KIND_RELAY, data.as_slice()))
        );
        assert!(outgoing.bytes.len() <= 1000, "fits Mumble plugin data");
        // Larger, or no fallback allowed: relay-unavailable.
        assert_eq!(
            io.relay_send_to(
                "dice",
                Target::Direct(2),
                vec![7; MAX_FALLBACK + 1],
                true,
                true
            ),
            Err(SendError::Unavailable)
        );
        assert_eq!(
            io.relay_send_to("dice", Target::Direct(2), vec![7], false, true),
            Err(SendError::Unavailable)
        );
    }

    #[test]
    fn switched_off_moves_traffic_off_the_relay_at_once() {
        let (io, mut rx) = io();
        available(&io);
        io.relay().switch_on = false;
        assert_eq!(
            io.relay_send_to("dice", Target::Direct(2), vec![1; 2000], true, true),
            Err(SendError::Unavailable)
        );
        io.relay_send_to("dice", Target::Direct(2), vec![1; 10], true, true)
            .expect("fallback");
        assert!(matches!(
            drain(&mut rx).as_slice(),
            [ServiceEvent::PeerSend { .. }]
        ));
    }

    #[test]
    fn without_the_permission_sends_behave_as_switched_off() {
        let (io, mut rx) = io();
        available(&io);
        assert_eq!(
            io.relay_send_to("dice", Target::Channel, vec![1; 2000], true, false),
            Err(SendError::Unavailable)
        );
        io.relay_send_to("dice", Target::Channel, vec![1; 10], true, false)
            .expect("fallback");
        assert!(matches!(
            drain(&mut rx).as_slice(),
            [ServiceEvent::PeerSend { .. }]
        ));
    }

    /// The fallback receivers and the broadcast's receivers of the drained events.
    fn split(rx: &mut UnboundedReceiver<ServiceEvent>) -> (Vec<u32>, Vec<u32>) {
        let mut fallback = Vec::new();
        let mut broadcast = Vec::new();
        for event in drain(rx) {
            match event {
                ServiceEvent::PeerSend { outgoing, .. } => fallback.extend(outgoing.receivers),
                ServiceEvent::RelayBroadcast {
                    server: SERVER,
                    api: (1, 1),
                    receivers,
                    ..
                } => broadcast.extend(receivers),
                other => panic!("{other:?}"),
            }
        }
        (fallback, broadcast)
    }

    #[test]
    fn channel_sends_go_through_the_broadcast_and_fall_back_for_members_outside() {
        let (io, mut rx) = io();
        available(&io);
        // Nobody joined yet: everyone with the plugin gets the fallback too.
        io.relay_send_to("dice", Target::Channel, vec![1; 10], true, true)
            .expect("sent");
        assert_eq!(split(&mut rx), (vec![2, 4], vec![2, 4]));
        // Only the member outside the broadcast gets the fallback.
        io.relay().joined.insert(SERVER, vec![2]);
        io.relay_send_to("dice", Target::Channel, vec![1; MAX_FALLBACK], true, true)
            .expect("sent");
        assert_eq!(split(&mut rx), (vec![4], vec![2, 4]));
        // Too large for Mumble plugin data, or no fallback allowed: the broadcast only.
        io.relay_send_to(
            "dice",
            Target::Channel,
            vec![1; MAX_FALLBACK + 1],
            true,
            true,
        )
        .expect("sent");
        assert_eq!(split(&mut rx), (vec![], vec![2, 4]));
        io.relay_send_to("dice", Target::Channel, vec![1; 10], false, true)
            .expect("sent");
        assert_eq!(split(&mut rx), (vec![], vec![2, 4]));
        // Everyone joined: no fallback.
        io.relay().joined.insert(SERVER, vec![2, 4]);
        io.relay_send_to("dice", Target::Channel, vec![1; 10], true, true)
            .expect("sent");
        assert_eq!(split(&mut rx), (vec![], vec![2, 4]));
        assert_eq!(
            io.relay().usage.get("dice"),
            Some(&(31 + 2 * MAX_FALLBACK as u64))
        );
    }

    #[test]
    fn relay_only_channel_sends_wait_for_the_broadcast_or_fail() {
        let (io, mut rx) = io();
        available(&io);
        // The first send starts the broadcast; it and the next ones wait in its start queue.
        for _ in 0..8 {
            io.relay_send_to("dice", Target::Channel, vec![1; 10], false, true)
                .expect("queued");
        }
        assert_eq!(
            io.relay_send_to("dice", Target::Channel, vec![1; 10], false, true),
            Err(SendError::Error(Error::RateLimited))
        );
        assert_eq!(split(&mut rx), (vec![], [2, 4].repeat(8)));
        // The broadcast could not start: relay-only messages cannot arrive.
        io.relay().broadcast.insert(
            SERVER,
            Broadcast::reported(
                BroadcastState::Down {
                    retry_in: std::time::Duration::from_secs(30),
                },
                Instant::now(),
            ),
        );
        assert_eq!(
            io.relay_send_to("dice", Target::Channel, vec![1; 10], false, true),
            Err(SendError::Unavailable)
        );
        // With the fallback everyone got it through Mumble.
        io.relay_send_to("dice", Target::Channel, vec![1; 10], true, true)
            .expect("fallback");
        assert_eq!(split(&mut rx), (vec![2, 4], vec![2, 4]));
    }

    #[test]
    fn channel_sends_share_the_plugin_rate() {
        let (io, mut rx) = io();
        available(&io);
        io.relay().joined.insert(SERVER, vec![2, 4]);
        io.relay().broadcast.insert(SERVER, Broadcast::Live);
        for _ in 0..16 {
            io.relay_send_to("dice", Target::Channel, vec![0; MAX_MESSAGE], false, true)
                .expect("within the burst");
        }
        assert_eq!(
            io.relay_send_to("dice", Target::Channel, vec![0; MAX_MESSAGE], false, true),
            Err(SendError::Error(Error::RateLimited))
        );
        assert_eq!(drain(&mut rx).len(), 16);
    }

    #[test]
    fn without_the_relay_channel_sends_fall_back_when_they_fit() {
        let (io, mut rx) = io();
        io.relay_send_to("dice", Target::Channel, vec![1; 10], true, true)
            .expect("fallback");
        assert_eq!(split(&mut rx), (vec![2, 4], vec![]));
        assert_eq!(
            io.relay_send_to("dice", Target::Channel, vec![1; 2000], true, true),
            Err(SendError::Unavailable)
        );
    }

    #[test]
    fn a_lost_connection_forgets_the_joined_receivers() {
        let (io, _rx) = io();
        io.relay().joined.insert(SERVER, vec![2]);
        io.relay_forget_server(SERVER);
        assert!(io.relay().joined.is_empty());
    }
}
