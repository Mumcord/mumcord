//! `notify` (FR-6.22): an entry in the notification centre, never an OS notification. At most
//! one per 10 minutes and three per 24 hours per plugin. During Do Not Disturb or quiet time, or
//! when the user switched the plugin's notifications off, a notification is dropped (not
//! queued, not counted) and the call still succeeds.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use wasmtime::component::{HasSelf, Linker};

use crate::host::HostEvent;
use crate::runtime::bindings::mumcord::plugin::notify::{Host, add_to_linker};
use crate::runtime::wit::types::Error;
use crate::runtime::{HostApi, PluginState};

pub const PERMISSION: &str = "notify";
pub const MAX_TITLE_CHARS: usize = 64;
pub const MAX_BODY_CHARS: usize = 200;
/// Minimum gap between two notifications of one plugin.
pub const MIN_GAP: Duration = Duration::from_secs(10 * 60);
/// At most [`MAX_PER_DAY`] notifications per plugin within this window.
pub const DAY: Duration = Duration::from_secs(24 * 60 * 60);
pub const MAX_PER_DAY: usize = 3;

pub(crate) struct NotifyApi;

impl HostApi for NotifyApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
        add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
    }
}

impl Host for PluginState {
    fn send(&mut self, title: String, body: String) -> Result<(), Error> {
        self.ctx.grants.check(&self.id, PERMISSION)?;
        if self
            .ctx
            .notify
            .admit(&self.id, &title, &body, Instant::now())?
        {
            // The service is gone only while Mumcord shuts down.
            let _ = self.outbox.send(HostEvent::Notify {
                id: self.id.to_string(),
                title: clean(&title),
                body: clean(&body),
            });
        }
        Ok(())
    }
}

/// Rate limits plus the quiet and per-plugin switches; lives in [`crate::HostContext`].
#[derive(Debug, Default)]
pub struct Notify {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    quiet: bool,
    muted: HashSet<String>,
    sent: HashMap<String, VecDeque<Instant>>,
}

impl Notify {
    /// Do Not Disturb or quiet time is on (the service combines both).
    pub fn set_quiet(&self, quiet: bool) {
        self.lock().quiet = quiet;
    }

    /// The per-plugin "notifications" switch in the plugin's settings.
    pub fn set_enabled(&self, id: &str, enabled: bool) {
        let mut state = self.lock();
        if enabled {
            state.muted.remove(id);
        } else {
            state.muted.insert(id.to_owned());
        }
    }

    /// Forgets a removed plugin.
    pub fn forget(&self, id: &str) {
        let mut state = self.lock();
        state.muted.remove(id);
        state.sent.remove(id);
    }

    /// `Ok(true)`: show it. `Ok(false)`: dropped silently. Errors are returned to the plugin.
    fn admit(&self, id: &str, title: &str, body: &str, now: Instant) -> Result<bool, Error> {
        if title.chars().count() > MAX_TITLE_CHARS || body.chars().count() > MAX_BODY_CHARS {
            return Err(Error::TooLarge);
        }
        let mut state = self.lock();
        if state.quiet || state.muted.contains(id) {
            return Ok(false);
        }
        let sent = state.sent.entry(id.to_owned()).or_default();
        while sent
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= DAY)
        {
            sent.pop_front();
        }
        let too_soon = sent
            .back()
            .is_some_and(|at| now.saturating_duration_since(*at) < MIN_GAP);
        if too_soon || sent.len() >= MAX_PER_DAY {
            return Err(Error::RateLimited);
        }
        sent.push_back(now);
        Ok(true)
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

    #[test]
    fn five_calls_give_one_notification() {
        let notify = Notify::default();
        let now = Instant::now();
        assert_eq!(notify.admit("fx", "t", "b", now), Ok(true));
        for _ in 0..4 {
            assert_eq!(notify.admit("fx", "t", "b", now), Err(Error::RateLimited));
        }
        assert_eq!(notify.admit("other", "t", "b", now), Ok(true));
    }

    #[test]
    fn one_per_ten_minutes_three_per_day() {
        let notify = Notify::default();
        let start = Instant::now();
        let at = |minutes: u64| start + Duration::from_secs(minutes * 60);
        assert_eq!(notify.admit("fx", "t", "b", at(0)), Ok(true));
        assert_eq!(notify.admit("fx", "t", "b", at(9)), Err(Error::RateLimited));
        assert_eq!(notify.admit("fx", "t", "b", at(10)), Ok(true));
        assert_eq!(notify.admit("fx", "t", "b", at(20)), Ok(true));
        assert_eq!(
            notify.admit("fx", "t", "b", at(30)),
            Err(Error::RateLimited)
        );
        assert_eq!(
            notify.admit("fx", "t", "b", at(24 * 60 - 1)),
            Err(Error::RateLimited)
        );
        assert_eq!(notify.admit("fx", "t", "b", at(24 * 60)), Ok(true));
    }

    #[test]
    fn quiet_and_muted_drop_without_counting() {
        let notify = Notify::default();
        let now = Instant::now();
        notify.set_quiet(true);
        for _ in 0..5 {
            assert_eq!(notify.admit("fx", "t", "b", now), Ok(false));
        }
        notify.set_quiet(false);
        notify.set_enabled("fx", false);
        assert_eq!(notify.admit("fx", "t", "b", now), Ok(false));
        notify.set_enabled("fx", true);
        assert_eq!(notify.admit("fx", "t", "b", now), Ok(true));
    }

    #[test]
    fn size_limits_count_characters() {
        let notify = Notify::default();
        let now = Instant::now();
        let title = "ä".repeat(MAX_TITLE_CHARS);
        let body = "ü".repeat(MAX_BODY_CHARS);
        assert_eq!(notify.admit("fx", &title, &body, now), Ok(true));
        let long = "x".repeat(MAX_TITLE_CHARS + 1);
        assert_eq!(notify.admit("a", &long, "", now), Err(Error::TooLarge));
        let long = "x".repeat(MAX_BODY_CHARS + 1);
        assert_eq!(notify.admit("a", "", &long, now), Err(Error::TooLarge));
        assert_eq!(clean("a\nb\u{7}"), "a b ");
    }
}
