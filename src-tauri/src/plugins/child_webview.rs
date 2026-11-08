//! Plugin frames as child web views (FR-9.3, macOS only). S0 found that on macOS a busy page in
//! an iframe of the main window freezes the whole UI, while a separate web view next to it stays
//! smooth. So on macOS every plugin frame is its own web view, laid over a placeholder the UI
//! draws where the iframe would be; Linux and Windows keep the iframe.
//!
//! The page sees the same world as in an iframe: the same `mumcord-web` responses (the header
//! policy, its `sandbox` gives the page an opaque origin), the same bootstrap and the same bridge
//! messages. Only the way between page and UI differs:
//!
//! - page → UI: [`SHIM`] (run before the bootstrap) replaces `window.parent` with a stand-in
//!   whose `postMessage` sends the message as a POST to the page's own folder. The protocol
//!   handler passes it here ([`receive`]), which checks that it really comes from that frame's
//!   web view and forwards it to the main window as a `plugin://frame` event.
//! - UI → page: [`plugin_frame_post`] runs `__mumcordFrameDeliver(...)` in the frame, which
//!   dispatches the message as a `message` event whose source is the stand-in, exactly where an
//!   iframe page expects its parent's messages.
//!
//! The page code runs after the shim and cannot reach the originals it captured, so it can
//! neither read the bootstrap's gesture key on its way out nor change how messages come in.
//! Tauri still injects its IPC script into every web view; [`command_allowed`] refuses every
//! command from a frame (the app's invoke handler) and the capabilities name only the main web
//! view, so a page gets nothing from it.
//!
//! The UI watches each frame's heartbeat as before; a frozen frame is closed with
//! [`plugin_frame_close`], which needs nothing from the frame's own web process, and Reload opens
//! a new one, while the main window keeps running.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, Runtime};
use tauri_specta::Event;

use crate::commands::types::{ApiError, ApiResult, ErrorCode};

/// The label of every frame web view starts with this; the frame id follows.
const LABEL_PREFIX: &str = "plugin-frame-";
/// The file name (below the plugin's folder) the shim posts page messages to.
pub const SIGNAL_FILE: &str = ".mumcord-frame";
/// A page message on its way to the UI, as JSON (a 64 KiB bridge message escapes to more).
const MAX_SIGNAL_BYTES: usize = 512 * 1024;
/// A message for the page (the UI's bridge already limits it to the place's size).
const MAX_DELIVERY_TEXT_BYTES: usize = 64 * 1024;
/// Page messages per second, all kinds together: 60 bridge messages, the heartbeat and gestures
/// fit with room to spare; the UI's bridge applies the real limits.
const SIGNALS_PER_SECOND: f64 = 120.0;
/// Bounds beyond this are a broken layout, not a place on screen (physical pixels).
const MAX_EXTENT: i32 = 65_536;

/// Whether the web view `label` is a plugin frame.
pub fn is_frame(label: &str) -> bool {
    label.starts_with(LABEL_PREFIX)
}

/// Whether the web view `label` may call app or plugin commands: frames never may.
pub fn command_allowed(label: &str) -> bool {
    !is_frame(label)
}

/// A frame id from the UI: 16 to 32 lowercase hex digits (the UI picks it at random, so it can
/// listen for the frame's events before the frame exists).
fn valid_frame_id(id: &str) -> bool {
    (16..=32).contains(&id.len()) && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn label_of(frame: &str) -> String {
    format!("{LABEL_PREFIX}{frame}")
}

fn frame_of(label: &str) -> Option<&str> {
    label.strip_prefix(LABEL_PREFIX)
}

fn invalid(message: &str) -> ApiError {
    ApiError::new(ErrorCode::InvalidInput, message)
}

/// Where a frame sits in the main window, in physical (device) pixels from the top left corner
/// of its web view: the UI's CSS pixels times its device pixel ratio, so page zoom and display
/// scale are already in.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginFrameBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl PluginFrameBounds {
    /// Non-empty and on a sensible scale; `None` otherwise.
    fn checked(self) -> Option<Self> {
        let Self {
            x,
            y,
            width,
            height,
        } = self;
        let fits = |v: i32| (-MAX_EXTENT..=MAX_EXTENT).contains(&v);
        (fits(x)
            && fits(y)
            && (1..=MAX_EXTENT).contains(&width)
            && (1..=MAX_EXTENT).contains(&height))
        .then_some(self)
    }
}

/// A message for a frame's page, as the UI's bridge sends it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PluginFrameDelivery {
    /// A plugin's bridge message (a `message` event with the text as data).
    Text { text: String },
    /// The bootstrap's gesture key (`{mumcordWebview:"key", key}`).
    Key { key: String },
    /// Mute or unmute the page (`{mumcordWebview:"mute", muted}`).
    Mute { muted: bool },
}

impl PluginFrameDelivery {
    fn checked(self) -> Option<Self> {
        let fits = match &self {
            Self::Text { text } => text.len() <= MAX_DELIVERY_TEXT_BYTES,
            Self::Key { key } => key.len() <= 64,
            Self::Mute { .. } => true,
        };
        fits.then_some(self)
    }
}

/// `plugin://frame`: something from a frame's web view, for the main window only.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type, Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "plugin://frame")]
pub struct PluginFrameEvent {
    pub frame: String,
    pub change: PluginFrameChange,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PluginFrameChange {
    /// The page posted a string (a bridge message for the plugin).
    Message { text: String },
    /// The bootstrap (or the page pretending) posted `{mumcordWebview: kind, key?}`.
    Signal { kind: String, key: Option<String> },
    /// The page finished loading.
    Loaded,
    /// The page tried to leave its own file; the frame must be closed.
    Navigated,
}

/// The shim's POST body: `{"text": …}` or `{"signal": …, "key"?: …}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Posted {
    text: Option<String>,
    signal: Option<String>,
    key: Option<String>,
}

/// Reads one page message; anything else the page could send is `None`.
fn parse_posted(body: &[u8]) -> Option<PluginFrameChange> {
    if body.len() > MAX_SIGNAL_BYTES {
        return None;
    }
    let posted: Posted = serde_json::from_slice(body).ok()?;
    match posted {
        Posted {
            text: Some(text),
            signal: None,
            key: None,
        } => Some(PluginFrameChange::Message { text }),
        Posted {
            text: None,
            signal: Some(kind),
            key,
        } if kind.len() <= 32 && key.as_ref().is_none_or(|key| key.len() <= 64) => {
            Some(PluginFrameChange::Signal { kind, key })
        }
        _ => None,
    }
}

/// A token bucket of [`SIGNALS_PER_SECOND`], starting full, so a page flooding its endpoint
/// cannot flood the main window with events.
#[derive(Debug)]
struct RateGate {
    tokens: f64,
    last: Instant,
}

impl RateGate {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn new(now: Instant) -> Self {
        Self {
            tokens: SIGNALS_PER_SECOND,
            last: now,
        }
    }

    fn allow(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + elapsed * SIGNALS_PER_SECOND).min(SIGNALS_PER_SECOND);
        self.last = now;
        if self.tokens < 1.0 {
            return false;
        }
        self.tokens -= 1.0;
        true
    }
}

#[derive(Debug)]
struct Entry {
    plugin: String,
    gate: RateGate,
}

/// The open frames by id (managed state).
#[derive(Default)]
pub struct Frames(Mutex<HashMap<String, Entry>>);

impl Frames {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    /// Registers a new frame; `false` when the id is taken.
    fn insert(&self, frame: &str, plugin: &str, now: Instant) -> bool {
        let mut frames = self.lock();
        if frames.contains_key(frame) {
            return false;
        }
        frames.insert(
            frame.to_owned(),
            Entry {
                plugin: plugin.to_owned(),
                gate: RateGate::new(now),
            },
        );
        true
    }

    fn remove(&self, frame: &str) {
        self.lock().remove(frame);
    }

    /// Forgets frames whose web view is gone (the main window was rebuilt by the watchdog).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn retain(&self, alive: impl Fn(&str) -> bool) {
        self.lock().retain(|frame, _| alive(frame));
    }

    /// Whether a message from web view `label` to `path` passes: the label is an open frame,
    /// the path is that frame's plugin's signal file and the frame is within its rate.
    fn admit(&self, label: &str, path: &str, now: Instant) -> Option<String> {
        let frame = frame_of(label)?;
        let mut frames = self.lock();
        let entry = frames.get_mut(frame)?;
        let expected = format!("/{}/{SIGNAL_FILE}", entry.plugin);
        (path == expected && entry.gate.allow(now)).then(|| frame.to_owned())
    }
}

/// Handles a POST to the `mumcord-web` protocol: a page message from a frame web view. Returns
/// whether it was one (anything else gets the protocol's usual "not found").
pub fn receive<R: Runtime>(app: &AppHandle<R>, label: &str, path: &str, body: &[u8]) -> bool {
    let Some(frames) = app.try_state::<Frames>() else {
        return false;
    };
    let Some(frame) = frames.admit(label, path, Instant::now()) else {
        return false;
    };
    match parse_posted(body) {
        Some(change) => emit(app, &frame, change),
        // The page's own doing; its text is never logged.
        None => tracing::debug!("a plugin frame posted a message of an unknown shape"),
    }
    true
}

fn emit<R: Runtime>(app: &AppHandle<R>, frame: &str, change: PluginFrameChange) {
    let event = PluginFrameEvent {
        frame: frame.to_owned(),
        change,
    };
    // Only the main window hosts frames; other windows must not see what pages send.
    if let Err(error) = event.emit_to(app, "main") {
        tracing::warn!(%error, "a plugin frame event could not be sent");
    }
}

/// Run in the frame before the bootstrap and the page (see the module comment). `__ENDPOINT__`
/// becomes the JSON string of the frame's signal URL.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const SHIM: &str = r#"(function () {
  "use strict";
  var endpoint = __ENDPOINT__;
  // Taken before the page's code runs, which may replace any global or prototype method.
  var apply = Reflect.apply;
  var create = Object.create;
  var define = Object.defineProperty;
  var stringify = JSON.stringify;
  var fetchFn = window.fetch;
  var then = Promise.prototype.then;
  var dispatch = EventTarget.prototype.dispatchEvent;
  var Message = MessageEvent;
  var host = new MessageChannel().port1;
  function ignore() {}

  // Built without JSON.stringify on objects (it would ask the page's toJSON) and sent with
  // null-prototype options (the page's getters on Object.prototype never see them).
  function wire(data) {
    if (typeof data === "string") return '{"text":' + stringify(data) + "}";
    if (typeof data !== "object" || data === null) return null;
    var kind = data.mumcordWebview;
    if (typeof kind !== "string") return null;
    var key = data.key;
    return '{"signal":' + stringify(kind) +
      (typeof key === "string" ? ',"key":' + stringify(key) : "") + "}";
  }
  define(host, "postMessage", {
    value: function (data) {
      var body = wire(data);
      if (body === null) return;
      var init = create(null);
      init.method = "POST";
      init.body = body;
      // A failed send means the frame is closing; there is nobody to tell.
      apply(then, apply(fetchFn, window, [endpoint, init]), [undefined, ignore]);
    }
  });
  define(window, "parent", { value: host, enumerable: true });
  define(window, "__mumcordFrameDeliver", {
    value: function (message) {
      var data;
      if (message.type === "text") data = message.text;
      else if (message.type === "key") data = { mumcordWebview: "key", key: message.key };
      else if (message.type === "mute") data = { mumcordWebview: "mute", muted: message.muted };
      else return;
      var init = create(null);
      init.data = data;
      init.source = host;
      apply(dispatch, window, [new Message("message", init)]);
    }
  });
})();"#;

/// The shim for a frame whose signal URL is `endpoint`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn shim(endpoint: &str) -> String {
    let literal = serde_json::Value::String(endpoint.to_owned()).to_string();
    SHIM.replace("__ENDPOINT__", &literal)
}

/// The script that hands `delivery` to the page. Serialized JSON is a valid JS literal, so no
/// text from the plugin is ever read as code.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn delivery_script(delivery: &PluginFrameDelivery) -> Option<String> {
    let json = serde_json::to_string(delivery).ok()?;
    Some(format!(
        "window.__mumcordFrameDeliver&&window.__mumcordFrameDeliver({json})"
    ))
}

/// Whether the frame may go to `next` after loading `initial`: only the same file (a jump to an
/// anchor on the page changes the fragment only); anything else is the page navigating away.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn same_page(initial: &tauri::Url, next: &tauri::Url) -> bool {
    let mut a = initial.clone();
    let mut b = next.clone();
    a.set_fragment(None);
    b.set_fragment(None);
    a == b
}

/// The URL of a frame's page, checked to be a file of `plugin` on the `mumcord-web` protocol.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn page_url(plugin: &str, url: &str) -> Option<tauri::Url> {
    let folder = super::webview::url(plugin, "");
    if !mumcord_plugins::manifest::is_valid_id(plugin) || !url.starts_with(&folder) {
        return None;
    }
    let parsed = tauri::Url::parse(url).ok()?;
    parsed.as_str().starts_with(&folder).then_some(parsed)
}

/// Opens plugin `plugin`'s page `url` as frame `frame` at `bounds` in the main window. The UI
/// listens for `plugin://frame` events of `frame` before calling this. macOS only.
#[tauri::command]
#[specta::specta]
pub async fn plugin_frame_open(
    app: AppHandle,
    webview: tauri::Webview,
    frame: String,
    plugin: String,
    url: String,
    bounds: PluginFrameBounds,
) -> ApiResult<()> {
    if webview.label() != "main" {
        return Err(ApiError::new(
            ErrorCode::PermissionDenied,
            "plugin frames live in the main window",
        ));
    }
    if !valid_frame_id(&frame) {
        return Err(invalid("not a frame id"));
    }
    let bounds = bounds
        .checked()
        .ok_or_else(|| invalid("not a place on screen"))?;
    native::open(app, frame, plugin, url, bounds).await
}

/// Moves frame `frame` to `bounds`, or hides it (`None`: its place is off screen, covered or in
/// a hidden tab).
#[tauri::command]
#[specta::specta]
pub fn plugin_frame_bounds(
    app: AppHandle,
    frame: String,
    bounds: Option<PluginFrameBounds>,
) -> ApiResult<()> {
    let bounds = match bounds {
        Some(bounds) => Some(
            bounds
                .checked()
                .ok_or_else(|| invalid("not a place on screen"))?,
        ),
        None => None,
    };
    native::bounds(&app, &frame, bounds)
}

/// Hands one bridge message, the gesture key or the mute state to frame `frame`'s page.
#[tauri::command]
#[specta::specta]
pub fn plugin_frame_post(
    app: AppHandle,
    frame: String,
    delivery: PluginFrameDelivery,
) -> ApiResult<()> {
    let delivery = delivery
        .checked()
        .ok_or_else(|| ApiError::new(ErrorCode::MessageTooLarge, "the message is too large"))?;
    native::post(&app, &frame, &delivery)
}

/// Moves keyboard focus into frame `frame`, or back to the main window (`None`; Escape or F6
/// inside the page, FR-9.41).
#[tauri::command]
#[specta::specta]
pub fn plugin_frame_focus(app: AppHandle, frame: Option<String>) -> ApiResult<()> {
    native::focus(&app, frame.as_deref())
}

/// Closes frame `frame` (unmounted, stopped responding or navigated away). Closing needs nothing
/// from the frame's own web process, so it also ends a frozen page.
#[tauri::command]
#[specta::specta]
pub fn plugin_frame_close(app: AppHandle, frame: String) -> ApiResult<()> {
    if let Some(frames) = app.try_state::<Frames>() {
        frames.remove(&frame);
    }
    native::close(&app, &frame)
}

#[cfg(target_os = "macos")]
mod native {
    use std::time::Instant;

    use tauri::webview::{NewWindowResponse, PageLoadEvent};
    use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewBuilder, WebviewUrl};

    use super::{
        Frames, PluginFrameBounds, PluginFrameChange, PluginFrameDelivery, SIGNAL_FILE, emit,
        invalid, label_of, page_url, same_page, shim,
    };
    use crate::commands::types::{ApiError, ApiResult, ErrorCode};

    fn internal(error: impl std::fmt::Display) -> ApiError {
        ApiError::new(ErrorCode::Internal, error.to_string())
    }

    fn webview(app: &AppHandle, frame: &str) -> ApiResult<tauri::Webview> {
        app.get_webview(&label_of(frame))
            .ok_or_else(|| invalid("no such frame"))
    }

    pub(super) async fn open(
        app: AppHandle,
        frame: String,
        plugin: String,
        url: String,
        bounds: PluginFrameBounds,
    ) -> ApiResult<()> {
        let initial =
            page_url(&plugin, &url).ok_or_else(|| invalid("not a page of this plugin"))?;
        let frames = app.state::<Frames>();
        frames.retain(|frame| app.get_webview(&label_of(frame)).is_some());
        if !frames.insert(&frame, &plugin, Instant::now()) {
            return Err(invalid("the frame id is taken"));
        }
        let endpoint = super::super::webview::url(&plugin, SIGNAL_FILE);
        let (nav_app, nav_frame, load_app, load_frame) =
            (app.clone(), frame.clone(), app.clone(), frame.clone());
        let page = initial.clone();
        let builder = WebviewBuilder::new(label_of(&frame), WebviewUrl::CustomProtocol(initial))
            .initialization_script(shim(&endpoint))
            // Nothing the page stores outlives it (its opaque origin already has no storage).
            .incognito(true)
            .on_navigation(move |next| {
                if same_page(&page, next) {
                    return true;
                }
                emit(&nav_app, &nav_frame, PluginFrameChange::Navigated);
                false
            })
            .on_new_window(|_, _| NewWindowResponse::Deny)
            .on_download(|_, _| {
                tracing::debug!("refused a download in a plugin frame");
                false
            })
            .on_page_load(move |_, payload| {
                if payload.event() == PageLoadEvent::Finished {
                    emit(&load_app, &load_frame, PluginFrameChange::Loaded);
                }
            });
        let window = app
            .get_window("main")
            .ok_or_else(|| internal("the main window is gone"))?;
        let (dx, dy) = main_offset(&app);
        // `add_child` waits for the main thread, so it must not run on an async worker.
        let built = tauri::async_runtime::spawn_blocking(move || {
            window.add_child(
                builder,
                PhysicalPosition::new(bounds.x + dx, bounds.y + dy),
                PhysicalSize::new(bounds.width.unsigned_abs(), bounds.height.unsigned_abs()),
            )
        })
        .await
        .map_err(internal)
        .and_then(|built| built.map_err(internal));
        match built {
            Ok(webview) => {
                super::super::webview::harden_child(&webview);
                Ok(())
            }
            Err(error) => {
                frames.remove(&frame);
                tracing::error!(%error, "a plugin frame could not be opened");
                Err(error)
            }
        }
    }

    /// Where the main web view sits in its window, in physical pixels. The UI measures a frame's
    /// place from the main web view's top left corner, but a child web view is placed in window
    /// coordinates; when the main web view does not start at the window's corner (macOS reported
    /// a shift by about the title bar's height), the frame would land that far off.
    fn main_offset(app: &AppHandle) -> (i32, i32) {
        app.get_webview("main")
            .and_then(|main| main.position().ok())
            .map_or((0, 0), |p| (p.x, p.y))
    }

    pub(super) fn bounds(
        app: &AppHandle,
        frame: &str,
        bounds: Option<PluginFrameBounds>,
    ) -> ApiResult<()> {
        let webview = webview(app, frame)?;
        match bounds {
            None => webview.hide().map_err(internal),
            Some(bounds) => {
                let (dx, dy) = main_offset(app);
                webview
                    .set_bounds(tauri::Rect {
                        position: PhysicalPosition::new(bounds.x + dx, bounds.y + dy).into(),
                        size: PhysicalSize::new(
                            bounds.width.unsigned_abs(),
                            bounds.height.unsigned_abs(),
                        )
                        .into(),
                    })
                    .map_err(internal)?;
                webview.show().map_err(internal)
            }
        }
    }

    pub(super) fn post(
        app: &AppHandle,
        frame: &str,
        delivery: &PluginFrameDelivery,
    ) -> ApiResult<()> {
        let script =
            super::delivery_script(delivery).ok_or_else(|| internal("unserializable message"))?;
        webview(app, frame)?.eval(script).map_err(internal)
    }

    pub(super) fn focus(app: &AppHandle, frame: Option<&str>) -> ApiResult<()> {
        let target = match frame {
            Some(frame) => webview(app, frame)?,
            None => app
                .get_webview("main")
                .ok_or_else(|| internal("the main window is gone"))?,
        };
        target.set_focus().map_err(internal)
    }

    pub(super) fn close(app: &AppHandle, frame: &str) -> ApiResult<()> {
        // Already gone (closed with the window) is fine.
        match app.get_webview(&label_of(frame)) {
            Some(webview) => webview.close().map_err(internal),
            None => Ok(()),
        }
    }
}

/// Linux and Windows keep plugin frames in iframes; the UI never calls these there.
#[cfg(not(target_os = "macos"))]
mod native {
    use tauri::AppHandle;

    use super::{PluginFrameBounds, PluginFrameDelivery};
    use crate::commands::types::{ApiError, ApiResult, ErrorCode};

    fn unsupported<T>() -> ApiResult<T> {
        Err(ApiError::new(
            ErrorCode::NotImplemented,
            "plugin frames are child web views only on macOS",
        ))
    }

    pub(super) async fn open(
        _: AppHandle,
        _: String,
        _: String,
        _: String,
        _: PluginFrameBounds,
    ) -> ApiResult<()> {
        unsupported()
    }

    pub(super) fn bounds(_: &AppHandle, _: &str, _: Option<PluginFrameBounds>) -> ApiResult<()> {
        unsupported()
    }

    pub(super) fn post(_: &AppHandle, _: &str, _: &PluginFrameDelivery) -> ApiResult<()> {
        unsupported()
    }

    pub(super) fn focus(_: &AppHandle, _: Option<&str>) -> ApiResult<()> {
        unsupported()
    }

    pub(super) fn close(_: &AppHandle, _: &str) -> ApiResult<()> {
        unsupported()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::time::Duration;

    use super::*;

    const FRAME: &str = "0123456789abcdef";

    #[test]
    fn only_frame_web_views_are_refused_commands() {
        assert!(command_allowed("main"));
        assert!(command_allowed("licences"));
        assert!(!command_allowed(&label_of(FRAME)));
        assert_eq!(frame_of(&label_of(FRAME)), Some(FRAME));
    }

    #[test]
    fn frame_ids_are_random_hex_only() {
        assert!(valid_frame_id(FRAME));
        assert!(valid_frame_id(&"a".repeat(32)));
        for bad in [
            "",
            "0123456789abcde",
            "0123456789ABCDEF",
            "0123456789abcdeg",
            "../main",
        ] {
            assert!(!valid_frame_id(bad), "{bad}");
        }
        assert!(!valid_frame_id(&"a".repeat(33)));
    }

    #[test]
    fn bounds_must_be_a_real_place_on_screen() {
        let ok = PluginFrameBounds {
            x: 10,
            y: -20,
            width: 300,
            height: 200,
        };
        assert_eq!(ok.checked(), Some(ok));
        for bad in [
            PluginFrameBounds { width: 0, ..ok },
            PluginFrameBounds { height: -5, ..ok },
            PluginFrameBounds { x: i32::MIN, ..ok },
            PluginFrameBounds {
                width: 100_000,
                ..ok
            },
        ] {
            assert_eq!(bad.checked(), None, "{bad:?}");
        }
    }

    #[test]
    fn page_messages_route_only_from_their_own_frame_and_plugin() {
        let frames = Frames::default();
        let now = Instant::now();
        assert!(frames.insert(FRAME, "snacks", now));
        assert!(!frames.insert(FRAME, "dice", now), "ids are unique");
        let path = format!("/snacks/{SIGNAL_FILE}");
        let label = label_of(FRAME);
        assert_eq!(frames.admit(&label, &path, now), Some(FRAME.to_owned()));
        // Another plugin's folder, another file, the main window or an unknown frame.
        assert_eq!(
            frames.admit(&label, &format!("/dice/{SIGNAL_FILE}"), now),
            None
        );
        assert_eq!(frames.admit(&label, "/snacks/index.html", now), None);
        assert_eq!(frames.admit("main", &path, now), None);
        assert_eq!(
            frames.admit(&label_of("fedcba9876543210"), &path, now),
            None
        );
        frames.remove(FRAME);
        assert_eq!(frames.admit(&label, &path, now), None);
    }

    #[test]
    fn a_flooding_page_is_cut_to_the_rate_and_recovers() {
        let frames = Frames::default();
        let start = Instant::now();
        frames.insert(FRAME, "snacks", start);
        let path = format!("/snacks/{SIGNAL_FILE}");
        let label = label_of(FRAME);
        let passed = (0..1000)
            .filter(|_| frames.admit(&label, &path, start).is_some())
            .count();
        assert_eq!(passed, 120);
        let later = start + Duration::from_millis(500);
        let passed = (0..1000)
            .filter(|_| frames.admit(&label, &path, later).is_some())
            .count();
        assert_eq!(passed, 60);
    }

    #[test]
    fn stale_frames_are_forgotten() {
        let frames = Frames::default();
        let now = Instant::now();
        frames.insert(FRAME, "snacks", now);
        frames.insert("fedcba9876543210", "dice", now);
        frames.retain(|frame| frame == FRAME);
        let path = format!("/dice/{SIGNAL_FILE}");
        assert_eq!(
            frames.admit(&label_of("fedcba9876543210"), &path, now),
            None
        );
        assert!(!frames.insert(FRAME, "snacks", now));
    }

    #[test]
    fn posted_bodies_become_bridge_messages_or_signals() {
        assert_eq!(
            parse_posted(b"{\"text\":\"hi \\u2028\"}"),
            Some(PluginFrameChange::Message {
                text: "hi \u{2028}".into()
            })
        );
        assert_eq!(
            parse_posted(br#"{"signal":"gesture","key":"abc"}"#),
            Some(PluginFrameChange::Signal {
                kind: "gesture".into(),
                key: Some("abc".into())
            })
        );
        assert_eq!(
            parse_posted(br#"{"signal":"heartbeat"}"#),
            Some(PluginFrameChange::Signal {
                kind: "heartbeat".into(),
                key: None
            })
        );
        for bad in [
            &br#"{"text":"a","signal":"escape"}"#[..],
            br#"{"text":1}"#,
            br#"{"other":"x"}"#,
            br#"{}"#,
            b"not json",
        ] {
            assert_eq!(parse_posted(bad), None);
        }
        let long_kind = format!(r#"{{"signal":"{}"}}"#, "k".repeat(33));
        assert_eq!(parse_posted(long_kind.as_bytes()), None);
        let huge = format!(r#"{{"text":"{}"}}"#, "x".repeat(MAX_SIGNAL_BYTES));
        assert_eq!(parse_posted(huge.as_bytes()), None);
    }

    #[test]
    fn deliveries_are_data_never_code() {
        let script = delivery_script(&PluginFrameDelivery::Text {
            text: "\"});alert(1);({\"".into(),
        })
        .unwrap();
        assert_eq!(
            script,
            r#"window.__mumcordFrameDeliver&&window.__mumcordFrameDeliver({"type":"text","text":"\"});alert(1);({\""})"#
        );
        let key = delivery_script(&PluginFrameDelivery::Key { key: "ab".into() }).unwrap();
        assert!(key.ends_with(r#"({"type":"key","key":"ab"})"#), "{key}");
        let mute = delivery_script(&PluginFrameDelivery::Mute { muted: true }).unwrap();
        assert!(
            mute.ends_with(r#"({"type":"mute","muted":true})"#),
            "{mute}"
        );
    }

    #[test]
    fn oversized_deliveries_are_refused() {
        let big = PluginFrameDelivery::Text {
            text: "x".repeat(MAX_DELIVERY_TEXT_BYTES + 1),
        };
        assert_eq!(big.checked(), None);
        let key = PluginFrameDelivery::Key {
            key: "a".repeat(65),
        };
        assert_eq!(key.checked(), None);
    }

    #[test]
    fn the_shim_gets_its_endpoint_as_a_string_literal() {
        let script = shim("mumcord-web://localhost/snacks/.mumcord-frame");
        assert!(
            script.contains(r#"var endpoint = "mumcord-web://localhost/snacks/.mumcord-frame";"#)
        );
        assert!(!script.contains("__ENDPOINT__"));
    }

    #[test]
    fn pages_may_only_jump_within_their_own_file() {
        let page = tauri::Url::parse("mumcord-web://localhost/snacks/index.html?x=1").unwrap();
        let anchor =
            tauri::Url::parse("mumcord-web://localhost/snacks/index.html?x=1#top").unwrap();
        let other = tauri::Url::parse("mumcord-web://localhost/snacks/other.html").unwrap();
        let away = tauri::Url::parse("https://example.com/").unwrap();
        assert!(same_page(&page, &page));
        assert!(same_page(&page, &anchor));
        assert!(!same_page(&page, &other));
        assert!(!same_page(&page, &away));
    }

    #[test]
    fn frames_load_only_their_own_plugins_files() {
        let own = super::super::webview::url("snacks", "index.html");
        assert!(page_url("snacks", &own).is_some());
        assert!(page_url("dice", &own).is_none());
        assert!(page_url("snacks", "https://example.com/snacks/index.html").is_none());
        assert!(page_url("../x", &own).is_none());
        let escape = super::super::webview::url("snacks", "../dice/index.html");
        assert!(page_url("snacks", &escape).is_none());
    }
}
