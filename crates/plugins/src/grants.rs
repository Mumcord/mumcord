//! The permission gate (FR-6.30, FR-6.31): which permissions each plugin holds right now. Every
//! host call that needs a permission asks [`Grants::check`]; events go through
//! [`Grants::allows_event`] before they are queued. The service keeps this in step with the
//! stored grants, so a change applies to the next call or event.

use std::collections::{HashMap, HashSet};
use std::sync::{PoisonError, RwLock};

use crate::runtime::wit::events::Event;
use crate::runtime::wit::types::Error;

/// Granted permission ids per plugin id.
#[derive(Debug, Default)]
pub struct Grants {
    granted: RwLock<HashMap<String, HashSet<String>>>,
}

impl Grants {
    /// Replaces a plugin's grants. Takes effect for the next host call and event.
    pub fn set<I, S>(&self, id: &str, granted: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let granted = granted.into_iter().map(Into::into).collect();
        self.granted
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.to_owned(), granted);
    }

    /// Drops a removed plugin's grants; it then holds none.
    pub fn forget(&self, id: &str) {
        self.granted
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
    }

    pub fn allows(&self, id: &str, permission: &str) -> bool {
        self.granted
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .is_some_and(|granted| granted.contains(permission))
    }

    /// The gate for host calls: `not-permitted` unless `permission` is granted to `id` now.
    pub fn check(&self, id: &str, permission: &str) -> Result<(), Error> {
        if self.allows(id, permission) {
            Ok(())
        } else {
            Err(Error::NotPermitted)
        }
    }

    /// Whether `id` may receive `event` with its current grants.
    pub fn allows_event(&self, id: &str, event: &Event) -> bool {
        event_permission(event).is_none_or(|permission| self.allows(id, permission))
    }
}

/// The permission an event needs; `None` for events every plugin gets (theme, language,
/// settings, timers, its own UI actions).
pub fn event_permission(event: &Event) -> Option<&'static str> {
    Some(match event {
        Event::Talking(_) => "events.voice",
        Event::Joined(_) | Event::Left(_) | Event::ScreenShareStarted(_) => "events.presence",
        Event::ChatActivity(_) | Event::Mentioned(_) => "events.chat-activity",
        Event::SelfChanged(_) => "events.self",
        Event::PeerMessage(_) => "peers.messages",
        Event::ProfileCardOpened(_) => "ui.profile-card",
        Event::PanelVisibilityChanged(_) => "ui.panel",
        Event::UserMenuRequested(_) => "ui.user-menu",
        Event::DialogClosed => "ui.dialog",
        Event::HttpResponse(_) => "net.http",
        Event::WebviewMessage(_) => "ui.webview",
        Event::ChatTrigger(_) => "chat.triggers",
        Event::ChatMatch(_) => "chat.patterns",
        Event::ThemeChanged(_)
        | Event::LanguageChanged(_)
        | Event::SettingsChanged(_)
        | Event::UiAction(_)
        | Event::Timer(_) => return None,
    })
}

/// True if `granted` lacks one of the manifest's `required` permissions
/// (`manifest.permissions.required`): the plugin must then be disabled (FR-6.30), not just
/// refused per call.
pub fn revokes_required<S: AsRef<str>>(required: &[String], granted: &[S]) -> bool {
    required
        .iter()
        .any(|required| !granted.iter().any(|g| g.as_ref() == required))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::wit::types::Peer;

    fn peer() -> Peer {
        Peer {
            session: 1,
            name: "a".into(),
            key: None,
        }
    }

    #[test]
    fn check_follows_runtime_changes() {
        let grants = Grants::default();
        assert_eq!(grants.check("poll", "notify"), Err(Error::NotPermitted));
        grants.set("poll", ["notify", "peers.messages"]);
        assert_eq!(grants.check("poll", "notify"), Ok(()));
        assert_eq!(grants.check("dice", "notify"), Err(Error::NotPermitted));
        grants.set("poll", ["notify"]);
        assert_eq!(
            grants.check("poll", "peers.messages"),
            Err(Error::NotPermitted)
        );
        grants.forget("poll");
        assert_eq!(grants.check("poll", "notify"), Err(Error::NotPermitted));
    }

    #[test]
    fn events_stop_without_their_permission() {
        let grants = Grants::default();
        grants.set("poll", ["events.presence"]);
        assert!(grants.allows_event("poll", &Event::Joined(peer())));
        assert!(!grants.allows_event("poll", &Event::Talking((peer(), true))));
        assert!(grants.allows_event("poll", &Event::Timer(3)));
        assert!(grants.allows_event("poll", &Event::LanguageChanged("de".into())));
        grants.set("poll", Vec::<String>::new());
        assert!(!grants.allows_event("poll", &Event::Left(peer())));
    }

    #[test]
    fn event_permissions_are_catalogue_ids() {
        let events = [
            Event::Talking((peer(), true)),
            Event::Joined(peer()),
            Event::Mentioned(peer()),
            Event::ProfileCardOpened(peer()),
            Event::UserMenuRequested(peer()),
            Event::DialogClosed,
            Event::WebviewMessage(String::new()),
            Event::ChatTrigger(crate::wit::events::ChatTrigger {
                sender: peer(),
                trigger: "!poll".into(),
                args: String::new(),
            }),
            Event::ChatMatch(crate::wit::events::ChatMatch {
                sender: peer(),
                pattern_index: 0,
                text: String::new(),
            }),
        ];
        for event in &events {
            let id = event_permission(event).expect("needs a permission");
            assert!(crate::permissions::get(id).is_some(), "{id}");
        }
    }

    #[test]
    fn missing_required_permission_means_disable() {
        let required = vec!["ui.panel".to_owned(), "notify".to_owned()];
        assert!(!revokes_required(&required, &["notify", "ui.panel", "x"]));
        assert!(revokes_required(&required, &["notify"]));
    }
}
