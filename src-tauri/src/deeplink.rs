//! `mumble://` link handling (FR-1.11): links from the OS, from a second app launch (forwarded
//! by the single-instance plugin) and the link the app was started with.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, Url};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_specta::Event;

use crate::commands::servers::{DeepLinkOpened, MumbleLink, parse_link};
use crate::window;

/// URL scheme Mumcord handles (also declared in tauri.conf.json). macOS registers it through
/// Info.plist, so only the other platforms use it at runtime.
#[cfg(not(target_os = "macos"))]
pub const SCHEME: &str = "mumble";

/// Holds links until the UI is ready for them. A link that launches the app arrives before the
/// web UI listens, so until the UI's first `deep_link_take_pending` links are only kept (the
/// newest wins); afterwards they are emitted right away. That way no link is lost or opened
/// twice.
#[derive(Debug, Default)]
struct LinkBuffer {
    ui_ready: bool,
    pending: Option<MumbleLink>,
}

impl LinkBuffer {
    /// Accepts a link; returns it if it should be emitted now.
    fn offer(&mut self, link: MumbleLink) -> Option<MumbleLink> {
        if self.ui_ready {
            Some(link)
        } else {
            self.pending = Some(link);
            None
        }
    }

    /// Hands the kept link to the UI and switches to emitting.
    fn take(&mut self) -> Option<MumbleLink> {
        self.ui_ready = true;
        self.pending.take()
    }
}

#[derive(Default)]
struct PendingLinks(Mutex<LinkBuffer>);

/// Starts listening for links and picks up the one the app was launched with.
pub fn init(app: &AppHandle) {
    app.manage(PendingLinks::default());
    let deep_link = app.deep_link();
    match deep_link.get_current() {
        Ok(Some(urls)) => open_urls(app, &urls),
        Ok(None) => {}
        Err(e) => tracing::warn!(error = %e, "reading the start link failed"),
    }
    let handle = app.clone();
    deep_link.on_open_url(move |event| open_urls(&handle, &event.urls()));
}

fn open_urls(app: &AppHandle, urls: &[Url]) {
    for url in urls {
        match parse_link(url.as_str()) {
            Ok(link) => open(app, link),
            // The link itself may hold a password, so it is not logged.
            Err(e) => tracing::warn!(code = ?e.code, "ignoring a malformed mumble:// link"),
        }
    }
}

fn open(app: &AppHandle, link: MumbleLink) {
    window::show(app);
    let state = app.state::<PendingLinks>();
    let ready = state
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .offer(link);
    tracing::info!(buffered = ready.is_none(), "mumble:// link received");
    if let Some(link) = ready
        && let Err(e) = (DeepLinkOpened { link }).emit(app)
    {
        tracing::warn!(error = %e, "forwarding a mumble:// link to the UI failed");
    }
}

/// Returns the link kept for the UI (if any); from now on links are emitted directly.
pub fn take_pending(app: &AppHandle) -> Option<MumbleLink> {
    let link = app
        .try_state::<PendingLinks>()?
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    tracing::info!(pending = link.is_some(), "UI is ready for mumble:// links");
    link
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(path: &str) -> MumbleLink {
        parse_link(&format!("mumble://localhost:64738/{path}")).expect("valid link")
    }

    #[test]
    fn links_before_the_ui_is_ready_are_kept_newest_first() {
        let mut buffer = LinkBuffer::default();
        assert_eq!(buffer.offer(link("Lobby")), None);
        assert_eq!(buffer.offer(link("Raid")), None);
        assert_eq!(buffer.take(), Some(link("Raid")));
    }

    #[test]
    fn links_after_the_first_take_are_emitted_not_kept() {
        let mut buffer = LinkBuffer::default();
        assert_eq!(buffer.take(), None);
        assert_eq!(buffer.offer(link("Raid")), Some(link("Raid")));
        assert_eq!(buffer.take(), None);
    }

    #[test]
    fn start_link_parses_with_channel_path() {
        let parsed = link("Raid/Team%201");
        assert_eq!(parsed.address, "localhost");
        assert_eq!(parsed.port, 64738);
        assert_eq!(parsed.channel_path, vec!["Raid", "Team 1"]);
    }
}
