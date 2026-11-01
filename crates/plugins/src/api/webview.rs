//! `webview` (FR-6.46 … FR-6.48): an official plugin shows HTML from its own `web/` folder in a
//! sealed frame and talks to it through a message bridge.
//!
//! The rules live here so the client (plugin service, custom protocol, UI glue) and tests share
//! them: which entries and files are valid, the frame's Content-Security-Policy, the bridge
//! limits, and [`Webviews`], the per-plugin state of open web views. A file is only served while
//! its plugin has a web view open, so a stopped or hidden plugin exposes nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use wasmtime::component::{HasSelf, Linker};

use crate::runtime::bindings::mumcord::plugin::webview::{Host, Place, add_to_linker};
use crate::runtime::wit::types::Error;
use crate::runtime::{HostApi, PluginState};

/// The permission every `webview` call needs (official plugins only, FR-6.48).
pub const PERMISSION: &str = "ui.webview";
/// The URI scheme the client serves `web/` files under; the page's frame loads nothing else.
pub const SCHEME: &str = "mumcord-web";
/// One bridge message, in UTF-8 bytes (FR-6.47).
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;
/// Bridge messages per second, in each direction (FR-6.47).
pub const MAX_MESSAGES_PER_SECOND: u32 = 60;
/// An entry path (below `web/`) longer than this is refused.
const MAX_ENTRY_BYTES: usize = 512;
/// The file types a package may carry in `web/` (same list as the package reader).
const MIME_TYPES: &[(&str, &str)] = &[
    (".html", "text/html; charset=utf-8"),
    (".css", "text/css; charset=utf-8"),
    (".js", "text/javascript; charset=utf-8"),
    (".json", "application/json"),
    (".wasm", "application/wasm"),
    (".woff2", "font/woff2"),
    (".png", "image/png"),
    (".webp", "image/webp"),
    (".svg", "image/svg+xml"),
];

/// Moves keyboard focus out of the frame on Escape (FR-6.48): key events inside the frame never
/// reach the app, so the page tells the parent. The message is an object, never a string, so the
/// app tells it apart from the plugin's own bridge messages; a page faking it only loses focus.
const ESCAPE_BOOTSTRAP: &str = "<script>addEventListener(\"keydown\",function(e){if(e.key===\"Escape\"){parent.postMessage({mumcordWebview:\"escape\"},\"*\")}},true)</script>";

pub(crate) struct WebviewApi;

impl HostApi for WebviewApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
        add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
    }
}

impl Host for PluginState {
    fn show(&mut self, place: Place, entry: String) -> Result<(), Error> {
        self.ctx.grants.check(&self.id, PERMISSION)?;
        self.io()?.webview_show(&self.id, place, entry)
    }

    fn hide(&mut self) {
        if self.ctx.grants.allows(&self.id, PERMISSION)
            && let Ok(io) = self.io()
        {
            io.webview_hide(&self.id);
        }
    }

    fn post(&mut self, message: String) -> Result<(), Error> {
        self.ctx.grants.check(&self.id, PERMISSION)?;
        self.io()?.webview_post(&self.id, message)
    }
}

/// Splits a `show` entry into its file path (below `web/`) and an optional query string. The
/// file must be an `.html` page; every path part must be a plain name.
pub fn parse_entry(entry: &str) -> Option<(&str, Option<&str>)> {
    if entry.len() > MAX_ENTRY_BYTES || entry.contains('#') {
        return None;
    }
    let (path, query) = match entry.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (entry, None),
    };
    (is_web_path(path) && path.ends_with(".html")).then_some((path, query))
}

/// Whether `path` (relative to `web/`) is a plain relative path to a file type `web/` may hold.
/// Only ASCII letters, digits, `.`, `-` and `_` are allowed, so no escapes, drive letters or
/// percent-encoding tricks reach the file system.
pub fn is_web_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_ENTRY_BYTES
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        })
        && mime_type(path).is_some()
}

/// The `Content-Type` a `web/` file is served with; `None` for types `web/` may not hold.
pub fn mime_type(path: &str) -> Option<&'static str> {
    let lower = path.to_ascii_lowercase();
    MIME_TYPES
        .iter()
        .find(|(ext, _)| lower.ends_with(ext))
        .map(|(_, mime)| *mime)
}

/// The Content-Security-Policy every `web/` response carries. `origin` is the scheme's origin as
/// the system web view sees it (e.g. `mumcord-web://localhost`). Scripts, styles and images may
/// come inline or from this plugin's own folder only (WebAssembly may be compiled, data files
/// fetched and fonts loaded from there too); nothing else may be fetched, framed, submitted
/// or used as a base. The `sandbox` directive repeats the frame's sandbox, so even a page that
/// ends up outside its frame keeps an opaque origin without popups or navigation of others.
pub fn content_security_policy(origin: &str, plugin: &str) -> String {
    let own = format!("{origin}/{plugin}/");
    format!(
        "default-src 'none'; script-src {own} 'unsafe-inline' 'wasm-unsafe-eval'; style-src {own} 'unsafe-inline'; \
         img-src {own} data: blob:; font-src {own}; connect-src {own}; frame-src 'none'; worker-src 'none'; \
         object-src 'none'; form-action 'none'; base-uri 'none'; sandbox allow-scripts"
    )
}

/// An HTML page with the Escape bootstrap in front, after a leading doctype (a script before it
/// would switch the page into quirks mode).
pub fn with_bootstrap(html: &[u8]) -> Vec<u8> {
    let start = html
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(html.len());
    let has_doctype = html[start..]
        .get(..9)
        .is_some_and(|head| head.eq_ignore_ascii_case(b"<!doctype"));
    let split = if has_doctype {
        html[start..]
            .iter()
            .position(|&b| b == b'>')
            .map_or(start, |end| start + end + 1)
    } else {
        0
    };
    let mut out = Vec::with_capacity(html.len() + ESCAPE_BOOTSTRAP.len());
    out.extend_from_slice(&html[..split]);
    out.extend_from_slice(ESCAPE_BOOTSTRAP.as_bytes());
    out.extend_from_slice(&html[split..]);
    out
}

/// A token bucket of [`MAX_MESSAGES_PER_SECOND`], starting full.
#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    last: Instant,
}

impl Bucket {
    fn new(now: Instant) -> Self {
        Self {
            tokens: f64::from(MAX_MESSAGES_PER_SECOND),
            last: now,
        }
    }

    fn take(&mut self, now: Instant) -> bool {
        let rate = f64::from(MAX_MESSAGES_PER_SECOND);
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + elapsed * rate).min(rate);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Where an open web view is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebviewPlace {
    PanelPopover,
    Dialog,
}

impl From<Place> for WebviewPlace {
    fn from(place: Place) -> Self {
        match place {
            Place::PanelPopover => Self::PanelPopover,
            Place::Dialog => Self::Dialog,
        }
    }
}

/// A web view a plugin just opened: what the app shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub place: WebviewPlace,
    /// The validated entry below `web/`, with its query string if any.
    pub entry: String,
}

#[derive(Debug)]
struct Open {
    place: WebviewPlace,
    to_page: Bucket,
    from_page: Bucket,
}

#[derive(Debug, Default)]
struct PluginWeb {
    /// The plugin's installed `web/` folder; `None` unless it may show web views.
    root: Option<PathBuf>,
    dialog_open: bool,
    open: Option<Open>,
}

/// The web view state of every plugin. Shared by the plugin service (host calls run on plugin
/// threads), the custom protocol (file requests) and the bridge command; every method returns
/// at once.
#[derive(Debug, Default)]
pub struct Webviews {
    plugins: Mutex<HashMap<String, PluginWeb>>,
}

impl Webviews {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, PluginWeb>> {
        self.plugins.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets the folder a plugin's pages come from, at load. `None` (no `ui.webview` grant, not
    /// official, no `web/`) closes any web view and serves nothing.
    pub fn set_root(&self, plugin: &str, root: Option<PathBuf>) {
        let mut plugins = self.lock();
        let web = plugins.entry(plugin.to_owned()).or_default();
        if root.is_none() {
            web.open = None;
        }
        web.root = root;
    }

    /// `webview.show`: opens (or replaces) the plugin's web view. `invalid` for an entry that is
    /// not an existing `.html` page in `web/`, or for the dialog place without an open dialog.
    pub fn show(
        &self,
        plugin: &str,
        place: WebviewPlace,
        entry: &str,
        now: Instant,
    ) -> Result<Shown, Error> {
        let (path, _) = parse_entry(entry).ok_or(Error::Invalid)?;
        let mut plugins = self.lock();
        let web = plugins.get_mut(plugin).ok_or(Error::NotPermitted)?;
        let root = web.root.as_deref().ok_or(Error::NotPermitted)?;
        if resolve(root, path).is_none() || (place == WebviewPlace::Dialog && !web.dialog_open) {
            return Err(Error::Invalid);
        }
        web.open = Some(Open {
            place,
            to_page: Bucket::new(now),
            from_page: Bucket::new(now),
        });
        Ok(Shown {
            place,
            entry: entry.to_owned(),
        })
    }

    /// `webview.hide`; returns whether a web view was open.
    pub fn hide(&self, plugin: &str) -> bool {
        self.lock()
            .get_mut(plugin)
            .and_then(|web| web.open.take())
            .is_some()
    }

    /// The plugin opened its dialog, so a web view may now go there.
    pub fn dialog_opened(&self, plugin: &str) {
        self.lock()
            .entry(plugin.to_owned())
            .or_default()
            .dialog_open = true;
    }

    /// The plugin closed its dialog; returns whether that closed a web view shown in it.
    pub fn dialog_closed(&self, plugin: &str) -> bool {
        let mut plugins = self.lock();
        let Some(web) = plugins.get_mut(plugin) else {
            return false;
        };
        web.dialog_open = false;
        if web
            .open
            .as_ref()
            .is_some_and(|open| open.place == WebviewPlace::Dialog)
        {
            web.open = None;
            return true;
        }
        false
    }

    /// `webview.post`: checks a message from the plugin to its page. Order: size, an open web
    /// view, then the rate (only a message that passes everything uses up the rate).
    pub fn post(&self, plugin: &str, message: &str, now: Instant) -> Result<(), Error> {
        if message.len() > MAX_MESSAGE_BYTES {
            return Err(Error::TooLarge);
        }
        let mut plugins = self.lock();
        let open = plugins
            .get_mut(plugin)
            .and_then(|web| web.open.as_mut())
            .ok_or(Error::Invalid)?;
        if open.to_page.take(now) {
            Ok(())
        } else {
            Err(Error::RateLimited)
        }
    }

    /// Whether a message from the page may reach the plugin: the web view is open, the message
    /// fits and the rate allows it. Anything else is dropped.
    pub fn accept_from_page(&self, plugin: &str, message: &str, now: Instant) -> bool {
        if message.len() > MAX_MESSAGE_BYTES {
            return false;
        }
        self.lock()
            .get_mut(plugin)
            .and_then(|web| web.open.as_mut())
            .is_some_and(|open| open.from_page.take(now))
    }

    /// The file to serve for `path` below the plugin's `web/` folder, only while the plugin has a
    /// web view open. The resolved file must stay inside the folder (no links out of it).
    pub fn file(&self, plugin: &str, path: &str) -> Option<PathBuf> {
        if !is_web_path(path) {
            return None;
        }
        let root = {
            let plugins = self.lock();
            let web = plugins.get(plugin)?;
            web.open.as_ref()?;
            web.root.clone()?
        };
        resolve(&root, path)
    }

    /// Closes the web views of plugins that stopped; their folders stay known for a restart.
    pub fn close_unless(&self, running: impl Fn(&str) -> bool) {
        for (id, web) in self.lock().iter_mut() {
            if !running(id) {
                web.open = None;
                web.dialog_open = false;
            }
        }
    }

    /// Drops everything about an unloaded plugin.
    pub fn forget(&self, plugin: &str) {
        self.lock().remove(plugin);
    }
}

/// `root/path` if it is a regular file whose real location is inside `root`.
fn resolve(root: &Path, path: &str) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let file = root.join(path).canonicalize().ok()?;
    (file.starts_with(&root) && file.is_file()).then_some(file)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn web_dir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("web");
        std::fs::create_dir_all(root.join("game")).unwrap();
        std::fs::write(root.join("index.html"), "<p>hi</p>").unwrap();
        std::fs::write(root.join("game/app.js"), "1").unwrap();
        std::fs::write(dir.path().join("secret.html"), "x").unwrap();
        (dir, root)
    }

    #[test]
    fn entries_are_plain_html_paths_with_an_optional_query() {
        assert_eq!(parse_entry("index.html"), Some(("index.html", None)));
        assert_eq!(
            parse_entry("game/index.html?probe=x"),
            Some(("game/index.html", Some("probe=x")))
        );
        for bad in [
            "",
            "app.js",
            "../index.html",
            "/index.html",
            "a//index.html",
            "./index.html",
            "a\\index.html",
            "C:index.html",
            "%2e%2e/index.html",
            "index.html#x",
            "ünï.html",
        ] {
            assert_eq!(parse_entry(bad), None, "{bad}");
        }
    }

    #[test]
    fn only_web_file_types_are_served() {
        assert_eq!(mime_type("a/b.JS"), Some("text/javascript; charset=utf-8"));
        assert_eq!(mime_type("x.svg"), Some("image/svg+xml"));
        assert_eq!(mime_type("d/sde.json"), Some("application/json"));
        assert_eq!(mime_type("e.wasm"), Some("application/wasm"));
        assert_eq!(mime_type("f.woff2"), Some("font/woff2"));
        assert!(is_web_path("x.wasm"));
        assert!(is_web_path("x.json"));
        assert!(!is_web_path("x.exe"));
        assert!(is_web_path("img/a-b_c.png"));
    }

    #[test]
    fn the_policy_blocks_the_network_and_scopes_files_to_the_plugin() {
        let csp = content_security_policy("mumcord-web://localhost", "snacks");
        assert!(csp.starts_with("default-src 'none';"));
        assert!(csp.contains(
            "script-src mumcord-web://localhost/snacks/ 'unsafe-inline' 'wasm-unsafe-eval';"
        ));
        assert!(csp.contains("connect-src mumcord-web://localhost/snacks/;"));
        assert!(csp.contains("font-src mumcord-web://localhost/snacks/;"));
        assert!(csp.contains("form-action 'none'"));
        assert!(csp.ends_with("sandbox allow-scripts"));
        assert!(!csp.contains("allow-same-origin"));
    }

    #[test]
    fn the_bootstrap_goes_after_the_doctype() {
        let page = with_bootstrap(b"\n<!DOCTYPE html><p>x</p>");
        let text = String::from_utf8(page).unwrap();
        assert!(text.starts_with("\n<!DOCTYPE html><script>"));
        assert!(text.ends_with("</script><p>x</p>"));
        let bare = String::from_utf8(with_bootstrap(b"<p>x</p>")).unwrap();
        assert!(bare.starts_with("<script>"));
    }

    #[test]
    fn files_are_served_only_while_a_web_view_is_open() {
        let (_dir, root) = web_dir();
        let views = Webviews::default();
        let now = Instant::now();
        assert_eq!(
            views.show("p", WebviewPlace::PanelPopover, "index.html", now),
            Err(Error::NotPermitted)
        );
        views.set_root("p", Some(root));
        assert!(views.file("p", "index.html").is_none());
        assert_eq!(
            views.show("p", WebviewPlace::PanelPopover, "missing.html", now),
            Err(Error::Invalid)
        );
        views
            .show("p", WebviewPlace::PanelPopover, "index.html?a=1", now)
            .unwrap();
        assert!(views.file("p", "game/app.js").is_some());
        assert!(views.file("p", "../secret.html").is_none());
        assert!(views.file("other", "index.html").is_none());
        assert!(views.hide("p"));
        assert!(views.file("p", "index.html").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn links_out_of_the_folder_are_not_followed() {
        let (dir, root) = web_dir();
        std::os::unix::fs::symlink(dir.path().join("secret.html"), root.join("out.html")).unwrap();
        let views = Webviews::default();
        views.set_root("p", Some(root));
        assert_eq!(
            views.show("p", WebviewPlace::PanelPopover, "out.html", Instant::now()),
            Err(Error::Invalid)
        );
    }

    #[test]
    fn the_dialog_place_needs_an_open_dialog_and_closes_with_it() {
        let (_dir, root) = web_dir();
        let views = Webviews::default();
        let now = Instant::now();
        views.set_root("p", Some(root));
        assert_eq!(
            views.show("p", WebviewPlace::Dialog, "index.html", now),
            Err(Error::Invalid)
        );
        views.dialog_opened("p");
        views
            .show("p", WebviewPlace::Dialog, "index.html", now)
            .unwrap();
        assert!(views.dialog_closed("p"));
        assert!(views.file("p", "index.html").is_none());
    }

    #[test]
    fn bridge_messages_are_limited_in_size_and_rate() {
        let (_dir, root) = web_dir();
        let views = Webviews::default();
        let now = Instant::now();
        assert_eq!(views.post("p", "x", now), Err(Error::Invalid));
        views.set_root("p", Some(root));
        views
            .show("p", WebviewPlace::PanelPopover, "index.html", now)
            .unwrap();
        let big = "x".repeat(MAX_MESSAGE_BYTES + 1);
        assert_eq!(views.post("p", &big, now), Err(Error::TooLarge));
        assert!(!views.accept_from_page("p", &big, now));
        for _ in 0..MAX_MESSAGES_PER_SECOND {
            assert_eq!(views.post("p", "x", now), Ok(()));
            assert!(views.accept_from_page("p", "x", now));
        }
        assert_eq!(views.post("p", "x", now), Err(Error::RateLimited));
        assert!(!views.accept_from_page("p", "x", now));
        let later = now + Duration::from_millis(100);
        assert_eq!(views.post("p", "x", later), Ok(()));
        assert!(views.accept_from_page("p", "x", later));
    }

    #[test]
    fn stopped_and_ungranted_plugins_lose_their_web_view() {
        let (_dir, root) = web_dir();
        let views = Webviews::default();
        let now = Instant::now();
        views.set_root("p", Some(root.clone()));
        views
            .show("p", WebviewPlace::PanelPopover, "index.html", now)
            .unwrap();
        views.close_unless(|_| false);
        assert!(!views.hide("p"));
        views
            .show("p", WebviewPlace::PanelPopover, "index.html", now)
            .unwrap();
        views.set_root("p", None);
        assert!(views.file("p", "index.html").is_none());
    }
}
