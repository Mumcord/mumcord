//! The client's [`PluginIo`]: what `ui`, `peers` and `chat` host calls do once their permission
//! passed. Runs on the plugin's thread inside its call budget, so nothing here waits: messages
//! and UI changes are queued as [`ServiceEvent`]s for the app.
//!
//! `peers` works on the messaging state per connection ([`PeersState`], one
//! [`Messenger`] each), which the service feeds from engine events (see `messaging.rs`).

mod relay;
mod stage;
mod user_menu;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use mumcord_store::ServerId;
use tokio::sync::mpsc::UnboundedSender;

use super::ServiceEvent;
use crate::api::host_calls::{HostCall, HostRequest, Picture, PostOutcome, Waiter, reply_channel};
use crate::api::io::PluginIo;
use crate::api::webview::{Shown, WebviewPlace, Webviews};
use crate::api::webview_status::{Admit, StatusLimiter};
use crate::chat::{self, GestureGate, MessageClaims, Supersede, SupersedeTracker};
use crate::manifest::Visibility;
use crate::messaging::{
    AnnounceEntry, AnnounceThrottle, Member, Messenger, PluginRef, SendError, SendRequest,
    SourceTag, Target,
};
use crate::peer_key::PeerKeys;
use crate::wit::peers;
use crate::wit::types::{BotInfo as WitBotInfo, Error, Peer};
use crate::wit::ui::{Node, Slot, UserMenuFilter};

pub use stage::StageActivity;
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
        /// For a chat card: the server whose chat a new card goes into (the server of the event
        /// the plugin was handling, else the voice server). `None` for other slots.
        server: Option<ServerId>,
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
    /// Settings → Plugins → Installed opens on the plugin's details (`ui.open-permissions`).
    OpenPermissions,
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
    /// The web view in `place` closes; the place falls back to its tree (or closes).
    WebviewHide {
        place: WebviewPlace,
    },
    /// A bridge message for the pages in `places` (untrusted plugin text, never logged).
    WebviewPost {
        message: String,
        places: Vec<WebviewPlace>,
    },
    /// The tree of the plugin's stage activity (`stage.start` with a tree, `stage.set-tree`).
    /// Empty when the user left the activity. Untrusted, as every tree.
    StageTree {
        nodes: Vec<Node>,
    },
    /// `webview.set-size-hint`: the smallest size the side panel should get, in CSS pixels.
    /// The UI clamps it to the panel's limits.
    WebviewSizeHint {
        min_width: u32,
        min_height: u32,
    },
    /// `webview.set-status`: the plain-text line under the side panel; empty clears it.
    /// Untrusted text, at most 80 characters, without markup or control characters.
    SidePanelStatus {
        text: String,
    },
}

/// How a switched-on plugin takes part in messaging and chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PluginMeta {
    /// Shown in the "[Name] " chat prefix.
    pub name: String,
    /// `None` without `peers.messages` and `ui.stage`: not announced, messages refused.
    pub messaging: Option<MessagingMeta>,
    /// With `ui.stage`: the most participants of its activities (manifest, default 16).
    pub stage: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MessagingMeta {
    pub source: SourceTag,
    pub visibility: Visibility,
    pub max_payload: u32,
    /// `peers.messages` is granted. Without it the plugin is still announced for its stage
    /// activities (FR-9.47), but sends and receives no messages.
    pub messages: bool,
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
    claims: MessageClaims,
}

/// See the module docs.
#[derive(Debug)]
pub(super) struct ServiceIo {
    peers: Mutex<PeersState>,
    chat: Mutex<ChatState>,
    keys: Option<PeerKeys>,
    events: UnboundedSender<ServiceEvent>,
    webviews: Arc<Webviews>,
    statuses: Arc<StatusLimiter>,
    app_menus: crate::api::app_menu::AppMenus,
    stages: Mutex<crate::stage::Stages>,
    relay: Mutex<relay::RelayState>,
}

impl ServiceIo {
    pub fn new(keys: Option<PeerKeys>, events: UnboundedSender<ServiceEvent>) -> Self {
        Self {
            peers: Mutex::default(),
            chat: Mutex::default(),
            keys,
            events,
            webviews: Arc::default(),
            statuses: Arc::default(),
            app_menus: Default::default(),
            stages: Mutex::default(),
            relay: Mutex::default(),
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

    /// A trusted click or key press reported from the plugin's page in `place` (FR-9.45). The
    /// app passes it on only with the frame's secret key, so a page cannot forge it; it counts as
    /// a gesture like a panel click (same 8 s window, each host call once) when that frame is
    /// open and the report is not too soon after the last one. Returns whether it counted.
    pub fn page_gesture(&self, plugin: &str, place: WebviewPlace, now: Instant) -> bool {
        let counted = self.webviews.accept_gesture(plugin, place, now);
        if counted {
            self.chat().gate.gesture(plugin, now);
        }
        counted
    }

    /// A channel text line on `server` (plain text, as other clients see it), incoming or the
    /// user's own copy; pairs it with a pending card and hides it live (FR-6.45). Only its hash
    /// is kept.
    pub fn chat_text(&self, server: ServerId, message: u64, sender: u32, line: &str) -> Supersede {
        let result = self
            .chat()
            .supersede
            .text(server, message, sender, line, Instant::now());
        self.emit_supersede(&result);
        result
    }

    fn emit_supersede(&self, result: &Supersede) {
        if let Supersede::Replace {
            plugin,
            card,
            message,
        } = result
        {
            self.emit(ServiceEvent::Supersede {
                plugin: plugin.clone(),
                card: Some(card.clone()),
                message: *message,
            });
        }
    }

    /// The server a chat call goes to: the one of the event being handled, else the voice
    /// server. Never another one, so a reply or card stays in the chat its event came from.
    fn chat_server(&self, event_server: Option<ServerId>) -> Option<ServerId> {
        event_server.or(self.peers().focused)
    }

    /// The opaque token plugin `plugin` gets for a message it is handed in a chat event.
    pub fn issue_token(&self, plugin: &str, server: ServerId, message: u64) -> u64 {
        self.chat().claims.issue(plugin, server, message)
    }

    pub fn app_menus(&self) -> &crate::api::app_menu::AppMenus {
        &self.app_menus
    }

    pub fn webviews(&self) -> &Arc<Webviews> {
        &self.webviews
    }

    pub fn forget(&self, plugin: &str) {
        {
            let mut chat = self.chat();
            chat.gate.forget(plugin);
            chat.claims.forget_plugin(plugin);
        }
        self.forget_stage(plugin);
        self.webviews.forget(plugin);
        self.statuses.forget(plugin);
        self.app_menus.forget(plugin);
        self.menu_forget_plugin(plugin);
    }

    fn emit(&self, event: ServiceEvent) {
        // The app holds the receiver for as long as plugins run; a closed channel only happens
        // at shutdown, when nobody needs the event any more.
        let _ = self.events.send(event);
    }

    /// Shows the merged status update once the rate limit allows. A short-lived thread, because
    /// the plugin may not call again; at most one per plugin waits at a time.
    fn flush_status_after(&self, id: &str, wait: std::time::Duration) {
        let plugin = id.to_owned();
        let statuses = Arc::clone(&self.statuses);
        let webviews = Arc::clone(&self.webviews);
        let events = self.events.clone();
        let spawned = std::thread::Builder::new()
            .name("plugin-status".into())
            .spawn(move || {
                std::thread::sleep(wait);
                let Some(text) = statuses.flush(&plugin, Instant::now()) else {
                    return;
                };
                if webviews.is_open(&plugin, WebviewPlace::SidePanel) {
                    let update = UiUpdate::SidePanelStatus { text };
                    // A closed channel only happens at shutdown.
                    let _ = events.send(ServiceEvent::Ui { plugin, update });
                }
            });
        if let Err(error) = spawned {
            // Without the thread the update would wait forever and block later ones.
            tracing::warn!(%error, "could not schedule a plugin status update");
            self.statuses.forget(id);
        }
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
    fn relay_status(&self, _plugin: &str) -> crate::api::relay::RelayStatus {
        self.relay_status_of()
    }

    fn relay_send(
        &self,
        plugin: &str,
        target: crate::messaging::Target,
        data: Vec<u8>,
        fallback: bool,
    ) -> Result<(), crate::api::relay::SendError> {
        self.relay_send_to(plugin, target, data, fallback)
    }

    fn set_tree(
        &self,
        plugin: &str,
        slot: Slot,
        nodes: Vec<Node>,
        server: Option<ServerId>,
    ) -> Result<(), Error> {
        let server = matches!(slot, Slot::ChatCard(_))
            .then(|| self.chat_server(server))
            .flatten();
        self.ui(
            plugin,
            UiUpdate::SetTree {
                slot,
                nodes,
                server,
            },
        );
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
            self.ui(
                plugin,
                UiUpdate::WebviewHide {
                    place: WebviewPlace::Dialog,
                },
            );
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

    fn open_permissions(&self, plugin: &str) -> Result<(), Error> {
        if !self.chat().gate.is_fresh(plugin, Instant::now()) {
            return Err(Error::NotPermitted);
        }
        self.ui(plugin, UiUpdate::OpenPermissions);
        Ok(())
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
            .filter(|meta| meta.messages)
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

    fn chat_send(&self, plugin: &str, text: String, server: Option<ServerId>) -> Result<(), Error> {
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
        let Some(server) = self.chat_server(server) else {
            tracing::debug!(plugin, "plugin chat line dropped: not connected");
            return Ok(());
        };
        self.emit(ServiceEvent::ChatPost {
            plugin: plugin.to_owned(),
            server,
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
        server: Option<ServerId>,
    ) -> Result<(), Error> {
        let Some(server) = self.chat_server(server) else {
            return Ok(());
        };
        let name = self.name(plugin);
        let result = self.chat().supersede.card(
            server,
            plugin,
            &name,
            &card_id,
            sender,
            hash,
            Instant::now(),
        );
        self.emit_supersede(&result);
        Ok(())
    }

    fn replace_message(&self, plugin: &str, token: u64, card: Option<String>) -> Result<(), Error> {
        let claim = self
            .chat()
            .claims
            .claim(plugin, token)
            .ok_or(Error::NotPermitted)?;
        self.emit(ServiceEvent::Supersede {
            plugin: plugin.to_owned(),
            card,
            message: claim.message,
        });
        Ok(())
    }

    fn webview_show(&self, plugin: &str, place: WebviewPlace, entry: String) -> Result<(), Error> {
        let now = Instant::now();
        // A new side panel needs a fresh gesture (FR-9.45); a new page in an open one does not.
        if place == WebviewPlace::SidePanel
            && !self.webviews.is_open(plugin, place)
            && !self.chat().gate.recent(plugin, now)
        {
            return Err(Error::NotPermitted);
        }
        // A new web dialog uses up the gesture (once per gesture, FR-9.40/FR-9.45); a new page
        // in an open one does not. Taken only after it opened, so a refused show keeps it.
        let new_dialog = place == WebviewPlace::Dialog && !self.webviews.is_open(plugin, place);
        if new_dialog
            && !self
                .chat()
                .gate
                .host_call_allowed(plugin, HostCall::OpenDialog, now)
        {
            return Err(Error::NotPermitted);
        }
        let shown = self.webviews.show(plugin, place, &entry, now)?;
        if new_dialog {
            self.chat()
                .gate
                .take_host_call(plugin, HostCall::OpenDialog, now);
        }
        self.shown(plugin, shown);
        Ok(())
    }

    fn webview_set_size_hint(
        &self,
        plugin: &str,
        min_width: u32,
        min_height: u32,
    ) -> Result<(), Error> {
        if !self.webviews.is_open(plugin, WebviewPlace::SidePanel) {
            return Err(Error::Invalid);
        }
        self.ui(
            plugin,
            UiUpdate::WebviewSizeHint {
                min_width,
                min_height,
            },
        );
        Ok(())
    }

    fn webview_set_status(&self, plugin: &str, text: String) -> Result<(), Error> {
        if !self.webviews.is_open(plugin, WebviewPlace::SidePanel) {
            return Err(Error::Invalid);
        }
        match self.statuses.admit(plugin, text, Instant::now()) {
            Admit::Send(text) => self.ui(plugin, UiUpdate::SidePanelStatus { text }),
            Admit::Schedule(wait) => self.flush_status_after(plugin, wait),
            Admit::Merged => {}
        }
        Ok(())
    }

    fn stage_web_show(&self, plugin: &str, entry: String) -> Result<(), Error> {
        let shown = self.webviews.open_stage(plugin, &entry, Instant::now())?;
        self.shown(plugin, shown);
        Ok(())
    }

    fn stage_start(
        &self,
        plugin: &str,
        title: String,
        content: crate::api::io::StageContent,
    ) -> Result<String, Error> {
        self.start_activity(plugin, title, content)
    }

    fn stage_join(&self, plugin: &str, activity: &str) -> Result<(), Error> {
        self.join_activity_from_plugin(plugin, activity)
    }

    fn stage_leave(&self, plugin: &str) {
        self.leave_activity(plugin);
    }

    fn stage_end(&self, plugin: &str) {
        self.end_activity(plugin);
    }

    fn stage_set_tree(&self, plugin: &str, nodes: Vec<Node>) -> Result<(), Error> {
        self.set_stage_tree(plugin, nodes)
    }

    fn app_menu_set_state(
        &self,
        plugin: &str,
        item: &str,
        enabled: bool,
        reason: Option<String>,
    ) -> Result<(), Error> {
        if self.app_menus.set_state(plugin, item, enabled, reason)? {
            self.emit(ServiceEvent::AppMenuChanged);
        }
        Ok(())
    }

    fn webview_hide(&self, plugin: &str) {
        for place in self.webviews.hide(plugin) {
            self.ui(plugin, UiUpdate::WebviewHide { place });
        }
    }

    fn webview_post(&self, plugin: &str, message: String) -> Result<(), Error> {
        let places = self.webviews.post(plugin, &message, Instant::now())?;
        self.ui(plugin, UiUpdate::WebviewPost { message, places });
        Ok(())
    }

    fn files_save(&self, plugin: &str, name: String, data: Vec<u8>) -> Result<Waiter<bool>, Error> {
        self.take_host_call(plugin, HostCall::Save)?;
        let (reply, waiter) = reply_channel();
        self.host_call(plugin, HostRequest::Save { name, data, reply });
        Ok(waiter)
    }

    fn files_import_picture(
        &self,
        plugin: &str,
    ) -> Result<Waiter<Result<Option<Picture>, Error>>, Error> {
        self.take_host_call(plugin, HostCall::ImportPicture)?;
        let (reply, waiter) = reply_channel();
        self.host_call(plugin, HostRequest::ImportPicture { reply });
        Ok(waiter)
    }

    fn chat_post_image(
        &self,
        plugin: &str,
        png: Vec<u8>,
        alt: String,
        server: Option<ServerId>,
    ) -> Result<Waiter<Result<PostOutcome, Error>>, Error> {
        if !self
            .chat()
            .gate
            .host_call_allowed(plugin, HostCall::PostImage, Instant::now())
        {
            return Err(Error::NotPermitted);
        }
        // Not connected keeps the gesture: nothing was sent.
        let server = self.chat_server(server).ok_or(Error::NoRecipients)?;
        self.take_host_call(plugin, HostCall::PostImage)?;
        let line = chat::prefixed(&self.name(plugin), &alt)
            .trim_end()
            .to_owned();
        let (reply, waiter) = reply_channel();
        self.host_call(
            plugin,
            HostRequest::PostImage {
                server,
                line,
                png,
                reply,
            },
        );
        Ok(waiter)
    }
}

impl ServiceIo {
    /// Tells the UI about a frame that opened or got a new page. A side panel it replaced
    /// closes, and that plugin learns it through [`ServiceEvent::SurfaceClosed`].
    fn shown(&self, plugin: &str, shown: Shown) {
        if let Some(other) = shown.replaced {
            let place = WebviewPlace::SidePanel;
            self.ui(&other, UiUpdate::WebviewHide { place });
            self.emit(ServiceEvent::SurfaceClosed {
                plugin: other,
                place,
            });
        }
        self.ui(
            plugin,
            UiUpdate::WebviewShow {
                place: shown.place,
                entry: shown.entry,
            },
        );
    }

    /// Uses the plugin's last gesture for `call`, or refuses with not-permitted (FR-9.106).
    fn take_host_call(&self, plugin: &str, call: HostCall) -> Result<(), Error> {
        if self
            .chat()
            .gate
            .take_host_call(plugin, call, Instant::now())
        {
            Ok(())
        } else {
            Err(Error::NotPermitted)
        }
    }

    fn host_call(&self, plugin: &str, request: HostRequest) {
        self.emit(ServiceEvent::HostCall {
            plugin: plugin.to_owned(),
            request,
        });
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
    use std::time::Duration;

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
        io.webviews().set_root(
            "snacks",
            Some(dir.path().to_path_buf()),
            vec![WebviewPlace::Dialog],
        );
        let mut next = || match rx.try_recv() {
            Ok(ServiceEvent::Ui { update, .. }) => Some(update),
            _ => None,
        };
        io.gesture("snacks");
        assert_eq!(
            io.webview_show("snacks", WebviewPlace::Dialog, "index.html".into()),
            Err(Error::Invalid)
        );
        io.open_dialog("snacks", "Snacks".into(), None)
            .expect("dialog opens");
        assert!(matches!(next(), Some(UiUpdate::OpenDialog { .. })));
        io.webview_show("snacks", WebviewPlace::Dialog, "index.html?x=1".into())
            .expect("web view shows");
        assert!(matches!(
            next(),
            Some(UiUpdate::WebviewShow { place: WebviewPlace::Dialog, entry }) if entry == "index.html?x=1"
        ));
        io.webview_post("snacks", "hi".into())
            .expect("web view shows");
        assert!(
            matches!(next(), Some(UiUpdate::WebviewPost { message, places })
            if message == "hi" && places == [WebviewPlace::Dialog])
        );
        io.close_dialog("snacks");
        assert!(matches!(
            next(),
            Some(UiUpdate::WebviewHide {
                place: WebviewPlace::Dialog
            })
        ));
        assert!(matches!(next(), Some(UiUpdate::CloseDialog)));
        assert_eq!(io.webview_post("snacks", "hi".into()), Err(Error::Invalid));
    }

    #[test]
    fn a_web_dialog_opens_once_per_gesture_within_the_window() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("index.html"), "<p>hi</p>").expect("page written");
        let (events, mut rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        io.webviews().set_root(
            "snacks",
            Some(dir.path().to_path_buf()),
            vec![WebviewPlace::Dialog],
        );
        let dialog = WebviewPlace::Dialog;
        let open = |io: &ServiceIo| {
            io.open_dialog("snacks", "Snacks".into(), None)
                .expect("dialog opens");
            io.webview_show("snacks", dialog, "index.html".into())
        };
        assert_eq!(open(&io), Err(Error::NotPermitted), "no gesture");

        io.gesture("snacks");
        assert_eq!(open(&io), Ok(()), "within the window");
        assert_eq!(
            io.webview_show("snacks", dialog, "index.html?page=2".into()),
            Ok(()),
            "a new page in the open dialog needs no gesture"
        );
        io.close_dialog("snacks");
        assert_eq!(open(&io), Err(Error::NotPermitted), "once per gesture");

        let stale = Instant::now() - chat::GESTURE_WINDOW - Duration::from_secs(1);
        io.chat().gate.gesture("snacks", stale);
        assert_eq!(open(&io), Err(Error::NotPermitted), "the window is 8 s");
        while rx.try_recv().is_ok() {}
    }

    #[test]
    fn a_side_panel_opens_after_a_gesture_and_replaces_another_plugins() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("index.html"), "<p>hi</p>").expect("page written");
        let (events, mut rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        for id in ["a", "b"] {
            io.webviews().set_root(
                id,
                Some(dir.path().to_path_buf()),
                vec![WebviewPlace::SidePanel],
            );
        }
        let side = WebviewPlace::SidePanel;
        assert_eq!(
            io.webview_show("a", side, "index.html".into()),
            Err(Error::NotPermitted),
            "no gesture"
        );
        assert!(rx.try_recv().is_err());
        io.gesture("a");
        io.webview_show("a", side, "index.html".into())
            .expect("side panel opens");
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Ui { plugin, update: UiUpdate::WebviewShow { place: WebviewPlace::SidePanel, .. } })
                if plugin == "a"
        ));
        assert_eq!(io.webview_set_size_hint("a", 300, 200), Ok(()));
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Ui {
                update: UiUpdate::WebviewSizeHint {
                    min_width: 300,
                    min_height: 200
                },
                ..
            })
        ));
        assert_eq!(io.webview_set_size_hint("b", 300, 200), Err(Error::Invalid));
        io.gesture("b");
        io.webview_show("b", side, "index.html".into())
            .expect("b's side panel opens");
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Ui { plugin, update: UiUpdate::WebviewHide { place: WebviewPlace::SidePanel } })
                if plugin == "a"
        ));
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::SurfaceClosed { plugin, place: WebviewPlace::SidePanel }) if plugin == "a"
        ));
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Ui { plugin, update: UiUpdate::WebviewShow { .. } }) if plugin == "b"
        ));
    }

    #[test]
    fn the_side_panel_status_needs_an_open_side_panel_and_keeps_the_last_quick_update() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("index.html"), "<p>hi</p>").expect("page written");
        let (events, mut rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        let side = WebviewPlace::SidePanel;
        io.webviews()
            .set_root("a", Some(dir.path().to_path_buf()), vec![side]);
        assert_eq!(io.webview_set_status("a", "x".into()), Err(Error::Invalid));
        io.webviews()
            .show("a", side, "index.html", Instant::now())
            .expect("opened");
        let mut status = || match rx.try_recv() {
            Ok(ServiceEvent::Ui {
                update: UiUpdate::SidePanelStatus { text },
                ..
            }) => Some(text),
            _ => None,
        };
        for text in ["1", "2", "3", ""] {
            assert_eq!(io.webview_set_status("a", text.into()), Ok(()));
            assert_eq!(status().as_deref(), Some(text));
        }
        assert_eq!(io.webview_set_status("a", "5".into()), Ok(()));
        assert_eq!(io.webview_set_status("a", "last".into()), Ok(()));
        assert_eq!(status(), None, "over the limit: merged, not sent yet");
        let deadline = Instant::now() + std::time::Duration::from_secs(3);
        let flushed = loop {
            if let Some(text) = status() {
                break text;
            }
            assert!(Instant::now() < deadline, "the merged update never came");
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert_eq!(flushed, "last");
        assert_eq!(status(), None);
    }

    #[test]
    fn a_new_page_in_an_open_side_panel_needs_no_gesture() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("index.html"), "<p>hi</p>").expect("page written");
        let (events, _rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        let side = WebviewPlace::SidePanel;
        io.webviews()
            .set_root("a", Some(dir.path().to_path_buf()), vec![side]);
        io.webviews()
            .show("a", side, "index.html", Instant::now())
            .expect("opened");
        assert_eq!(io.webview_show("a", side, "index.html?2".into()), Ok(()));
    }

    #[test]
    fn stage_web_content_opens_only_through_the_stage_start_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("index.html"), "<p>hi</p>").expect("page written");
        let (events, mut rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        io.webviews().set_root(
            "a",
            Some(dir.path().to_path_buf()),
            vec![WebviewPlace::Stage],
        );
        assert_eq!(
            io.webview_show("a", WebviewPlace::Stage, "index.html".into()),
            Err(Error::Invalid)
        );
        assert_eq!(io.stage_web_show("a", "index.html".into()), Ok(()));
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Ui {
                update: UiUpdate::WebviewShow {
                    place: WebviewPlace::Stage,
                    ..
                },
                ..
            })
        ));
        assert_eq!(
            io.webview_show("a", WebviewPlace::Stage, "index.html?2".into()),
            Ok(())
        );
    }

    #[test]
    fn permissions_open_only_after_a_gesture() {
        let (events, mut rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        assert_eq!(io.open_permissions("poll"), Err(Error::NotPermitted));
        assert!(rx.try_recv().is_err());
        io.gesture("poll");
        assert_eq!(io.open_permissions("poll"), Ok(()));
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Ui {
                update: UiUpdate::OpenPermissions,
                ..
            })
        ));
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
        assert!(io.chat().gate.is_fresh("poll", Instant::now()));
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
                stage: None,
            },
        );
        io.peers().focused = Some(1);
        assert_eq!(
            io.chat_send("dice", "rolled 4".into(), None),
            Err(Error::NotPermitted)
        );
        io.gesture("dice");
        assert_eq!(io.chat_send("dice", "rolled 4".into(), None), Ok(()));
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::ChatPost { line, server: 1, .. }) if line == "[Dice] rolled 4"
        ));
        assert_eq!(
            io.chat_send("dice", "again".into(), None),
            Err(Error::NotPermitted)
        );
        assert_eq!(io.line_hash("dice", "x"), chat::line_hash("Dice", "x"));
    }

    fn io_with(
        plugin: &str,
        name: &str,
    ) -> (
        ServiceIo,
        tokio::sync::mpsc::UnboundedReceiver<ServiceEvent>,
    ) {
        let (events, rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        io.peers().plugins.insert(
            plugin.into(),
            PluginMeta {
                name: name.into(),
                messaging: None,
                stage: None,
            },
        );
        (io, rx)
    }

    #[test]
    fn host_calls_need_a_gesture_once_each() {
        let (io, mut rx) = io_with("board", "Board");
        assert!(matches!(
            io.files_save("board", "a.png".into(), vec![1]),
            Err(Error::NotPermitted)
        ));
        assert!(matches!(
            io.files_import_picture("board"),
            Err(Error::NotPermitted)
        ));
        assert!(rx.try_recv().is_err(), "nothing opens without a gesture");
        io.gesture("board");
        let waiter = io
            .files_save("board", "a.png".into(), vec![1, 2])
            .expect("allowed");
        match rx.try_recv() {
            Ok(ServiceEvent::HostCall {
                plugin,
                request: HostRequest::Save { name, data, reply },
            }) => {
                assert_eq!(
                    (plugin.as_str(), name.as_str(), data),
                    ("board", "a.png", vec![1, 2])
                );
                assert!(reply.send(true));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(waiter.wait(std::time::Duration::from_secs(1)), Some(true));
        assert!(matches!(
            io.files_save("board", "b.png".into(), vec![1]),
            Err(Error::NotPermitted)
        ));
        assert!(io.files_import_picture("board").is_ok());
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::HostCall {
                request: HostRequest::ImportPicture { .. },
                ..
            })
        ));
    }

    #[test]
    fn a_trusted_page_click_is_a_gesture_with_the_same_rules() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("index.html"), "<p>hi</p>").expect("page written");
        let (io, mut rx) = io_with("board", "Board");
        let side = WebviewPlace::SidePanel;
        io.webviews()
            .set_root("board", Some(dir.path().to_path_buf()), vec![side]);
        let now = Instant::now();
        assert!(
            !io.page_gesture("board", side, now),
            "no frame is open, so the report is forged"
        );
        io.webviews()
            .show("board", side, "index.html", now)
            .expect("side panel frame");
        assert!(
            !io.page_gesture("board", WebviewPlace::Dialog, now),
            "a report for a frame that is not open does not count"
        );
        assert!(matches!(
            io.files_save("board", "a.png".into(), vec![1]),
            Err(Error::NotPermitted)
        ));
        if let Some(stale) = now.checked_sub(chat::GESTURE_WINDOW + Duration::from_secs(1)) {
            let (old, _old_rx) = io_with("old", "Old");
            old.webviews()
                .set_root("old", Some(dir.path().to_path_buf()), vec![side]);
            old.webviews()
                .show("old", side, "index.html", stale)
                .expect("frame");
            assert!(old.page_gesture("old", side, stale));
            assert!(
                matches!(
                    old.files_save("old", "a.png".into(), vec![1]),
                    Err(Error::NotPermitted)
                ),
                "the gesture window is 8 s"
            );
        }
        assert!(io.page_gesture("board", side, now));
        assert!(io.files_save("board", "a.png".into(), vec![1]).is_ok());
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::HostCall {
                request: HostRequest::Save { .. },
                ..
            })
        ));
        assert!(
            matches!(
                io.files_save("board", "b.png".into(), vec![1]),
                Err(Error::NotPermitted)
            ),
            "one save per gesture"
        );
        assert!(
            !io.page_gesture("board", side, now + Duration::from_millis(50)),
            "reports right after another do not count"
        );
        assert!(matches!(
            io.files_save("board", "b.png".into(), vec![1]),
            Err(Error::NotPermitted)
        ));
        assert!(io.page_gesture("board", side, Instant::now() + Duration::from_secs(1)));
        assert!(io.files_save("board", "b.png".into(), vec![1]).is_ok());
    }

    #[test]
    fn post_image_goes_to_the_current_server_with_the_name_line() {
        let (io, mut rx) = io_with("board", "Board");
        io.gesture("board");
        assert!(matches!(
            io.chat_post_image("board", vec![1], "Sketch".into(), None),
            Err(Error::NoRecipients)
        ));
        io.peers().focused = Some(3);
        assert!(
            io.chat_post_image("board", vec![1], "Sketch".into(), None)
                .is_ok(),
            "not connected kept the gesture"
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::HostCall {
                request: HostRequest::PostImage { server: 3, line, .. },
                ..
            }) if line == "[Board] Sketch"
        ));
        assert!(matches!(
            io.chat_post_image("board", vec![1], String::new(), None),
            Err(Error::NotPermitted)
        ));
    }

    #[test]
    fn a_card_replaces_its_line_in_either_order() {
        let (events, mut rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        io.peers().plugins.insert(
            "poll".into(),
            PluginMeta {
                name: "Poll".into(),
                messaging: None,
                stage: None,
            },
        );
        let hash = chat::line_hash("Poll", "Lunch?");
        assert_eq!(io.chat_text(2, 11, 3, "[Poll] Lunch?"), Supersede::None);
        io.supersede("poll", "c1".into(), 3, hash, Some(2))
            .expect("ok");
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Supersede { card, message: 11, .. }) if card.as_deref() == Some("c1")
        ));
    }

    #[test]
    fn a_line_arriving_after_its_card_is_replaced_live() {
        let (events, mut rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        io.peers().plugins.insert(
            "poll".into(),
            PluginMeta {
                name: "Poll".into(),
                messaging: None,
                stage: None,
            },
        );
        let hash = chat::line_hash("Poll", "Lunch?");
        io.supersede("poll", "c1".into(), 3, hash, Some(2))
            .expect("ok");
        assert!(rx.try_recv().is_err(), "the line is not there yet");
        io.chat_text(1, 11, 3, "[Poll] Lunch?");
        assert!(rx.try_recv().is_err(), "another server's line stays");
        io.chat_text(2, 12, 3, "[Poll] Lunch?");
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Supersede { plugin, card, message: 12 })
                if plugin == "poll" && card.as_deref() == Some("c1")
        ));
    }

    #[test]
    fn replies_and_cards_stay_in_the_chat_their_event_came_from() {
        let (events, mut rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        io.peers().plugins.insert(
            "poll".into(),
            PluginMeta {
                name: "Poll".into(),
                messaging: None,
                stage: None,
            },
        );
        // The user talks on server 1 and reads the chat of server 2.
        io.peers().focused = Some(1);

        io.gesture("poll");
        io.chat_send("poll", "Lunch?".into(), Some(2))
            .expect("sent");
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::ChatPost { server: 2, .. })
        ));
        let card = Slot::ChatCard("c1".into());
        io.set_tree("poll", card.clone(), vec![], Some(2))
            .expect("shown");
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Ui {
                update: UiUpdate::SetTree {
                    server: Some(2),
                    ..
                },
                ..
            })
        ));
        io.set_tree("poll", Slot::Panel, vec![], Some(2))
            .expect("shown");
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Ui {
                update: UiUpdate::SetTree { server: None, .. },
                ..
            })
        ));

        // Same session number on both servers: only server 2's line is the card's.
        let hash = chat::line_hash("Poll", "Lunch?");
        io.chat_text(1, 10, 3, "[Poll] Lunch?");
        io.supersede("poll", "c1".into(), 3, hash, Some(2))
            .expect("ok");
        assert!(rx.try_recv().is_err(), "server 1's line stays");
        assert_eq!(
            io.chat_text(2, 11, 3, "[Poll] Lunch?"),
            Supersede::Replace {
                plugin: "poll".into(),
                card: "c1".into(),
                message: 11
            }
        );
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Supersede { message: 11, .. })
        ));

        // Outside a chat event (a timer, an old plugin): the voice server, as before.
        io.gesture("poll");
        io.chat_send("poll", "Results".into(), None).expect("sent");
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::ChatPost { server: 1, .. })
        ));
        io.set_tree("poll", card, vec![], None).expect("shown");
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Ui {
                update: UiUpdate::SetTree {
                    server: Some(1),
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn only_the_plugin_handed_a_message_may_replace_or_hide_it() {
        let (events, mut rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        let token = io.issue_token("fit", 1, 42);
        assert_eq!(
            io.replace_message("dice", token, Some("c1".into())),
            Err(Error::NotPermitted)
        );
        assert_eq!(
            io.replace_message("fit", token ^ 1, None),
            Err(Error::NotPermitted)
        );
        assert!(rx.try_recv().is_err(), "nothing was hidden");
        io.replace_message("fit", token, Some("c1".into()))
            .expect("its own message");
        assert!(matches!(
            rx.try_recv(),
            Ok(ServiceEvent::Supersede { card, message: 42, .. }) if card.as_deref() == Some("c1")
        ));
        io.forget("fit");
        assert_eq!(
            io.replace_message("fit", token, None),
            Err(Error::NotPermitted),
            "claims end with the plugin"
        );
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
