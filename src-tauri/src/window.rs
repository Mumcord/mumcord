//! Main window behaviour: what closing the window does (FR-1.38) and clean shutdown.
use mumcord_engine::{ConnState, Engine, store::CloseBehaviour};
use tauri::{AppHandle, Manager, WebviewWindow, Window, WindowEvent};
use tauri_specta::Event;

use crate::commands::app::CloseRequested;

/// Label of the only window (tauri.conf.json).
pub const MAIN: &str = "main";

/// What a close request of the main window leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseAction {
    Quit,
    MinimizeToTray,
    Ask,
}

/// Closing quits right away unless at least one server session is active (or being set up),
/// in which case the stored close behaviour decides.
pub fn decide_close(connected_servers: usize, behaviour: CloseBehaviour) -> CloseAction {
    if connected_servers == 0 {
        return CloseAction::Quit;
    }
    match behaviour {
        CloseBehaviour::Ask => CloseAction::Ask,
        CloseBehaviour::MinimizeToTray => CloseAction::MinimizeToTray,
        CloseBehaviour::Quit => CloseAction::Quit,
    }
}

/// Number of servers that are connected or being (re)connected.
fn connected_servers(engine: &Engine) -> usize {
    engine
        .connections()
        .iter()
        .filter(|c| !matches!(c, ConnState::Disconnected { .. }))
        .count()
}

/// Window event hook registered on the Tauri builder.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != MAIN {
        return;
    }
    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    // Closing never destroys the window directly; quitting goes through `quit` so the
    // connection ends cleanly.
    api.prevent_close();
    let app = window.app_handle();
    let engine = app.state::<Engine>();
    let behaviour = engine.settings().map_or_else(
        |e| {
            tracing::warn!(error = %e, "reading the close behaviour failed; asking");
            CloseBehaviour::Ask
        },
        |s| s.close_behaviour,
    );
    let connected = connected_servers(&engine);
    match decide_close(connected, behaviour) {
        CloseAction::Quit => quit(app),
        CloseAction::MinimizeToTray => minimize_to_tray(app),
        CloseAction::Ask => {
            if let Err(e) = (CloseRequested {
                connected_servers: u32::try_from(connected).unwrap_or(u32::MAX),
            })
            .emit(app)
            {
                tracing::warn!(error = %e, "asking the UI about closing failed; minimizing");
                minimize_to_tray(app);
            }
        }
    }
}

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    let window = app.get_webview_window(MAIN);
    if window.is_none() {
        tracing::warn!("main window not found");
    }
    window
}

/// Brings the main window back from the tray, the taskbar or behind other windows.
pub fn show(app: &AppHandle) {
    let Some(window) = main_window(app) else {
        return;
    };
    let result = window
        .show()
        .and_then(|()| window.unminimize())
        .and_then(|()| window.set_focus());
    if let Err(e) = result {
        tracing::warn!(error = %e, "showing the main window failed");
    }
}

/// The window starts invisible (tauri.conf.json) so a start at login can stay in the tray
/// without flashing up first. Without a visible tray it goes to the taskbar instead, so it can
/// never get lost.
pub fn reveal_at_start(app: &AppHandle, start_hidden: bool) {
    if !start_hidden {
        show(app);
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if crate::tray::is_visible(&app).await {
            tracing::info!("started at login; staying in the tray");
            return;
        }
        let Some(window) = main_window(&app) else {
            return;
        };
        tracing::warn!("no system tray available; starting minimized to the taskbar instead");
        if let Err(e) = window.show().and_then(|()| window.minimize()) {
            tracing::warn!(error = %e, "minimizing the main window failed");
        }
    });
}

/// Hides the window while staying connected. Without a tray to bring it back, the window is
/// only minimized to the taskbar so it can never get lost.
pub fn minimize_to_tray(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(window) = main_window(&app) else {
            return;
        };
        let result = if crate::tray::is_visible(&app).await {
            window.hide()
        } else {
            tracing::warn!("no system tray available; minimizing to the taskbar instead");
            window.minimize()
        };
        if let Err(e) = result {
            tracing::warn!(error = %e, "minimizing the main window failed");
        }
    });
}

/// Disconnects cleanly and exits the app.
pub fn quit(app: &AppHandle) {
    after_disconnect(app, |app| app.exit(0));
}

/// Disconnects cleanly and restarts the app. `request_restart` exits through the event loop,
/// so the single-instance lock is released before the new process starts.
pub fn restart(app: &AppHandle) {
    after_disconnect(app, AppHandle::request_restart);
}

fn after_disconnect(app: &AppHandle, then: impl FnOnce(&AppHandle) + Send + 'static) {
    if connected_servers(&app.state::<Engine>()) == 0 {
        then(app);
        return;
    }
    // Keeps the voice channel so the next start can offer to rejoin it (QA.9). Waits (bounded)
    // until the connections have sent what is still queued and their TLS close.
    let closed = app.state::<Engine>().shutdown();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        closed.await;
        then(&app);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_without_a_connection_always_quits() {
        for behaviour in [
            CloseBehaviour::Ask,
            CloseBehaviour::MinimizeToTray,
            CloseBehaviour::Quit,
        ] {
            assert_eq!(decide_close(0, behaviour), CloseAction::Quit);
        }
    }

    #[test]
    fn closing_while_connected_follows_the_setting() {
        for count in [1, 3] {
            assert_eq!(decide_close(count, CloseBehaviour::Ask), CloseAction::Ask);
            assert_eq!(
                decide_close(count, CloseBehaviour::MinimizeToTray),
                CloseAction::MinimizeToTray
            );
            assert_eq!(decide_close(count, CloseBehaviour::Quit), CloseAction::Quit);
        }
    }
}
