//! Pure logic of the Windows overlay window (FR-2.28): which foreground program counts as a game,
//! and where the overlay panel goes over it. Kept free of Win32 so it is tested on every host.

use mumcord_engine::now_gaming::detect::{self, Os, Process};
use mumcord_engine::store::{GameKey, InGameOverlaySettings};
use mumcord_overlay_ipc::wire::Corner;

/// Distance of the panel from the game's edges at 1080p, in pixels (same as the Linux layer).
const MARGIN: f32 = 16.0;

/// A screen rectangle in physical pixels; `right` and `bottom` are exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(self) -> i32 {
        self.right - self.left
    }

    pub fn height(self) -> i32 {
        self.bottom - self.top
    }
}

/// The game key for the program at `exe_path`, or `None` when it is not a game. A program counts
/// as a game when the user configured it, when it is on the bundled game list, or when it runs
/// from a Steam library (`steamapps\common`). `steam_app` is the game Steam reports as running;
/// it names a game from a Steam library, as the Linux layer prefers the Steam id too.
pub fn game_key(
    exe_path: &str,
    settings: &InGameOverlaySettings,
    steam_app: Option<u32>,
) -> Option<GameKey> {
    let exe = detect::exe_name(exe_path);
    if exe.is_empty() {
        return None;
    }
    let by_exe = GameKey::Exe(exe.to_owned());
    if settings.entry(&by_exe).is_some() {
        return Some(by_exe);
    }
    let process = Process {
        name: exe.to_owned(),
        args: Vec::new(),
    };
    if let Some(known) = detect::catalog().find_running(Os::Windows, &[process]) {
        return Some(known.steam.map_or(by_exe, GameKey::Steam));
    }
    if in_steam_library(exe_path) {
        return Some(steam_app.map_or(by_exe, GameKey::Steam));
    }
    None
}

fn in_steam_library(exe_path: &str) -> bool {
    exe_path
        .to_ascii_lowercase()
        .replace('/', "\\")
        .contains("\\steamapps\\common\\")
}

/// What the overlay window should do for the current foreground window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Foreground {
    /// The game's client area on screen.
    pub client: Rect,
    pub minimized: bool,
    /// A Direct3D game owns the screen exclusively; nothing can be drawn over it.
    pub exclusive_fullscreen: bool,
}

/// The game area to draw over, or `None` when the overlay must be hidden: no allowed game in the
/// foreground, or it is minimized, empty or in exclusive fullscreen.
pub fn target(foreground: Option<Foreground>, allowed: bool) -> Option<Rect> {
    let fg = foreground?;
    if !allowed || fg.minimized || fg.exclusive_fullscreen {
        return None;
    }
    (fg.client.width() > 0 && fg.client.height() > 0).then_some(fg.client)
}

/// Panel scale for a game area `height` pixels tall: 1 up to 1080p, then growing with the height
/// (the Linux layer scales by the screen the same way).
pub fn display_scale_for(height: i32) -> f32 {
    // Window heights are far below f32 precision limits.
    #[allow(clippy::cast_precision_loss)]
    let scale = (height as f32 / 1080.0).max(1.0);
    scale
}

/// Screen position of a `size` panel in `corner` of `area`, or `None` when it does not fit.
pub fn panel_position(corner: Corner, size: (i32, i32), area: Rect) -> Option<(i32, i32)> {
    let free_x = area.width() - size.0;
    let free_y = area.height() - size.1;
    if free_x < 0 || free_y < 0 {
        return None;
    }
    // A margin of a few dozen pixels; the cast cannot overflow.
    #[allow(clippy::cast_possible_truncation)]
    let margin = (MARGIN * display_scale_for(area.height())).round() as i32;
    let left = area.left + margin.min(free_x);
    let top = area.top + margin.min(free_y);
    let right = area.left + (free_x - margin).max(0);
    let bottom = area.top + (free_y - margin).max(0);
    Some(match corner {
        Corner::TopLeft => (left, top),
        Corner::TopRight => (right, top),
        Corner::BottomLeft => (left, bottom),
        Corner::BottomRight => (right, bottom),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mumcord_engine::store::{GameOverlayEntry, InGameOverlayMode};

    const AREA: Rect = Rect {
        left: 100,
        top: 50,
        right: 2020,
        bottom: 1130,
    };

    fn fg(client: Rect) -> Foreground {
        Foreground {
            client,
            minimized: false,
            exclusive_fullscreen: false,
        }
    }

    #[test]
    fn steam_library_programs_are_games_keyed_by_the_running_app() {
        let settings = InGameOverlaySettings::default();
        let path = r"D:\Games\SteamLibrary\steamapps\common\Some Game\bin\game.exe";
        assert_eq!(
            game_key(path, &settings, Some(1234)),
            Some(GameKey::Steam(1234))
        );
        assert_eq!(
            game_key(path, &settings, None),
            Some(GameKey::Exe("game.exe".into()))
        );
    }

    #[test]
    fn other_programs_are_not_games() {
        let settings = InGameOverlaySettings::default();
        assert_eq!(
            game_key(r"C:\Windows\explorer.exe", &settings, Some(1234)),
            None
        );
        assert_eq!(game_key("", &settings, None), None);
    }

    #[test]
    fn a_configured_program_is_a_game_by_its_exe_name() {
        let settings = InGameOverlaySettings {
            mode: InGameOverlayMode::On,
            games: vec![GameOverlayEntry {
                key: GameKey::Exe("MyGame.exe".into()),
                ..GameOverlayEntry::default()
            }],
        };
        assert_eq!(
            game_key(r"C:\Tools\mygame.exe", &settings, None),
            Some(GameKey::Exe("mygame.exe".into()))
        );
    }

    #[test]
    fn hidden_unless_an_allowed_game_is_visible() {
        assert_eq!(target(Some(fg(AREA)), true), Some(AREA));
        assert_eq!(target(None, true), None);
        assert_eq!(target(Some(fg(AREA)), false), None);
        let minimized = Foreground {
            minimized: true,
            ..fg(AREA)
        };
        assert_eq!(target(Some(minimized), true), None);
        let exclusive = Foreground {
            exclusive_fullscreen: true,
            ..fg(AREA)
        };
        assert_eq!(target(Some(exclusive), true), None);
        let empty = Rect {
            right: AREA.left,
            ..AREA
        };
        assert_eq!(target(Some(fg(empty)), true), None);
    }

    #[test]
    fn panel_sits_in_the_configured_corner_of_the_game() {
        let size = (200, 100);
        assert_eq!(panel_position(Corner::TopLeft, size, AREA), Some((116, 66)));
        assert_eq!(
            panel_position(Corner::BottomRight, size, AREA),
            Some((2020 - 200 - 16, 1130 - 100 - 16))
        );
        assert_eq!(panel_position(Corner::TopLeft, (3000, 100), AREA), None);
    }

    #[test]
    fn margin_and_scale_grow_above_1080p() {
        assert!((display_scale_for(720) - 1.0).abs() < f32::EPSILON);
        assert!((display_scale_for(2160) - 2.0).abs() < f32::EPSILON);
        let area = Rect {
            left: 0,
            top: 0,
            right: 3840,
            bottom: 2160,
        };
        assert_eq!(
            panel_position(Corner::TopLeft, (10, 10), area),
            Some((32, 32))
        );
    }
}
