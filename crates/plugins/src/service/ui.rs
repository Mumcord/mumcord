//! What the user does in a plugin's UI, delivered as plugin events (FR-6.16, FR-6.43, FR-6.44).
//!
//! The app only knows sessions; the service turns them into peers with the plugin's own peer
//! keys, and opens the gesture window that lets the plugin post one chat line.

use super::PluginService;
use super::plugin_io::{PeerInfo, PeersState, wit_peer};
use crate::wit::events::{ActionSource, ActionValue, Event, PanelVisibility, UiAction};

/// One user input for a plugin's UI.
#[derive(Debug, Clone)]
pub enum UiInput {
    /// A click, toggle, typed value … or a slash command (`ActionSource::Command`).
    Action {
        source: ActionSource,
        action: String,
        value: ActionValue,
    },
    /// The user opened the profile card of `session`.
    ProfileCardOpened(u32),
    /// The plugin's panel was hidden, shown inline or expanded.
    PanelVisibility(PanelVisibility),
    /// The user chose the plugin's entry in the right-click menu of `session`.
    UserMenuRequested(u32),
    /// The user closed the plugin's dialog.
    DialogClosed,
    /// A bridge message from the plugin's web view page (untrusted, never logged). Dropped
    /// unless the web view is open and the message fits the size and rate limits.
    WebviewMessage(String),
}

impl PluginService {
    /// The web view state, shared with the app's `mumcord-web` protocol and bridge (FR-6.47).
    pub fn webviews(&self) -> std::sync::Arc<crate::api::webview::Webviews> {
        std::sync::Arc::clone(self.io.webviews())
    }

    /// Delivers a UI input to a running plugin. Returns false when the plugin is not running,
    /// lacks the permission for the event, or `session` is not someone the plugin may see (the
    /// user or a member of the user's channel on the focused server).
    pub fn ui_input(&self, id: &str, input: UiInput) -> bool {
        let keys = self.ctx.peer_keys.as_ref();
        let peer = |session| find_peer(&self.io.peers(), session).map(|p| wit_peer(keys, id, &p));
        let event = match input {
            UiInput::Action {
                source,
                action,
                value,
            } => {
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
            UiInput::WebviewMessage(text) => {
                if !self
                    .io
                    .webviews()
                    .accept_from_page(id, &text, std::time::Instant::now())
                {
                    return false;
                }
                Event::WebviewMessage(text)
            }
        };
        self.host.send(id, event)
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
