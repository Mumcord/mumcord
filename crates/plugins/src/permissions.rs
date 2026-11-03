//! The closed permission catalogue (FR-6.27, FR-9.22). Only these ids exist: an unknown id in a
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
    /// First plugin API minor `(major, minor)` that has this permission (FR-9.9). A plugin built
    /// for an older minor can never hold it.
    pub min_api: (u64, u64),
}

/// The API minor every permission of the first catalogue comes with.
pub const API_1_0: (u64, u64) = (1, 0);
/// The API minor that added the surface permissions of FR-9.22.
pub const API_1_1: (u64, u64) = (1, 1);

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
        min_api: API_1_0,
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
        min_api: API_1_0,
    }
}

/// A permission that exists from API 1.1 on, available to every plugin.
const fn all_1_1(
    id: &'static str,
    risk: Risk,
    text_en: &'static str,
    explanation_en: &'static str,
) -> Permission {
    Permission {
        min_api: API_1_1,
        ..all(id, risk, text_en, explanation_en)
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
    all(
        "chat.patterns",
        Risk::Yellow,
        "Recognise certain chat messages, such as pasted game fits",
        "It only receives chat messages in your channel that start the way it declares, such as a pasted game fit. No other chat text reaches it.",
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
    all(
        "ui.webview",
        Risk::Yellow,
        "Show its own web content in a sealed frame (no internet)",
        "It can show its own web content in a sealed frame with no internet access.",
    ),
    all_1_1(
        "ui.stage",
        Risk::Green,
        "Show an activity in the media area",
        "Only when you start or join it. People in your channel who have {{plugin}} can join.",
    ),
    all_1_1(
        "ui.side-panel",
        Risk::Green,
        "Show its own content in a side panel",
        "Its content appears in a column next to the chat, only when you open it.",
    ),
    all_1_1(
        "ui.app-menu",
        Risk::Green,
        "Add items to the Apps menu",
        "Up to {{count}} items, shown under “Plugins”.",
    ),
    all_1_1(
        "net.origins",
        Risk::Red,
        "Talk to these websites",
        "{{plugin}} and its page can send and receive data only with the websites listed below.",
    ),
    all_1_1(
        "peers.relay",
        Risk::Yellow,
        "Send data through the server's relay",
        "Faster and larger moves. End-to-end encrypted; the relay can't read it.",
    ),
    all_1_1(
        "audio.effects",
        Risk::Green,
        "Play short sounds that come with the plugin",
        "Short sounds that come with the plugin. You can turn them down or mute them in Settings → Plugins.",
    ),
    all_1_1(
        "files.save",
        Risk::Green,
        "Save files you choose",
        "Mumcord asks you where to save each file after you click; the plugin never sees your folders.",
    ),
    all_1_1(
        "chat.post-image",
        Risk::Yellow,
        "Post an image in chat after you click",
        "After you click in the plugin, it can post one picture into your current channel, labelled with its name.",
    ),
    all_1_1(
        "files.import-picture",
        Risk::Green,
        "Open pictures you choose",
        "Mumcord lets you pick a picture after you click; the plugin gets only that picture, never your folders.",
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
        assert_eq!(CATALOGUE.len(), 30);
        let mut ids: Vec<_> = CATALOGUE.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 30);
        assert!(get("files").is_none());
    }

    #[test]
    fn only_net_http_and_secrets_are_official_only() {
        let official: Vec<_> = CATALOGUE
            .iter()
            .filter(|p| p.official_only)
            .map(|p| p.id)
            .collect();
        assert_eq!(official, ["net.http", "secrets"]);
        assert_eq!(get("net.http").map(|p| p.risk), Some(Risk::Red));
        assert_eq!(get("events.voice").map(|p| p.risk), Some(Risk::Yellow));
    }

    #[test]
    fn api_1_1_permissions_follow_fr_9_22() {
        let added: Vec<_> = CATALOGUE
            .iter()
            .filter(|p| p.min_api == API_1_1)
            .map(|p| (p.id, p.risk, p.official_only))
            .collect();
        assert_eq!(
            added,
            [
                ("ui.stage", Risk::Green, false),
                ("ui.side-panel", Risk::Green, false),
                ("ui.app-menu", Risk::Green, false),
                ("net.origins", Risk::Red, false),
                ("peers.relay", Risk::Yellow, false),
                ("audio.effects", Risk::Green, false),
                ("files.save", Risk::Green, false),
                ("chat.post-image", Risk::Yellow, false),
                ("files.import-picture", Risk::Green, false),
            ]
        );
        assert!(
            CATALOGUE
                .iter()
                .filter(|p| p.min_api != API_1_1)
                .all(|p| p.min_api == API_1_0)
        );
    }

    #[test]
    fn ui_webview_is_available_to_all() {
        let webview = get("ui.webview").expect("in the catalogue");
        assert!(!webview.official_only);
        assert_eq!(webview.risk, Risk::Yellow);
        assert_eq!(webview.min_api, API_1_0);
    }
}
