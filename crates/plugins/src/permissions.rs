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
}

const fn all(id: &'static str, risk: Risk, text_en: &'static str) -> Permission {
    Permission {
        id,
        risk,
        official_only: false,
        text_en,
    }
}

const fn official(id: &'static str, risk: Risk, text_en: &'static str) -> Permission {
    Permission {
        id,
        risk,
        official_only: true,
        text_en,
    }
}

pub const CATALOGUE: &[Permission] = &[
    all("ui.panel", Risk::Green, "Show a panel in the Plugins strip"),
    all(
        "ui.member-badge",
        Risk::Green,
        "Show a small badge next to user names",
    ),
    all(
        "ui.profile-card",
        Risk::Green,
        "Add a section to profile cards",
    ),
    all("ui.chat-card", Risk::Green, "Show cards in the chat"),
    all(
        "ui.user-menu",
        Risk::Green,
        "Add an entry to a user's right-click menu",
    ),
    all("ui.dialog", Risk::Green, "Open its own window"),
    all(
        "events.self",
        Risk::Green,
        "Know your own status (mute, deafen, channel)",
    ),
    all("events.voice", Risk::Yellow, "Know who is talking"),
    all(
        "events.presence",
        Risk::Yellow,
        "Know who joins and leaves your channel",
    ),
    all(
        "events.chat-activity",
        Risk::Yellow,
        "Know when someone writes (not what)",
    ),
    all(
        "peers.messages",
        Risk::Yellow,
        "Exchange data with this plugin for other people in your channel",
    ),
    all("notify", Risk::Green, "Show notifications"),
    all("random.bytes", Risk::Green, "Get secure random numbers"),
    all("commands", Risk::Green, "Add chat commands such as /roll"),
    all(
        "chat.send",
        Risk::Yellow,
        "Post a chat message for you after you click or type a command",
    ),
    all(
        "chat.supersedes",
        Risk::Green,
        "Hide its own text line for people who see its card",
    ),
    all(
        "chat.triggers",
        Risk::Yellow,
        "Read chat messages that start with its trigger words, such as !poll",
    ),
    official("net.http", Risk::Red, "Talk to a server you choose"),
    official(
        "secrets",
        Risk::Red,
        "Keep a secret (e.g. an API key) in your system keychain",
    ),
    official(
        "ui.webview",
        Risk::Yellow,
        "Show its own web content in a sealed frame (no internet)",
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
