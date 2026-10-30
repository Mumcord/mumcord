//! What the tray shows (FR-3.132): the connection, channels and own voice of every server, kept
//! from engine events, plus the unread summary and Do Not Disturb state the app reports.

use std::collections::{HashMap, HashSet};

use mumcord_engine::{ChannelView, ConnState, EngineEvent, UserView};

use crate::commands::tray::TrayAttention;
use crate::commands::types::ServerId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conn {
    Connecting,
    Reconnecting,
    Connected,
    /// The connection ended with an error; the server stays listed until it connects again or
    /// is disconnected on request.
    Lost,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channel {
    pub parent: Option<u32>,
    pub name: String,
    pub position: i32,
    pub enter: bool,
}

impl From<&ChannelView> for Channel {
    fn from(c: &ChannelView) -> Self {
        Self {
            parent: c.parent_id,
            name: c.name.clone(),
            position: c.position,
            enter: c.permissions.enter,
        }
    }
}

/// One server the user is connected (or connecting) to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub id: ServerId,
    pub name: String,
    pub conn: Conn,
    pub self_session: Option<u32>,
    pub channels: HashMap<u32, Channel>,
    /// The channel of every user, by session.
    pub users: HashMap<u32, u32>,
    /// Sessions currently talking.
    pub talking: HashSet<u32>,
    pub muted: bool,
    pub deafened: bool,
}

impl Server {
    fn new(id: ServerId, name: String, conn: Conn) -> Self {
        Self {
            id,
            name,
            conn,
            self_session: None,
            channels: HashMap::new(),
            users: HashMap::new(),
            talking: HashSet::new(),
            muted: false,
            deafened: false,
        }
    }

    /// The channel the user is in.
    pub fn own_channel(&self) -> Option<u32> {
        self.users.get(&self.self_session?).copied()
    }

    /// Whether the user or someone in the user's channel is talking.
    fn channel_talking(&self) -> bool {
        let own = self.own_channel();
        own.is_some()
            && self
                .talking
                .iter()
                .any(|session| self.users.get(session).copied() == own)
    }

    fn take_voice(&mut self, me: &UserView) {
        self.muted = me.self_mute;
        self.deafened = me.self_deaf;
    }

    fn forget_session(&mut self) {
        *self = Self::new(self.id, std::mem::take(&mut self.name), self.conn);
    }
}

/// Do Not Disturb as the tray shows it (FR-3.111, FR-3.112).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dnd {
    /// The Mumcord switch.
    pub manual: bool,
    /// The operating system's Do Not Disturb, when "Follow system" is on.
    pub system: bool,
}

impl Dnd {
    pub fn active(self) -> bool {
        self.manual || self.system
    }
}

/// The state shown by the icon itself; badges are drawn on top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Disconnected,
    Connecting,
    Idle,
    Talking,
    Muted,
}

/// Everything the tray icon draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconState {
    pub look: Look,
    pub unread: bool,
    pub mentions: u32,
    pub dnd: bool,
}

/// What an update changed, so only that part of the tray is redrawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Change {
    None,
    Icon,
    Menu,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrayState {
    /// In connect order.
    pub servers: Vec<Server>,
    pub voice: Option<ServerId>,
    pub dnd: Dnd,
    pub attention: TrayAttention,
}

impl TrayState {
    /// Applies an engine event of `server_id`; `name` looks up the server's name when it first
    /// shows up.
    pub fn apply(
        &mut self,
        server_id: ServerId,
        event: &EngineEvent,
        name: impl FnOnce() -> String,
    ) -> Change {
        if let EngineEvent::StateChanged(state) = event {
            self.set_conn(server_id, state, name);
            return Change::Menu;
        }
        if let EngineEvent::VoiceServerChanged(voice) = event {
            self.voice = voice.and_then(|id| ServerId::try_from(id).ok());
            return Change::Menu;
        }
        let look = self.look();
        let Some(server) = self.servers.iter_mut().find(|s| s.id == server_id) else {
            return Change::None;
        };
        match event {
            EngineEvent::ServerSynced(snapshot) => {
                server.self_session = Some(snapshot.self_session);
                server.channels = snapshot.channels.iter().map(|c| (c.id, c.into())).collect();
                server.users = snapshot
                    .users
                    .iter()
                    .map(|u| (u.session, u.channel_id))
                    .collect();
                if let Some(me) = snapshot
                    .users
                    .iter()
                    .find(|u| u.session == snapshot.self_session)
                {
                    server.take_voice(me);
                }
            }
            EngineEvent::ChannelUpserted(channel) => {
                server.channels.insert(channel.id, channel.into());
            }
            EngineEvent::ChannelsUpserted(channels) => {
                for channel in channels {
                    server.channels.insert(channel.id, channel.into());
                }
            }
            EngineEvent::ChannelRemoved(id) => {
                server.channels.remove(id);
            }
            EngineEvent::UserUpserted(user) => {
                let moved = server.users.insert(user.session, user.channel_id);
                if Some(user.session) == server.self_session {
                    let voice = (server.muted, server.deafened);
                    server.take_voice(user);
                    if voice != (server.muted, server.deafened) {
                        return Change::Menu;
                    }
                }
                if moved == Some(user.channel_id) {
                    return Change::None;
                }
            }
            EngineEvent::UserRemoved(session) => {
                server.talking.remove(session);
                server.users.remove(session);
            }
            EngineEvent::TalkStateChanged { session, talking } => {
                if *talking {
                    server.talking.insert(*session);
                } else {
                    server.talking.remove(session);
                }
                return if self.look() == look {
                    Change::None
                } else {
                    Change::Icon
                };
            }
            _ => return Change::None,
        }
        Change::Menu
    }

    fn set_conn(&mut self, server_id: ServerId, state: &ConnState, name: impl FnOnce() -> String) {
        let conn = match state {
            ConnState::Disconnected { error: None } => None,
            ConnState::Disconnected { error: Some(_) } => Some(Conn::Lost),
            ConnState::Connecting { .. } => Some(Conn::Connecting),
            ConnState::Reconnecting { .. } => Some(Conn::Reconnecting),
            ConnState::Connected { .. } => Some(Conn::Connected),
        };
        let Some(conn) = conn else {
            self.servers.retain(|s| s.id != server_id);
            return;
        };
        match self.servers.iter_mut().find(|s| s.id == server_id) {
            Some(server) => {
                server.conn = conn;
                if conn != Conn::Connected {
                    server.forget_session();
                }
            }
            None => self.servers.push(Server::new(server_id, name(), conn)),
        }
    }

    pub fn connected(&self) -> impl Iterator<Item = &Server> {
        self.servers.iter().filter(|s| s.conn == Conn::Connected)
    }

    fn server(&self, id: Option<ServerId>) -> Option<&Server> {
        let id = id?;
        self.connected().find(|s| s.id == id)
    }

    /// The server mute and deafen apply to: the voice server, else the only connected one (as
    /// in [`mumcord_engine::Engine::set_self_mute`]).
    pub fn voice_target(&self) -> Option<&Server> {
        self.server(self.voice).or_else(|| {
            let mut connected = self.connected();
            match (connected.next(), connected.next()) {
                (Some(only), None) => Some(only),
                _ => None,
            }
        })
    }

    /// The server whose channels the "Voice server and channel" menu lists: the voice server,
    /// else the first connected one.
    pub fn channel_server(&self) -> Option<&Server> {
        self.server(self.voice).or_else(|| self.connected().next())
    }

    pub fn look(&self) -> Look {
        if self.connected().next().is_none() {
            let connecting = self
                .servers
                .iter()
                .any(|s| matches!(s.conn, Conn::Connecting | Conn::Reconnecting));
            return if connecting {
                Look::Connecting
            } else {
                Look::Disconnected
            };
        }
        if self.voice_target().is_some_and(|s| s.muted || s.deafened) {
            Look::Muted
        } else if self.server(self.voice).is_some_and(Server::channel_talking) {
            Look::Talking
        } else {
            Look::Idle
        }
    }

    pub fn icon(&self) -> IconState {
        IconState {
            look: self.look(),
            unread: self.attention.unread > 0,
            mentions: self.attention.mentions,
            dnd: self.dnd.active(),
        }
    }
}

#[cfg(test)]
pub mod fixtures {
    use mumcord_engine::{ChannelPermissions, ServerInfoView, Snapshot};

    use super::*;

    pub fn user(session: u32, channel_id: u32) -> UserView {
        UserView {
            session,
            channel_id,
            name: format!("user{session}"),
            cert_hash: None,
            registered: false,
            self_mute: false,
            self_deaf: false,
            server_mute: false,
            server_deaf: false,
            suppressed: false,
            listening_channel_ids: vec![],
            has_avatar: false,
            avatar_hash: None,
            has_comment: false,
            volume: 100,
            local_mute: false,
            mumcord_features: None,
        }
    }

    pub fn channel(id: u32, parent_id: Option<u32>, name: &str, position: i32) -> ChannelView {
        ChannelView {
            id,
            parent_id,
            name: name.into(),
            position,
            permissions: ChannelPermissions {
                enter: true,
                text_message: true,
                listen: true,
                register_self: false,
            },
            description: None,
            has_description: false,
        }
    }

    pub fn snapshot(
        self_session: u32,
        channels: Vec<ChannelView>,
        users: Vec<UserView>,
    ) -> Snapshot {
        Snapshot {
            server_id: 1,
            self_session,
            info: ServerInfoView {
                version: "1.5".into(),
                follow_supported: true,
                welcome_text: None,
                default_channel_id: None,
                max_message_length: 5000,
                max_image_message_length: 0,
                allow_html: true,
            },
            channels,
            users,
            followed_channel_ids: vec![],
            unread: vec![],
            read_only: false,
            rejoin_channel_id: None,
        }
    }

    /// A state connected to `id` (named "S<id>") and synced with the user as session 1 in
    /// channel 1 of a Root / Lobby tree.
    pub fn synced(state: &mut TrayState, id: ServerId) {
        state.apply(
            id,
            &EngineEvent::StateChanged(ConnState::Connected {
                server_id: id.into(),
            }),
            || format!("S{id}"),
        );
        let channels = vec![channel(0, None, "Root", 0), channel(1, Some(0), "Lobby", 0)];
        state.apply(
            id,
            &EngineEvent::ServerSynced(Box::new(snapshot(1, channels, vec![user(1, 1)]))),
            String::new,
        );
    }
}

#[cfg(test)]
mod tests {
    use mumcord_engine::{EngineError, ErrorKind};

    use super::fixtures::{synced, user};
    use super::*;

    fn talk(state: &mut TrayState, id: ServerId, session: u32, talking: bool) -> Change {
        state.apply(
            id,
            &EngineEvent::TalkStateChanged { session, talking },
            String::new,
        )
    }

    #[test]
    fn servers_are_listed_in_connect_order_with_their_state() {
        let mut state = TrayState::default();
        synced(&mut state, 2);
        let connecting = ConnState::Connecting { server_id: 5 };
        assert_eq!(
            state.apply(5, &EngineEvent::StateChanged(connecting), || "Five".into()),
            Change::Menu
        );
        let ids: Vec<_> = state
            .servers
            .iter()
            .map(|s| (s.id, s.conn, s.name.as_str()))
            .collect();
        assert_eq!(
            ids,
            vec![(2, Conn::Connected, "S2"), (5, Conn::Connecting, "Five")]
        );
    }

    #[test]
    fn a_dropped_server_stays_listed_and_a_closed_one_goes() {
        let mut state = TrayState::default();
        synced(&mut state, 1);
        let error = EngineError::new(ErrorKind::ServerUnreachable, "gone");
        state.apply(
            1,
            &EngineEvent::StateChanged(ConnState::Disconnected { error: Some(error) }),
            String::new,
        );
        assert_eq!(state.servers[0].conn, Conn::Lost);
        assert!(state.servers[0].channels.is_empty());
        assert_eq!(state.look(), Look::Disconnected);
        state.apply(
            1,
            &EngineEvent::StateChanged(ConnState::Disconnected { error: None }),
            String::new,
        );
        assert!(state.servers.is_empty());
    }

    #[test]
    fn talking_in_the_own_channel_rings_and_elsewhere_does_not() {
        let mut state = TrayState::default();
        synced(&mut state, 1);
        state.apply(1, &EngineEvent::VoiceServerChanged(Some(1)), String::new);
        state.apply(1, &EngineEvent::UserUpserted(user(7, 1)), String::new);
        state.apply(1, &EngineEvent::UserUpserted(user(8, 0)), String::new);
        assert_eq!(talk(&mut state, 1, 8, true), Change::None);
        assert_eq!(state.look(), Look::Idle);
        assert_eq!(talk(&mut state, 1, 7, true), Change::Icon);
        assert_eq!(state.look(), Look::Talking);
        assert_eq!(talk(&mut state, 1, 1, true), Change::None);
        state.apply(1, &EngineEvent::UserRemoved(7), String::new);
        talk(&mut state, 1, 1, false);
        assert_eq!(state.look(), Look::Idle);
    }

    #[test]
    fn talking_on_a_read_only_server_does_not_ring() {
        let mut state = TrayState::default();
        synced(&mut state, 1);
        synced(&mut state, 2);
        talk(&mut state, 2, 1, true);
        assert_eq!(state.look(), Look::Idle);
    }

    #[test]
    fn mute_follows_the_voice_server_or_the_only_server() {
        let mut state = TrayState::default();
        synced(&mut state, 1);
        let mut me = user(1, 1);
        me.self_mute = true;
        assert_eq!(
            state.apply(1, &EngineEvent::UserUpserted(me.clone()), String::new),
            Change::Menu
        );
        assert_eq!(state.voice_target().map(|s| s.id), Some(1));
        assert_eq!(state.look(), Look::Muted);
        synced(&mut state, 2);
        assert!(state.voice_target().is_none());
        assert_eq!(state.look(), Look::Idle);
        state.apply(2, &EngineEvent::VoiceServerChanged(Some(1)), String::new);
        assert_eq!(state.look(), Look::Muted);
        assert_eq!(state.channel_server().map(|s| s.id), Some(1));
    }

    #[test]
    fn moving_users_change_the_menu_but_repeats_do_not() {
        let mut state = TrayState::default();
        synced(&mut state, 1);
        assert_eq!(
            state.apply(1, &EngineEvent::UserUpserted(user(4, 1)), String::new),
            Change::Menu
        );
        assert_eq!(
            state.apply(1, &EngineEvent::UserUpserted(user(4, 1)), String::new),
            Change::None
        );
        assert_eq!(
            state.apply(9, &EngineEvent::UserUpserted(user(4, 1)), String::new),
            Change::None
        );
    }

    #[test]
    fn connecting_shows_until_a_server_is_connected() {
        let mut state = TrayState::default();
        assert_eq!(state.look(), Look::Disconnected);
        state.apply(
            3,
            &EngineEvent::StateChanged(ConnState::Connecting { server_id: 3 }),
            String::new,
        );
        assert_eq!(state.look(), Look::Connecting);
    }

    #[test]
    fn icon_carries_unread_mentions_and_dnd() {
        let mut state = TrayState::default();
        state.attention.unread = 3;
        state.attention.mentions = 2;
        state.dnd.system = true;
        let icon = state.icon();
        assert!(icon.unread && icon.dnd);
        assert_eq!(icon.mentions, 2);
    }
}
