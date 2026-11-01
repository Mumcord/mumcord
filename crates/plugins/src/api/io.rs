//! The seam between the `ui`, `peers` and `chat` host functions and whoever carries them out:
//! the plugin service in the client (UI pushes, plugin messaging, chat posting) and the mock host
//! of `mumcord-plugin test`. The host functions check the permission first and then hand the
//! call over unchanged; without a [`PluginIo`] these calls return `not-permitted`.

use std::fmt::Debug;

use crate::runtime::wit::peers::Target;
use crate::runtime::wit::types::{BotInfo, Error, Peer};
use crate::runtime::wit::ui::{Node, Slot, UserMenuFilter};
use crate::runtime::wit::webview::Place;

/// Carries out what a plugin asks of the UI, other users and the chat. `plugin` is the id of
/// the calling plugin; its permission for the call has already been checked.
///
/// Every method runs on the plugin's own thread inside its call budget (50 ms, 500 ms during
/// `start`), so it must return quickly: queue work, don't wait for the network or the UI.
pub trait PluginIo: Send + Sync + Debug {
    /// `ui.set-tree`: replaces the tree of a slot.
    fn set_tree(&self, plugin: &str, slot: Slot, nodes: Vec<Node>) -> Result<(), Error>;
    /// `ui.clear`.
    fn clear(&self, plugin: &str, slot: Slot);
    /// `ui.open-dialog`.
    fn open_dialog(&self, plugin: &str, title: String) -> Result<(), Error>;
    /// `ui.close-dialog`.
    fn close_dialog(&self, plugin: &str);
    /// `ui.set-user-menu-entry`.
    fn set_user_menu_entry(&self, plugin: &str, label: Option<String>) -> Result<(), Error>;
    /// `ui.set-user-menu-entry-for`: the entry only for users matching `filter` (label and
    /// session count already checked).
    fn set_user_menu_entry_for(
        &self,
        plugin: &str,
        label: String,
        filter: UserMenuFilter,
    ) -> Result<(), Error>;
    /// `ui.open-panel-popover`: the caller (api/ui.rs) checked `ui.panel`; the implementation
    /// checks that a user gesture is fresh and answers not-permitted otherwise.
    fn open_panel_popover(&self, plugin: &str) -> Result<(), Error>;
    /// `ui.close-panel-popover`.
    fn close_panel_popover(&self, plugin: &str);
    /// `peers.send`.
    fn peer_send(&self, plugin: &str, to: Target, kind: u8, payload: Vec<u8>) -> Result<(), Error>;
    /// `peers.start-game-burst`.
    fn start_game_burst(&self, plugin: &str) -> Result<(), Error>;
    /// `peers.presence`: the user's channel, the user first.
    fn presence(&self, plugin: &str) -> Result<Vec<Peer>, Error>;
    /// `peers.bot`: the bot info of a user in the user's channel.
    fn bot(&self, plugin: &str, session: u32) -> Result<Option<BotInfo>, Error>;
    /// `chat.send`: posts "[Name] text" if a user gesture allows it.
    fn chat_send(&self, plugin: &str, text: String) -> Result<(), Error>;
    /// `chat.line-hash`: the hash of the line `chat_send` would post (it includes the plugin's
    /// name, which only the implementation knows).
    fn line_hash(&self, plugin: &str, text: &str) -> u32;
    /// `chat.supersede`.
    fn supersede(&self, plugin: &str, card_id: String, sender: u32, hash: u32)
    -> Result<(), Error>;
    /// `webview.show` (FR-6.46). Hosts without web views refuse it.
    fn webview_show(&self, _plugin: &str, _place: Place, _entry: String) -> Result<(), Error> {
        Err(Error::NotPermitted)
    }
    /// `webview.hide`.
    fn webview_hide(&self, _plugin: &str) {}
    /// `webview.post`: a message to the plugin's page.
    fn webview_post(&self, _plugin: &str, _message: String) -> Result<(), Error> {
        Err(Error::NotPermitted)
    }
}
