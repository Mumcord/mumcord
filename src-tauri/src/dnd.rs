//! Do Not Disturb (FR-3.111, FR-3.112): combines the Mumcord switch with the followed system
//! state and hands the result to everything that notifies. While it is in effect the sound hub
//! keeps notification sounds silent and `notify_message` shows neither a toast nor a system
//! notification; unread badges keep counting.
//!
//! The system state is read live where the desktop exposes it:
//! - Linux: the notification service's `Inhibited` property (KDE Plasma and other desktops
//!   that implement it), else GNOME's "show-banners" setting through the settings portal.
//! - Windows: the Focus Assist / Do not disturb profile from the shell's notification state
//!   (polled; there is no public change notification).
//! - macOS and other platforms: no public API for the Focus state, so only the Mumcord switch
//!   works there and the settings say so.

use std::sync::Mutex;

use mumcord_engine::{Engine, store};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::commands::dnd::{DndChanged, DndState, SystemDnd};
use crate::commands::types::ApiResult;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

/// The last system state read; `SystemDnd::default()` until a reader reports.
#[derive(Default)]
struct SystemState(Mutex<SystemDnd>);

/// Registers the state, applies the stored switch and starts reading the system state. Call
/// once after the engine is managed.
pub fn init(app: &AppHandle) {
    app.manage(SystemState::default());
    refresh(app);
    #[cfg(target_os = "linux")]
    tauri::async_runtime::spawn(linux::watch(app.clone()));
    #[cfg(target_os = "windows")]
    windows::watch(app.clone());
}

pub fn state_from(settings: &store::Settings, system: SystemDnd) -> DndState {
    DndState {
        manual: settings.do_not_disturb,
        follow_system: settings.follow_system_dnd,
        system,
    }
}

pub fn system(app: &AppHandle) -> SystemDnd {
    app.try_state::<SystemState>()
        .map(|s| s.0.lock().unwrap_or_else(|e| e.into_inner()).clone())
        .unwrap_or_default()
}

/// Whether Do Not Disturb is in effect right now. A storage error does not silence anything.
pub fn active(app: &AppHandle) -> bool {
    match app.state::<Engine>().settings() {
        Ok(settings) => state_from(&settings, system(app)).active(),
        Err(e) => {
            tracing::warn!(error = %e, "reading the Do Not Disturb setting failed");
            false
        }
    }
}

/// A reader reports the system state; only a change is passed on.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn set_system(app: &AppHandle, system: SystemDnd) {
    let Some(state) = app.try_state::<SystemState>() else {
        return;
    };
    {
        let mut current = state.0.lock().unwrap_or_else(|e| e.into_inner());
        if *current == system {
            return;
        }
        *current = system;
    }
    refresh(app);
}

/// Changes and saves the switch or the follow setting, then applies the result.
pub async fn set(
    app: &AppHandle,
    change: impl FnOnce(&mut store::Settings),
) -> ApiResult<DndState> {
    let engine = app.state::<Engine>();
    let mut settings = engine.settings()?;
    change(&mut settings);
    let stored = engine.update_settings(settings).await?;
    Ok(publish(app, &stored))
}

fn refresh(app: &AppHandle) {
    match app.state::<Engine>().settings() {
        Ok(settings) => {
            publish(app, &settings);
        }
        Err(e) => tracing::warn!(error = %e, "reading the Do Not Disturb setting failed"),
    }
}

/// Hands the state to the sound hub and the tray and tells the UI.
fn publish(app: &AppHandle, settings: &store::Settings) -> DndState {
    let state = state_from(settings, system(app));
    app.state::<Engine>().set_do_not_disturb(state.active());
    crate::tray::dnd_changed(app, &state);
    crate::plugins::set_quiet(app, state.active());
    if let Err(e) = (DndChanged {
        state: state.clone(),
    })
    .emit(app)
    {
        tracing::warn!(error = %e, "sending the Do Not Disturb state to the UI failed");
    }
    state
}
