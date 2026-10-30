//! The tray menu as plain data (FR-3.132), drawn by both tray backends: the state of every
//! server, Mute, Deafen, Do Not Disturb, the voice server and channel and recent chats submenus,
//! Open, Settings and Quit.

use crate::commands::types::{Conversation, ServerId};

use super::Labels;
use super::state::{Conn, Server, TrayState};

/// Longest channel list in the menu; larger servers are browsed in the window.
const MAX_CHANNELS: usize = 100;

/// What a tray menu item does when clicked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Show,
    Mute,
    Deafen,
    Dnd,
    /// Moves voice to this server, into the channel the user is in there.
    VoiceServer(ServerId),
    Join {
        server_id: ServerId,
        channel_id: u32,
    },
    OpenChat {
        server_id: ServerId,
        conversation: Conversation,
    },
    Settings,
    Quit,
}

/// The coloured dot in front of a menu row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dot {
    Connected,
    Connecting,
    Disconnected,
    Unread,
}

impl Dot {
    pub fn colour(self) -> [u8; 3] {
        match self {
            Self::Connected => [35, 165, 90],
            Self::Connecting => [240, 178, 50],
            Self::Disconnected => [128, 132, 142],
            Self::Unread => [167, 139, 250],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// Greyed-out text: a server's state or a section title.
    Info {
        label: String,
        dot: Option<Dot>,
    },
    Separator,
    Button {
        action: Action,
        label: String,
        dot: Option<Dot>,
    },
    Toggle {
        action: Action,
        label: String,
        enabled: bool,
        checked: bool,
    },
    /// Disabled while it has no entries.
    Submenu {
        label: String,
        entries: Vec<Entry>,
    },
}

pub fn menu_model(state: &TrayState, labels: &Labels) -> Vec<Entry> {
    let mut menu: Vec<Entry> = state
        .servers
        .iter()
        .map(|s| server_row(s, labels))
        .collect();
    if menu.is_empty() {
        menu.push(Entry::Info {
            label: labels.disconnected.clone(),
            dot: Some(Dot::Disconnected),
        });
    }
    let connected = state.connected().next().is_some();
    let target = state.voice_target();
    menu.extend([
        Entry::Separator,
        Entry::Toggle {
            action: Action::Mute,
            label: labels.mute.clone(),
            enabled: connected,
            checked: target.is_some_and(|s| s.muted),
        },
        Entry::Toggle {
            action: Action::Deafen,
            label: labels.deafen.clone(),
            enabled: connected,
            checked: target.is_some_and(|s| s.deafened),
        },
        Entry::Toggle {
            action: Action::Dnd,
            label: if state.dnd.system && !state.dnd.manual {
                labels.dnd_system.clone()
            } else {
                labels.dnd.clone()
            },
            // The Mumcord switch cannot turn off the system's Do Not Disturb (FR-3.112).
            enabled: !(state.dnd.system && !state.dnd.manual),
            checked: state.dnd.active(),
        },
        Entry::Separator,
        Entry::Submenu {
            label: labels.voice_menu.clone(),
            entries: voice_entries(state, labels),
        },
        Entry::Submenu {
            label: labels.recent_chats.clone(),
            entries: chat_entries(state, labels),
        },
        Entry::Separator,
        Entry::Button {
            action: Action::Show,
            label: labels.show.clone(),
            dot: None,
        },
        Entry::Button {
            action: Action::Settings,
            label: labels.settings.clone(),
            dot: None,
        },
        Entry::Button {
            action: Action::Quit,
            label: labels.quit.clone(),
            dot: None,
        },
    ]);
    menu
}

fn server_row(server: &Server, labels: &Labels) -> Entry {
    let (state, dot) = match server.conn {
        Conn::Connected => {
            let channel = server
                .own_channel()
                .and_then(|id| server.channels.get(&id))
                .map(|c| c.name.as_str());
            let text = match channel {
                Some(channel) => fill(&labels.connected_in, &[("channel", channel)]),
                None => labels.connected.clone(),
            };
            (text, Dot::Connected)
        }
        Conn::Connecting => (labels.connecting.clone(), Dot::Connecting),
        Conn::Reconnecting => (labels.reconnecting.clone(), Dot::Connecting),
        Conn::Lost => (labels.disconnected.clone(), Dot::Disconnected),
    };
    Entry::Info {
        label: fill(
            &labels.server_state,
            &[("server", &server.name), ("state", &state)],
        ),
        dot: Some(dot),
    }
}

/// "Voice server" with every connected server, then the channels of the voice server.
fn voice_entries(state: &TrayState, labels: &Labels) -> Vec<Entry> {
    let Some(shown) = state.channel_server() else {
        return Vec::new();
    };
    let mut entries = vec![Entry::Info {
        label: labels.voice_server.clone(),
        dot: None,
    }];
    entries.extend(state.connected().map(|s| {
        let current = state.voice == Some(s.id);
        Entry::Toggle {
            action: Action::VoiceServer(s.id),
            label: s.name.clone(),
            enabled: !current && s.own_channel().is_some(),
            checked: current,
        }
    }));
    entries.push(Entry::Separator);
    entries.push(Entry::Info {
        label: fill(&labels.channels_of, &[("server", &shown.name)]),
        dot: None,
    });
    let channels = channel_rows(shown);
    let more = channels.len() > MAX_CHANNELS;
    let own = shown.own_channel();
    let voice_here = state.voice == Some(shown.id);
    entries.extend(channels.into_iter().take(MAX_CHANNELS).map(|row| {
        let here = voice_here && own == Some(row.id);
        let label = if row.users > 0 {
            format!("{} ({})", row.path, row.users)
        } else {
            row.path
        };
        Entry::Toggle {
            action: Action::Join {
                server_id: shown.id,
                channel_id: row.id,
            },
            label,
            enabled: row.enter && !here,
            checked: here,
        }
    }));
    if more {
        entries.push(Entry::Button {
            action: Action::Show,
            label: labels.more_channels.clone(),
            dot: None,
        });
    }
    entries
}

struct ChannelRow {
    id: u32,
    path: String,
    users: usize,
    enter: bool,
}

/// The channels in tree order (by position, then name), each labelled with its path below the
/// root ("Gaming / Squad A").
fn channel_rows(server: &Server) -> Vec<ChannelRow> {
    let children = |parent: Option<u32>| {
        let mut ids: Vec<u32> = server
            .channels
            .iter()
            .filter(|(_, c)| match parent {
                Some(p) => c.parent == Some(p),
                // Roots, and channels whose parent is not known (yet).
                None => c.parent.is_none_or(|p| !server.channels.contains_key(&p)),
            })
            .map(|(id, _)| *id)
            .collect();
        ids.sort_by(|a, b| {
            let (a, b) = (&server.channels[a], &server.channels[b]);
            (a.position, &a.name).cmp(&(b.position, &b.name))
        });
        ids
    };
    let mut rows = Vec::new();
    // Depth-first, keeping the path of names below the root.
    let mut stack: Vec<(u32, Vec<String>)> = children(None)
        .into_iter()
        .rev()
        .map(|id| (id, Vec::new()))
        .collect();
    while let Some((id, above)) = stack.pop() {
        let channel = &server.channels[&id];
        let mut path = above;
        let label = if path.is_empty() && channel.parent.is_none() {
            channel.name.clone()
        } else {
            path.push(channel.name.clone());
            path.join(" / ")
        };
        rows.push(ChannelRow {
            id,
            path: label,
            users: server.users.values().filter(|&&c| c == id).count(),
            enter: channel.enter,
        });
        stack.extend(
            children(Some(id))
                .into_iter()
                .rev()
                .map(|child| (child, path.clone())),
        );
    }
    rows
}

fn chat_entries(state: &TrayState, labels: &Labels) -> Vec<Entry> {
    let chats = &state.attention.chats;
    let several_servers = chats.iter().any(|c| c.server_id != chats[0].server_id);
    chats
        .iter()
        .map(|chat| {
            let mut label = chat.name.clone();
            if several_servers
                && let Some(server) = state.servers.iter().find(|s| s.id == chat.server_id)
            {
                label = format!("{label} · {}", server.name);
            }
            if chat.unread > 0 {
                let count = chat.unread.to_string();
                label = fill(&labels.chat_unread, &[("name", &label), ("count", &count)]);
            }
            Entry::Button {
                action: Action::OpenChat {
                    server_id: chat.server_id,
                    conversation: chat.conversation.clone(),
                },
                label,
                dot: (chat.unread > 0).then_some(Dot::Unread),
            }
        })
        .collect()
}

/// Replaces `{name}` placeholders in a translated text.
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    values
        .iter()
        .fold(template.to_owned(), |text, (name, value)| {
            text.replace(&format!("{{{name}}}"), value)
        })
}

#[cfg(test)]
mod tests {
    use mumcord_engine::EngineEvent;

    use super::super::labels;
    use super::super::state::fixtures::{channel, synced, user};
    use super::*;
    use crate::commands::tray::{TrayAttention, TrayChat};

    fn labels_en() -> Labels {
        labels(Some("en"), None)
    }

    fn find<'a>(entries: &'a [Entry], label: &str) -> &'a Entry {
        entries
            .iter()
            .find(|e| match e {
                Entry::Info { label: l, .. }
                | Entry::Button { label: l, .. }
                | Entry::Toggle { label: l, .. }
                | Entry::Submenu { label: l, .. } => l == label,
                Entry::Separator => false,
            })
            .unwrap_or_else(|| panic!("no entry {label:?} in {entries:#?}"))
    }

    fn submenu<'a>(entries: &'a [Entry], label: &str) -> &'a [Entry] {
        match find(entries, label) {
            Entry::Submenu { entries, .. } => entries,
            other => panic!("not a submenu: {other:?}"),
        }
    }

    #[test]
    fn offline_menu_has_disabled_voice_items_and_empty_submenus() {
        let l = labels_en();
        let menu = menu_model(&TrayState::default(), &l);
        assert_eq!(
            menu[0],
            Entry::Info {
                label: "Not connected".into(),
                dot: Some(Dot::Disconnected)
            }
        );
        assert!(matches!(
            find(&menu, "Mute"),
            Entry::Toggle { enabled: false, .. }
        ));
        assert!(submenu(&menu, "Voice server and channel").is_empty());
        assert!(submenu(&menu, "Recent chats").is_empty());
        let tail: Vec<_> = menu.iter().rev().take(3).cloned().collect();
        assert_eq!(
            tail,
            vec![
                Entry::Button {
                    action: Action::Quit,
                    label: "Quit".into(),
                    dot: None
                },
                Entry::Button {
                    action: Action::Settings,
                    label: "Settings…".into(),
                    dot: None
                },
                Entry::Button {
                    action: Action::Show,
                    label: "Open Mumcord".into(),
                    dot: None
                },
            ]
        );
    }

    #[test]
    fn every_server_shows_its_state_and_channel() {
        let l = labels_en();
        let mut state = TrayState::default();
        synced(&mut state, 1);
        state.apply(
            2,
            &EngineEvent::StateChanged(mumcord_engine::ConnState::Reconnecting {
                server_id: 2,
                attempt: 1,
                max_attempts: 5,
                next_retry_secs: 3,
            }),
            || "Work".into(),
        );
        let menu = menu_model(&state, &l);
        assert_eq!(
            menu[0],
            Entry::Info {
                label: "S1 – Connected · Lobby".into(),
                dot: Some(Dot::Connected)
            }
        );
        assert_eq!(
            menu[1],
            Entry::Info {
                label: "Work – Reconnecting…".into(),
                dot: Some(Dot::Connecting)
            }
        );
    }

    #[test]
    fn voice_submenu_lists_servers_then_channels_in_tree_order() {
        let l = labels_en();
        let mut state = TrayState::default();
        synced(&mut state, 1);
        synced(&mut state, 2);
        state.apply(1, &EngineEvent::VoiceServerChanged(Some(1)), String::new);
        state.apply(
            1,
            &EngineEvent::ChannelsUpserted(vec![
                channel(5, Some(0), "Gaming", 1),
                channel(6, Some(5), "Squad A", 0),
                channel(7, Some(0), "AFK", 2),
            ]),
            String::new,
        );
        state.apply(1, &EngineEvent::UserUpserted(user(9, 6)), String::new);
        state.apply(1, &EngineEvent::UserUpserted(user(10, 6)), String::new);
        let menu = menu_model(&state, &l);
        let voice = submenu(&menu, "Voice server and channel");
        let labels: Vec<_> = voice
            .iter()
            .map(|e| match e {
                Entry::Info { label, .. } | Entry::Toggle { label, .. } => label.as_str(),
                Entry::Separator => "-",
                _ => "?",
            })
            .collect();
        assert_eq!(
            labels,
            vec![
                "Voice server",
                "S1",
                "S2",
                "-",
                "Channels · S1",
                "Root",
                "Lobby (1)",
                "Gaming",
                "Gaming / Squad A (2)",
                "AFK",
            ]
        );
        assert!(matches!(
            find(voice, "S1"),
            Entry::Toggle {
                checked: true,
                enabled: false,
                ..
            }
        ));
        assert_eq!(
            find(voice, "S2"),
            &Entry::Toggle {
                action: Action::VoiceServer(2),
                label: "S2".into(),
                enabled: true,
                checked: false
            }
        );
        assert!(matches!(
            find(voice, "Lobby (1)"),
            Entry::Toggle {
                checked: true,
                enabled: false,
                ..
            }
        ));
        assert_eq!(
            find(voice, "AFK"),
            &Entry::Toggle {
                action: Action::Join {
                    server_id: 1,
                    channel_id: 7
                },
                label: "AFK".into(),
                enabled: true,
                checked: false
            }
        );
    }

    #[test]
    fn large_servers_end_the_channel_list_with_a_link_to_the_window() {
        let l = labels_en();
        let mut state = TrayState::default();
        synced(&mut state, 1);
        let many = (10..10 + MAX_CHANNELS as u32)
            .map(|id| channel(id, Some(0), &format!("c{id}"), 0))
            .collect();
        state.apply(1, &EngineEvent::ChannelsUpserted(many), String::new);
        let menu = menu_model(&state, &l);
        let voice = submenu(&menu, "Voice server and channel");
        let joins = voice
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Entry::Toggle {
                        action: Action::Join { .. },
                        ..
                    }
                )
            })
            .count();
        assert_eq!(joins, MAX_CHANNELS);
        assert!(matches!(
            voice.last(),
            Some(Entry::Button {
                action: Action::Show,
                ..
            })
        ));
    }

    #[test]
    fn mute_and_deafen_follow_the_voice_target() {
        let l = labels_en();
        let mut state = TrayState::default();
        synced(&mut state, 1);
        let mut me = user(1, 1);
        me.self_deaf = true;
        state.apply(1, &EngineEvent::UserUpserted(me), String::new);
        let menu = menu_model(&state, &l);
        assert!(matches!(
            find(&menu, "Mute"),
            Entry::Toggle {
                enabled: true,
                checked: false,
                ..
            }
        ));
        assert!(matches!(
            find(&menu, "Deafen"),
            Entry::Toggle {
                enabled: true,
                checked: true,
                ..
            }
        ));
    }

    #[test]
    fn dnd_names_the_system_source() {
        let l = labels_en();
        let mut state = TrayState::default();
        assert!(matches!(
            find(&menu_model(&state, &l), "Do Not Disturb"),
            Entry::Toggle {
                enabled: true,
                checked: false,
                ..
            }
        ));
        state.dnd.system = true;
        assert!(matches!(
            find(&menu_model(&state, &l), "Do Not Disturb (system)"),
            Entry::Toggle {
                enabled: false,
                checked: true,
                ..
            }
        ));
        state.dnd.manual = true;
        assert!(matches!(
            find(&menu_model(&state, &l), "Do Not Disturb"),
            Entry::Toggle {
                enabled: true,
                checked: true,
                ..
            }
        ));
    }

    #[test]
    fn recent_chats_open_the_chat_and_show_unread() {
        let l = labels_en();
        let mut state = TrayState::default();
        synced(&mut state, 1);
        synced(&mut state, 2);
        let maya = Conversation::Private {
            cert_hash: "ab".into(),
        };
        state.attention = TrayAttention {
            unread: 2,
            mentions: 0,
            chats: vec![
                TrayChat {
                    server_id: 1,
                    conversation: maya.clone(),
                    name: "Maya".into(),
                    unread: 2,
                },
                TrayChat {
                    server_id: 2,
                    conversation: Conversation::PrivateSession { session: 4 },
                    name: "Leo".into(),
                    unread: 0,
                },
            ],
        };
        let menu = menu_model(&state, &l);
        let chats = submenu(&menu, "Recent chats");
        assert_eq!(
            chats[0],
            Entry::Button {
                action: Action::OpenChat {
                    server_id: 1,
                    conversation: maya
                },
                label: "Maya · S1 – 2 unread".into(),
                dot: Some(Dot::Unread),
            }
        );
        assert!(matches!(&chats[1], Entry::Button { label, dot: None, .. } if label == "Leo · S2"));
    }

    #[test]
    fn placeholders_are_filled() {
        assert_eq!(fill("{a} – {b}", &[("a", "x"), ("b", "y")]), "x – y");
    }
}
