//! Chat integration rules (FR-6.43 … FR-6.45), free of I/O.
//!
//! The command registry decides who owns a slash command, the gesture gate decides whether a
//! plugin may post a chat line right now, and the supersede tracker pairs a plugin card with the
//! plain-text fallback line. Time is passed in as `now` so every rule is testable. Nothing here
//! logs or keeps chat text beyond a 4-byte hash.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::manifest::Command;

/// How long after a user gesture `chat.send` is allowed (FR-6.44).
pub const GESTURE_WINDOW: Duration = Duration::from_secs(2);
/// Longer window for a typed slash command, whose plugin may need a moment to answer.
pub const COMMAND_GESTURE_WINDOW: Duration = Duration::from_secs(8);
/// Longest text a plugin may post, in characters, without the prefix (FR-6.44).
pub const MAX_SEND_CHARS: usize = 500;
/// How long a text line and a card may be apart and still match (FR-6.45).
pub const SUPERSEDE_WINDOW: Duration = Duration::from_secs(10);

/// Who handles a typed slash command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// A Mumcord command; always wins.
    Builtin,
    /// A plugin command; the plugin receives it as a `ui-action` with source `command`.
    Plugin {
        plugin: String,
        action: UiAction,
    },
    Unknown,
}

/// The `ui-action` a typed command turns into: the action is the command name and the value is
/// the rest of the line (empty if there is none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiAction {
    pub action: String,
    pub text: String,
}

/// One autocomplete suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub name: String,
    pub description: String,
    pub usage: Option<String>,
    /// The plugin name shown next to the command; `None` for built-ins.
    pub plugin_name: Option<String>,
}

#[derive(Debug, Clone)]
struct Registered {
    plugin: String,
    plugin_name: String,
    commands: Vec<Command>,
}

/// Slash commands of all installed plugins. Registration order is install order, which decides
/// clashes between plugins ("first installed wins").
#[derive(Debug, Default)]
pub struct CommandRegistry {
    builtins: Vec<(String, String)>,
    plugins: Vec<Registered>,
}

impl CommandRegistry {
    /// `builtins` are `(name, description)` pairs of the Mumcord commands.
    pub fn new(builtins: &[(&str, &str)]) -> Self {
        Self {
            builtins: builtins
                .iter()
                .map(|(name, description)| ((*name).to_owned(), (*description).to_owned()))
                .collect(),
            plugins: Vec::new(),
        }
    }

    /// Registers (or replaces, keeping its place in the install order) the commands of a plugin.
    /// Call it for enabled plugins that hold the `commands` permission.
    pub fn register(&mut self, plugin: &str, plugin_name: &str, commands: &[Command]) {
        let entry = Registered {
            plugin: plugin.to_owned(),
            plugin_name: plugin_name.to_owned(),
            commands: commands.to_vec(),
        };
        match self.plugins.iter_mut().find(|p| p.plugin == plugin) {
            Some(existing) => *existing = entry,
            None => self.plugins.push(entry),
        }
    }

    /// Drops a plugin (disabled, removed or permission revoked); a later plugin may now own its
    /// command names.
    pub fn unregister(&mut self, plugin: &str) {
        self.plugins.retain(|p| p.plugin != plugin);
    }

    fn is_builtin(&self, name: &str) -> bool {
        self.builtins.iter().any(|(n, _)| n == name)
    }

    /// The plugin that owns `name`, if a plugin does (built-ins win, then the first installed).
    fn owner(&self, name: &str) -> Option<&Registered> {
        if self.is_builtin(name) {
            return None;
        }
        self.plugins
            .iter()
            .find(|p| p.commands.iter().any(|c| c.name == name))
    }

    /// For the plugin's settings: `Some("Command taken by <owner>")` if `name` is declared by
    /// `plugin` but owned by someone else (a built-in counts as "Mumcord").
    pub fn taken_by(&self, plugin: &str, name: &str) -> Option<String> {
        let declared = self
            .plugins
            .iter()
            .any(|p| p.plugin == plugin && p.commands.iter().any(|c| c.name == name));
        if !declared {
            return None;
        }
        if self.is_builtin(name) {
            return Some("Mumcord".to_owned());
        }
        self.owner(name)
            .filter(|owner| owner.plugin != plugin)
            .map(|owner| owner.plugin_name.clone())
    }

    /// All declared command names of `plugin` that it does not own.
    pub fn lost_commands(&self, plugin: &str) -> Vec<(String, String)> {
        let Some(entry) = self.plugins.iter().find(|p| p.plugin == plugin) else {
            return Vec::new();
        };
        entry
            .commands
            .iter()
            .filter_map(|c| {
                self.taken_by(plugin, &c.name)
                    .map(|by| (c.name.clone(), by))
            })
            .collect()
    }

    /// Decides who handles a typed line. `line` is the chat box content without trimming; only
    /// lines starting with `/` are commands.
    pub fn resolve(&self, line: &str) -> Resolution {
        let Some(rest) = line.strip_prefix('/') else {
            return Resolution::Unknown;
        };
        let (name, text) = match rest.split_once(char::is_whitespace) {
            Some((name, text)) => (name, text.trim()),
            None => (rest, ""),
        };
        if self.is_builtin(name) {
            return Resolution::Builtin;
        }
        match self.owner(name) {
            Some(owner) => Resolution::Plugin {
                plugin: owner.plugin.clone(),
                action: UiAction {
                    action: name.to_owned(),
                    text: text.to_owned(),
                },
            },
            None => Resolution::Unknown,
        }
    }

    /// Autocomplete for the text after the `/`. Built-ins first, then plugin commands the
    /// plugin owns, each with its plugin name.
    pub fn suggest(&self, prefix: &str) -> Vec<Suggestion> {
        let mut out: Vec<Suggestion> = self
            .builtins
            .iter()
            .filter(|(name, _)| name.starts_with(prefix))
            .map(|(name, description)| Suggestion {
                name: name.clone(),
                description: description.clone(),
                usage: None,
                plugin_name: None,
            })
            .collect();
        let mut seen: HashSet<&str> = HashSet::new();
        for plugin in &self.plugins {
            for command in &plugin.commands {
                if command.name.starts_with(prefix)
                    && self
                        .owner(&command.name)
                        .is_some_and(|owner| owner.plugin == plugin.plugin)
                    && seen.insert(&command.name)
                {
                    out.push(Suggestion {
                        name: command.name.clone(),
                        description: command.description.clone(),
                        usage: command.usage.clone(),
                        plugin_name: Some(plugin.plugin_name.clone()),
                    });
                }
            }
        }
        out
    }
}

/// Why `chat.send` was refused (the WIT `types.error` subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    #[error("not permitted")]
    NotPermitted,
    #[error("message too large")]
    TooLarge,
    #[error("invalid")]
    Invalid,
}

/// Remembers the last user gesture per plugin and lets exactly one `chat.send` through it.
#[derive(Debug, Default)]
pub struct GestureGate {
    gestures: HashMap<String, (Instant, Duration)>,
}

impl GestureGate {
    /// A button click for this plugin: allows one `chat.send` for `GESTURE_WINDOW`.
    pub fn gesture(&mut self, plugin: &str, now: Instant) {
        self.gestures
            .insert(plugin.to_owned(), (now, GESTURE_WINDOW));
    }

    /// A typed slash command for this plugin: allows one `chat.send` for
    /// `COMMAND_GESTURE_WINDOW`.
    pub fn command_gesture(&mut self, plugin: &str, now: Instant) {
        self.gestures
            .insert(plugin.to_owned(), (now, COMMAND_GESTURE_WINDOW));
    }

    /// Checks a `chat.send` and returns the line to post, with the `[Name] ` prefix. A refused
    /// text (too large or empty) keeps the gesture; a posted one uses it up.
    pub fn send(
        &mut self,
        plugin: &str,
        plugin_name: &str,
        text: &str,
        now: Instant,
    ) -> Result<String, SendError> {
        let fresh = self
            .gestures
            .get(plugin)
            .is_some_and(|(at, window)| now.saturating_duration_since(*at) <= *window);
        if !fresh {
            return Err(SendError::NotPermitted);
        }
        if text.trim().is_empty() {
            return Err(SendError::Invalid);
        }
        if text.chars().count() > MAX_SEND_CHARS {
            return Err(SendError::TooLarge);
        }
        self.gestures.remove(plugin);
        Ok(prefixed(plugin_name, text))
    }

    pub fn forget(&mut self, plugin: &str) {
        self.gestures.remove(plugin);
    }
}

/// The line as it appears in the channel.
pub fn prefixed(plugin_name: &str, text: &str) -> String {
    format!("[{plugin_name}] {text}")
}

/// The 4-byte line hash (FR-6.45) over a full line, first four bytes of its SHA-256, big-endian.
/// Callers pass the plain text of the line exactly as other clients see it.
pub fn hash_line(line: &str) -> u32 {
    let digest = Sha256::digest(line.as_bytes());
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
}

/// `chat.line-hash`: the hash of `text` with the `[Plugin name] ` prefix.
pub fn line_hash(plugin_name: &str, text: &str) -> u32 {
    hash_line(&prefixed(plugin_name, text))
}

/// What to do with an incoming text message or a new card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Supersede {
    /// Show the text line normally / the card has nothing to replace yet.
    None,
    /// Hide message `message` and show card `card` with "Show text".
    Replace { card: String, message: u64 },
}

#[derive(Debug)]
struct PendingCard {
    plugin: String,
    card: String,
    sender: u32,
    hash: u32,
    at: Instant,
}

#[derive(Debug)]
struct SeenLine {
    message: u64,
    sender: u32,
    hash: u32,
    at: Instant,
}

/// Pairs a card with its fallback line in either arrival order (FR-6.45).
#[derive(Debug, Default)]
pub struct SupersedeTracker {
    cards: Vec<PendingCard>,
    lines: Vec<SeenLine>,
}

impl SupersedeTracker {
    fn prune(&mut self, now: Instant) {
        let fresh = |at: Instant| now.saturating_duration_since(at) <= SUPERSEDE_WINDOW;
        self.cards.retain(|c| fresh(c.at));
        self.lines.retain(|l| fresh(l.at));
    }

    /// `chat.supersede`: a card for a line from `sender` with `hash`. If the line was already
    /// shown, returns the replacement now; otherwise waits for it.
    pub fn card(
        &mut self,
        plugin: &str,
        card: &str,
        sender: u32,
        hash: u32,
        now: Instant,
    ) -> Supersede {
        self.prune(now);
        if let Some(index) = self
            .lines
            .iter()
            .position(|l| l.sender == sender && l.hash == hash)
        {
            let line = self.lines.remove(index);
            return Supersede::Replace {
                card: card.to_owned(),
                message: line.message,
            };
        }
        self.cards.push(PendingCard {
            plugin: plugin.to_owned(),
            card: card.to_owned(),
            sender,
            hash,
            at: now,
        });
        Supersede::None
    }

    /// An incoming text message with the line's plain text. Only the hash is remembered.
    pub fn text(&mut self, message: u64, sender: u32, line: &str, now: Instant) -> Supersede {
        self.prune(now);
        let hash = hash_line(line);
        if let Some(index) = self
            .cards
            .iter()
            .position(|c| c.sender == sender && c.hash == hash)
        {
            let card = self.cards.remove(index);
            return Supersede::Replace {
                card: card.card,
                message,
            };
        }
        self.lines.push(SeenLine {
            message,
            sender,
            hash,
            at: now,
        });
        Supersede::None
    }

    /// A plugin was disabled or removed: its waiting cards are dropped, so its lines show.
    pub fn forget_plugin(&mut self, plugin: &str) {
        self.cards.retain(|c| c.plugin != plugin);
    }

    /// The sender left the server.
    pub fn forget_session(&mut self, sender: u32) {
        self.cards.retain(|c| c.sender != sender);
        self.lines.retain(|l| l.sender != sender);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(name: &str) -> Command {
        Command {
            name: name.to_owned(),
            description: format!("{name} desc"),
            usage: None,
        }
    }

    fn registry() -> CommandRegistry {
        let mut r = CommandRegistry::new(&[("help", "Show help")]);
        r.register("dice", "Dice roller", &[cmd("roll"), cmd("help")]);
        r.register("other", "Other", &[cmd("roll"), cmd("flip")]);
        r
    }

    #[test]
    fn typed_command_becomes_ui_action_for_the_first_installed() {
        let r = registry();
        assert_eq!(
            r.resolve("/roll 2d6"),
            Resolution::Plugin {
                plugin: "dice".into(),
                action: UiAction {
                    action: "roll".into(),
                    text: "2d6".into()
                }
            }
        );
        assert!(
            matches!(r.resolve("/flip"), Resolution::Plugin { plugin, .. } if plugin == "other")
        );
        assert_eq!(r.resolve("/nope"), Resolution::Unknown);
        assert_eq!(r.resolve("roll"), Resolution::Unknown);
    }

    #[test]
    fn builtins_win_and_clashes_are_reported() {
        let r = registry();
        assert_eq!(r.resolve("/help"), Resolution::Builtin);
        assert_eq!(r.taken_by("dice", "help").as_deref(), Some("Mumcord"));
        assert_eq!(r.taken_by("other", "roll").as_deref(), Some("Dice roller"));
        assert_eq!(r.taken_by("dice", "roll"), None);
        assert_eq!(
            r.lost_commands("other"),
            vec![("roll".to_owned(), "Dice roller".to_owned())]
        );
    }

    #[test]
    fn unregistering_hands_the_name_to_the_next_plugin() {
        let mut r = registry();
        r.unregister("dice");
        assert!(
            matches!(r.resolve("/roll"), Resolution::Plugin { plugin, .. } if plugin == "other")
        );
        assert_eq!(r.taken_by("other", "roll"), None);
    }

    #[test]
    fn suggestions_carry_the_plugin_name_and_skip_lost_commands() {
        let r = registry();
        let roll = r.suggest("ro");
        assert_eq!(roll.len(), 1);
        assert_eq!(roll[0].plugin_name.as_deref(), Some("Dice roller"));
        let all: Vec<_> = r.suggest("").into_iter().map(|s| s.name).collect();
        assert_eq!(all, ["help", "roll", "flip"]);
    }

    #[test]
    fn send_needs_a_fresh_gesture_and_uses_it_up() {
        let t0 = Instant::now();
        let mut gate = GestureGate::default();
        assert_eq!(
            gate.send("dice", "Dice", "hi", t0),
            Err(SendError::NotPermitted)
        );
        gate.gesture("dice", t0);
        assert_eq!(
            gate.send("dice", "Dice", "hi", t0 + Duration::from_secs(1))
                .expect("fresh gesture"),
            "[Dice] hi"
        );
        assert_eq!(
            gate.send("dice", "Dice", "again", t0 + Duration::from_secs(1)),
            Err(SendError::NotPermitted)
        );
    }

    #[test]
    fn gesture_expires_after_two_seconds_and_is_per_plugin() {
        let t0 = Instant::now();
        let mut gate = GestureGate::default();
        gate.gesture("dice", t0);
        assert_eq!(
            gate.send("poll", "Poll", "x", t0),
            Err(SendError::NotPermitted)
        );
        assert_eq!(
            gate.send("dice", "Dice", "x", t0 + Duration::from_millis(2001)),
            Err(SendError::NotPermitted)
        );
    }

    #[test]
    fn command_gesture_lasts_eight_seconds_for_one_message() {
        let t0 = Instant::now();
        let mut gate = GestureGate::default();
        gate.command_gesture("dice", t0);
        assert!(
            gate.send("dice", "Dice", "x", t0 + Duration::from_millis(7999))
                .is_ok()
        );
        assert_eq!(
            gate.send("dice", "Dice", "y", t0 + Duration::from_secs(1)),
            Err(SendError::NotPermitted)
        );
        gate.command_gesture("dice", t0);
        assert_eq!(
            gate.send("dice", "Dice", "x", t0 + Duration::from_millis(8001)),
            Err(SendError::NotPermitted)
        );
    }

    #[test]
    fn a_later_click_replaces_a_command_gesture_window() {
        let t0 = Instant::now();
        let mut gate = GestureGate::default();
        gate.command_gesture("dice", t0);
        gate.gesture("dice", t0);
        assert_eq!(
            gate.send("dice", "Dice", "x", t0 + Duration::from_secs(3)),
            Err(SendError::NotPermitted)
        );
    }

    #[test]
    fn oversize_and_empty_texts_keep_the_gesture() {
        let t0 = Instant::now();
        let mut gate = GestureGate::default();
        gate.gesture("dice", t0);
        assert_eq!(
            gate.send("dice", "Dice", &"a".repeat(501), t0),
            Err(SendError::TooLarge)
        );
        assert_eq!(gate.send("dice", "Dice", "  ", t0), Err(SendError::Invalid));
        assert!(gate.send("dice", "Dice", &"ä".repeat(500), t0).is_ok());
    }

    #[test]
    fn line_hash_includes_the_prefix() {
        assert_eq!(line_hash("Poll", "Lunch?"), hash_line("[Poll] Lunch?"));
        assert_ne!(line_hash("Poll", "Lunch?"), hash_line("Lunch?"));
    }

    #[test]
    fn card_after_text_replaces_the_line() {
        let t0 = Instant::now();
        let mut s = SupersedeTracker::default();
        let line = "[Poll] Lunch?";
        assert_eq!(s.text(7, 3, line, t0), Supersede::None);
        assert_eq!(
            s.card(
                "poll",
                "c1",
                3,
                hash_line(line),
                t0 + Duration::from_secs(5)
            ),
            Supersede::Replace {
                card: "c1".into(),
                message: 7
            }
        );
    }

    #[test]
    fn text_after_card_replaces_the_line() {
        let t0 = Instant::now();
        let mut s = SupersedeTracker::default();
        let line = "[Poll] Lunch?";
        assert_eq!(
            s.card("poll", "c1", 3, hash_line(line), t0),
            Supersede::None
        );
        assert_eq!(
            s.text(9, 3, line, t0 + Duration::from_secs(3)),
            Supersede::Replace {
                card: "c1".into(),
                message: 9
            }
        );
    }

    #[test]
    fn different_session_or_late_line_is_not_hidden() {
        let t0 = Instant::now();
        let mut s = SupersedeTracker::default();
        let line = "[Poll] Lunch?";
        s.card("poll", "c1", 3, hash_line(line), t0);
        assert_eq!(s.text(1, 4, line, t0), Supersede::None);
        assert_eq!(
            s.text(2, 3, line, t0 + Duration::from_millis(10_001)),
            Supersede::None
        );
    }

    #[test]
    fn disabled_plugin_cards_do_not_hide_lines() {
        let t0 = Instant::now();
        let mut s = SupersedeTracker::default();
        let line = "[Poll] Lunch?";
        s.card("poll", "c1", 3, hash_line(line), t0);
        s.forget_plugin("poll");
        assert_eq!(s.text(1, 3, line, t0), Supersede::None);
    }
}
