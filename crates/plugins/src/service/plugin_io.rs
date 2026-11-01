//! The client's [`PluginIo`]: what `ui`, `peers` and `chat` host calls do once their permission
//! passed. Runs on the plugin's thread inside its call budget, so nothing here waits: messages
//! and UI changes are queued as [`ServiceEvent`]s for the app.
//!
//! `peers` works on the messaging state per connection ([`PeersState`], one
//! [`Messenger`] each), which the service feeds from engine events (see `messaging.rs`).

mod user_menu;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use mumcord_store::ServerId;
use tokio::sync::mpsc::UnboundedSender;

use super::ServiceEvent;
use crate::api::io::PluginIo;
use crate::api::webview::{WebviewPlace, Webviews};
use crate::chat::{self, GestureGate, Supersede, SupersedeTracker};
use crate::manifest::Visibility;
use crate::messaging::{
    AnnounceEntry, AnnounceThrottle, Member, Messenger, PluginRef, SendError, SendRequest,
    SourceTag, Target,
};
use crate::peer_key::PeerKeys;
use crate::wit::peers;
use crate::wit::types::{BotInfo as WitBotInfo, Error, Peer};
use crate::wit::ui::{Node, Slot, UserMenuFilter};
use crate::wit::webview::Place;

pub use user_menu::BotInfo;

/// The plugin API major this host speaks; written into every envelope and announce entry.
pub(super) const API_MAJOR: u8 = 1;
/// The WIT limit for names a plugin sees.
const MAX_NAME_BYTES: usize = 128;

/// A user as the engine reports them, plus whether they are a friend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    pub session: u32,
    /// Untrusted text from the server.
    pub name: String,
    /// Only for computing the per-plugin key; never reaches a plugin.
    pub cert_hash: Option<String>,
    pub is_friend: bool,
}

/// The local user on one server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfInfo {
    pub me: PeerInfo,
    /// Channel id and (untrusted) name.
    pub channel: Option<(u32, String)>,
    pub muted: bool,
    pub deafened: bool,
}

/// A UI change a plugin asked for, for the app's `plugin://ui` pushes. Trees are untrusted.
#[derive(Debug, Clone)]
pub enum UiUpdate {
    SetTree {
        slot: Slot,
        nodes: Vec<Node>,
    },
    Clear {
        slot: Slot,
    },
    OpenDialog {
        title: String,
        subtitle: Option<String>,
    },
    CloseDialog,
    /// The plugin's panel opens in (or closes from) the large popover.
    PanelPopover {
        open: bool,
    },
    /// `None` removes the entry. `sessions` lists the users it shows on (`None`: everyone).
    UserMenuEntry {
        label: Option<String>,
        sessions: Option<Vec<u32>>,
    },
    /// The plugin's web view opens (or switches) to `entry` below `web/`, query included.
    WebviewShow {
        place: WebviewPlace,
        entry: String,
    },
    /// The web view closes; the place falls back to its tree.
    WebviewHide,
    /// A bridge message to the page (untrusted plugin text, never logged).
    WebviewPost {
        message: String,
    },
}

/// How a switched-on plugin takes part in messaging and chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PluginMeta {
    /// Shown in the "[Name] " chat prefix.
    pub name: String,
    /// `None` without `peers.messages`: not announced, messages refused.
    pub messaging: Option<MessagingMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MessagingMeta {
    pub source: SourceTag,
    pub visibility: Visibility,
    pub max_payload: u32,
}

#[derive(Debug, Default)]
pub(super) struct PeersState {
    /// The server the user looks at: plugins see this one only.
    pub focused: Option<ServerId>,
    /// Loaded, switched-on plugins by id.
    pub plugins: HashMap<String, PluginMeta>,
    pub connections: HashMap<ServerId, Connection>,
    /// User-menu entries by plugin.
    menus: HashMap<String, user_menu::MenuEntry>,
}

#[derive(Debug)]
pub(super) struct Connection {
    pub messenger: Messenger,
    /// The server passes plugin data (FR-6.38).
    pub supported: bool,
    pub me: Option<SelfInfo>,
    /// Other users in the user's channel.
    pub members: BTreeMap<u32, PeerInfo>,
    /// Each user's last announce.
    pub announced: HashMap<u32, Vec<AnnounceEntry>>,
    /// Users whose hello lists `plugins`: they receive our announce.
    pub capable: BTreeSet<u32>,
    pub throttle: AnnounceThrottle,
    /// Bot info from hellos by session (people in other channels too).
    pub bots: HashMap<u32, BotInfo>,
}

impl Connection {
    pub fn new(now: Instant) -> Self {
        Self {
            messenger: Messenger::new(now),
            supported: false,
            me: None,
            members: BTreeMap::new(),
            announced: HashMap::new(),
            capable: BTreeSet::new(),
            throttle: AnnounceThrottle::default(),
            bots: HashMap::new(),
        }
    }
}

#[derive(Debug, Default)]
struct ChatState {
    gate: GestureGate,
    supersede: SupersedeTracker,
}

/// See the module docs.
#[derive(Debug)]
pub(super) struct ServiceIo {
    peers: Mutex<PeersState>,
    chat: Mutex<ChatState>,
    keys: Option<PeerKeys>,
    events: UnboundedSender<ServiceEvent>,
    webviews: Arc<Webviews>,
}

impl ServiceIo {
    pub fn new(keys: Option<PeerKeys>, events: UnboundedSender<ServiceEvent>) -> Self {
        Self {
            peers: Mutex::default(),
            chat: Mutex::default(),
            keys,
            events,
            webviews: Arc::default(),
        }
    }

    pub fn peers(&self) -> MutexGuard<'_, PeersState> {
        self.peers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn chat(&self) -> MutexGuard<'_, ChatState> {
        self.chat.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn keys(&self) -> Option<&PeerKeys> {
        self.keys.as_ref()
    }

    /// A click in the plugin's UI: allows one `chat.send` for 2 s.
    pub fn gesture(&self, plugin: &str) {
        self.chat().gate.gesture(plugin, Instant::now());
    }

    /// A typed slash command of the plugin: allows one `chat.send` for 8 s.
    pub fn command_gesture(&self, plugin: &str) {
        self.chat().gate.command_gesture(plugin, Instant::now());
    }

    /// An incoming channel text line (plain text, as other clients see it); pairs it with a
    /// pending card. Only its hash is kept.
    pub fn chat_text(&self, message: u64, sender: u32, line: &str) -> Supersede {
        self.chat()
            .supersede
            .text(message, sender, line, Instant::now())
    }

    pub fn webviews(&self) -> &Arc<Webviews> {
        &self.webviews
    }

    pub fn forget(&self, plugin: &str) {
        self.chat().gate.forget(plugin);
        self.webviews.forget(plugin);
        self.menu_forget_plugin(plugin);
    }

    fn emit(&self, event: ServiceEvent) {
        // The app holds the receiver for as long as plugins run; a closed channel only happens
        // at shutdown, when nobody needs the event any more.
        let _ = self.events.send(event);
    }

    fn ui(&self, plugin: &str, update: UiUpdate) {
        self.emit(ServiceEvent::Ui {
            plugin: plugin.to_owned(),
            update,
        });
    }

    fn name(&self, plugin: &str) -> String {
        self.peers()
            .plugins
            .get(plugin)
            .map_or_else(|| plugin.to_owned(), |meta| meta.name.clone())
    }
}

impl PluginIo for ServiceIo {
    fn set_tree(&self, plugin: &str, slot: Slot, nodes: Vec<Node>) -> Result<(), Error> {
        self.ui(plugin, UiUpdate::SetTree { slot, nodes });
        Ok(())
    }

    fn clear(&self, plugin: &str, slot: Slot) {
        self.ui(plugin, UiUpdate::Clear { slot });
    }

    fn open_dialog(
        &self,
        plugin: &str,
        title: String,
        subtitle: Option<String>,
    ) -> Result<(), Error> {
        self.webviews.dialog_opened(plugin);
        self.ui(plugin, UiUpdate::OpenDialog { title, subtitle });
        Ok(())
    }

    fn close_dialog(&self, plugin: &str) {
        if self.webviews.dialog_closed(plugin) {
            self.ui(plugin, UiUpdate::WebviewHide);
        }
        self.ui(plugin, UiUpdate::CloseDialog);
    }

    fn open_panel_popover(&self, plugin: &str) -> Result<(), Error> {
        if !self.chat().gate.is_fresh(plugin, Instant::now()) {
            return Err(Error::NotPermitted);
        }
        self.ui(plugin, UiUpdate::PanelPopover { open: true });
        Ok(())
    }

    fn close_panel_popover(&self, plugin: &str) {
        self.ui(plugin, UiUpdate::PanelPopover { open: false });
    }

    fn set_user_menu_entry(&self, plugin: &str, label: Option<String>) -> Result<(), Error> {
        self.menu_set(plugin, label);
        Ok(())
    }

    fn set_user_menu_entry_for(
        &self,
        plugin: &str,
        label: String,
        filter: UserMenuFilter,
    ) -> Result<(), Error> {
        self.menu_set_filtered(plugin, label, filter);
        Ok(())
    }

    fn bot(&self, _plugin: &str, session: u32) -> Result<Option<WitBotInfo>, Error> {
        self.bot_of(session)
    }

    fn peer_send(
        &self,
        plugin: &str,
        to: peers::Target,
        kind: u8,
        payload: Vec<u8>,
    ) -> Result<(), Error> {
        let mut state = self.peers();
        let PeersState {
            focused,
            plugins,
            connections,
            ..
        } = &mut *state;
        let meta = plugins
            .get(plugin)
            .and_then(|meta| meta.messaging.clone())
            .ok_or(Error::NotPermitted)?;
        let (server, connection) = focused
            .and_then(|server| connections.get_mut(&server).map(|c| (server, c)))
            .filter(|(_, c)| c.supported)
            .ok_or(Error::NoRecipients)?;
        let Connection {
            messenger,
            members,
            announced,
            ..
        } = connection;
        let members: Vec<Member<'_>> = members
            .values()
            .map(|peer| Member {
                session: peer.session,
                is_friend: peer.is_friend,
                plugins: announced.get(&peer.session).map_or(&[], Vec::as_slice),
            })
            .collect();
        let request = SendRequest {
            plugin: PluginRef {
                id: plugin,
                source: meta.source,
                visibility: meta.visibility,
            },
            // The host call already checked `peers.messages`.
            permitted: true,
            api_major: API_MAJOR,
            max_payload: meta.max_payload,
            target: match to {
                peers::Target::Channel => Target::Channel,
                peers::Target::Direct(session) => Target::Direct(session),
            },
            kind,
            payload: &payload,
        };
        let outgoing = messenger
            .send(&request, &members, Instant::now())
            .map_err(wit_error)?;
        drop(state);
        self.emit(ServiceEvent::PeerSend {
            server,
            plugin: plugin.to_owned(),
            outgoing,
        });
        Ok(())
    }

    fn start_game_burst(&self, plugin: &str) -> Result<(), Error> {
        let mut state = self.peers();
        let PeersState {
            focused,
            connections,
            ..
        } = &mut *state;
        let connection = focused
            .and_then(|server| connections.get_mut(&server))
            .ok_or(Error::RateLimited)?;
        connection
            .messenger
            .start_game_burst(plugin, true, Instant::now())
            .map_err(wit_error)
    }

    fn presence(&self, plugin: &str) -> Result<Vec<Peer>, Error> {
        let state = self.peers();
        let Some(connection) = state
            .focused
            .and_then(|server| state.connections.get(&server))
        else {
            return Ok(Vec::new());
        };
        let Some(me) = &connection.me else {
            return Ok(Vec::new());
        };
        Ok(std::iter::once(&me.me)
            .chain(connection.members.values())
            .map(|peer| wit_peer(self.keys(), plugin, peer))
            .collect())
    }

    fn chat_send(&self, plugin: &str, text: String) -> Result<(), Error> {
        let name = self.name(plugin);
        let line = self
            .chat()
            .gate
            .send(plugin, &name, &text, Instant::now())
            .map_err(|error| match error {
                chat::SendError::NotPermitted => Error::NotPermitted,
                chat::SendError::TooLarge => Error::TooLarge,
                chat::SendError::Invalid => Error::Invalid,
            })?;
        self.emit(ServiceEvent::ChatPost {
            plugin: plugin.to_owned(),
            line,
        });
        Ok(())
    }

    fn line_hash(&self, plugin: &str, text: &str) -> u32 {
        chat::line_hash(&self.name(plugin), text)
    }

    fn supersede(
        &self,
        plugin: &str,
        card_id: String,
        sender: u32,
        hash: u32,
    ) -> Result<(), Error> {
        let result = self
            .chat()
            .supersede
            .card(plugin, &card_id, sender, hash, Instant::now());
        if let Supersede::Replace { card, message } = result {
            self.emit(ServiceEvent::Supersede {
                plugin: plugin.to_owned(),
                card,
                message,
            });
        }
        Ok(())
    }

    fn webview_show(&self, plugin: &str, place: Place, entry: String) -> Result<(), Error> {
        let shown = self
            .webviews
            .show(plugin, place.into(), &entry, Instant::now())?;
        self.ui(
            plugin,
            UiUpdate::WebviewShow {
                place: shown.place,
                entry: shown.entry,
            },
        );
        Ok(())
    }

    fn webview_hide(&self, plugin: &str) {
        if self.webviews.hide(plugin) {
            self.ui(plugin, UiUpdate::WebviewHide);
        }
    }

    fn webview_post(&self, plugin: &str, message: String) -> Result<(), Error> {
        self.webviews.post(plugin, &message, Instant::now())?;
        self.ui(plugin, UiUpdate::WebviewPost { message });
        Ok(())
    }
}

/// A user as plugin `plugin` sees them: with that plugin's own key, never the certificate hash.
pub(super) fn wit_peer(keys: Option<&PeerKeys>, plugin: &str, peer: &PeerInfo) -> Peer {
    let mut name = peer.name.clone();
    if name.len() > MAX_NAME_BYTES {
        let mut end = MAX_NAME_BYTES;
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        name.truncate(end);
    }
    Peer {
        session: peer.session,
        name,
        key: keys.and_then(|keys| keys.key(plugin, peer.cert_hash.as_deref())),
    }
}

fn wit_error(error: SendError) -> Error {
    match error {
        SendError::NotPermitted => Error::NotPermitted,
        SendError::TooLarge => Error::TooLarge,
        SendError::RateLimited => Error::RateLimited,
        SendError::NoRecipients => Error::NoRecipients,
        SendError::Hidden => Error::Hidden,
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc::unbounded_channel;

    use super::*;

    #[test]
    fn plugins_get_their_own_key_and_a_bounded_name() {
        let keys = PeerKeys::new([7; 32]);
        let peer = PeerInfo {
            session: 4,
            name: "ä".repeat(100),
            cert_hash: Some("ab".repeat(20)),
            is_friend: false,
        };
        let poll = wit_peer(Some(&keys), "poll", &peer);
        let dice = wit_peer(Some(&keys), "dice", &peer);
        assert_eq!(poll.session, 4);
        assert!(poll.name.len() <= MAX_NAME_BYTES);
        assert!(poll.key.is_some());
        assert_ne!(poll.key, dice.key);
        assert_eq!(wit_peer(None, "poll", &peer).key, None);
    }

    #[test]
    fn web_views_follow_the_dialog_and_carry_posts() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("index.html"), "<p>hi</p>").expect("page written");
        let (events, mut rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        io.webviews()
            .set_root("snacks", Some(dir.path().to_path_buf()));
        let mut next = || match rx.try_recv() {
            Ok(ServiceEvent::Ui { update, .. }) => Some(update),
            _ => None,
        };
        assert_eq!(
            io.webview_show("snacks", Place::Dialog, "index.html".into()),
            Err(Error::Invalid)
        );
        io.open_dialog("snacks", "Snacks".into(), None)
            .expect("dialog opens");
        assert!(matches!(next(), Some(UiUpdate::OpenDialog { .. })));
        io.webview_show("snacks", Place::Dialog, "index.html?x=1".into())
            .expect("web view shows");
        assert!(matches!(
            next(),
            Some(UiUpdate::WebviewShow { place: WebviewPlace::Dialog, entry }) if entry == "index.html?x=1"
        ));
        io.webview_post("snacks", "hi".into())
            .expect("web view shows");
        assert!(matches!(next(), Some(UiUpdate::WebviewPost { message }) if message == "hi"));
        io.close_dialog("snacks");
        assert!(matches!(next(), Some(UiUpdate::WebviewHide)));
        assert!(matches!(next(), Some(UiUpdate::CloseDialog)));
        assert_eq!(io.webview_post("snacks", "hi".into()), Err(Error::Invalid));
    }

    #[test]
    fn panel_popover_opens_only_after_a_gesture() {
        let (events, mut rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        assert_eq!(io.open_panel_popover("poll"), Err(Error::NotPermitted));
        assert!(rx.try_recv().is_err());
        io.gesture("poll");
        assert_eq!(io.open_panel_popover("poll"), Ok(()));
        // A chat.send afterwards still works: opening does not use the gesture up.
        assert_eq!(io.chat().gate.is_fresh("poll", Instant::now()), true);
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Ui {
                update: UiUpdate::PanelPopover { open: true },
                ..
            })
        ));
        io.close_panel_popover("poll");
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Ui {
                update: UiUpdate::PanelPopover { open: false },
                ..
            })
        ));
    }

    #[test]
    fn chat_send_needs_a_gesture_and_gets_the_name_prefix() {
        let (events, mut rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        io.peers().plugins.insert(
            "dice".into(),
            PluginMeta {
                name: "Dice".into(),
                messaging: None,
            },
        );
        assert_eq!(
            io.chat_send("dice", "rolled 4".into()),
            Err(Error::NotPermitted)
        );
        io.gesture("dice");
        assert_eq!(io.chat_send("dice", "rolled 4".into()), Ok(()));
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::ChatPost { line, .. }) if line == "[Dice] rolled 4"
        ));
        assert_eq!(
            io.chat_send("dice", "again".into()),
            Err(Error::NotPermitted)
        );
        assert_eq!(io.line_hash("dice", "x"), chat::line_hash("Dice", "x"));
    }

    #[test]
    fn a_card_replaces_its_line_in_either_order() {
        let (events, mut rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        let hash = chat::line_hash("Poll", "Lunch?");
        assert_eq!(io.chat_text(11, 3, "[Poll] Lunch?"), Supersede::None);
        io.supersede("poll", "c1".into(), 3, hash).expect("ok");
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Supersede { card, message: 11, .. }) if card == "c1"
        ));
    }

    #[test]
    fn messaging_without_a_connection() {
        let (events, _rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        assert_eq!(
            io.peer_send("poll", peers::Target::Channel, 1, vec![]),
            Err(Error::NotPermitted),
            "a plugin without messaging meta"
        );
        assert!(io.presence("poll").is_ok_and(|peers| peers.is_empty()));
        assert_eq!(io.start_game_burst("poll"), Err(Error::RateLimited));
    }
}
