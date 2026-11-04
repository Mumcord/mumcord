//! What the user does in a plugin's UI, delivered as plugin events (FR-6.16, FR-6.43, FR-6.44).
//!
//! The app only knows sessions; the service turns them into peers with the plugin's own peer
//! keys, and opens the gesture window that lets the plugin post one chat line.

use super::PluginService;
use super::plugin_io::{PeerInfo, PeersState, wit_peer};
use crate::api::app_menu::{self, AppMenuEntry};
use crate::api::webview::WebviewPlace;
use crate::host::RunState;
use crate::wit::events::{ActionSource, ActionValue, Event, PanelVisibility, UiAction};

/// One user input for a plugin's UI.
#[derive(Debug, Clone)]
pub enum UiInput {
    /// A click, toggle, typed value … or a slash command (`ActionSource::Command`).
    Action {
        source: ActionSource,
        action: String,
        value: ActionValue,
        /// The server whose chat the action came from (a chat card, a typed slash command):
        /// a reply or new card from the handler goes there. `None`: the voice server.
        server: Option<mumcord_store::ServerId>,
    },
    /// The user opened the profile card of `session`.
    ProfileCardOpened(u32),
    /// The plugin's panel was hidden, shown inline or expanded.
    PanelVisibility(PanelVisibility),
    /// The user chose the plugin's entry in the right-click menu of `session`.
    UserMenuRequested(u32),
    /// The user closed the plugin's dialog.
    DialogClosed,
    /// A bridge message from the plugin's page in `place` (untrusted, never logged). Dropped
    /// unless that frame is open and the message fits its size limit and the rate. The plugin
    /// gets the text only: `webview-message` carries no place (pages that share one plugin tell
    /// themselves apart in their own payload).
    WebviewMessage { place: WebviewPlace, text: String },
    /// A genuine click or key press inside the plugin's page in `place`, reported by the page's
    /// bootstrap with its frame key (checked by the app). Counts as a user gesture for the plugin
    /// (FR-9.45) while that frame is open, at most every [`MIN_GESTURE_INTERVAL`]; the plugin
    /// gets no event.
    ///
    /// [`MIN_GESTURE_INTERVAL`]: crate::api::webview::MIN_GESTURE_INTERVAL
    WebviewGesture(WebviewPlace),
    /// The user closed the plugin's frame in `place` (its close button), or another plugin's
    /// side panel replaced it ([`super::ServiceEvent::SurfaceClosed`]). The frame closes; a
    /// plugin built for API 1.1 gets `surface-closed`.
    SurfaceClosed(WebviewPlace),
}

impl PluginService {
    /// The web view state, shared with the app's `mumcord-web` protocol and bridge (FR-6.47).
    pub fn webviews(&self) -> std::sync::Arc<crate::api::webview::Webviews> {
        std::sync::Arc::clone(self.io.webviews())
    }

    /// The Apps-menu items of the running plugins that hold `ui.app-menu`, in menu order
    /// (plugins by id, items as in the manifest). The UI shows the first 8 and puts the rest
    /// under "More plugins…" (FR-9.53).
    pub fn app_menu(&self) -> Vec<AppMenuEntry> {
        let shown = self
            .loaded_ids()
            .into_iter()
            .filter(|id| self.menu_shown(id))
            .filter_map(|id| {
                let name = self.host.manifest(&id)?.name.clone();
                Some((id, name))
            })
            .collect();
        self.io.app_menus().entries(&shown)
    }

    fn menu_shown(&self, id: &str) -> bool {
        matches!(self.host.state(id), Some(RunState::Running))
            && self.ctx.grants.allows(id, app_menu::PERMISSION)
    }

    /// The user chose Apps-menu item `item` of plugin `id`: opens the 8 s chat gesture window
    /// and sends `app-menu-activated` (FR-9.55). Returns false, sending nothing, when the item
    /// is unknown or disabled, or the plugin is not running or lacks `ui.app-menu`.
    pub fn app_menu_activate(&self, id: &str, item: &str) -> bool {
        if !self.menu_shown(id) || !self.io.app_menus().is_enabled(id, item) {
            return false;
        }
        self.command_gesture(id);
        self.host.send_app_menu_activated(id, item)
    }

    /// Delivers a UI input to a running plugin. Returns false when the plugin is not running,
    /// lacks the permission for the event, or `session` is not someone the plugin may see (the
    /// user or a member of the user's channel on the focused server).
    pub fn ui_input(&self, id: &str, input: UiInput) -> bool {
        let keys = self.ctx.peer_keys.as_ref();
        let peer = |session| find_peer(&self.io.peers(), session).map(|p| wit_peer(keys, id, &p));
        let mut chat = crate::chat::ChatRef::default();
        let event = match input {
            UiInput::Action {
                source,
                action,
                value,
                server,
            } => {
                chat.server = server;
                if matches!(source, ActionSource::Command) {
                    self.command_gesture(id);
                } else {
                    self.gesture(id);
                }
                Event::UiAction(UiAction {
                    source,
                    action,
                    value,
                })
            }
            UiInput::ProfileCardOpened(session) => match peer(session) {
                Some(peer) => Event::ProfileCardOpened(peer),
                None => return false,
            },
            UiInput::PanelVisibility(visibility) => Event::PanelVisibilityChanged(visibility),
            UiInput::UserMenuRequested(session) => match peer(session) {
                Some(peer) => Event::UserMenuRequested(peer),
                None => return false,
            },
            UiInput::DialogClosed => Event::DialogClosed,
            UiInput::WebviewMessage { place, text } => {
                if !self
                    .io
                    .webviews()
                    .accept_from_page(id, place, &text, std::time::Instant::now())
                {
                    return false;
                }
                Event::WebviewMessage(text)
            }
            UiInput::WebviewGesture(place) => {
                let counted =
                    self.io
                        .webviews()
                        .accept_gesture(id, place, std::time::Instant::now());
                if counted {
                    self.gesture(id);
                }
                return counted;
            }
            UiInput::SurfaceClosed(place) => {
                self.io.webviews().close(id, place);
                return self.host.send_surface_closed(id, place);
            }
        };
        self.host.send_chat(id, event, chat)
    }
}

/// The user or a member of the user's channel on the focused server.
fn find_peer(peers: &PeersState, session: u32) -> Option<PeerInfo> {
    let connection = peers.connections.get(&peers.focused?)?;
    if let Some(me) = connection.me.as_ref().filter(|me| me.me.session == session) {
        return Some(me.me.clone());
    }
    connection.members.get(&session).cloned()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::service::plugin_io::{Connection, SelfInfo};

    fn peer(session: u32, name: &str) -> PeerInfo {
        PeerInfo {
            session,
            name: name.into(),
            cert_hash: None,
            is_friend: false,
        }
    }

    #[test]
    fn only_the_user_and_channel_members_of_the_focused_server_are_found() {
        let mut state = PeersState::default();
        let mut connection = Connection::new(Instant::now());
        connection.me = Some(SelfInfo {
            me: peer(1, "Me"),
            channel: None,
            muted: false,
            deafened: false,
        });
        connection.members.insert(7, peer(7, "Bob"));
        state.connections.insert(3, connection);

        assert!(find_peer(&state, 7).is_none(), "no focused server");
        state.focused = Some(3);
        assert_eq!(find_peer(&state, 1).map(|p| p.name), Some("Me".into()));
        assert_eq!(find_peer(&state, 7).map(|p| p.name), Some("Bob".into()));
        assert!(find_peer(&state, 9).is_none(), "not in the channel");
        state.focused = Some(4);
        assert!(find_peer(&state, 7).is_none(), "another server");
    }
}
