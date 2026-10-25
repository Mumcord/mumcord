//! Overlay window options and control (FR-1.44–FR-1.46). The window itself lives in
//! `crate::overlay`; this file is the typed contract for the settings UI and the overlay page.

use mumcord_engine::{Engine, store};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use super::settings::{Language, Settings};
use super::types::{ApiError, ApiResult, ErrorCode};

/// Screen corner the overlay sits in unless it was dragged somewhere else.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum OverlayCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Top-left corner of the overlay in physical desktop pixels.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct OverlayPoint {
    pub x: i32,
    pub y: i32,
}

/// The stored overlay options (FR-1.45). Out-of-range values are clamped on save.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySettings {
    pub enabled: bool,
    /// Show everyone in the own channel (talking ones with the ring) instead of only talkers.
    pub show_whole_channel: bool,
    /// Show the own channel's name above the people.
    pub show_channel_name: bool,
    /// Percent, 20 – 100 (whole numbers keep the TypeScript type a plain `number`).
    pub opacity: u8,
    /// Used while `customPosition` is `null`.
    pub corner: OverlayCorner,
    /// Set by dragging in move mode; send `null` to go back to `corner`.
    pub custom_position: Option<OverlayPoint>,
    /// Logical pixels, 160 – 800.
    pub width: u32,
    /// Logical pixels, 80 – 1200.
    pub height: u32,
}

/// Everything the settings UI and the overlay page need to know about the overlay.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type)]
#[serde(rename_all = "camelCase")]
pub struct OverlayState {
    pub settings: OverlaySettings,
    /// Move mode: the overlay takes clicks so it can be dragged; ends with
    /// `overlay_move_mode(false)`, which stores the new place.
    pub move_mode: bool,
    /// Whether this desktop keeps the overlay above other windows (FR-1.46). False on a
    /// native Linux Wayland session.
    pub on_top_supported: bool,
    /// Whether the app can place the overlay (corner presets, restoring a dragged place).
    /// False on a native Linux Wayland session, where the compositor decides.
    pub position_supported: bool,
    /// i18n key (namespace `overlay`) of the text explaining a limitation of this desktop, or
    /// `null` if there is none. The exclusive-fullscreen note `overlay:limits.exclusiveFullscreen`
    /// applies everywhere and is not repeated here.
    pub explanation_key: Option<String>,
    /// KDE Plasma on native Wayland: the settings offer a KWin window rule for the overlay.
    pub kde_wayland: bool,
    /// Whether Mumcord's KWin window rule is in the user's `kwinrulesrc` (only checked on
    /// KDE Wayland, else `false`).
    pub kwin_rule_installed: bool,
    /// UI language from the settings, for the overlay page (`null` follows the OS).
    pub language: Option<Language>,
}

/// The overlay options or move mode changed (also sent when the UI language changes).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct OverlayChanged {
    pub state: OverlayState,
}

pub const OPACITY_RANGE: (u8, u8) = (20, 100);
pub const WIDTH_RANGE: (u32, u32) = (160, 800);
pub const HEIGHT_RANGE: (u32, u32) = (80, 1200);

impl From<&store::OverlaySettings> for OverlaySettings {
    fn from(s: &store::OverlaySettings) -> Self {
        Self {
            enabled: s.enabled,
            show_whole_channel: s.show_whole_channel,
            show_channel_name: s.show_channel_name,
            opacity: opacity_percent(s.opacity),
            corner: match s.corner {
                store::OverlayCorner::TopLeft => OverlayCorner::TopLeft,
                store::OverlayCorner::TopRight => OverlayCorner::TopRight,
                store::OverlayCorner::BottomLeft => OverlayCorner::BottomLeft,
                store::OverlayCorner::BottomRight => OverlayCorner::BottomRight,
            },
            custom_position: s.custom_position.map(|p| OverlayPoint { x: p.x, y: p.y }),
            width: s.width,
            height: s.height,
        }
    }
}

/// Stored fraction as whole percent within `OPACITY_RANGE`.
fn opacity_percent(fraction: f32) -> u8 {
    let fraction = if fraction.is_finite() {
        fraction
    } else {
        store::OverlaySettings::default().opacity
    };
    // Clamped to 20–100 first, so the cast cannot truncate or wrap.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let percent = (fraction * 100.0).round().clamp(20.0, 100.0) as u8;
    percent
}

impl OverlaySettings {
    /// The record to store, with every value in its allowed range.
    pub fn to_store(self) -> store::OverlaySettings {
        let opacity = f32::from(self.opacity.clamp(OPACITY_RANGE.0, OPACITY_RANGE.1)) / 100.0;
        store::OverlaySettings {
            enabled: self.enabled,
            show_whole_channel: self.show_whole_channel,
            show_channel_name: self.show_channel_name,
            opacity,
            corner: match self.corner {
                OverlayCorner::TopLeft => store::OverlayCorner::TopLeft,
                OverlayCorner::TopRight => store::OverlayCorner::TopRight,
                OverlayCorner::BottomLeft => store::OverlayCorner::BottomLeft,
                OverlayCorner::BottomRight => store::OverlayCorner::BottomRight,
            },
            custom_position: self
                .custom_position
                .map(|p| store::OverlayPoint { x: p.x, y: p.y }),
            width: self.width.clamp(WIDTH_RANGE.0, WIDTH_RANGE.1),
            height: self.height.clamp(HEIGHT_RANGE.0, HEIGHT_RANGE.1),
        }
    }
}

/// Builds the state from the stored settings and the current move mode.
pub fn state_of(stored: &store::Settings, move_mode: bool) -> OverlayState {
    let support = crate::overlay::support();
    let (kde_wayland, kwin_rule_installed) = crate::overlay::kwin_rule_state();
    OverlayState {
        settings: OverlaySettings::from(&stored.overlay),
        move_mode,
        on_top_supported: support.on_top,
        position_supported: support.position,
        explanation_key: support.explanation_key.map(str::to_owned),
        kde_wayland,
        kwin_rule_installed,
        language: Settings::from_store(stored).language,
    }
}

/// Current overlay options, move mode and what this desktop supports.
#[tauri::command]
#[specta::specta]
pub fn overlay_state(app: AppHandle, engine: State<'_, Engine>) -> ApiResult<OverlayState> {
    Ok(state_of(
        &engine.settings()?,
        crate::overlay::move_mode(&app),
    ))
}

/// Stores the overlay options and applies them right away (opens, closes, moves or resizes
/// the overlay). Returns the stored state; also emits `overlayChanged`.
// Async: creating a window from a synchronous command can deadlock on Windows.
#[tauri::command]
#[specta::specta]
pub async fn overlay_update(
    app: AppHandle,
    engine: State<'_, Engine>,
    settings: OverlaySettings,
) -> ApiResult<OverlayState> {
    let mut stored = engine.settings()?;
    stored.overlay = settings.to_store();
    let stored = engine.update_settings(stored).await?;
    Ok(crate::overlay::apply(&app, &stored))
}

/// Shows or hides the overlay (same as the tray menu item).
#[tauri::command]
#[specta::specta]
pub async fn overlay_set_enabled(
    app: AppHandle,
    engine: State<'_, Engine>,
    enabled: bool,
) -> ApiResult<OverlayState> {
    Ok(crate::overlay::set_enabled(&app, &engine, enabled).await?)
}

/// Turns move mode on (the overlay takes clicks and can be dragged) or off (click-through
/// again; a dragged place is stored as `customPosition`). Ignored while the overlay is off.
#[tauri::command]
#[specta::specta]
pub async fn overlay_move_mode(
    app: AppHandle,
    engine: State<'_, Engine>,
    active: bool,
) -> ApiResult<OverlayState> {
    Ok(crate::overlay::set_move_mode(&app, &engine, active).await?)
}

/// KDE Wayland only: adds or updates the KWin window rule that keeps the overlay on top and
/// in its corner, and asks KWin to reload its rules. `invalidInput` elsewhere, `io` if the
/// rules file cannot be written, `internal` if KWin cannot be reached (the rule is written).
#[tauri::command]
#[specta::specta]
pub async fn overlay_kwin_rule_install(
    app: AppHandle,
    engine: State<'_, Engine>,
) -> ApiResult<OverlayState> {
    #[cfg(target_os = "linux")]
    if crate::overlay::kde_wayland() {
        let stored = engine.settings()?;
        let placement = crate::overlay::kwin_placement(&app, &stored.overlay)
            .map_err(|e| ApiError::new(ErrorCode::Internal, e))?;
        let path = crate::kwin_rule::config_path()
            .ok_or_else(|| ApiError::new(ErrorCode::Io, "no config directory"))?;
        crate::kwin_rule::install_in(&path, placement)
            .map_err(|e| ApiError::new(ErrorCode::Io, e.to_string()))?;
        crate::kwin_rule::reconfigure()
            .await
            .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?;
        return Ok(state_of(&stored, crate::overlay::move_mode(&app)));
    }
    let _ = (&app, &engine);
    Err(ApiError::new(
        ErrorCode::InvalidInput,
        "not KDE Plasma on Wayland",
    ))
}

/// KDE Wayland only: removes Mumcord's KWin window rule again and asks KWin to reload.
#[tauri::command]
#[specta::specta]
pub async fn overlay_kwin_rule_remove(
    app: AppHandle,
    engine: State<'_, Engine>,
) -> ApiResult<OverlayState> {
    #[cfg(target_os = "linux")]
    if crate::overlay::kde_wayland() {
        let path = crate::kwin_rule::config_path()
            .ok_or_else(|| ApiError::new(ErrorCode::Io, "no config directory"))?;
        crate::kwin_rule::remove_in(&path)
            .map_err(|e| ApiError::new(ErrorCode::Io, e.to_string()))?;
        crate::kwin_rule::reconfigure()
            .await
            .map_err(|e| ApiError::new(ErrorCode::Internal, e.to_string()))?;
        return Ok(state_of(
            &engine.settings()?,
            crate::overlay::move_mode(&app),
        ));
    }
    let _ = (&app, &engine);
    Err(ApiError::new(
        ErrorCode::InvalidInput,
        "not KDE Plasma on Wayland",
    ))
}

/// KDE only: opens System Settings → Window Rules (`systemsettings kcm_kwinrules`, else
/// `kcmshell6 kwinrules`). `io` if neither could be started.
#[tauri::command]
#[specta::specta]
pub fn overlay_open_kde_window_rules() -> ApiResult<()> {
    #[cfg(target_os = "linux")]
    return crate::overlay::open_kde_window_rules()
        .map_err(|e| ApiError::new(ErrorCode::Io, e.to_string()));
    #[cfg(not(target_os = "linux"))]
    Err(ApiError::new(ErrorCode::InvalidInput, "not KDE Plasma"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_clamped_and_round_trip() {
        let stored = store::OverlaySettings {
            enabled: true,
            show_whole_channel: true,
            show_channel_name: false,
            opacity: 0.5,
            corner: store::OverlayCorner::BottomLeft,
            custom_position: Some(store::OverlayPoint { x: -20, y: 40 }),
            width: 300,
            height: 200,
        };
        let api = OverlaySettings::from(&stored);
        assert_eq!(api.to_store(), stored);

        assert_eq!(api.opacity, 50);

        let wild = OverlaySettings {
            opacity: 7,
            width: 1,
            height: 99_999,
            ..api
        }
        .to_store();
        assert!((wild.opacity - 0.2).abs() < f32::EPSILON);
        assert_eq!((wild.width, wild.height), (WIDTH_RANGE.0, HEIGHT_RANGE.1));
        assert_eq!(opacity_percent(f32::NAN), 90);
        assert_eq!(opacity_percent(3.0), 100);
    }

    #[test]
    fn options_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("mumcord-overlay-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create dir");
        let wanted = OverlaySettings {
            enabled: true,
            show_whole_channel: false,
            show_channel_name: true,
            opacity: 60,
            corner: OverlayCorner::TopRight,
            custom_position: Some(OverlayPoint { x: 100, y: 50 }),
            width: 280,
            height: 360,
        };
        {
            let store = store::Store::open(&dir).expect("open");
            let mut settings = store.settings().expect("settings");
            settings.overlay = wanted.to_store();
            store.save_settings(&settings).expect("save");
        }
        let reopened = store::Store::open(&dir).expect("reopen");
        let state = state_of(&reopened.settings().expect("reload"), false);
        assert_eq!(state.settings, wanted);
        drop(reopened);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
