//! Chat triggers: the one narrow way a plugin hears chat text. A plugin declares trigger words
//! such as `!poll` in its manifest and holds `chat.triggers`; a channel message in the user's
//! channel that starts with one reaches only the plugin that owns the word, as `chat-trigger`
//! with the rest of the line. People on plain Mumble clients use this to take part (e.g.
//! `!poll 2` votes).
//!
//! The engine already forwards only lines of the trigger shape and never from blocked users.
//! Here the word is matched to its owner and each sender is rate-limited. Lines nobody owns
//! leave no trace: they are neither stored nor counted.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use mumcord_store::ServerId;

/// Most trigger lines one sender may cause per [`RATE_WINDOW`]; more are dropped.
pub const RATE_LIMIT: usize = 2;
pub const RATE_WINDOW: Duration = Duration::from_secs(1);
/// Longest `args` a plugin gets, in characters.
pub const MAX_ARGS_CHARS: usize = 200;

/// A plugin that may own trigger words.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub plugin: &'a str,
    /// Install time; the earliest installed plugin owns a word ("first installed wins").
    pub installed_at: i64,
    /// Enabled, running and holding `chat.triggers`.
    pub active: bool,
    pub triggers: &'a [String],
}

/// The active plugin that owns `trigger`, if any. A word declared by several plugins belongs
/// to the one installed first; the others never see it.
pub fn owner<'a>(candidates: &[Candidate<'a>], trigger: &str) -> Option<&'a str> {
    candidates
        .iter()
        .filter(|c| c.active && c.triggers.iter().any(|t| t == trigger))
        .min_by_key(|c| c.installed_at)
        .map(|c| c.plugin)
}

/// At most [`RATE_LIMIT`] trigger lines per sender and [`RATE_WINDOW`]. Only lines that a
/// plugin owns are counted, so ordinary chat leaves nothing here.
#[derive(Debug, Default)]
pub struct RateLimit {
    /// Recent accepted lines per sender key (e.g. server and session).
    recent: BTreeMap<(ServerId, u32), Vec<Instant>>,
}

impl RateLimit {
    /// True if a line from `sender` at `now` may pass; counts it if so.
    pub fn allow(&mut self, sender: (ServerId, u32), now: Instant) -> bool {
        // Forget senders whose window is over, so the map stays small.
        self.recent.retain(|_, times| {
            times.retain(|t| now.saturating_duration_since(*t) < RATE_WINDOW);
            !times.is_empty()
        });
        let times = self.recent.entry(sender).or_default();
        if times.len() >= RATE_LIMIT {
            return false;
        }
        times.push(now);
        true
    }
}

/// `args` cut to [`MAX_ARGS_CHARS`] characters (the engine already cuts; this keeps the plugin
/// contract even if it did not).
pub fn clip_args(args: &str) -> String {
    args.chars().take(MAX_ARGS_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn the_first_installed_active_plugin_owns_a_word() {
        let poll = words(&["!poll", "!vote"]);
        let other = words(&["!poll"]);
        let mut candidates = vec![
            Candidate {
                plugin: "late",
                installed_at: 20,
                active: true,
                triggers: &other,
            },
            Candidate {
                plugin: "poll",
                installed_at: 10,
                active: true,
                triggers: &poll,
            },
        ];
        assert_eq!(owner(&candidates, "!poll"), Some("poll"));
        assert_eq!(owner(&candidates, "!vote"), Some("poll"));
        assert_eq!(owner(&candidates, "!dice"), None);
        candidates[1].active = false;
        assert_eq!(owner(&candidates, "!poll"), Some("late"));
        assert_eq!(owner(&candidates, "!vote"), None);
    }

    #[test]
    fn each_sender_gets_two_lines_per_second() {
        let mut limit = RateLimit::default();
        let start = Instant::now();
        assert!(limit.allow((1, 5), start));
        assert!(limit.allow((1, 5), start + Duration::from_millis(100)));
        assert!(!limit.allow((1, 5), start + Duration::from_millis(200)));
        assert!(limit.allow((1, 6), start + Duration::from_millis(200)));
        assert!(limit.allow((2, 5), start + Duration::from_millis(200)));
        assert!(limit.allow((1, 5), start + Duration::from_millis(1000)));
        assert!(!limit.allow((1, 5), start + Duration::from_millis(1050)));
        assert!(limit.allow((1, 5), start + Duration::from_millis(1100)));
    }

    #[test]
    fn old_senders_are_forgotten() {
        let mut limit = RateLimit::default();
        let start = Instant::now();
        for session in 0..50 {
            assert!(limit.allow((1, session), start));
        }
        assert!(limit.allow((1, 99), start + Duration::from_secs(2)));
        assert_eq!(limit.recent.len(), 1);
    }

    #[test]
    fn args_are_clipped_by_characters() {
        assert_eq!(clip_args("2"), "2");
        let long = "ä".repeat(300);
        assert_eq!(clip_args(&long).chars().count(), MAX_ARGS_CHARS);
    }
}
