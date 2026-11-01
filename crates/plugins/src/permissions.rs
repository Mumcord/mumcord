//! The closed permission catalogue of API 1.0 (FR-6.27). Only these ids exist: an unknown id in a
//! manifest fails the load, so a plugin never runs without something it depends on. Things that are
//! never grantable (raw network, files, audio, certificates, clipboard, settings, chat text,
//! FR-6.28) simply have no id here.

/// Risk colour shown next to the permission in the install prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// Low risk.
    Green,
    /// Medium risk.
    Yellow,
    /// High risk.
    Red,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permission {
    pub id: &'static str,
    pub risk: Risk,
    /// Only plugins that are official (FR-6.51) may ask for it.
    pub official_only: bool,
    /// Plain-language prompt text; the English source of the `pluginPermissions` locales.
    pub text_en: &'static str,
    /// What the permission means for the user, in one sentence; the English source of the
    /// `pluginPermissions` `explanations` locales.
    pub explanation_en: &'static str,
}

const fn all(
    id: &'static str,
    risk: Risk,
    text_en: &'static str,
    explanation_en: &'static str,
) -> Permission {
    Permission {
        id,
        risk,
        official_only: false,
        text_en,
        explanation_en,
    }
}

const fn official(
    id: &'static str,
    risk: Risk,
    text_en: &'static str,
    explanation_en: &'static str,
) -> Permission {
    Permission {
        id,
        risk,
        official_only: true,
        text_en,
        explanation_en,
    }
}

pub const CATALOGUE: &[Permission] = &[
    all(
        "ui.panel",
        Risk::Green,
        "Show a panel in the Plugins strip",
        "A small panel for this plugin appears in the Plugins strip.",
    ),
    all(
        "ui.member-badge",
        Risk::Green,
        "Show a small badge next to user names",
        "A small icon can appear next to people's names in the member list.",
    ),
    all(
        "ui.profile-card",
        Risk::Green,
        "Add a section to profile cards",
        "It can add a section to people's profile cards.",
    ),
    all(
        "ui.chat-card",
        Risk::Green,
        "Show cards in the chat",
        "It can show its own cards in the chat, for example a poll or a roll result.",
    ),
    all(
        "ui.user-menu",
        Risk::Green,
        "Add an entry to a user's right-click menu",
        "It can add an entry to the right-click menu of a user.",
    ),
    all(
        "ui.dialog",
        Risk::Green,
        "Open its own window",
        "It can open its own window, for example to set something up.",
    ),
    all(
        "events.self",
        Risk::Green,
        "Know your own status (mute, deafen, channel)",
        "It sees whether you are muted or deafened and which channel you are in.",
    ),
    all(
        "events.voice",
        Risk::Yellow,
        "Know who is talking",
        "It sees when people start and stop talking, never what is said.",
    ),
    all(
        "events.presence",
        Risk::Yellow,
        "Know who joins and leaves your channel",
        "It sees who joins or leaves your channel.",
    ),
    all(
        "events.chat-activity",
        Risk::Yellow,
        "Know when someone writes (not what)",
        "It sees that someone is writing, never what they write.",
    ),
    all(
        "peers.messages",
        Risk::Yellow,
        "Exchange data with this plugin for other people in your channel",
        "It can send small amounts of data to this plugin on other people's computers in your channel.",
    ),
    all(
        "notify",
        Risk::Green,
        "Show notifications",
        "It can show notifications. They stay quiet during Do Not Disturb.",
    ),
    all(
        "random.bytes",
        Risk::Green,
        "Get secure random numbers",
        "It can ask for secure random numbers, for example for dice.",
    ),
    all(
        "commands",
        Risk::Green,
        "Add chat commands such as /roll",
        "It can add chat commands that you type, such as /roll.",
    ),
    all(
        "chat.send",
        Risk::Yellow,
        "Post a chat message for you after you click or type a command",
        "It can post a chat message in your name, only after you click or type a command.",
    ),
    all(
        "chat.supersedes",
        Risk::Green,
        "Hide its own text line for people who see its card",
        "People who see its card don't also see its plain text line.",
    ),
    all(
        "chat.triggers",
        Risk::Yellow,
        "Read chat messages that start with its trigger words, such as !poll",
        "It reads chat messages that start with its trigger words, such as !poll, and no others.",
    ),
    official(
        "net.http",
        Risk::Red,
        "Talk to a server you choose",
        "It can talk to one server you choose, and only to that one.",
    ),
    official(
        "secrets",
        Risk::Red,
        "Keep a secret (e.g. an API key) in your system keychain",
        "It can keep a secret, such as an API key, in your system keychain.",
    ),
    official(
        "ui.webview",
        Risk::Yellow,
        "Show its own web content in a sealed frame (no internet)",
        "It can show its own web content in a sealed frame with no internet access.",
    ),
];

/// Looks up a permission by id.
pub fn get(id: &str) -> Option<&'static Permission> {
    CATALOGUE.iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_is_closed_and_unique() {
        assert_eq!(CATALOGUE.len(), 20);
        let mut ids: Vec<_> = CATALOGUE.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 20);
        assert!(get("files").is_none());
    }

    #[test]
    fn only_three_permissions_are_official_only() {
        let official: Vec<_> = CATALOGUE
            .iter()
            .filter(|p| p.official_only)
            .map(|p| p.id)
            .collect();
        assert_eq!(official, ["net.http", "secrets", "ui.webview"]);
        assert_eq!(get("net.http").map(|p| p.risk), Some(Risk::Red));
        assert_eq!(get("events.voice").map(|p| p.risk), Some(Risk::Yellow));
    }
}
