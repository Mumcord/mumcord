//! Full screen for the pages Mumcord shows (issue #419).
//!
//! A page cannot put itself on the screen by itself: WebKitGTK hands the request to the
//! application through the `enter-fullscreen` signal, and without a handler nothing happens. That
//! is why the full-screen button inside an embedded YouTube or Twitch player did nothing on Linux.
//! Mumcord answers the request with its own window full screen and takes the screen back when the
//! page leaves it (the player's button, Escape, or the app's control under the player).
//!
//! Which frames may ask is decided by the permissions policy the UI puts on every iframe:
//! `allow="...; fullscreen"` only for the player pages, nothing for plugin frames (FR-9.34). A
//! page Mumcord does not show as a player therefore never reaches these signals. On macOS and
//! Windows the engine answers the page's request on its own, so there is nothing for Mumcord to
//! add there.

#[cfg(target_os = "linux")]
pub use linux::allow;

/// What the window has to do for one full-screen request of a page.
#[cfg(target_os = "linux")]
mod linux {
    use std::cell::RefCell;
    use std::rc::Rc;

    use tauri::{Manager as _, WebviewWindow};
    use webkit2gtk::{SettingsExt as _, WebViewExt as _};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Action {
        /// The window goes full screen for the video.
        Enter,
        /// The window gives the screen back.
        Leave,
        /// Nothing changes: the window already owned the screen.
        Keep,
    }

    /// The window side of one window's full-screen state.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct State {
        /// The window went full screen because a page asked for it, so leaving full screen gives
        /// it back. A window the user put on the screen themselves keeps it.
        taken_by_page: bool,
    }

    impl State {
        /// A page asked for full screen; the window was `already_fullscreen` at that moment.
        pub fn enter(&mut self, already_fullscreen: bool) -> Action {
            if self.taken_by_page || already_fullscreen {
                return Action::Keep;
            }
            self.taken_by_page = true;
            Action::Enter
        }

        /// The page left full screen.
        pub fn leave(&mut self) -> Action {
            if !self.taken_by_page {
                return Action::Keep;
            }
            self.taken_by_page = false;
            Action::Leave
        }
    }

    /// Lets this window answer a page's full-screen request. Called for the windows that show
    /// players: the main window and the media pop-outs.
    pub fn allow(window: &WebviewWindow) {
        let app = window.app_handle().clone();
        let label = window.label().to_owned();
        if let Err(error) = window.with_webview(move |platform| {
            let webview = platform.inner();
            // On by default; set it so the handler is the only thing a request depends on.
            if let Some(settings) = webview.settings() {
                settings.set_enable_fullscreen(true);
            }
            let state = Rc::new(RefCell::new(State::default()));

            let app_enter = app.clone();
            let label_enter = label.clone();
            let state_enter = state.clone();
            webview.connect_enter_fullscreen(move |_view| {
                let Some(window) = app_enter.get_webview_window(&label_enter) else {
                    return false;
                };
                let already = window.is_fullscreen().unwrap_or(false);
                let action = state_enter.borrow_mut().enter(already);
                apply(&window, action);
                // WebKit shows the frame on the whole view only when the handler accepts.
                true
            });

            let app_leave = app;
            let state_leave = state;
            webview.connect_leave_fullscreen(move |_view| {
                let Some(window) = app_leave.get_webview_window(&label) else {
                    return false;
                };
                apply(&window, state_leave.borrow_mut().leave());
                true
            });
        }) {
            tracing::warn!(error = %error, window = window.label(), "full-screen requests cannot be handled");
        }
    }

    fn apply(window: &WebviewWindow, action: Action) {
        match action {
            Action::Keep => {}
            Action::Enter | Action::Leave => {
                let fullscreen = matches!(action, Action::Enter);
                if let Err(error) = window.set_fullscreen(fullscreen) {
                    tracing::warn!(error = %error, window = window.label(), "full screen failed");
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{Action, State};

        #[test]
        fn a_page_takes_the_screen_and_gives_it_back() {
            let mut state = State::default();
            assert_eq!(state.enter(false), Action::Enter);
            assert_eq!(state.leave(), Action::Leave);
        }

        #[test]
        fn a_window_that_was_already_full_screen_keeps_it() {
            let mut state = State::default();
            assert_eq!(state.enter(true), Action::Keep);
            // Leaving the video's full screen must not un-fullscreen the user's own window.
            assert_eq!(state.leave(), Action::Keep);
        }

        #[test]
        fn a_second_request_in_the_same_window_changes_nothing() {
            let mut state = State::default();
            state.enter(false);
            assert_eq!(state.enter(false), Action::Keep);
            assert_eq!(state.leave(), Action::Leave);
            assert_eq!(state.leave(), Action::Keep);
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn allow(window: &tauri::WebviewWindow) {
    // WKWebView and WebView2 answer the page's request themselves; nothing to hook up here.
    let _ = window;
}
