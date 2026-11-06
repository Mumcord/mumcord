//! `activity` (FR-11.9): one event in the activity log of the server the user is on. Needs the
//! `activity` permission. At most one push per 10 seconds and 50 per hour per plugin.
//!
//! The text and the action label are untrusted plugin text, so they are cleaned here. The action
//! is only carried through with the event: the UI shows it, and clicking it reaches the plugin
//! through the existing `ui-action` path (FR-11.13), so no new callback is needed.
//!
//! Every rule of one push (permission, caps, rate limit, cleaning) is decided in
//! [`Activity::push`], so the host function in [`crate::api::v1_1`] only hands the call over and
//! sends the returned event to the service.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use mumcord_store::ServerId;

use crate::grants::Grants;
use crate::host::HostEvent;
use crate::runtime::wit::types::Error;

/// The permission every `activity` call needs.
pub const PERMISSION: &str = "activity";
pub const MAX_TEXT_CHARS: usize = 128;
pub const MAX_LABEL_CHARS: usize = 24;
/// Minimum gap between two pushes of one plugin.
pub const MIN_GAP: Duration = Duration::from_secs(10);
/// At most [`MAX_PER_HOUR`] pushes per plugin within this window.
pub const HOUR: Duration = Duration::from_secs(60 * 60);
pub const MAX_PER_HOUR: usize = 50;

/// The rate limits plus the server a push belongs to; lives in [`crate::HostContext`].
#[derive(Debug, Default)]
pub struct Activity {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    /// The server the user is on (the service's focused server); `None` while none is.
    server: Option<ServerId>,
    sent: HashMap<String, VecDeque<Instant>>,
}

impl Activity {
    /// The server pushes belong to, kept in step by the service, as
    /// [`Notify::set_quiet`](crate::api::notify::Notify) keeps its quiet switch.
    pub fn set_server(&self, server: Option<ServerId>) {
        self.lock().server = server;
    }

    /// Forgets a removed plugin.
    pub fn forget(&self, id: &str) {
        self.lock().sent.remove(id);
    }

    /// One push by `plugin`. `Ok(event)`: the event goes to the service; a refused push is not
    /// counted. Errors are returned to the plugin.
    pub fn push(
        &self,
        grants: &Grants,
        plugin: &str,
        text: &str,
        action: Option<(&str, &str)>,
        now: Instant,
    ) -> Result<HostEvent, Error> {
        grants.check(plugin, PERMISSION)?;
        if text.chars().count() > MAX_TEXT_CHARS
            || action.is_some_and(|(label, _)| label.chars().count() > MAX_LABEL_CHARS)
        {
            return Err(Error::TooLarge);
        }
        let mut state = self.lock();
        let sent = state.sent.entry(plugin.to_owned()).or_default();
        while sent
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= HOUR)
        {
            sent.pop_front();
        }
        let too_soon = sent
            .back()
            .is_some_and(|at| now.saturating_duration_since(*at) < MIN_GAP);
        if too_soon || sent.len() >= MAX_PER_HOUR {
            return Err(Error::RateLimited);
        }
        sent.push_back(now);
        Ok(HostEvent::Activity {
            server: state.server,
            plugin: plugin.to_owned(),
            text: clean(text),
            action: action.map(|(label, id)| (clean(label), id.to_owned())),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Plugin text is untrusted: no control characters (line breaks become spaces).
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVER: ServerId = 1;

    /// The activity state with the server the service has focused, so every event names it.
    fn activity() -> Activity {
        let activity = Activity::default();
        activity.set_server(Some(SERVER));
        activity
    }

    fn event(
        plugin: &str,
        text: &str,
        action: Option<(String, String)>,
    ) -> Result<HostEvent, Error> {
        Ok(HostEvent::Activity {
            server: Some(SERVER),
            plugin: plugin.to_owned(),
            text: text.to_owned(),
            action,
        })
    }

    fn granted() -> Grants {
        let grants = Grants::default();
        grants.set("fit", [PERMISSION]);
        grants
    }

    fn at(now: Instant, seconds: u64) -> Instant {
        now + Duration::from_secs(seconds)
    }

    #[test]
    fn a_push_without_the_permission_is_refused() {
        let activity = activity();
        let grants = granted();
        let now = Instant::now();
        assert_eq!(
            activity.push(&grants, "fit", "t", None, now),
            event("fit", "t", None)
        );
        assert_eq!(
            activity.push(&grants, "other", "t", None, now),
            Err(Error::NotPermitted)
        );
    }

    #[test]
    fn one_push_per_ten_seconds() {
        let activity = activity();
        let grants = granted();
        let start = Instant::now();
        assert_eq!(
            activity.push(&grants, "fit", "t", None, start),
            event("fit", "t", None)
        );
        assert_eq!(
            activity.push(&grants, "fit", "t", None, at(start, 9)),
            Err(Error::RateLimited)
        );
        assert_eq!(
            activity.push(&grants, "fit", "t", None, at(start, 10)),
            event("fit", "t", None)
        );
    }

    #[test]
    fn fifty_pushes_per_hour_and_the_window_rolls() {
        let activity = activity();
        let grants = granted();
        let start = Instant::now();
        // Every push is far enough apart, so only the hourly cap stops them.
        for index in 0..MAX_PER_HOUR {
            let count = u64::try_from(index).expect("count");
            assert_eq!(
                activity.push(&grants, "fit", "t", None, at(start, count * 10)),
                event("fit", "t", None)
            );
        }
        assert_eq!(
            activity.push(&grants, "fit", "t", None, at(start, 500)),
            Err(Error::RateLimited)
        );
        // The first push falls out of the hour, so one slot is free again.
        assert_eq!(
            activity.push(&grants, "fit", "t", None, at(start, 3600 + 10)),
            event("fit", "t", None)
        );
        assert_eq!(
            activity.push(&grants, "fit", "t", None, at(start, 3600 + 19)),
            Err(Error::RateLimited)
        );
    }

    #[test]
    fn plugins_are_counted_apart_and_forgetting_one_frees_its_slots() {
        let activity = activity();
        let grants = Grants::default();
        grants.set("fit", [PERMISSION]);
        grants.set("other", [PERMISSION]);
        let now = Instant::now();
        assert_eq!(
            activity.push(&grants, "fit", "t", None, now),
            event("fit", "t", None)
        );
        assert_eq!(
            activity.push(&grants, "other", "t", None, now),
            event("other", "t", None)
        );
        assert_eq!(
            activity.push(&grants, "fit", "t", None, now),
            Err(Error::RateLimited)
        );
        activity.forget("fit");
        assert_eq!(
            activity.push(&grants, "fit", "t", None, now),
            event("fit", "t", None)
        );
    }

    #[test]
    fn caps_count_characters() {
        let activity = activity();
        let grants = granted();
        let start = Instant::now();
        let text = "ä".repeat(MAX_TEXT_CHARS);
        let label = "é".repeat(MAX_LABEL_CHARS);
        assert_eq!(
            activity.push(&grants, "fit", &text, Some((label.as_str(), "open")), start),
            event("fit", &text, Some((label.to_owned(), "open".to_owned())))
        );
        // The caps are checked before the rate limit, so a refused push shows which one failed.
        assert_eq!(
            activity.push(&grants, "fit", &"x".repeat(MAX_TEXT_CHARS + 1), None, start),
            Err(Error::TooLarge)
        );
        assert_eq!(
            activity.push(
                &grants,
                "fit",
                "t",
                Some((&"x".repeat(MAX_LABEL_CHARS + 1), "open")),
                start
            ),
            Err(Error::TooLarge)
        );
        assert_eq!(
            activity.push(
                &grants,
                "fit",
                "t",
                Some((label.as_str(), "open")),
                at(start, 10)
            ),
            event("fit", "t", Some((label.to_owned(), "open".to_owned())))
        );
    }

    #[test]
    fn the_event_carries_the_cleaned_text_the_action_and_the_server() {
        let activity = activity();
        let grants = granted();
        let now = Instant::now();
        assert_eq!(
            activity.push(
                &grants,
                "fit",
                "a\nb\u{7}",
                Some(("Notify me", "notify")),
                now
            ),
            event(
                "fit",
                "a b ",
                Some(("Notify me".to_owned(), "notify".to_owned()))
            )
        );
        // Nothing is focused: the event has no server of its own.
        activity.set_server(None);
        assert_eq!(
            activity.push(&grants, "fit", "t", None, at(now, 10)),
            Ok(HostEvent::Activity {
                server: None,
                plugin: "fit".into(),
                text: "t".into(),
                action: None,
            })
        );
    }
}
