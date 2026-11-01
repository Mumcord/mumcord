//! The watch party and screen share pop-outs (issue #164): either view can move into a window of
//! its own. Which kind is popped out, for which party or share, and whether it stays on top live
//! here, in one place, so the main window and the pop-out never disagree; every change goes to
//! all windows as `mediaPopoutChanged`. Closing a pop-out by any means docks the view again.
//! Always on top is remembered per kind on this device.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, Runtime, State, Window, WindowEvent};
use tauri_specta::Event;

use super::types::{ApiError, ApiResult, ErrorCode, ServerId, SessionId};
use crate::aux_window::{SHARE_POPOUT, Spec, WATCH_POPOUT};

/// File in the data folder that keeps always on top per kind.
const ON_TOP_FILE: &str = "media-popout.json";

/// The view a pop-out shows.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub enum MediaPopoutKind {
    /// The watch party player.
    Watch,
    /// A watched screen share.
    Share,
}

impl MediaPopoutKind {
    fn spec(self) -> &'static Spec {
        match self {
            Self::Watch => &WATCH_POPOUT,
            Self::Share => &SHARE_POPOUT,
        }
    }

    fn of_label(label: &str) -> Option<Self> {
        [Self::Watch, Self::Share]
            .into_iter()
            .find(|kind| kind.spec().label == label)
    }
}

/// What a pop-out shows: the party of `serverId`, or the share of `session` on `serverId`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct MediaPopoutTarget {
    pub server_id: ServerId,
    /// The sharer; `null` for the watch party.
    pub session: Option<SessionId>,
}

/// One kind of pop-out.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct MediaPopoutSlot {
    /// What the open pop-out shows; `null` while the view is docked in the main window.
    pub target: Option<MediaPopoutTarget>,
    /// Whether the pop-out stays above other windows; remembered while docked.
    pub on_top: bool,
}

/// Both pop-outs.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct MediaPopoutState {
    pub watch: MediaPopoutSlot,
    pub share: MediaPopoutSlot,
}

impl MediaPopoutState {
    fn slot(&mut self, kind: MediaPopoutKind) -> &mut MediaPopoutSlot {
        match kind {
            MediaPopoutKind::Watch => &mut self.watch,
            MediaPopoutKind::Share => &mut self.share,
        }
    }

    /// The pop-out of `kind` shows `target`; `on_top` replaces the remembered flag when given.
    fn opened(
        &mut self,
        kind: MediaPopoutKind,
        target: MediaPopoutTarget,
        on_top: Option<bool>,
    ) -> bool {
        let before = *self;
        let slot = self.slot(kind);
        slot.target = Some(target);
        if let Some(on_top) = on_top {
            slot.on_top = on_top;
        }
        *self != before
    }

    /// The pop-out of `kind` is gone; its view is docked again.
    fn closed(&mut self, kind: MediaPopoutKind) -> bool {
        self.slot(kind).target.take().is_some()
    }

    fn set_on_top(&mut self, kind: MediaPopoutKind, on: bool) -> bool {
        let slot = self.slot(kind);
        let changed = slot.on_top != on;
        slot.on_top = on;
        changed
    }

    fn on_top(&self) -> OnTop {
        OnTop {
            watch: self.watch.on_top,
            share: self.share.on_top,
        }
    }
}

/// Always on top per kind, as stored on this device.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
struct OnTop {
    watch: bool,
    share: bool,
}

/// The pop-out state, managed by the app.
#[derive(Default)]
pub struct MediaPopouts(Mutex<MediaPopoutState>);

impl MediaPopouts {
    /// Starts docked, with always on top as remembered.
    pub fn load<R: Runtime>(app: &AppHandle<R>) -> Self {
        let on_top = on_top_path(app).and_then(|path| read_on_top(&path));
        let mut state = MediaPopoutState::default();
        if let Some(on_top) = on_top {
            state.watch.on_top = on_top.watch;
            state.share.on_top = on_top.share;
        }
        Self(Mutex::new(state))
    }

    fn lock(&self) -> MutexGuard<'_, MediaPopoutState> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Which views are popped out changed; sent to every window.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct MediaPopoutChanged {
    pub state: MediaPopoutState,
}

/// Pops the view of `kind` out into its own window showing `target`, or brings the open one to
/// the front (and switches it to `target`). `title` is the localised window title; `onTop`
/// replaces the remembered always-on-top flag when given (picture-in-picture opens with `true`).
#[tauri::command]
#[specta::specta]
pub async fn media_popout_open(
    app: AppHandle,
    popouts: State<'_, MediaPopouts>,
    kind: MediaPopoutKind,
    target: MediaPopoutTarget,
    title: String,
    on_top: Option<bool>,
) -> ApiResult<()> {
    // Updated before the window exists, so the new page reads its target at once.
    let (changed, on_top_changed, on_top_now) = {
        let mut state = popouts.lock();
        let before = state.on_top();
        let changed = state.opened(kind, target, on_top);
        (changed, before != state.on_top(), state.slot(kind).on_top)
    };
    // Async: creating a window from a synchronous command deadlocks on Windows.
    let opened = crate::aux_window::open_with(&app, kind.spec(), Some(&title), on_top_now);
    if let Err(e) = opened {
        tracing::warn!(error = %e, window = kind.spec().label, "opening a pop-out failed");
        if popouts.lock().closed(kind) {
            emit_state(&app, &popouts);
        }
        return Err(ApiError::new(
            ErrorCode::Io,
            format!("opening the pop-out window failed: {e}"),
        ));
    }
    if on_top_changed {
        save_on_top(&app, &popouts);
    }
    if changed {
        emit_state(&app, &popouts);
    }
    Ok(())
}

/// Closes the pop-out of `kind` ("Pop back in", Ctrl+W); the view docks once the window is gone
/// and `mediaPopoutChanged` follows. Nothing happens when it is not open.
#[tauri::command]
#[specta::specta]
pub fn media_popout_close(app: AppHandle, kind: MediaPopoutKind) -> ApiResult<()> {
    crate::aux_window::close(&app, kind.spec());
    Ok(())
}

/// Turns always on top of the pop-out of `kind` on or off, applied at once when it is open and
/// remembered for the next time.
#[tauri::command]
#[specta::specta]
pub fn media_popout_set_on_top(
    app: AppHandle,
    popouts: State<'_, MediaPopouts>,
    kind: MediaPopoutKind,
    on: bool,
) -> ApiResult<()> {
    if let Some(window) = app.get_webview_window(kind.spec().label) {
        window.set_always_on_top(on).map_err(|e| {
            tracing::warn!(error = %e, "setting always on top failed");
            ApiError::new(ErrorCode::Io, format!("setting always on top failed: {e}"))
        })?;
    }
    if popouts.lock().set_on_top(kind, on) {
        save_on_top(&app, &popouts);
        emit_state(&app, &popouts);
    }
    Ok(())
}

/// Renames the open pop-out of `kind`, e.g. when the video or the channel changes.
#[tauri::command]
#[specta::specta]
pub fn media_popout_set_title(
    app: AppHandle,
    kind: MediaPopoutKind,
    title: String,
) -> ApiResult<()> {
    let Some(window) = app.get_webview_window(kind.spec().label) else {
        return Ok(());
    };
    window.set_title(&title).map_err(|e| {
        tracing::warn!(error = %e, "setting the pop-out title failed");
        ApiError::new(ErrorCode::Io, format!("setting the title failed: {e}"))
    })
}

/// Which views are popped out right now, for a window that opens later.
#[tauri::command]
#[specta::specta]
pub fn media_popout_state(popouts: State<'_, MediaPopouts>) -> ApiResult<MediaPopoutState> {
    Ok(*popouts.lock())
}

/// Window event hook: a pop-out that is gone, however it was closed, docks its view again.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if !matches!(event, WindowEvent::Destroyed) {
        return;
    }
    let Some(kind) = MediaPopoutKind::of_label(window.label()) else {
        return;
    };
    let app = window.app_handle();
    let popouts = app.state::<MediaPopouts>();
    if popouts.lock().closed(kind) {
        emit_state(app, &popouts);
    }
}

/// Closes both pop-outs; they only show what the main window runs.
pub fn close_all(app: &AppHandle) {
    for kind in [MediaPopoutKind::Watch, MediaPopoutKind::Share] {
        crate::aux_window::close(app, kind.spec());
    }
}

fn emit_state(app: &AppHandle, popouts: &MediaPopouts) {
    let state = *popouts.lock();
    if let Err(e) = (MediaPopoutChanged { state }).emit(app) {
        tracing::warn!(error = %e, "announcing the pop-out state failed");
    }
}

fn on_top_path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    match crate::portable::data_dir(app) {
        Ok(dir) => Some(dir.join(ON_TOP_FILE)),
        Err(e) => {
            tracing::warn!(error = %e, "no data folder for the pop-out settings");
            None
        }
    }
}

fn read_on_top(path: &std::path::Path) -> Option<OnTop> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!(error = %e, "reading the pop-out settings failed");
            return None;
        }
    };
    serde_json::from_str(&text)
        .inspect_err(|e| tracing::warn!(error = %e, "the stored pop-out settings are invalid"))
        .ok()
}

fn save_on_top(app: &AppHandle, popouts: &MediaPopouts) {
    let Some(path) = on_top_path(app) else {
        return;
    };
    let on_top = popouts.lock().on_top();
    let result = serde_json::to_string(&on_top)
        .map_err(std::io::Error::other)
        .and_then(|text| std::fs::write(&path, text));
    if let Err(e) = result {
        tracing::warn!(error = %e, "saving the pop-out settings failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARTY: MediaPopoutTarget = MediaPopoutTarget {
        server_id: 1,
        session: None,
    };
    const SHARE: MediaPopoutTarget = MediaPopoutTarget {
        server_id: 1,
        session: Some(7),
    };

    #[test]
    fn opening_pops_out_only_that_kind() {
        let mut state = MediaPopoutState::default();
        assert!(state.opened(MediaPopoutKind::Share, SHARE, None));
        assert_eq!(state.share.target, Some(SHARE));
        assert_eq!(state.watch, MediaPopoutSlot::default());
        assert!(!state.share.on_top);
    }

    #[test]
    fn opening_again_changes_nothing_unless_the_target_or_flag_differs() {
        let mut state = MediaPopoutState::default();
        assert!(state.opened(MediaPopoutKind::Watch, PARTY, None));
        assert!(!state.opened(MediaPopoutKind::Watch, PARTY, None));
        assert!(!state.opened(MediaPopoutKind::Watch, PARTY, Some(false)));
        assert!(state.opened(MediaPopoutKind::Watch, PARTY, Some(true)));
        let other_server = MediaPopoutTarget {
            server_id: 2,
            session: None,
        };
        assert!(state.opened(MediaPopoutKind::Watch, other_server, None));
        assert_eq!(state.watch.target, Some(other_server));
    }

    #[test]
    fn picture_in_picture_turns_on_top_on_and_it_stays_remembered() {
        let mut state = MediaPopoutState::default();
        state.opened(MediaPopoutKind::Watch, PARTY, Some(true));
        assert!(state.watch.on_top);
        assert!(state.closed(MediaPopoutKind::Watch));
        assert_eq!(state.watch.target, None);
        assert!(state.watch.on_top, "on top survives docking");
        state.opened(MediaPopoutKind::Watch, PARTY, None);
        assert!(
            state.watch.on_top,
            "a plain pop-out keeps the remembered flag"
        );
    }

    #[test]
    fn closing_docks_once() {
        let mut state = MediaPopoutState::default();
        state.opened(MediaPopoutKind::Share, SHARE, None);
        state.opened(MediaPopoutKind::Watch, PARTY, None);
        assert!(state.closed(MediaPopoutKind::Share));
        assert!(!state.closed(MediaPopoutKind::Share));
        assert_eq!(state.watch.target, Some(PARTY), "the other kind stays out");
    }

    #[test]
    fn on_top_is_kept_per_kind() {
        let mut state = MediaPopoutState::default();
        assert!(state.set_on_top(MediaPopoutKind::Share, true));
        assert!(!state.set_on_top(MediaPopoutKind::Share, true));
        assert_eq!(
            state.on_top(),
            OnTop {
                watch: false,
                share: true
            }
        );
    }

    #[test]
    fn the_labels_map_to_their_kind() {
        assert_eq!(
            MediaPopoutKind::of_label(WATCH_POPOUT.label),
            Some(MediaPopoutKind::Watch)
        );
        assert_eq!(
            MediaPopoutKind::of_label(SHARE_POPOUT.label),
            Some(MediaPopoutKind::Share)
        );
        assert_eq!(MediaPopoutKind::of_label("main"), None);
    }

    #[test]
    fn the_state_serializes_for_the_ui() {
        let mut state = MediaPopoutState::default();
        state.opened(MediaPopoutKind::Share, SHARE, Some(true));
        let json = serde_json::to_value(state).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "watch": { "target": null, "onTop": false },
                "share": { "target": { "serverId": 1, "session": 7 }, "onTop": true },
            })
        );
    }
}
