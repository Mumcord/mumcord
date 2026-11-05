//! `webview.set-status` (1.1): the plain-text status line under a plugin's side panel.
//!
//! The text is untrusted: markup is refused rather than escaped, so a plugin cannot hope to get
//! formatting through, and control characters become spaces. A plugin may update the line from a
//! fast loop (a drawing board counting strokes); only [`MAX_PER_WINDOW`] updates per [`WINDOW`]
//! reach the UI. Quicker calls are merged: the last text is kept and shown once the window
//! allows, so the line never stays on a stale value.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Longest status line, in characters.
pub const MAX_STATUS_CHARS: usize = 80;
/// Updates that reach the UI per [`WINDOW`] and plugin.
pub const MAX_PER_WINDOW: usize = 4;
pub const WINDOW: Duration = Duration::from_secs(1);

/// The status text to show, or `None` when it is too long or contains markup.
pub fn clean_status(text: &str) -> Option<String> {
    if text.chars().count() > MAX_STATUS_CHARS || text.contains(['<', '>']) {
        return None;
    }
    let plain: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    Some(plain.trim().to_owned())
}

/// What to do with an admitted status update.
#[derive(Debug, PartialEq, Eq)]
pub enum Admit {
    /// Show it now.
    Send(String),
    /// Kept as the pending value; call [`StatusLimiter::flush`] after the delay.
    Schedule(Duration),
    /// Replaced the pending value; a flush is already scheduled.
    Merged,
}

#[derive(Debug, Default)]
struct PluginStatus {
    /// When the last updates reached the UI, oldest first (at most [`MAX_PER_WINDOW`]).
    sent: VecDeque<Instant>,
    pending: Option<String>,
}

impl PluginStatus {
    fn forget_old(&mut self, now: Instant) {
        while self
            .sent
            .front()
            .is_some_and(|at| now.duration_since(*at) >= WINDOW)
        {
            self.sent.pop_front();
        }
    }

    fn record(&mut self, now: Instant) {
        if self.sent.len() == MAX_PER_WINDOW {
            self.sent.pop_front();
        }
        self.sent.push_back(now);
    }
}

/// Per-plugin rate limit for status updates.
#[derive(Debug, Default)]
pub struct StatusLimiter {
    plugins: Mutex<HashMap<String, PluginStatus>>,
}

impl StatusLimiter {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, PluginStatus>> {
        self.plugins
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn admit(&self, plugin: &str, text: String, now: Instant) -> Admit {
        let mut plugins = self.lock();
        let status = plugins.entry(plugin.to_owned()).or_default();
        if status.pending.is_some() {
            status.pending = Some(text);
            return Admit::Merged;
        }
        status.forget_old(now);
        match status.sent.front() {
            Some(oldest) if status.sent.len() >= MAX_PER_WINDOW => {
                let wait = WINDOW.saturating_sub(now.duration_since(*oldest));
                status.pending = Some(text);
                Admit::Schedule(wait)
            }
            _ => {
                status.record(now);
                Admit::Send(text)
            }
        }
    }

    /// The pending text, now due; `None` if there is none (the plugin was forgotten meanwhile).
    pub fn flush(&self, plugin: &str, now: Instant) -> Option<String> {
        let mut plugins = self.lock();
        let status = plugins.get_mut(plugin)?;
        let text = status.pending.take()?;
        status.record(now);
        Some(text)
    }

    /// The side panel closed or the plugin stopped: drop its pending update.
    pub fn forget(&self, plugin: &str) {
        self.lock().remove(plugin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_kept_and_control_characters_become_spaces() {
        assert_eq!(
            clean_status("3 drawing · relay").as_deref(),
            Some("3 drawing · relay")
        );
        assert_eq!(clean_status("a\nb\t").as_deref(), Some("a b"));
        assert_eq!(clean_status("").as_deref(), Some(""));
    }

    #[test]
    fn markup_and_long_text_are_refused() {
        assert_eq!(clean_status("<b>bold</b>"), None);
        assert_eq!(clean_status("a > b"), None);
        let max = "é".repeat(MAX_STATUS_CHARS);
        assert!(clean_status(&max).is_some());
        assert_eq!(clean_status(&format!("{max}x")), None);
    }

    #[test]
    fn four_updates_per_second_then_the_last_one_waits() {
        let limiter = StatusLimiter::default();
        let start = Instant::now();
        for i in 0..4 {
            let at = start + Duration::from_millis(i * 10);
            assert_eq!(
                limiter.admit("p", format!("{i}"), at),
                Admit::Send(format!("{i}"))
            );
        }
        let at = start + Duration::from_millis(100);
        assert_eq!(
            limiter.admit("p", "4".into(), at),
            Admit::Schedule(Duration::from_millis(900))
        );
        assert_eq!(limiter.admit("p", "5".into(), at), Admit::Merged);
        // Other plugins have their own budget.
        assert_eq!(limiter.admit("q", "x".into(), at), Admit::Send("x".into()));
        let due = start + Duration::from_secs(1);
        assert_eq!(limiter.flush("p", due).as_deref(), Some("5"));
        assert_eq!(limiter.flush("p", due), None);
        // The flush used one slot of the new window; the second update still passes.
        assert_eq!(
            limiter.admit("p", "6".into(), due + Duration::from_millis(20)),
            Admit::Send("6".into())
        );
    }

    #[test]
    fn forgetting_drops_the_pending_update() {
        let limiter = StatusLimiter::default();
        let now = Instant::now();
        for _ in 0..4 {
            limiter.admit("p", "a".into(), now);
        }
        assert!(matches!(
            limiter.admit("p", "b".into(), now),
            Admit::Schedule(_)
        ));
        limiter.forget("p");
        assert_eq!(limiter.flush("p", now + WINDOW), None);
    }
}
