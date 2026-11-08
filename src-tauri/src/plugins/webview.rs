//! The `mumcord-web` protocol: serves an official plugin's `web/` files to its sealed frame
//! (FR-6.46 … FR-6.48). Pages never touch the network: the system web view asks this handler,
//! which reads the plugin's installed folder, and only while the plugin has a web view open.
//!
//! Every response carries the frame's own Content-Security-Policy and Permissions-Policy
//! (FR-9.33, FR-9.34: files of this plugin and its approved websites only, no frames, no forms,
//! no devices), so nothing depends on the app window's policy.
//!
//! The web view settings below close what those headers cannot (S0 result, FR-9.2); every app
//! window gets them, since plugin frames live in the main window and all windows of the app
//! share one web view process configuration on Windows.

use std::sync::Arc;

use mumcord_plugins::api::webview::{self, PERMISSIONS_POLICY, SCHEME};
use mumcord_plugins::manifest::is_valid_id;
use mumcord_plugins::origins::Origin;
use mumcord_plugins::package::MAX_OFFICIAL_WEB_FILE_BYTES;
use tauri::http::{Method, Request, Response, StatusCode, header};
use tauri::{
    Manager, Runtime, UriSchemeContext, UriSchemeResponder, WebviewWindow, WebviewWindowBuilder,
};

use super::Plugins;

/// The protocol's origin as the system web view sees it: custom schemes are served as
/// `http://<scheme>.localhost` on Windows (Tauri's `useHttpsScheme` is off).
pub fn origin() -> String {
    if cfg!(windows) {
        format!("http://{SCHEME}.localhost")
    } else {
        format!("{SCHEME}://localhost")
    }
}

/// The URL of `entry` (below `web/`, query included) in `plugin`'s web view.
pub fn url(plugin: &str, entry: &str) -> String {
    format!("{}/{plugin}/{entry}", origin())
}

/// The protocol handler; register with `register_asynchronous_uri_scheme_protocol(SCHEME, …)`.
/// File reads run on a blocking thread so the web view is never held up.
pub fn handle<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let plugins = ctx
        .app_handle()
        .try_state::<Arc<Plugins>>()
        .map(|s| Arc::clone(&s));
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    // A page message from a plugin frame web view (macOS, see `child_webview`).
    if method == Method::POST
        && super::child_webview::receive(
            ctx.app_handle(),
            ctx.webview_label(),
            &path,
            request.body(),
        )
    {
        responder.respond(signal_received());
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        responder.respond(respond(plugins.as_deref(), &method, &path));
    });
}

fn respond(plugins: Option<&Plugins>, method: &Method, path: &str) -> Response<Vec<u8>> {
    let Some((plugin, file)) = path.strip_prefix('/').and_then(|p| p.split_once('/')) else {
        return not_found(None);
    };
    if !is_valid_id(plugin) {
        return not_found(None);
    }
    if method != Method::GET {
        return not_found(Some(plugin));
    }
    let Some(plugins) = plugins else {
        return not_found(Some(plugin));
    };
    let Some(found) = plugins.webviews().file(plugin, file) else {
        return not_found(Some(plugin));
    };
    // Tauri answers with a whole body, so the size is checked before anything is read: a file
    // above the package limit (a hand-edited install folder) is never loaded into memory.
    let too_big = std::fs::metadata(&found)
        .map(|meta| meta.len() > MAX_OFFICIAL_WEB_FILE_BYTES)
        .unwrap_or(false);
    if too_big {
        tracing::warn!(plugin, "a web view file is larger than the allowed size");
        return not_found(Some(plugin));
    }
    let body = match std::fs::read(&found) {
        Ok(body) => body,
        Err(error) => {
            tracing::warn!(plugin, %error, "a web view file could not be read");
            return not_found(Some(plugin));
        }
    };
    let Some(mime) = webview::mime_type(file) else {
        return not_found(Some(plugin));
    };
    let body = if mime.starts_with("text/html") {
        let theme = plugins.webviews().page_theme(plugin);
        webview::with_bootstrap(&body, theme.as_deref())
    } else {
        body
    };
    let origins = plugins.webviews().origins(plugin);
    response(StatusCode::OK, Some((plugin, &origins)), Some(mime), body)
}

/// The answer to a frame's page message: empty, readable by the frame's opaque origin.
fn signal_received() -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'; sandbox",
        )
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Vec::new())
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "a web view response could not be built");
            Response::new(Vec::new())
        })
}

fn not_found(plugin: Option<&str>) -> Response<Vec<u8>> {
    response(
        StatusCode::NOT_FOUND,
        plugin.map(|plugin| (plugin, &[][..])),
        None,
        Vec::new(),
    )
}

/// A response for `plugin` and its approved websites (`None`: no valid plugin).
fn response(
    status: StatusCode,
    plugin: Option<(&str, &[Origin])>,
    mime: Option<&str>,
    body: Vec<u8>,
) -> Response<Vec<u8>> {
    // Without a valid plugin the policy allows nothing at all.
    let csp = match plugin {
        Some((plugin, origins)) => webview::content_security_policy(&origin(), plugin, origins),
        None => "default-src 'none'; sandbox".to_owned(),
    };
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_SECURITY_POLICY, csp)
        .header("Permissions-Policy", PERMISSIONS_POLICY)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::REFERRER_POLICY, "no-referrer")
        .header(header::CACHE_CONTROL, "no-store");
    if let Some(mime) = mime {
        builder = builder.header(header::CONTENT_TYPE, mime);
    }
    if plugin.is_some() && status.is_success() {
        // The frame's origin is opaque ("null"), so its CORS requests to its own files (CSS
        // masks, fetch, fonts) need this. The files are the package's public content, and each
        // frame's CSP still limits it to its own plugin's folder.
        builder = builder.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    }
    builder.body(body).unwrap_or_else(|error| {
        tracing::warn!(%error, "a web view response could not be built");
        Response::new(Vec::new())
    })
}

/// Settings every app window is built with: downloads are refused (FR-9.32; new windows are
/// already refused by `browser::deny_new_windows`) and, on Windows, all traffic goes through
/// the frame proxy. WebView2 refuses a second window whose browser arguments differ from the
/// first, so every window must come through here.
pub fn harden_builder<'a, R: Runtime, M: Manager<R>>(
    app: &M,
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    let builder = builder.on_download(|_, _| {
        tracing::debug!("refused a download in a web view");
        false
    });
    #[cfg(windows)]
    let builder = builder.additional_browser_args(windows_browser_args(app.app_handle()));
    #[cfg(not(windows))]
    let _ = app;
    builder
}

/// Tauri's own WebView2 defaults: wry drops them once any arguments are given.
#[cfg(any(windows, test))]
const WEBVIEW2_DEFAULT_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// Stops WebRTC from using UDP outside the frame proxy, which would leak the user's addresses
/// (the proxy cannot carry UDP). The force and Blink-feature switches do not work in WebView2.
#[cfg(any(windows, test))]
const WEBRTC_POLICY_ARG: &str = "--webrtc-ip-handling-policy=disable_non_proxied_udp";

/// The WebView2 argument string: defaults, the WebRTC policy and, when it started, the proxy.
#[cfg(any(windows, test))]
fn compose_browser_args(proxy: Option<&str>) -> String {
    match proxy {
        Some(proxy) => format!("{WEBVIEW2_DEFAULT_ARGS} {WEBRTC_POLICY_ARG} {proxy}"),
        None => format!("{WEBVIEW2_DEFAULT_ARGS} {WEBRTC_POLICY_ARG}"),
    }
}

/// The WebView2 arguments of every window (the frame proxy is started with the first window).
#[cfg(windows)]
fn windows_browser_args<R: Runtime>(app: &tauri::AppHandle<R>) -> &'static str {
    static ARGS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ARGS.get_or_init(|| compose_browser_args(super::frame_proxy::start(app).as_deref()))
}

/// Engine settings applied once a window exists (S0 closing measures, FR-9.2).
pub fn harden_window<R: Runtime>(window: &WebviewWindow<R>) {
    #[cfg(target_os = "linux")]
    let result = window.with_webview(|webview| linux::harden(&webview.inner()));
    #[cfg(target_os = "macos")]
    let result = window.with_webview(|webview| macos::harden(webview.inner()));
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Err(error) = result {
        tracing::error!(%error, "the web view settings could not be applied");
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = window;
}

/// The macOS closing measures for a plugin frame's own web view (`child_webview`).
#[cfg(target_os = "macos")]
pub fn harden_child<R: Runtime>(webview: &tauri::Webview<R>) {
    if let Err(error) = webview.with_webview(|webview| macos::harden(webview.inner())) {
        tracing::error!(%error, "the plugin frame settings could not be applied");
    }
}

/// WebKitGTK: no link preconnect (S0: a `<link rel="preconnect">` and `window.open` opened TCP
/// connections from a sandboxed frame) and no WebRTC.
#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::{CStr, c_char, c_int, c_void};

    use gtk::glib::object::ObjectType;

    /// The runtime features turned off; not all of them exist in every WebKitGTK version.
    const DISABLED: &[&str] = &["LinkPreconnect"];

    // In libwebkit2gtk-4.1, which wry already links. The feature list exists since 2.42.
    unsafe extern "C" {
        fn webkit_web_view_get_settings(view: *mut c_void) -> *mut c_void;
        fn webkit_settings_set_enable_webrtc(settings: *mut c_void, enabled: c_int);
        fn webkit_settings_get_all_features() -> *mut c_void;
        fn webkit_feature_list_get_length(list: *mut c_void) -> usize;
        fn webkit_feature_list_get(list: *mut c_void, index: usize) -> *mut c_void;
        fn webkit_feature_list_unref(list: *mut c_void);
        fn webkit_feature_get_identifier(feature: *mut c_void) -> *const c_char;
        fn webkit_settings_set_feature_enabled(
            settings: *mut c_void,
            feature: *mut c_void,
            enabled: c_int,
        );
    }

    pub(super) fn harden(view: &impl ObjectType) {
        let mut disabled = 0;
        // SAFETY: `view` is a live WebKitWebView owned by the window; the settings object and
        // the feature list are used only within this call (the list is released at the end),
        // and identifiers are static NUL-terminated strings.
        unsafe {
            let settings = webkit_web_view_get_settings(view.as_ptr().cast());
            if settings.is_null() {
                tracing::error!("the web view has no settings to harden");
                return;
            }
            webkit_settings_set_enable_webrtc(settings, 0);
            let list = webkit_settings_get_all_features();
            for index in 0..webkit_feature_list_get_length(list) {
                let feature = webkit_feature_list_get(list, index);
                let id = CStr::from_ptr(webkit_feature_get_identifier(feature));
                if DISABLED.iter().any(|name| id.to_bytes() == name.as_bytes()) {
                    webkit_settings_set_feature_enabled(settings, feature, 0);
                    disabled += 1;
                }
            }
            webkit_feature_list_unref(list);
        }
        if disabled < DISABLED.len() {
            tracing::warn!("this WebKitGTK lacks a feature switch; link preconnect may stay on");
        }
        #[cfg(debug_assertions)]
        if let Some(file) = mumcord_plugins::test_hooks::test_ca() {
            test_ca::allow_fixture_certificates(view, &file);
        }
    }

    /// E2E hook `MUMCORD_TEST_CA`, compiled into debug builds only: the plugin frames must reach
    /// the HTTPS fixture, whose certificates are made per run. WebKitGTK cannot add a CA from the
    /// app (its network process uses the system store), but it can accept one exact server
    /// certificate for a host. So every certificate in the file is accepted for the DNS names it
    /// names, and nothing else; the frames' CSP still decides which origins they may reach.
    #[cfg(debug_assertions)]
    mod test_ca {
        use std::ffi::{CString, c_char, c_uint, c_void};
        use std::path::Path;
        use std::ptr;

        use gtk::glib::object::ObjectType;

        #[repr(C)]
        struct GList {
            data: *mut c_void,
            next: *mut GList,
            prev: *mut GList,
        }

        #[repr(C)]
        struct GPtrArray {
            pdata: *mut *mut c_void,
            len: c_uint,
        }

        #[repr(C)]
        struct GError {
            domain: u32,
            code: i32,
            message: *mut c_char,
        }

        unsafe extern "C" {
            fn webkit_web_view_get_context(view: *mut c_void) -> *mut c_void;
            fn webkit_web_context_allow_tls_certificate_for_host(
                context: *mut c_void,
                certificate: *mut c_void,
                host: *const c_char,
            );
            fn g_tls_certificate_list_new_from_file(
                file: *const c_char,
                error: *mut *mut GError,
            ) -> *mut GList;
            fn g_tls_certificate_get_dns_names(certificate: *mut c_void) -> *mut GPtrArray;
            fn g_bytes_get_data(bytes: *mut c_void, size: *mut usize) -> *const u8;
            fn g_ptr_array_unref(array: *mut GPtrArray);
            fn g_object_unref(object: *mut c_void);
            fn g_list_free(list: *mut GList);
            fn g_error_free(error: *mut GError);
        }

        pub(super) fn allow_fixture_certificates(view: &impl ObjectType, file: &Path) {
            let Some(path) = file.to_str().and_then(|p| CString::new(p).ok()) else {
                tracing::error!("MUMCORD_TEST_CA is not a usable path");
                return;
            };
            let mut allowed = 0;
            // SAFETY: `view` is a live WebKitWebView; the certificate list, each certificate,
            // the name array and the error are owned here and released before returning; the
            // name bytes are read only while the array holds them.
            unsafe {
                let context = webkit_web_view_get_context(view.as_ptr().cast());
                let mut error: *mut GError = ptr::null_mut();
                let list = g_tls_certificate_list_new_from_file(path.as_ptr(), &raw mut error);
                if !error.is_null() {
                    tracing::error!("MUMCORD_TEST_CA is unreadable for the web view");
                    g_error_free(error);
                    return;
                }
                let mut node = list;
                while !node.is_null() {
                    let certificate = (*node).data;
                    let names = g_tls_certificate_get_dns_names(certificate);
                    if !names.is_null() {
                        for index in 0..(*names).len as usize {
                            let mut size = 0;
                            let data = g_bytes_get_data(*(*names).pdata.add(index), &raw mut size);
                            let name = std::slice::from_raw_parts(data, size);
                            if let Ok(host) = CString::new(name) {
                                webkit_web_context_allow_tls_certificate_for_host(
                                    context,
                                    certificate,
                                    host.as_ptr(),
                                );
                                allowed += 1;
                            }
                        }
                        g_ptr_array_unref(names);
                    }
                    g_object_unref(certificate);
                    node = (*node).next;
                }
                g_list_free(list);
            }
            tracing::warn!(
                "MUMCORD_TEST_CA: the web view accepts the fixture certificate for {allowed} names"
            );
        }
    }
}

/// WKWebView: no WebRTC and no link preconnect (S0: both reached the network from a sandboxed
/// frame), switched off through WebKit's runtime features as the S0 spike did.
#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{CStr, c_char, c_void};

    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};

    const DISABLED: &[&str] = &["PeerConnectionEnabled", "LinkPreconnect"];

    pub(super) fn harden(view: *mut c_void) {
        let mut disabled = 0;
        // SAFETY: `view` is the window's live WKWebView; every object is only borrowed for this
        // call on the main thread, where `with_webview` runs.
        unsafe {
            let view = view.cast::<AnyObject>();
            let config: *mut AnyObject = msg_send![view, configuration];
            let prefs: *mut AnyObject = msg_send![config, preferences];
            let features: *mut AnyObject = msg_send![class!(WKPreferences), _features];
            let count: usize = msg_send![features, count];
            for index in 0..count {
                let feature: *mut AnyObject = msg_send![features, objectAtIndex: index];
                let key: *mut AnyObject = msg_send![feature, key];
                let utf8: *const c_char = msg_send![key, UTF8String];
                let id = CStr::from_ptr(utf8);
                if DISABLED.iter().any(|name| id.to_bytes() == name.as_bytes()) {
                    let _: () = msg_send![prefs, _setEnabled: Bool::NO, forFeature: feature];
                    disabled += 1;
                }
            }
        }
        if disabled < DISABLED.len() {
            tracing::error!("this WebKit lacks a feature switch; WebRTC or preconnect may stay on");
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_arguments_close_webrtc_with_and_without_the_proxy() {
        let proxy = "--proxy-server=http://127.0.0.1:1 --proxy-bypass-list=<-loopback>";
        let with = compose_browser_args(Some(proxy));
        assert!(with.contains("--webrtc-ip-handling-policy=disable_non_proxied_udp"));
        assert!(with.contains(proxy));
        assert!(with.starts_with(WEBVIEW2_DEFAULT_ARGS));
        let without = compose_browser_args(None);
        assert!(without.contains("--webrtc-ip-handling-policy=disable_non_proxied_udp"));
        assert!(!without.contains("--proxy-server"));
    }

    #[test]
    fn urls_use_the_platform_origin() {
        let url = url("snacks", "index.html?x=1");
        assert!(url.ends_with("/snacks/index.html?x=1"));
        assert!(url.starts_with(&origin()));
    }

    #[test]
    fn unknown_requests_get_an_empty_locked_down_answer() {
        for path in ["/", "/x", "/Bad/index.html", "/snacks/../index.html"] {
            let answer = respond(None, &Method::GET, path);
            assert_eq!(answer.status(), StatusCode::NOT_FOUND, "{path}");
            assert!(answer.body().is_empty());
            let csp = answer.headers()[header::CONTENT_SECURITY_POLICY]
                .to_str()
                .unwrap();
            assert!(csp.starts_with("default-src 'none'"), "{csp}");
            assert!(csp.contains("sandbox"));
            assert!(
                !answer
                    .headers()
                    .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            );
        }
    }

    #[test]
    fn plugin_files_allow_cors_from_the_opaque_frame() {
        let answer = response(
            StatusCode::OK,
            Some(("snacks", &[])),
            Some("image/webp"),
            Vec::new(),
        );
        assert_eq!(answer.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        let csp = answer.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap();
        // CORS is open, but the frame may still only reach its own plugin's folder.
        assert!(
            csp.contains(&format!("connect-src {}/snacks/;", origin())),
            "{csp}"
        );
        assert_eq!(answer.headers()["Permissions-Policy"], PERMISSIONS_POLICY);
    }

    #[test]
    fn approved_websites_go_into_the_header_policy() {
        let declared = [mumcord_plugins::origins::validate("https://api.example", false).unwrap()];
        let answer = response(
            StatusCode::OK,
            Some(("snacks", &declared)),
            Some("text/html"),
            Vec::new(),
        );
        let csp = answer.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap();
        assert!(
            csp.contains(&format!(
                "connect-src {}/snacks/ https://api.example;",
                origin()
            )),
            "{csp}"
        );
        assert!(csp.contains("worker-src 'none'"), "{csp}");
    }

    #[test]
    fn every_answer_denies_device_and_feature_access() {
        let answer = respond(None, &Method::GET, "/x");
        assert_eq!(answer.headers()["Permissions-Policy"], PERMISSIONS_POLICY);
    }
}
