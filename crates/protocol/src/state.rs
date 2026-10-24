//! The client's view of the server: channel tree, connected users and server settings, built
//! from the deltas in `ChannelState`/`UserState`/`*Remove` messages.

use std::collections::{BTreeMap, BTreeSet};

use crate::proto;
use crate::version::MumbleVersion;

/// One channel. `parent` is `None` only for the root channel (id 0).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Channel {
    pub id: u32,
    pub parent: Option<u32>,
    pub name: String,
    /// HTML description (untrusted). May be absent while only `description_hash` is known;
    /// request it with `Handle::request_blobs`.
    pub description: Option<String>,
    pub description_hash: Option<Vec<u8>>,
    pub links: BTreeSet<u32>,
    pub temporary: bool,
    pub position: i32,
    pub max_users: Option<u32>,
    pub is_enter_restricted: bool,
    /// Whether this user may enter (only sent for enter-restricted channels).
    pub can_enter: Option<bool>,
}

/// One connected user, keyed by session id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct User {
    pub session: u32,
    pub name: String,
    /// Registration id; `None` for unregistered users.
    pub user_id: Option<u32>,
    pub channel_id: u32,
    /// Server-side mute/deafen by an admin.
    pub mute: bool,
    pub deaf: bool,
    pub suppress: bool,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub priority_speaker: bool,
    pub recording: bool,
    /// HTML comment (untrusted); may be absent while only `comment_hash` is known.
    pub comment: Option<String>,
    pub comment_hash: Option<Vec<u8>>,
    /// Avatar image bytes (untrusted); may be absent while only `texture_hash` is known.
    pub texture: Option<Vec<u8>>,
    pub texture_hash: Option<Vec<u8>>,
    /// SHA-1 hex of the user's certificate; `None` for users without one.
    pub cert_hash: Option<String>,
    /// Channels this user listens to (follows).
    pub listening: BTreeSet<u32>,
    /// Listener volume factors per channel (1.0 = unchanged). Servers only send these for the
    /// own user.
    pub listening_volume: BTreeMap<u32, f32>,
}

/// UDP crypto parameters, passed through untouched for the voice transport.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct CryptSetup {
    pub key: Option<Vec<u8>>,
    pub client_nonce: Option<Vec<u8>>,
    pub server_nonce: Option<Vec<u8>>,
}

impl std::fmt::Debug for CryptSetup {
    // Key material stays out of logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CryptSetup")
            .field("has_key", &self.key.is_some())
            .field("has_client_nonce", &self.client_nonce.is_some())
            .field("has_server_nonce", &self.server_nonce.is_some())
            .finish()
    }
}

impl From<proto::CryptSetup> for CryptSetup {
    fn from(m: proto::CryptSetup) -> Self {
        Self {
            key: m.key,
            client_nonce: m.client_nonce,
            server_nonce: m.server_nonce,
        }
    }
}

impl From<CryptSetup> for proto::CryptSetup {
    fn from(c: CryptSetup) -> Self {
        Self {
            key: c.key,
            client_nonce: c.client_nonce,
            server_nonce: c.server_nonce,
        }
    }
}

/// Server facts gathered from `Version`, `ServerSync` and `ServerConfig`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerInfo {
    pub version: Option<MumbleVersion>,
    pub release: Option<String>,
    pub os: Option<String>,
    pub max_bandwidth: Option<u32>,
    /// Welcome text HTML (untrusted).
    pub welcome_text: Option<String>,
    pub allow_html: Option<bool>,
    /// Maximum text message length in characters (0 = unlimited).
    pub message_length: Option<u32>,
    /// Maximum length of a message containing images (0 = unlimited).
    pub image_message_length: Option<u32>,
    pub max_users: Option<u32>,
    pub recording_allowed: Option<bool>,
}

impl ServerInfo {
    /// Whether the server supports following channels. Unknown version counts as unsupported.
    pub fn supports_listening(&self) -> bool {
        self.version.is_some_and(MumbleVersion::supports_listening)
    }

    pub fn supports_listener_volume(&self) -> bool {
        self.version
            .is_some_and(MumbleVersion::supports_listener_volume)
    }
}

/// Everything known about the connected server.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerState {
    pub channels: BTreeMap<u32, Channel>,
    pub users: BTreeMap<u32, User>,
    /// Own session id, known after `ServerSync`.
    pub session: Option<u32>,
    pub server: ServerInfo,
    /// Latest UDP crypto parameters from the server.
    pub crypt: Option<CryptSetup>,
    /// Effective permissions per channel, see [`crate::permissions`].
    pub permissions: BTreeMap<u32, u32>,
}

/// What a `UserState` did to the user list.
#[derive(Debug, Clone, PartialEq)]
pub struct UserChange {
    pub user: User,
    /// `true` if this session was not known before.
    pub joined: bool,
    /// Channel before the update, if the user moved.
    pub moved_from: Option<u32>,
    /// Session that caused the change (e.g. an admin moving someone).
    pub actor: Option<u32>,
}

impl ServerState {
    /// The own user, once the session is known.
    pub fn own_user(&self) -> Option<&User> {
        self.session.and_then(|s| self.users.get(&s))
    }

    /// Direct children of a channel, sorted by position then name (Mumble's order).
    pub fn children(&self, parent: u32) -> Vec<&Channel> {
        let mut out: Vec<&Channel> = self
            .channels
            .values()
            .filter(|c| c.parent == Some(parent))
            .collect();
        out.sort_by(|a, b| {
            a.position
                .cmp(&b.position)
                .then_with(|| a.name.cmp(&b.name))
        });
        out
    }

    /// Users whose voice channel is `channel`.
    pub fn users_in(&self, channel: u32) -> impl Iterator<Item = &User> {
        self.users.values().filter(move |u| u.channel_id == channel)
    }

    /// Users listening to `channel`.
    pub fn listeners_of(&self, channel: u32) -> impl Iterator<Item = &User> {
        self.users
            .values()
            .filter(move |u| u.listening.contains(&channel))
    }

    /// Finds a direct child channel by name.
    pub fn child_by_name(&self, parent: u32, name: &str) -> Option<&Channel> {
        self.channels
            .values()
            .find(|c| c.parent == Some(parent) && c.name == name)
    }

    pub fn apply_channel_state(&mut self, m: proto::ChannelState) -> Option<&Channel> {
        let id = m.channel_id?;
        let ch = self.channels.entry(id).or_insert_with(|| Channel {
            id,
            ..Default::default()
        });
        if let Some(parent) = m.parent {
            // The root has no parent; servers never send one for it.
            ch.parent = (id != 0).then_some(parent);
        }
        if let Some(name) = m.name {
            ch.name = name;
        }
        if let Some(hash) = m.description_hash {
            // A new hash invalidates the cached text until it is requested again.
            if ch.description_hash.as_ref() != Some(&hash) {
                ch.description = None;
            }
            ch.description_hash = Some(hash);
        }
        if let Some(description) = m.description {
            ch.description = Some(description);
        }
        if !m.links.is_empty() {
            ch.links = m.links.into_iter().collect();
        }
        for l in m.links_add {
            ch.links.insert(l);
        }
        for l in &m.links_remove {
            ch.links.remove(l);
        }
        if let Some(t) = m.temporary {
            ch.temporary = t;
        }
        if let Some(p) = m.position {
            ch.position = p;
        }
        if let Some(max) = m.max_users {
            ch.max_users = Some(max);
        }
        if let Some(r) = m.is_enter_restricted {
            ch.is_enter_restricted = r;
        }
        if let Some(c) = m.can_enter {
            ch.can_enter = Some(c);
        }
        self.channels.get(&id)
    }

    /// Removes a channel; returns it if it was known.
    pub fn apply_channel_remove(&mut self, id: u32) -> Option<Channel> {
        let removed = self.channels.remove(&id)?;
        for ch in self.channels.values_mut() {
            ch.links.remove(&id);
        }
        for user in self.users.values_mut() {
            user.listening.remove(&id);
            user.listening_volume.remove(&id);
        }
        self.permissions.remove(&id);
        Some(removed)
    }

    pub fn apply_user_state(&mut self, m: proto::UserState) -> Option<UserChange> {
        let session = m.session?;
        let joined = !self.users.contains_key(&session);
        let user = self.users.entry(session).or_insert_with(|| User {
            session,
            ..Default::default()
        });
        let mut moved_from = None;
        if let Some(name) = m.name {
            user.name = name;
        }
        if let Some(id) = m.user_id {
            // Mumble uses u32::MAX ("-1") to clear a registration.
            user.user_id = (id != u32::MAX).then_some(id);
        }
        if let Some(c) = m.channel_id {
            if !joined && user.channel_id != c {
                moved_from = Some(user.channel_id);
            }
            user.channel_id = c;
        }
        set(&mut user.mute, m.mute);
        set(&mut user.deaf, m.deaf);
        set(&mut user.suppress, m.suppress);
        set(&mut user.self_mute, m.self_mute);
        set(&mut user.self_deaf, m.self_deaf);
        set(&mut user.priority_speaker, m.priority_speaker);
        set(&mut user.recording, m.recording);
        // Hashes first: a changed hash drops the cached blob unless the blob itself is included.
        if let Some(hash) = m.comment_hash {
            if user.comment_hash.as_ref() != Some(&hash) {
                user.comment = None;
            }
            user.comment_hash = Some(hash);
        }
        if let Some(comment) = m.comment {
            // An empty comment means "removed".
            user.comment = (!comment.is_empty()).then_some(comment);
            if user.comment.is_none() {
                user.comment_hash = None;
            }
        }
        if let Some(hash) = m.texture_hash {
            if user.texture_hash.as_ref() != Some(&hash) {
                user.texture = None;
            }
            user.texture_hash = Some(hash);
        }
        if let Some(texture) = m.texture {
            user.texture = (!texture.is_empty()).then_some(texture);
            if user.texture.is_none() {
                user.texture_hash = None;
            }
        }
        if let Some(hash) = m.hash {
            user.cert_hash = (!hash.is_empty()).then_some(hash);
        }
        for c in m.listening_channel_add {
            user.listening.insert(c);
        }
        for c in &m.listening_channel_remove {
            user.listening.remove(c);
            user.listening_volume.remove(c);
        }
        for adj in m.listening_volume_adjustment {
            if let (Some(c), Some(v)) = (adj.listening_channel, adj.volume_adjustment) {
                user.listening_volume.insert(c, v);
            }
        }
        Some(UserChange {
            user: user.clone(),
            joined,
            moved_from,
            actor: m.actor,
        })
    }

    pub fn apply_user_remove(&mut self, session: u32) -> Option<User> {
        self.users.remove(&session)
    }

    pub(crate) fn apply_server_sync(&mut self, m: &proto::ServerSync) {
        if let Some(s) = m.session {
            self.session = Some(s);
        }
        if m.max_bandwidth.is_some() {
            self.server.max_bandwidth = m.max_bandwidth;
        }
        if m.welcome_text.is_some() {
            self.server.welcome_text.clone_from(&m.welcome_text);
        }
        if let Some(p) = m.permissions {
            // ServerSync carries the root channel's permissions (truncated to 32 bits on the wire).
            self.permissions.insert(0, p as u32);
        }
    }

    pub(crate) fn apply_server_config(&mut self, m: proto::ServerConfig) {
        let s = &mut self.server;
        if m.max_bandwidth.is_some() {
            s.max_bandwidth = m.max_bandwidth;
        }
        if m.welcome_text.is_some() {
            s.welcome_text = m.welcome_text;
        }
        if m.allow_html.is_some() {
            s.allow_html = m.allow_html;
        }
        if m.message_length.is_some() {
            s.message_length = m.message_length;
        }
        if m.image_message_length.is_some() {
            s.image_message_length = m.image_message_length;
        }
        if m.max_users.is_some() {
            s.max_users = m.max_users;
        }
        if m.recording_allowed.is_some() {
            s.recording_allowed = m.recording_allowed;
        }
    }

    pub(crate) fn apply_permission_query(&mut self, m: &proto::PermissionQuery) {
        if m.flush == Some(true) {
            self.permissions.clear();
        }
        if let (Some(c), Some(p)) = (m.channel_id, m.permissions) {
            self.permissions.insert(c, p);
        }
    }
}

fn set(target: &mut bool, value: Option<bool>) {
    if let Some(v) = value {
        *target = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(id: u32, parent: Option<u32>, name: &str) -> proto::ChannelState {
        proto::ChannelState {
            channel_id: Some(id),
            parent,
            name: Some(name.into()),
            ..Default::default()
        }
    }

    fn tree() -> ServerState {
        let mut s = ServerState::default();
        s.apply_channel_state(channel(0, None, "Root"));
        s.apply_channel_state(proto::ChannelState {
            position: Some(1),
            ..channel(1, Some(0), "Lobby")
        });
        s.apply_channel_state(channel(2, Some(0), "Raid"));
        s.apply_channel_state(channel(3, Some(0), "AFK"));
        s
    }

    #[test]
    fn builds_tree_in_mumble_order() {
        let s = tree();
        let names: Vec<&str> = s.children(0).iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["AFK", "Raid", "Lobby"]);
        assert_eq!(s.channels[&0].parent, None);
        assert_eq!(s.child_by_name(0, "Raid").map(|c| c.id), Some(2));
    }

    #[test]
    fn partial_channel_update_keeps_fields() {
        let mut s = tree();
        s.apply_channel_state(proto::ChannelState {
            channel_id: Some(2),
            description: Some("raid night".into()),
            ..Default::default()
        });
        assert_eq!(s.channels[&2].name, "Raid");
        assert_eq!(s.channels[&2].description.as_deref(), Some("raid night"));
    }

    #[test]
    fn user_join_move_and_remove() {
        let mut s = tree();
        let c = s
            .apply_user_state(proto::UserState {
                session: Some(5),
                name: Some("alice".into()),
                channel_id: Some(1),
                hash: Some("abc".into()),
                ..Default::default()
            })
            .expect("change");
        assert!(c.joined);
        assert_eq!(c.moved_from, None);

        let c = s
            .apply_user_state(proto::UserState {
                session: Some(5),
                channel_id: Some(2),
                self_mute: Some(true),
                ..Default::default()
            })
            .expect("change");
        assert!(!c.joined);
        assert_eq!(c.moved_from, Some(1));
        assert_eq!(c.user.name, "alice");
        assert!(c.user.self_mute);
        assert_eq!(c.user.cert_hash.as_deref(), Some("abc"));
        assert_eq!(s.users_in(2).count(), 1);

        assert!(s.apply_user_remove(5).is_some());
        assert!(s.users.is_empty());
    }

    #[test]
    fn listening_add_volume_remove() {
        let mut s = tree();
        s.apply_user_state(proto::UserState {
            session: Some(1),
            channel_id: Some(1),
            listening_channel_add: vec![2, 3],
            listening_volume_adjustment: vec![proto::user_state::VolumeAdjustment {
                listening_channel: Some(2),
                volume_adjustment: Some(0.0),
            }],
            ..Default::default()
        });
        assert_eq!(s.listeners_of(2).count(), 1);
        assert_eq!(s.users[&1].listening_volume.get(&2), Some(&0.0));

        s.apply_user_state(proto::UserState {
            session: Some(1),
            listening_channel_remove: vec![2],
            ..Default::default()
        });
        assert_eq!(s.listeners_of(2).count(), 0);
        assert!(s.users[&1].listening_volume.is_empty());

        s.apply_channel_remove(3);
        assert!(s.users[&1].listening.is_empty());
        assert!(!s.channels.contains_key(&3));
    }

    #[test]
    fn blobs_and_registration() {
        let mut s = tree();
        s.apply_user_state(proto::UserState {
            session: Some(1),
            comment: Some("hi".into()),
            texture: Some(vec![1, 2]),
            ..Default::default()
        });
        // A new hash invalidates the cached blob until it is requested again.
        s.apply_user_state(proto::UserState {
            session: Some(1),
            comment_hash: Some(vec![9]),
            texture: Some(vec![]),
            user_id: Some(7),
            ..Default::default()
        });
        let u = &s.users[&1];
        assert_eq!(u.comment, None);
        assert_eq!(u.texture, None);
        assert_eq!(u.user_id, Some(7));
    }

    #[test]
    fn sync_config_and_permissions() {
        let mut s = ServerState::default();
        s.apply_server_sync(&proto::ServerSync {
            session: Some(3),
            max_bandwidth: Some(72000),
            welcome_text: Some("hello".into()),
            permissions: Some(0x200),
        });
        s.apply_server_config(proto::ServerConfig {
            message_length: Some(5000),
            allow_html: Some(true),
            ..Default::default()
        });
        s.apply_permission_query(&proto::PermissionQuery {
            channel_id: Some(2),
            permissions: Some(0x4),
            flush: None,
        });
        assert_eq!(s.session, Some(3));
        assert_eq!(s.server.max_bandwidth, Some(72000));
        assert_eq!(s.server.message_length, Some(5000));
        assert_eq!(s.permissions.get(&0), Some(&0x200));
        assert_eq!(s.permissions.get(&2), Some(&0x4));
        assert!(!s.server.supports_listening());
    }
}
