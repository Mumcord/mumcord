//! In-game overlay (FR-1.46b): the bundled blocklist and the allow-decision logic that the app
//! shell (`src-tauri`) uses to answer each game's IPC hello. Pure logic, tested here; the layer
//! install, IPC server and injection live in the app shell and the platform crates.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::store::{GameKey, InGameOverlayMode, InGameOverlaySettings};

/// One game on the bundled blocklist (QA.56).
#[derive(Debug, Clone, Deserialize)]
pub struct BlockedGame {
    pub name: String,
    #[serde(default)]
    pub steam: Option<u32>,
    #[serde(default)]
    windows: Vec<String>,
    #[serde(default)]
    linux: Vec<String>,
    /// The anti-cheat or system that blocks overlays, e.g. "Riot Vanguard".
    pub anti_cheat: String,
    /// Shown to the user; plain English text.
    pub reason: String,
    /// URL backing the reason.
    pub source: String,
}

impl BlockedGame {
    /// A stable key for this game: its Steam id if it has one, else its first executable name.
    pub fn first_key(&self) -> GameKey {
        match self.steam {
            Some(id) => GameKey::Steam(id),
            None => GameKey::Exe(
                self.windows
                    .first()
                    .or_else(|| self.linux.first())
                    .cloned()
                    .unwrap_or_default(),
            ),
        }
    }

    /// Whether this entry covers `key` (executable names compared ignoring case).
    pub fn matches(&self, key: &GameKey) -> bool {
        match key {
            GameKey::Steam(id) => self.steam == Some(*id),
            GameKey::Exe(exe) => self
                .windows
                .iter()
                .chain(&self.linux)
                .any(|n| n.eq_ignore_ascii_case(exe)),
        }
    }
}

#[derive(Deserialize)]
struct BlocklistFile {
    games: Vec<BlockedGame>,
}

/// The bundled blocklist.
pub struct Blocklist {
    games: Vec<BlockedGame>,
}

static BLOCKLIST: LazyLock<Blocklist> = LazyLock::new(|| {
    match serde_json::from_str::<BlocklistFile>(include_str!("blocklist.json")) {
        Ok(file) => Blocklist { games: file.games },
        Err(e) => {
            // Covered by a test; never expected in a release.
            tracing::error!(error = %e, "the bundled overlay blocklist is invalid");
            Blocklist { games: Vec::new() }
        }
    }
});

/// The bundled blocklist.
pub fn blocklist() -> &'static Blocklist {
    &BLOCKLIST
}

impl Blocklist {
    pub fn len(&self) -> usize {
        self.games.len()
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    /// The blocklist entry for `key`, if the game is on it.
    pub fn find(&self, key: &GameKey) -> Option<&BlockedGame> {
        self.games.iter().find(|g| g.matches(key))
    }

    /// All blocked games.
    pub fn iter(&self) -> impl Iterator<Item = &BlockedGame> {
        self.games.iter()
    }
}

/// Why a game shows the overlay or not, for the IPC answer and the settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayDecision {
    /// Draw the overlay in this game.
    Show,
    /// Global mode is Off.
    GlobalOff,
    /// Switched off for this game.
    GameOff,
    /// On the blocklist and the risk was not confirmed (QA.56).
    Blocked,
}

impl OverlayDecision {
    pub fn shows(self) -> bool {
        self == OverlayDecision::Show
    }
}

/// Decides whether the in-game overlay is drawn for `key` (FR-1.46b, QA.55, QA.56):
/// - global mode Off → never;
/// - on the blocklist and not confirmed for this game → never, even when On;
/// - a per-game `enabled` override wins over the global mode;
/// - otherwise the global mode decides (On → show, Off → hide).
///
/// A blocklisted game is only shown when its entry both overrides to enabled and has the risk
/// confirmed; confirming the risk without enabling it still leaves it following the mode.
pub fn decide(settings: &InGameOverlaySettings, key: &GameKey) -> OverlayDecision {
    if settings.mode == InGameOverlayMode::Off {
        return OverlayDecision::GlobalOff;
    }
    let entry = settings.entry(key);
    let blocked = blocklist().find(key).is_some();
    if blocked && !entry.is_some_and(|e| e.risk_confirmed) {
        return OverlayDecision::Blocked;
    }
    match entry.and_then(|e| e.enabled) {
        Some(true) => OverlayDecision::Show,
        Some(false) => OverlayDecision::GameOff,
        // On, no per-game override: shown (blocklisted games needed the confirmation above).
        None if blocked => OverlayDecision::Blocked,
        None => OverlayDecision::Show,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::GameOverlayEntry;

    #[test]
    fn the_bundled_blocklist_parses_and_has_sources() {
        let list = blocklist();
        assert!(list.len() >= 5, "expected several blocked games");
        for game in &list.games {
            assert!(
                game.source.starts_with("https://"),
                "{} has no source",
                game.name
            );
            assert!(!game.reason.is_empty());
            assert!(game.steam.is_some() || !game.windows.is_empty() || !game.linux.is_empty());
        }
        assert!(list.find(&GameKey::Steam(730)).is_some());
        assert!(
            list.find(&GameKey::Exe("VALORANT-Win64-Shipping.exe".into()))
                .is_some(),
            "Valorant should be blocked"
        );
        assert!(
            list.find(&GameKey::Exe("VALORANT-WIN64-SHIPPING.EXE".into()))
                .is_some()
        );
        assert!(list.find(&GameKey::Steam(570)).is_none());
    }

    fn on() -> InGameOverlaySettings {
        InGameOverlaySettings {
            mode: InGameOverlayMode::On,
            games: Vec::new(),
        }
    }

    fn entry(key: GameKey) -> GameOverlayEntry {
        GameOverlayEntry {
            key,
            ..GameOverlayEntry::default()
        }
    }

    #[test]
    fn global_off_hides_everything() {
        let settings = InGameOverlaySettings::default();
        assert_eq!(
            decide(&settings, &GameKey::Exe("cs2".into())),
            OverlayDecision::GlobalOff
        );
        assert_eq!(
            decide(&settings, &GameKey::Steam(730)),
            OverlayDecision::GlobalOff
        );
    }

    #[test]
    fn unknown_games_follow_the_global_mode() {
        assert_eq!(
            decide(&on(), &GameKey::Exe("mygame.exe".into())),
            OverlayDecision::Show
        );
    }

    #[test]
    fn per_game_override_wins() {
        let mut settings = on();
        settings.games.push(GameOverlayEntry {
            enabled: Some(false),
            ..entry(GameKey::Exe("mygame.exe".into()))
        });
        assert_eq!(
            decide(&settings, &GameKey::Exe("MyGame.exe".into())),
            OverlayDecision::GameOff
        );
        settings.games[0].enabled = Some(true);
        assert_eq!(
            decide(&settings, &GameKey::Exe("mygame.exe".into())),
            OverlayDecision::Show
        );
    }

    #[test]
    fn blocklisted_games_stay_off_until_confirmed_and_enabled() {
        let cs2 = GameKey::Steam(730);
        // On, no entry: blocked despite the global On.
        assert_eq!(decide(&on(), &cs2), OverlayDecision::Blocked);
        // Confirmed but not enabled: still blocked (confirmation alone is not consent to show).
        let mut settings = on();
        settings.games.push(GameOverlayEntry {
            risk_confirmed: true,
            enabled: None,
            ..entry(cs2.clone())
        });
        assert_eq!(decide(&settings, &cs2), OverlayDecision::Blocked);
        // Enabled but not confirmed: blocked.
        settings.games[0].risk_confirmed = false;
        settings.games[0].enabled = Some(true);
        assert_eq!(decide(&settings, &cs2), OverlayDecision::Blocked);
        // Enabled and confirmed: shown.
        settings.games[0].risk_confirmed = true;
        assert_eq!(decide(&settings, &cs2), OverlayDecision::Show);
        // Confirmed but switched off: off.
        settings.games[0].enabled = Some(false);
        assert_eq!(decide(&settings, &cs2), OverlayDecision::GameOff);
    }

    #[test]
    fn global_off_beats_a_confirmed_blocklisted_override() {
        let mut settings = InGameOverlaySettings::default();
        settings.games.push(GameOverlayEntry {
            risk_confirmed: true,
            enabled: Some(true),
            ..entry(GameKey::Steam(730))
        });
        assert_eq!(
            decide(&settings, &GameKey::Steam(730)),
            OverlayDecision::GlobalOff
        );
    }
}
