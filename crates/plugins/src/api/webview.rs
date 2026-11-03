//! `webview` (FR-6.46 … FR-6.48): an official plugin shows HTML from its own `web/` folder in a
//! sealed frame and talks to it through a message bridge.
//!
//! The rules live here so the client (plugin service, custom protocol, UI glue) and tests share
//! them: which entries and files are valid, the frame's Content-Security-Policy, the bridge
//! limits, and [`Webviews`], the per-plugin state of open web views. A file is only served while
//! its plugin has a web view open, so a stopped or hidden plugin exposes nothing.
//!
//! Since 1.1 (FR-9.32 … FR-9.38) a plugin may have up to three frames at once, one per slot:
//! stage, side panel and dialog (the panel popover uses the dialog slot). The frame policy also
//! lets the page reach the websites the plugin declares in `net.origins`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use base64::Engine as _;
use sha2::{Digest, Sha256};
use wasmtime::component::{HasSelf, Linker};

use crate::origins::{Origin, Scheme};
use crate::runtime::bindings::mumcord::plugin::webview::{Host, Place, add_to_linker};
use crate::runtime::wit::types::Error;
use crate::runtime::{HostApi, PluginState};

/// The permission every `webview` call needs (official plugins only, FR-6.48).
pub const PERMISSION: &str = "ui.webview";
/// The URI scheme the client serves `web/` files under; the page's frame loads nothing else.
pub const SCHEME: &str = "mumcord-web";
/// One bridge message, in UTF-8 bytes (FR-6.47).
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;
/// One bridge message to or from a stage frame (FR-9.37).
pub const MAX_STAGE_MESSAGE_BYTES: usize = 64 * 1024;
/// Web frames one plugin may have loaded at once: one per [`Slot`] (FR-9.38).
pub const MAX_FRAMES: usize = 3;
/// The Permissions-Policy every `web/` response carries (FR-9.34): every powerful feature is
/// off, whatever the page asks for. Same list as the S0 spike verified on all three platforms.
pub const PERMISSIONS_POLICY: &str = "camera=(), microphone=(), geolocation=(), clipboard-read=(), \
    clipboard-write=(), display-capture=(), usb=(), serial=(), hid=(), bluetooth=(), payment=(), \
    fullscreen=(), autoplay=()";
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

/// The Content-Security-Policy every `web/` response carries (FR-9.33), sent as a header so the
/// page cannot weaken it. `origin` is the scheme's origin as the system web view sees it (e.g.
/// `mumcord-web://localhost`); `declared` are the plugin's approved `net.origins`.
///
/// Scripts come only from this plugin's own folder (plus the host's bootstrap, by hash) and may
/// compile WebAssembly; styles from the folder or inline. Images, media and fonts may also come
/// from `blob:`, `data:` and the declared websites, which may serve data (`connect-src`, with
/// `wss:` origins) but never scripts. Nothing may be framed, run as a worker, embedded, submitted
/// or used as a base. The `sandbox` directive repeats the frame's sandbox, so even a page that
/// ends up outside its frame keeps an opaque origin without popups or navigation of others.
pub fn content_security_policy(origin: &str, plugin: &str, declared: &[Origin]) -> String {
    let own = format!("{origin}/{plugin}/");
    let mut sites = String::new();
    let mut connect = String::new();
    for declared in declared {
        let ascii = declared.ascii();
        if declared.scheme != Scheme::Wss {
            sites.push(' ');
            sites.push_str(&ascii);
        }
        connect.push(' ');
        connect.push_str(&ascii);
    }
    let bootstrap = bootstrap_hash();
    format!(
        "default-src 'none'; script-src {own} '{bootstrap}' 'wasm-unsafe-eval'; style-src {own} 'unsafe-inline'; \
         img-src {own} blob: data:{sites}; media-src {own} blob: data:{sites}; font-src {own} blob: data:{sites}; \
         connect-src {own}{connect}; frame-src 'none'; worker-src 'none'; object-src 'none'; \
         form-action 'none'; base-uri 'none'; sandbox allow-scripts"
    )
}

/// The CSP hash source of the inline bootstrap script, the only inline script a page may run.
fn bootstrap_hash() -> String {
    let script = ESCAPE_BOOTSTRAP
        .strip_prefix("<script>")
        .and_then(|rest| rest.strip_suffix("</script>"))
        .unwrap_or(ESCAPE_BOOTSTRAP);
    let digest = Sha256::digest(script.as_bytes());
    format!(
        "sha256-{}",
        base64::engine::general_purpose::STANDARD.encode(digest)
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
    /// Since 1.1: the docked column right of the chat (FR-9.39).
    SidePanel,
    /// Since 1.1: the plugin's activity in the media area (FR-9.45).
    Stage,
}

impl WebviewPlace {
    fn slot(self) -> Slot {
        match self {
            Self::PanelPopover | Self::Dialog => Slot::Dialog,
            Self::SidePanel => Slot::SidePanel,
            Self::Stage => Slot::Stage,
        }
    }

    /// The largest bridge message to or from a frame in this place (FR-9.37).
    pub fn max_message_bytes(self) -> usize {
        match self {
            Self::Stage => MAX_STAGE_MESSAGE_BYTES,
            Self::PanelPopover | Self::Dialog | Self::SidePanel => MAX_MESSAGE_BYTES,
        }
    }
}

impl From<Place> for WebviewPlace {
    fn from(place: Place) -> Self {
        match place {
            Place::PanelPopover => Self::PanelPopover,
            Place::Dialog => Self::Dialog,
        }
    }
}

/// The three frames a plugin may have at once (FR-9.38). The panel popover and the dialog share
/// one slot, so a popover while the dialog shows a page (or the other way round) is a fourth
/// frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Stage = 0,
    SidePanel = 1,
    Dialog = 2,
}

/// A web view a plugin just opened: what the app shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub place: WebviewPlace,
    /// The validated entry below `web/`, with its query string if any.
    pub entry: String,
}

#[derive(Debug, Default)]
struct PluginWeb {
    /// The plugin's installed `web/` folder; `None` unless it may show web views.
    root: Option<PathBuf>,
    /// The plugin's declared `net.origins`.
    origins: Vec<Origin>,
    /// Whether the `net.origins` grant lets its pages reach them.
    origins_granted: bool,
    dialog_open: bool,
    /// The place shown in each [`Slot`], indexed by the slot.
    frames: [Option<WebviewPlace>; MAX_FRAMES],
    /// One bridge rate per direction for all of the plugin's frames, set while any is open.
    rates: Option<Rates>,
}

#[derive(Debug, Clone, Copy)]
struct Rates {
    to_page: Bucket,
    from_page: Bucket,
}

impl PluginWeb {
    fn open_places(&self) -> impl Iterator<Item = WebviewPlace> + '_ {
        self.frames.iter().flatten().copied()
    }

    /// The largest message any open frame takes; `None` without an open frame.
    fn max_message_bytes(&self) -> Option<usize> {
        self.open_places()
            .map(WebviewPlace::max_message_bytes)
            .max()
    }

    /// Closes the frame in `slot` if `close` says so; returns whether one closed.
    fn close(&mut self, slot: Slot, close: impl Fn(WebviewPlace) -> bool) -> bool {
        let frame = &mut self.frames[slot as usize];
        if frame.is_some_and(close) {
            *frame = None;
            if self.frames.iter().all(Option::is_none) {
                self.rates = None;
            }
            return true;
        }
        false
    }

    fn close_all(&mut self) {
        self.frames = [None; MAX_FRAMES];
        self.rates = None;
    }
}

/// The web view state of every plugin. Shared by the plugin service (host calls run on plugin
/// threads), the custom protocol (file requests and frame policy) and the bridge command; every
/// method returns at once.
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
            web.close_all();
        }
        web.root = root;
    }

    /// Sets the websites a plugin declares in `net.origins`, at load. Its pages reach them only
    /// while [`Self::set_origins_granted`] says so.
    pub fn set_origins(&self, plugin: &str, origins: Vec<Origin>) {
        self.lock().entry(plugin.to_owned()).or_default().origins = origins;
    }

    /// Sets whether the plugin holds the `net.origins` grant, whenever its grants change. Takes
    /// effect for the next page load; the Windows frame proxy reads it on every connection.
    pub fn set_origins_granted(&self, plugin: &str, granted: bool) {
        self.lock()
            .entry(plugin.to_owned())
            .or_default()
            .origins_granted = granted;
    }

    /// The approved websites of `plugin`, for its frame policy.
    pub fn origins(&self, plugin: &str) -> Vec<Origin> {
        self.lock()
            .get(plugin)
            .filter(|web| web.origins_granted)
            .map(|web| web.origins.clone())
            .unwrap_or_default()
    }

    /// Whether any plugin's pages may reach `host:port` (`host` in lower-case ASCII).
    pub fn allows_host(&self, host: &str, port: u16) -> bool {
        self.lock()
            .values()
            .filter(|web| web.origins_granted)
            .any(|web| {
                web.origins
                    .iter()
                    .any(|origin| origin.host == host && origin.effective_port() == port)
            })
    }

    /// `webview.show`: opens a frame in `place`, or replaces the page of the frame already there.
    /// `invalid` for an entry that is not an existing `.html` page in `web/`, or for the dialog
    /// place without an open dialog; `quota-exceeded` when the place's slot holds the other
    /// dialog-slot place (a fourth frame, FR-9.38; the spec calls it `limit`).
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
        let frame = &mut web.frames[place.slot() as usize];
        if frame.is_some_and(|open| open != place) {
            return Err(Error::QuotaExceeded);
        }
        *frame = Some(place);
        web.rates.get_or_insert(Rates {
            to_page: Bucket::new(now),
            from_page: Bucket::new(now),
        });
        Ok(Shown {
            place,
            entry: entry.to_owned(),
        })
    }

    /// `webview.hide`: closes the side panel and the dialog-slot frame; returns whether one was
    /// open. A stage frame belongs to its activity and closes with [`Self::stage_closed`].
    pub fn hide(&self, plugin: &str) -> bool {
        let mut plugins = self.lock();
        let Some(web) = plugins.get_mut(plugin) else {
            return false;
        };
        let side = web.close(Slot::SidePanel, |_| true);
        let dialog = web.close(Slot::Dialog, |_| true);
        side || dialog
    }

    /// The plugin's stage activity ended; returns whether that closed its stage frame.
    pub fn stage_closed(&self, plugin: &str) -> bool {
        self.lock()
            .get_mut(plugin)
            .is_some_and(|web| web.close(Slot::Stage, |_| true))
    }

    /// The plugin opened its dialog, so a web view may now go there. The modal dialog takes the
    /// dialog slot, so a panel popover frame closes.
    pub fn dialog_opened(&self, plugin: &str) {
        let mut plugins = self.lock();
        let web = plugins.entry(plugin.to_owned()).or_default();
        web.dialog_open = true;
        web.close(Slot::Dialog, |open| open == WebviewPlace::PanelPopover);
    }

    /// The plugin closed its dialog; returns whether that closed a web view shown in it.
    pub fn dialog_closed(&self, plugin: &str) -> bool {
        let mut plugins = self.lock();
        let Some(web) = plugins.get_mut(plugin) else {
            return false;
        };
        web.dialog_open = false;
        web.close(Slot::Dialog, |open| open == WebviewPlace::Dialog)
    }

    /// `webview.post`: checks a message from the plugin to its pages and returns the open frames
    /// it goes to. Order: an open frame, size, then the rate (only a message that passes
    /// everything uses up the rate). A message above 16 KiB goes to the stage frame only.
    pub fn post(
        &self,
        plugin: &str,
        message: &str,
        now: Instant,
    ) -> Result<Vec<WebviewPlace>, Error> {
        let mut plugins = self.lock();
        let web = plugins.get_mut(plugin).ok_or(Error::Invalid)?;
        let max = web.max_message_bytes().ok_or(Error::Invalid)?;
        if message.len() > max {
            return Err(Error::TooLarge);
        }
        let targets = web
            .open_places()
            .filter(|place| message.len() <= place.max_message_bytes())
            .collect();
        if web
            .rates
            .as_mut()
            .is_some_and(|rates| rates.to_page.take(now))
        {
            Ok(targets)
        } else {
            Err(Error::RateLimited)
        }
    }

    /// Whether a message from a page may reach the plugin: a frame is open, the message fits the
    /// largest open frame and the rate allows it. Anything else is dropped.
    pub fn accept_from_page(&self, plugin: &str, message: &str, now: Instant) -> bool {
        let mut plugins = self.lock();
        let Some(web) = plugins.get_mut(plugin) else {
            return false;
        };
        if web
            .max_message_bytes()
            .is_none_or(|max| message.len() > max)
        {
            return false;
        }
        web.rates
            .as_mut()
            .is_some_and(|rates| rates.from_page.take(now))
    }

    /// The file to serve for `path` below the plugin's `web/` folder, only while the plugin has a
    /// frame open. The resolved file must stay inside the folder (no links out of it).
    pub fn file(&self, plugin: &str, path: &str) -> Option<PathBuf> {
        if !is_web_path(path) {
            return None;
        }
        let root = {
            let plugins = self.lock();
            let web = plugins.get(plugin)?;
            web.open_places().next()?;
            web.root.clone()?
        };
        resolve(&root, path)
    }

    /// Closes the web views of plugins that stopped; their folders stay known for a restart.
    pub fn close_unless(&self, running: impl Fn(&str) -> bool) {
        for (id, web) in self.lock().iter_mut() {
            if !running(id) {
                web.close_all();
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
        let csp = content_security_policy("mumcord-web://localhost", "snacks", &[]);
        assert!(csp.starts_with("default-src 'none';"));
        assert!(csp.contains(&format!(
            "script-src mumcord-web://localhost/snacks/ '{}' 'wasm-unsafe-eval';",
            bootstrap_hash()
        )));
        // WebAssembly may be compiled, but JavaScript eval and other inline scripts stay blocked.
        assert!(!csp.contains("'unsafe-eval'"));
        assert_eq!(csp.matches("'unsafe-inline'").count(), 1);
        assert!(csp.contains("style-src mumcord-web://localhost/snacks/ 'unsafe-inline';"));
        assert!(csp.contains("connect-src mumcord-web://localhost/snacks/;"));
        assert_eq!(csp.matches("connect-src").count(), 1);
        assert!(csp.contains("font-src mumcord-web://localhost/snacks/ blob: data:;"));
        assert!(csp.contains("img-src mumcord-web://localhost/snacks/ blob: data:;"));
        assert!(csp.contains("media-src mumcord-web://localhost/snacks/ blob: data:;"));
        for directive in [
            "frame-src 'none'",
            "worker-src 'none'",
            "object-src 'none'",
            "form-action 'none'",
            "base-uri 'none'",
        ] {
            assert!(csp.contains(directive), "{directive}");
        }
        assert!(csp.ends_with("sandbox allow-scripts"));
        assert!(!csp.contains("allow-same-origin"));
    }

    #[test]
    fn the_policy_lets_pages_reach_the_declared_websites_only_for_data() {
        let declared = [
            "https://api.example",
            "wss://live.example:8443",
            "https://cdn.example:444",
        ]
        .map(|origin| crate::origins::validate(origin, false).unwrap());
        let csp = content_security_policy("mumcord-web://localhost", "snacks", &declared);
        let own = "mumcord-web://localhost/snacks/";
        assert!(csp.contains(&format!(
            "connect-src {own} https://api.example wss://live.example:8443 https://cdn.example:444;"
        )));
        for media in ["img-src", "media-src", "font-src"] {
            assert!(
                csp.contains(&format!(
                    "{media} {own} blob: data: https://api.example https://cdn.example:444;"
                )),
                "{media}"
            );
        }
        let scripts = csp.split(';').find(|d| d.contains("script-src")).unwrap();
        assert!(!scripts.contains("example"), "websites never serve scripts");
    }

    #[test]
    fn a_developer_localhost_origin_is_reachable_from_the_page() {
        let declared = [crate::origins::validate("http://localhost:47180", true).unwrap()];
        let csp = content_security_policy("mumcord-web://localhost", "dev", &declared);
        assert!(csp.contains("connect-src mumcord-web://localhost/dev/ http://localhost:47180;"));
    }

    #[test]
    fn the_bootstrap_hash_matches_the_injected_script() {
        let page = String::from_utf8(with_bootstrap(b"<p>x</p>")).unwrap();
        let script = page
            .strip_prefix("<script>")
            .and_then(|rest| rest.split_once("</script>"))
            .unwrap()
            .0;
        let digest = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(script));
        assert_eq!(bootstrap_hash(), format!("sha256-{digest}"));
    }

    #[test]
    fn the_permissions_policy_denies_every_listed_feature() {
        for feature in [
            "camera",
            "microphone",
            "geolocation",
            "clipboard-read",
            "clipboard-write",
            "display-capture",
            "usb",
            "serial",
            "hid",
            "bluetooth",
            "payment",
            "fullscreen",
            "autoplay",
        ] {
            assert!(
                PERMISSIONS_POLICY.contains(&format!("{feature}=()")),
                "{feature}"
            );
        }
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
        assert_eq!(views.post("p", "x", now), Err(Error::Invalid));
        views
            .show("p", WebviewPlace::PanelPopover, "index.html", now)
            .unwrap();
        let big = "x".repeat(MAX_MESSAGE_BYTES + 1);
        assert_eq!(views.post("p", &big, now), Err(Error::TooLarge));
        assert!(!views.accept_from_page("p", &big, now));
        for _ in 0..MAX_MESSAGES_PER_SECOND {
            assert_eq!(
                views.post("p", "x", now),
                Ok(vec![WebviewPlace::PanelPopover])
            );
            assert!(views.accept_from_page("p", "x", now));
        }
        assert_eq!(views.post("p", "x", now), Err(Error::RateLimited));
        assert!(!views.accept_from_page("p", "x", now));
        let later = now + Duration::from_millis(100);
        assert!(views.post("p", "x", later).is_ok());
        assert!(views.accept_from_page("p", "x", later));
    }

    #[test]
    fn the_stage_takes_bigger_messages_and_the_rate_is_shared() {
        let (_dir, root) = web_dir();
        let views = Webviews::default();
        let now = Instant::now();
        views.set_root("p", Some(root));
        views
            .show("p", WebviewPlace::SidePanel, "index.html", now)
            .unwrap();
        views
            .show("p", WebviewPlace::Stage, "index.html", now)
            .unwrap();
        let stage_sized = "x".repeat(MAX_STAGE_MESSAGE_BYTES);
        assert_eq!(
            views.post("p", &stage_sized, now),
            Ok(vec![WebviewPlace::Stage])
        );
        assert!(views.accept_from_page("p", &stage_sized, now));
        let too_big = "x".repeat(MAX_STAGE_MESSAGE_BYTES + 1);
        assert_eq!(views.post("p", &too_big, now), Err(Error::TooLarge));
        assert!(!views.accept_from_page("p", &too_big, now));
        assert_eq!(
            views.post("p", "x", now),
            Ok(vec![WebviewPlace::Stage, WebviewPlace::SidePanel])
        );
        // Two frames still share one rate per direction (the two posts above count).
        for _ in 2..MAX_MESSAGES_PER_SECOND {
            assert!(views.post("p", "x", now).is_ok());
        }
        assert_eq!(views.post("p", "x", now), Err(Error::RateLimited));
        assert!(views.stage_closed("p"));
        let big = "x".repeat(MAX_MESSAGE_BYTES + 1);
        assert_eq!(views.post("p", &big, now), Err(Error::TooLarge));
    }

    #[test]
    fn a_plugin_has_at_most_three_frames_one_per_slot() {
        let (_dir, root) = web_dir();
        let views = Webviews::default();
        let now = Instant::now();
        views.set_root("p", Some(root));
        views.dialog_opened("p");
        for place in [
            WebviewPlace::Stage,
            WebviewPlace::SidePanel,
            WebviewPlace::Dialog,
        ] {
            views.show("p", place, "index.html", now).unwrap();
        }
        // The popover shares the dialog's slot: a fourth frame.
        assert_eq!(
            views.show("p", WebviewPlace::PanelPopover, "index.html", now),
            Err(Error::QuotaExceeded)
        );
        // Showing a place again only replaces its page.
        for place in [
            WebviewPlace::Stage,
            WebviewPlace::SidePanel,
            WebviewPlace::Dialog,
        ] {
            views.show("p", place, "index.html?again", now).unwrap();
        }
        assert_eq!(
            views.post("p", "x", now),
            Ok(vec![
                WebviewPlace::Stage,
                WebviewPlace::SidePanel,
                WebviewPlace::Dialog
            ])
        );
        // `hide` leaves the stage to its activity.
        assert!(views.hide("p"));
        assert_eq!(views.post("p", "x", now), Ok(vec![WebviewPlace::Stage]));
        assert!(views.file("p", "index.html").is_some());
        assert!(views.stage_closed("p"));
        assert!(views.file("p", "index.html").is_none());
        assert!(!views.hide("p"));
    }

    #[test]
    fn opening_the_dialog_closes_a_popover_frame() {
        let (_dir, root) = web_dir();
        let views = Webviews::default();
        let now = Instant::now();
        views.set_root("p", Some(root));
        views
            .show("p", WebviewPlace::PanelPopover, "index.html", now)
            .unwrap();
        assert_eq!(
            views.show("p", WebviewPlace::Dialog, "index.html", now),
            Err(Error::Invalid)
        );
        views.dialog_opened("p");
        views
            .show("p", WebviewPlace::Dialog, "index.html", now)
            .unwrap();
        assert_eq!(views.post("p", "x", now), Ok(vec![WebviewPlace::Dialog]));
    }

    #[test]
    fn approved_origins_are_kept_per_plugin() {
        let views = Webviews::default();
        let origin = crate::origins::validate("https://api.example:8443", false).unwrap();
        views.set_origins("p", vec![origin.clone()]);
        assert!(views.origins("p").is_empty(), "not granted yet");
        assert!(!views.allows_host("api.example", 8443));
        views.set_origins_granted("p", true);
        assert_eq!(views.origins("p"), vec![origin]);
        assert!(views.origins("other").is_empty());
        assert!(views.allows_host("api.example", 8443));
        assert!(!views.allows_host("api.example", 443));
        assert!(!views.allows_host("evil.example", 8443));
        // A revoked grant takes effect at once.
        views.set_origins_granted("p", false);
        assert!(!views.allows_host("api.example", 8443));
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
