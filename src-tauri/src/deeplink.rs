//! `mumble://` link handling (FR-1.11): links from the OS, from a second app launch (forwarded
//! by the single-instance plugin) and the link the app was started with.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, Url};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_specta::Event;

use mumcord_engine::{Engine, ServerId};

use crate::commands::servers::{DeepLinkOpened, LinkOutcome, MumbleLink, parse_link};
use crate::commands::types::{ApiError, to_api_server_id};
use crate::window;

/// URL scheme Mumcord handles (also declared in tauri.conf.json). macOS registers it through
/// Info.plist and Linux through the app's desktop entry (`desktop_entry`), so only Windows
/// uses it at runtime.
#[cfg(target_os = "windows")]
pub const SCHEME: &str = "mumble";

/// Holds links until the UI is ready for them. A link that launches the app arrives before the
/// web UI listens, so until the UI's first `deep_link_take_pending` links are only kept (the
/// newest wins); afterwards they are emitted right away. That way no link is lost or opened
/// twice.
#[derive(Debug, Default)]
struct LinkBuffer {
    ui_ready: bool,
    pending: Option<DeepLinkOpened>,
}

impl LinkBuffer {
    /// Accepts a link; returns it if it should be emitted now.
    fn offer(&mut self, link: DeepLinkOpened) -> Option<DeepLinkOpened> {
        if self.ui_ready {
            Some(link)
        } else {
            self.pending = Some(link);
            None
        }
    }

    /// Hands the kept link to the UI and switches to emitting.
    fn take(&mut self) -> Option<DeepLinkOpened> {
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

/// What a link leads to, decided from the saved server it names and the running connections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinkPlan {
    UnknownServer,
    /// Already connected: only the channel path (if any) is joined.
    Show(ServerId),
    Connect(ServerId),
    LimitReached(ServerId),
}

fn plan(saved: Option<ServerId>, connected: &[ServerId], max: u8) -> LinkPlan {
    let Some(id) = saved else {
        return LinkPlan::UnknownServer;
    };
    if connected.contains(&id) {
        LinkPlan::Show(id)
    } else if connected.len() >= usize::from(max) {
        LinkPlan::LimitReached(id)
    } else {
        LinkPlan::Connect(id)
    }
}

fn open(app: &AppHandle, link: MumbleLink) {
    window::show(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = resolve(&app.state::<Engine>(), &link).await;
        tracing::info!(?outcome, "mumble:// link received");
        deliver(&app, DeepLinkOpened { link, outcome });
    });
}

/// Connects or joins as FR-2.13 says; the connection itself enforces the limit again.
pub(crate) async fn resolve(engine: &Engine, link: &MumbleLink) -> LinkOutcome {
    let (saved, max) = match (engine.servers().await, engine.settings()) {
        (Ok(servers), Ok(settings)) => (
            servers
                .iter()
                .find(|s| s.address.eq_ignore_ascii_case(&link.address) && s.port == link.port)
                .map(|s| s.id),
            settings.max_connected_servers,
        ),
        (Err(e), _) | (_, Err(e)) => {
            tracing::warn!(error = %e, "reading saved servers for a link failed");
            return LinkOutcome::UnknownServer;
        }
    };
    let connected: Vec<ServerId> = engine
        .connections()
        .iter()
        .filter_map(|c| c.server_id())
        .collect();
    let path = (!link.channel_path.is_empty()).then(|| link.channel_path.clone());
    let (id, shown) = match plan(saved, &connected, max) {
        LinkPlan::UnknownServer => return LinkOutcome::UnknownServer,
        LinkPlan::LimitReached(id) => {
            return LinkOutcome::LimitReached {
                server_id: to_api_server_id(id),
            };
        }
        LinkPlan::Show(id) => (id, true),
        LinkPlan::Connect(id) => (id, false),
    };
    let server_id = to_api_server_id(id);
    match engine.connect(id, path) {
        Ok(()) if shown => LinkOutcome::Shown { server_id },
        Ok(()) => LinkOutcome::Connecting { server_id },
        Err(e) => LinkOutcome::Failed {
            server_id,
            error: ApiError::from(e),
        },
    }
}

pub(crate) fn deliver(app: &AppHandle, opened: DeepLinkOpened) {
    let state = app.state::<PendingLinks>();
    let ready = state
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .offer(opened);
    if let Some(opened) = ready
        && let Err(e) = opened.emit(app)
    {
        tracing::warn!(error = %e, "forwarding a mumble:// link to the UI failed");
    }
}

/// Returns the link kept for the UI (if any); from now on links are emitted directly.
pub fn take_pending(app: &AppHandle) -> Option<DeepLinkOpened> {
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

    fn opened(path: &str) -> DeepLinkOpened {
        DeepLinkOpened {
            link: link(path),
            outcome: LinkOutcome::UnknownServer,
        }
    }

    fn link(path: &str) -> MumbleLink {
        parse_link(&format!("mumble://localhost:64738/{path}")).expect("valid link")
    }

    #[test]
    fn links_before_the_ui_is_ready_are_kept_newest_first() {
        let mut buffer = LinkBuffer::default();
        assert_eq!(buffer.offer(opened("Lobby")), None);
        assert_eq!(buffer.offer(opened("Raid")), None);
        assert_eq!(buffer.take(), Some(opened("Raid")));
    }

    #[test]
    fn links_after_the_first_take_are_emitted_not_kept() {
        let mut buffer = LinkBuffer::default();
        assert_eq!(buffer.take(), None);
        assert_eq!(buffer.offer(opened("Raid")), Some(opened("Raid")));
        assert_eq!(buffer.take(), None);
    }

    #[test]
    fn start_link_parses_with_channel_path() {
        let parsed = link("Raid/Team%201");
        assert_eq!(parsed.address, "localhost");
        assert_eq!(parsed.port, 64738);
        assert_eq!(parsed.channel_path, vec!["Raid", "Team 1"]);
    }

    #[test]
    fn link_to_an_unsaved_server_is_unknown() {
        assert_eq!(plan(None, &[], 3), LinkPlan::UnknownServer);
    }

    #[test]
    fn link_with_no_connection_connects() {
        assert_eq!(plan(Some(1), &[], 3), LinkPlan::Connect(1));
    }

    #[test]
    fn link_connects_next_to_an_existing_connection() {
        assert_eq!(plan(Some(2), &[1], 3), LinkPlan::Connect(2));
    }

    #[test]
    fn link_to_a_connected_server_shows_it_even_at_the_limit() {
        assert_eq!(plan(Some(1), &[1, 2], 2), LinkPlan::Show(1));
    }

    #[test]
    fn link_at_the_limit_is_refused() {
        assert_eq!(plan(Some(3), &[1, 2], 2), LinkPlan::LimitReached(3));
    }
}
