//! Friends and their online state (FR-1.64).
//!
//! Friends are kept in the store by certificate hash. The [`FriendHub`] watches every emitted
//! engine event to know who is online on the connected server and emits
//! [`EngineEvent::FriendsChanged`] whenever what the friends list shows changes. Last-seen
//! times and new names are written to the store on a blocking task, because events are
//! emitted from places that may hold the store lock.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mumcord_store::{Friend, ServerId, Store, UnixMillis};
use tokio::runtime::Handle as Runtime;

use crate::chat::now_millis;
use crate::session::{ConnState, EngineEvent, UserView, lock};

/// Where a friend is online right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendPresence {
    pub server_id: ServerId,
    pub session: u32,
    pub channel_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendView {
    pub cert_hash: String,
    /// Current name while online, otherwise the name the friend was last seen with.
    pub name: String,
    pub added_at: UnixMillis,
    /// When the friend was last seen online; `None` if never seen since adding.
    pub last_seen_at: Option<UnixMillis>,
    /// `None` while offline.
    pub online: Option<FriendPresence>,
}

/// A person with a certificate on the connected server.
#[derive(Debug, Clone)]
struct Present {
    cert_hash: String,
    name: String,
    channel_id: u32,
}

/// "Seen under this name at this time" for the store.
type Seen = (String, String, UnixMillis);

/// Presence bookkeeping without store or runtime, so it can be tested directly.
#[derive(Debug, Default)]
struct Tracker {
    friends: HashMap<String, Friend>,
    server_id: Option<ServerId>,
    present: HashMap<u32, Present>,
    shown: Vec<FriendView>,
}

impl Tracker {
    fn new(friends: Vec<Friend>) -> Self {
        let mut tracker = Self {
            friends: friends
                .into_iter()
                .map(|f| (f.cert_hash.clone(), f))
                .collect(),
            ..Self::default()
        };
        tracker.shown = tracker.views();
        tracker
    }

    /// Online friends first, then by name.
    fn views(&self) -> Vec<FriendView> {
        let mut views: Vec<FriendView> = self
            .friends
            .values()
            .map(|f| {
                let live = self.server_id.and_then(|server_id| {
                    self.present
                        .iter()
                        .filter(|(_, p)| p.cert_hash == f.cert_hash)
                        .min_by_key(|(session, _)| **session)
                        .map(|(session, p)| (server_id, *session, p))
                });
                FriendView {
                    cert_hash: f.cert_hash.clone(),
                    name: live.map_or_else(|| f.name.clone(), |(_, _, p)| p.name.clone()),
                    added_at: f.added_at,
                    last_seen_at: f.last_seen_at,
                    online: live.map(|(server_id, session, p)| FriendPresence {
                        server_id,
                        session,
                        channel_id: p.channel_id,
                    }),
                }
            })
            .collect();
        views.sort_by(|a, b| {
            b.online
                .is_some()
                .cmp(&a.online.is_some())
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.cert_hash.cmp(&b.cert_hash))
        });
        views
    }

    fn is_online(&self, cert_hash: &str) -> bool {
        self.present.values().any(|p| p.cert_hash == cert_hash)
    }

    /// Notes that a friend was seen; returns what to store.
    fn seen(&mut self, cert_hash: &str, name: &str, now: UnixMillis) -> Option<Seen> {
        let friend = self.friends.get_mut(cert_hash)?;
        friend.name = name.to_owned();
        friend.last_seen_at = Some(now);
        Some((cert_hash.to_owned(), name.to_owned(), now))
    }

    fn upsert(&mut self, user: &UserView, now: UnixMillis, seen: &mut Vec<Seen>) {
        let Some(cert_hash) = &user.cert_hash else {
            return;
        };
        let before = self.present.insert(
            user.session,
            Present {
                cert_hash: cert_hash.clone(),
                name: user.name.clone(),
                channel_id: user.channel_id,
            },
        );
        let renamed = before.is_none_or(|b| b.name != user.name);
        if renamed {
            seen.extend(self.seen(cert_hash, &user.name, now));
        }
    }

    fn leave(&mut self, session: u32, now: UnixMillis, seen: &mut Vec<Seen>) {
        if let Some(gone) = self.present.remove(&session)
            && !self.is_online(&gone.cert_hash)
        {
            seen.extend(self.seen(&gone.cert_hash, &gone.name, now));
        }
    }

    /// Everyone went offline (the connection ended or is being replaced).
    fn leave_all(&mut self, now: UnixMillis, seen: &mut Vec<Seen>) {
        let sessions: Vec<u32> = self.present.keys().copied().collect();
        for session in sessions {
            self.leave(session, now, seen);
        }
        self.server_id = None;
    }

    /// Updates presence from `event`; returns what to store and the new list if it changed.
    fn observe(
        &mut self,
        event: &EngineEvent,
        now: UnixMillis,
    ) -> (Vec<Seen>, Option<Vec<FriendView>>) {
        let mut seen = Vec::new();
        match event {
            EngineEvent::ServerSynced(snapshot) => {
                self.leave_all(now, &mut seen);
                self.server_id = Some(snapshot.server_id);
                for user in snapshot
                    .users
                    .iter()
                    .filter(|u| u.session != snapshot.self_session)
                {
                    self.upsert(user, now, &mut seen);
                }
            }
            EngineEvent::UserUpserted(user) if self.server_id.is_some() => {
                self.upsert(user, now, &mut seen);
            }
            EngineEvent::UserRemoved(session) => self.leave(*session, now, &mut seen),
            EngineEvent::StateChanged(state) if !matches!(state, ConnState::Connected { .. }) => {
                self.leave_all(now, &mut seen);
            }
            _ => return (seen, None),
        }
        (seen, self.refresh())
    }

    /// The new list if it differs from the one shown last.
    fn refresh(&mut self) -> Option<Vec<FriendView>> {
        let views = self.views();
        (views != self.shown).then(|| {
            self.shown.clone_from(&views);
            views
        })
    }
}

/// Friend presence across the connected server; see the module docs.
pub struct FriendHub {
    store: Arc<Mutex<Store>>,
    rt: Runtime,
    tracker: Mutex<Tracker>,
}

impl FriendHub {
    pub(crate) fn new(store: Arc<Mutex<Store>>, rt: Runtime) -> Arc<Self> {
        let friends = lock(&store).friends().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "reading the friends failed");
            Vec::new()
        });
        Arc::new(Self {
            store,
            rt,
            tracker: Mutex::new(Tracker::new(friends)),
        })
    }

    /// Feeds one engine event; returns the friends list when it changed.
    pub(crate) fn observe(&self, event: &EngineEvent) -> Option<Vec<FriendView>> {
        if matches!(event, EngineEvent::FriendsChanged(_)) {
            return None;
        }
        let (seen, changed) = lock(&self.tracker).observe(event, now_millis());
        self.persist(seen);
        changed
    }

    /// The friends list as shown, online first.
    pub fn views(&self) -> Vec<FriendView> {
        lock(&self.tracker).shown.clone()
    }

    /// A friend was added (already stored); returns the new list.
    pub(crate) fn added(&self, friend: Friend) -> Vec<FriendView> {
        let mut tracker = lock(&self.tracker);
        tracker.friends.insert(friend.cert_hash.clone(), friend);
        tracker.refresh();
        tracker.shown.clone()
    }

    /// A friend was removed (already from the store); returns the new list.
    pub(crate) fn removed(&self, cert_hash: &str) -> Vec<FriendView> {
        let mut tracker = lock(&self.tracker);
        tracker.friends.remove(cert_hash);
        tracker.refresh();
        tracker.shown.clone()
    }

    fn persist(&self, seen: Vec<Seen>) {
        if seen.is_empty() {
            return;
        }
        let store = self.store.clone();
        self.rt.spawn_blocking(move || {
            let store = lock(&store);
            for (cert_hash, name, at) in seen {
                if let Err(e) = store.friend_seen(&cert_hash, &name, at) {
                    tracing::warn!(error = %e, "saving when a friend was last seen failed");
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{ServerInfoView, Snapshot};

    fn friend(hash: &str, name: &str) -> Friend {
        Friend {
            cert_hash: hash.into(),
            name: name.into(),
            added_at: 1,
            last_seen_at: None,
        }
    }

    fn user(session: u32, hash: Option<&str>, name: &str, channel_id: u32) -> UserView {
        UserView {
            session,
            channel_id,
            name: name.into(),
            cert_hash: hash.map(Into::into),
            registered: false,
            self_mute: false,
            self_deaf: false,
            server_mute: false,
            server_deaf: false,
            suppressed: false,
            listening_channel_ids: Vec::new(),
            has_avatar: false,
            avatar_hash: None,
            has_comment: false,
            volume: 100,
            local_mute: false,
            mumcord_features: None,
        }
    }

    fn synced(users: Vec<UserView>) -> EngineEvent {
        EngineEvent::ServerSynced(Box::new(Snapshot {
            server_id: 7,
            self_session: 1,
            info: ServerInfoView {
                version: String::new(),
                follow_supported: true,
                welcome_text: None,
                default_channel_id: None,
                max_message_length: 0,
                max_image_message_length: 0,
                allow_html: false,
            },
            channels: Vec::new(),
            users,
            followed_channel_ids: Vec::new(),
            unread: Vec::new(),
            read_only: false,
            rejoin_channel_id: None,
        }))
    }

    #[test]
    fn friend_goes_online_moves_renames_and_goes_offline() {
        let mut t = Tracker::new(vec![friend("bot", "Bot"), friend("amy", "Amy")]);
        assert!(t.shown.iter().all(|v| v.online.is_none()));

        let (seen, list) = t.observe(
            &synced(vec![
                user(1, Some("me"), "Me", 0),
                user(2, Some("bot"), "Bot", 0),
            ]),
            10,
        );
        assert_eq!(seen, [("bot".into(), "Bot".into(), 10)]);
        let list = list.expect("changed");
        assert_eq!(list[0].cert_hash, "bot", "online first");
        assert_eq!(
            list[0].online,
            Some(FriendPresence {
                server_id: 7,
                session: 2,
                channel_id: 0
            })
        );
        assert!(list[1].online.is_none());

        let (seen, list) = t.observe(
            &EngineEvent::UserUpserted(user(2, Some("bot"), "Bot", 3)),
            11,
        );
        assert!(seen.is_empty(), "a move is not a rename");
        assert_eq!(
            list.expect("moved")[0]
                .online
                .as_ref()
                .map(|p| p.channel_id),
            Some(3)
        );

        let (_, list) = t.observe(
            &EngineEvent::TalkStateChanged {
                session: 2,
                talking: true,
            },
            12,
        );
        assert!(list.is_none());

        let (seen, list) = t.observe(&EngineEvent::UserRemoved(2), 20);
        assert_eq!(seen, [("bot".into(), "Bot".into(), 20)]);
        let list = list.expect("offline");
        let bot = list.iter().find(|v| v.cert_hash == "bot").expect("bot");
        assert!(bot.online.is_none());
        assert_eq!(bot.last_seen_at, Some(20));

        // Back with another name: the same friend.
        let (seen, list) = t.observe(
            &EngineEvent::UserUpserted(user(5, Some("bot"), "Robot", 0)),
            30,
        );
        assert_eq!(seen, [("bot".into(), "Robot".into(), 30)]);
        let list = list.expect("back");
        assert_eq!(list[0].name, "Robot");
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn disconnect_takes_everyone_offline_and_strangers_do_not_count() {
        let mut t = Tracker::new(vec![friend("bot", "Bot")]);
        t.observe(
            &synced(vec![
                user(2, Some("bot"), "Bot", 0),
                user(3, None, "Guest", 0),
            ]),
            10,
        );
        let (_, list) = t.observe(&EngineEvent::UserUpserted(user(4, Some("x"), "X", 0)), 11);
        assert!(list.is_none());

        let (seen, list) = t.observe(
            &EngineEvent::StateChanged(ConnState::Reconnecting {
                server_id: 7,
                attempt: 1,
                max_attempts: 5,
                next_retry_secs: 1,
            }),
            15,
        );
        assert_eq!(seen, [("bot".into(), "Bot".into(), 15)]);
        assert!(list.expect("offline")[0].online.is_none());

        // Updates of an ended connection are ignored.
        let (_, list) = t.observe(
            &EngineEvent::UserUpserted(user(2, Some("bot"), "Bot", 0)),
            16,
        );
        assert!(list.is_none());
    }

    #[test]
    fn adding_and_removing_changes_the_list() {
        let mut t = Tracker::new(Vec::new());
        t.observe(&synced(vec![user(2, Some("bot"), "Bot", 0)]), 10);
        t.friends.insert("bot".into(), friend("bot", "Bot"));
        let list = t.refresh().expect("added");
        assert!(list[0].online.is_some(), "online right away when present");
        t.friends.remove("bot");
        assert_eq!(t.refresh(), Some(Vec::new()));
    }
}
