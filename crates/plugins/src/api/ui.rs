//! `ui` (FR-6.40): plugin UI trees, the dialog and the user-menu entry. Each slot needs its own
//! permission; the rest is the [`PluginIo`](super::io::PluginIo)'s job.

use wasmtime::component::{HasSelf, Linker};

use crate::runtime::bindings::mumcord::plugin::ui::{Host, Node, Slot, add_to_linker};
use crate::runtime::wit::types::Error;
use crate::runtime::{HostApi, PluginState};

pub const DIALOG_PERMISSION: &str = "ui.dialog";
pub const USER_MENU_PERMISSION: &str = "ui.user-menu";

pub(crate) struct UiApi;

impl HostApi for UiApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
        add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
    }
}

/// The permission a slot needs; the settings form needs none.
pub fn slot_permission(slot: &Slot) -> Option<&'static str> {
    match slot {
        Slot::Panel => Some("ui.panel"),
        Slot::MemberBadge(_) => Some("ui.member-badge"),
        Slot::ProfileCard(_) => Some("ui.profile-card"),
        Slot::ChatCard(_) | Slot::ChatDecoration(_) => Some("ui.chat-card"),
        Slot::Settings => None,
        Slot::Dialog => Some(DIALOG_PERMISSION),
    }
}

impl PluginState {
    fn check_slot(&self, slot: &Slot) -> Result<(), Error> {
        match slot_permission(slot) {
            Some(permission) => self.ctx.grants.check(&self.id, permission),
            None => Ok(()),
        }
    }
}

impl Host for PluginState {
    fn set_tree(&mut self, slot: Slot, nodes: Vec<Node>) -> Result<(), Error> {
        self.check_slot(&slot)?;
        self.io()?.set_tree(&self.id, slot, nodes)
    }

    fn clear(&mut self, slot: Slot) {
        if self.check_slot(&slot).is_ok()
            && let Ok(io) = self.io()
        {
            io.clear(&self.id, slot);
        }
    }

    fn open_dialog(&mut self, title: String) -> Result<(), Error> {
        self.ctx.grants.check(&self.id, DIALOG_PERMISSION)?;
        self.io()?.open_dialog(&self.id, title)
    }

    fn close_dialog(&mut self) {
        if self.ctx.grants.allows(&self.id, DIALOG_PERMISSION)
            && let Ok(io) = self.io()
        {
            io.close_dialog(&self.id);
        }
    }

    fn set_user_menu_entry(&mut self, label: Option<String>) -> Result<(), Error> {
        self.ctx.grants.check(&self.id, USER_MENU_PERMISSION)?;
        self.io()?.set_user_menu_entry(&self.id, label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_decorations_share_the_chat_card_permission() {
        assert_eq!(
            slot_permission(&Slot::ChatDecoration(3)),
            Some("ui.chat-card")
        );
        assert_eq!(
            slot_permission(&Slot::ChatCard("c".into())),
            Some("ui.chat-card")
        );
        assert_eq!(slot_permission(&Slot::Settings), None);
        assert_eq!(slot_permission(&Slot::Dialog), Some(DIALOG_PERMISSION));
    }
}
