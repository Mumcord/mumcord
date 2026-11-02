//! User-menu entries (`ui.set-user-menu-entry` / `set-user-menu-entry-for`). A filtered entry
//! shows only on users whose bot info (from their Mumcord hello) matches, or whose session the
//! plugin listed. The host resolves the filter to the matching sessions of the user's channel on
//! the focused server and pushes a new list whenever it changes, so the UI only compares
//! sessions.

use std::collections::BTreeSet;

use super::{Connection, PeersState, ServiceIo, UiUpdate};
use crate::wit::types::{BotInfo as WitBotInfo, Error};
use crate::wit::ui::UserMenuFilter;

/// What a bot says about itself in its hello (engine `PluginBot`), already sanitised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotInfo {
    pub name: String,
    pub modules: Vec<String>,
}

impl From<&BotInfo> for WitBotInfo {
    fn from(bot: &BotInfo) -> Self {
        Self {
            name: bot.name.clone(),
            modules: bot.modules.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::service) struct MenuFilter {
    bot_name: Option<String>,
    bot_module: Option<String>,
    sessions: BTreeSet<u32>,
}

impl From<UserMenuFilter> for MenuFilter {
    fn from(filter: UserMenuFilter) -> Self {
        Self {
            bot_name: filter.bot_name,
            bot_module: filter.bot_module,
            sessions: filter.sessions.into_iter().collect(),
        }
    }
}

impl MenuFilter {
    fn matches(&self, session: u32, bot: Option<&BotInfo>) -> bool {
        let wants_bot = self.bot_name.is_some() || self.bot_module.is_some();
        let bot_matches = bot.is_some_and(|bot| {
            self.bot_name.as_ref().is_none_or(|name| *name == bot.name)
                && self
                    .bot_module
                    .as_ref()
                    .is_none_or(|module| bot.modules.contains(module))
        });
        (wants_bot && bot_matches) || self.sessions.contains(&session)
    }

    /// The matching users of the user's channel, sorted.
    fn resolve(&self, connection: Option<&Connection>) -> Vec<u32> {
        connection.map_or_else(Vec::new, |c| {
            c.members
                .keys()
                .copied()
                .filter(|session| self.matches(*session, c.bots.get(session)))
                .collect()
        })
    }
}

/// One plugin's entry; `filter` is `None` for an entry on everyone.
#[derive(Debug)]
pub(in crate::service) struct MenuEntry {
    label: String,
    filter: Option<MenuFilter>,
    /// The sessions last pushed for a filtered entry.
    shown: Vec<u32>,
}

impl PeersState {
    fn focused_connection(&self) -> Option<&Connection> {
        self.focused
            .and_then(|server| self.connections.get(&server))
    }
}

impl ServiceIo {
    pub(super) fn menu_set(&self, plugin: &str, label: Option<String>) {
        let mut state = self.peers();
        match &label {
            Some(label) => {
                state.menus.insert(
                    plugin.to_owned(),
                    MenuEntry {
                        label: label.clone(),
                        filter: None,
                        shown: Vec::new(),
                    },
                );
            }
            None => {
                state.menus.remove(plugin);
            }
        }
        drop(state);
        self.ui(
            plugin,
            UiUpdate::UserMenuEntry {
                label,
                sessions: None,
            },
        );
    }

    pub(super) fn menu_set_filtered(&self, plugin: &str, label: String, filter: UserMenuFilter) {
        let filter = MenuFilter::from(filter);
        let mut state = self.peers();
        let shown = filter.resolve(state.focused_connection());
        state.menus.insert(
            plugin.to_owned(),
            MenuEntry {
                label: label.clone(),
                filter: Some(filter),
                shown: shown.clone(),
            },
        );
        drop(state);
        self.ui(
            plugin,
            UiUpdate::UserMenuEntry {
                label: Some(label),
                sessions: Some(shown),
            },
        );
    }

    /// `peers.bot`: only for users in the user's channel on the focused server.
    pub(super) fn bot_of(&self, session: u32) -> Result<Option<WitBotInfo>, Error> {
        let state = self.peers();
        Ok(state
            .focused_connection()
            .filter(|c| c.members.contains_key(&session))
            .and_then(|c| c.bots.get(&session))
            .map(WitBotInfo::from))
    }

    /// Re-resolves every filtered entry after people, bot info or the focused server changed,
    /// and pushes the ones whose sessions changed.
    pub fn refresh_menus(&self) {
        let mut updates = Vec::new();
        {
            let mut state = self.peers();
            let PeersState {
                focused,
                connections,
                menus,
                ..
            } = &mut *state;
            let connection = focused.and_then(|server| connections.get(&server));
            for (plugin, entry) in menus.iter_mut() {
                let Some(filter) = &entry.filter else {
                    continue;
                };
                let shown = filter.resolve(connection);
                if shown != entry.shown {
                    entry.shown = shown.clone();
                    updates.push((plugin.clone(), entry.label.clone(), shown));
                }
            }
        }
        for (plugin, label, sessions) in updates {
            self.ui(
                &plugin,
                UiUpdate::UserMenuEntry {
                    label: Some(label),
                    sessions: Some(sessions),
                },
            );
        }
    }

    /// Drops sessions from the plugins' lists: `Some` one user who left the channel, `None`
    /// all of them (another server is focused, where sessions mean other people).
    pub fn menu_forget_sessions(&self, session: Option<u32>) {
        for entry in self.peers().menus.values_mut() {
            if let Some(filter) = &mut entry.filter {
                match session {
                    Some(session) => {
                        filter.sessions.remove(&session);
                    }
                    None => filter.sessions.clear(),
                }
            }
        }
    }

    pub(super) fn menu_forget_plugin(&self, plugin: &str) {
        self.peers().menus.remove(plugin);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

    use super::super::{PeerInfo, ServiceEvent};
    use super::*;
    use crate::api::io::PluginIo;

    const SERVER: mumcord_store::ServerId = 1;

    fn person(session: u32) -> PeerInfo {
        PeerInfo {
            session,
            name: format!("u{session}"),
            cert_hash: None,
            is_friend: false,
        }
    }

    fn mumbot(modules: &[&str]) -> BotInfo {
        BotInfo {
            name: "mumbot".into(),
            modules: modules.iter().map(|m| (*m).to_owned()).collect(),
        }
    }

    fn setup() -> (ServiceIo, UnboundedReceiver<ServiceEvent>) {
        let (events, rx) = unbounded_channel();
        let io = ServiceIo::new(None, events);
        {
            let mut state = io.peers();
            state.focused = Some(SERVER);
            let mut connection = Connection::new(Instant::now());
            for session in [2, 3, 4] {
                connection.members.insert(session, person(session));
            }
            connection.bots.insert(2, mumbot(&["music"]));
            connection.bots.insert(3, mumbot(&["radio"]));
            state.connections.insert(SERVER, connection);
        }
        (io, rx)
    }

    fn pushed(rx: &mut UnboundedReceiver<ServiceEvent>) -> Vec<(Option<String>, Option<Vec<u32>>)> {
        let mut out = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let ServiceEvent::Ui {
                update: UiUpdate::UserMenuEntry { label, sessions },
                ..
            } = event
            {
                out.push((label, sessions));
            }
        }
        out
    }

    fn filter(name: Option<&str>, module: Option<&str>, sessions: &[u32]) -> UserMenuFilter {
        UserMenuFilter {
            bot_name: name.map(str::to_owned),
            bot_module: module.map(str::to_owned),
            sessions: sessions.to_vec(),
        }
    }

    #[test]
    fn a_filter_matches_bot_info_or_listed_sessions() {
        let (io, mut rx) = setup();
        let label = || Some("Configure bot".to_owned());
        let cases = [
            (filter(Some("mumbot"), Some("music"), &[]), vec![2]),
            (filter(Some("mumbot"), None, &[]), vec![2, 3]),
            (filter(None, Some("radio"), &[4]), vec![3, 4]),
            (filter(Some("otherbot"), None, &[]), vec![]),
            (filter(None, None, &[4, 99]), vec![4]),
        ];
        for (f, expected) in cases {
            io.set_user_menu_entry_for("music", "Configure bot".into(), f)
                .expect("accepted");
            assert_eq!(pushed(&mut rx), [(label(), Some(expected))]);
        }
        io.set_user_menu_entry("music", label()).expect("accepted");
        assert_eq!(pushed(&mut rx), [(label(), None)], "unfiltered: everyone");
        io.set_user_menu_entry("music", None).expect("accepted");
        assert_eq!(pushed(&mut rx), [(None, None)]);
    }

    #[test]
    fn entries_follow_people_and_bot_info() {
        let (io, mut rx) = setup();
        let label = || Some("Configure bot".to_owned());
        io.set_user_menu_entry_for(
            "music",
            "Configure bot".into(),
            filter(None, Some("music"), &[4]),
        )
        .expect("accepted");
        assert_eq!(pushed(&mut rx), [(label(), Some(vec![2, 4]))]);

        io.refresh_menus();
        assert!(
            pushed(&mut rx).is_empty(),
            "nothing changed, nothing pushed"
        );

        io.peers()
            .connections
            .get_mut(&SERVER)
            .expect("connection")
            .bots
            .insert(3, mumbot(&["music"]));
        io.refresh_menus();
        assert_eq!(pushed(&mut rx), [(label(), Some(vec![2, 3, 4]))]);

        io.menu_forget_sessions(Some(4));
        io.peers()
            .connections
            .get_mut(&SERVER)
            .expect("connection")
            .members
            .remove(&4);
        io.refresh_menus();
        assert_eq!(pushed(&mut rx), [(label(), Some(vec![2, 3]))]);

        io.peers().focused = None;
        io.refresh_menus();
        assert_eq!(pushed(&mut rx), [(label(), Some(vec![]))]);
    }

    #[test]
    fn bot_info_is_only_given_for_the_channel() {
        let (io, _rx) = setup();
        assert_eq!(
            io.bot("music", 2).expect("ok").map(|b| b.name),
            Some("mumbot".into())
        );
        assert!(io.bot("music", 4).expect("ok").is_none(), "a person");
        io.peers()
            .connections
            .get_mut(&SERVER)
            .expect("connection")
            .members
            .remove(&2);
        assert!(
            io.bot("music", 2).expect("ok").is_none(),
            "not in the channel"
        );
    }
}
