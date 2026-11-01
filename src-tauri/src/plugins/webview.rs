//! The `mumcord-web` protocol: serves an official plugin's `web/` files to its sealed frame
//! (FR-6.46 … FR-6.48). Pages never touch the network: the system web view asks this handler,
//! which reads the plugin's installed folder, and only while the plugin has a web view open.
//!
//! Every response carries the frame's own Content-Security-Policy (no fetch, no frames, no
//! forms, files of this plugin only), so nothing depends on the app window's policy.

use std::sync::Arc;

use mumcord_plugins::api::webview::{self, SCHEME};
use mumcord_plugins::manifest::is_valid_id;
use tauri::http::{Method, Request, Response, StatusCode, header};
use tauri::{Manager, Runtime, UriSchemeContext, UriSchemeResponder};

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
    let Some(found) = plugins.and_then(|p| p.webviews().file(plugin, file)) else {
        return not_found(Some(plugin));
    };
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
        webview::with_bootstrap(&body)
    } else {
        body
    };
    response(StatusCode::OK, Some(plugin), Some(mime), body)
}

fn not_found(plugin: Option<&str>) -> Response<Vec<u8>> {
    response(StatusCode::NOT_FOUND, plugin, None, Vec::new())
}

fn response(
    status: StatusCode,
    plugin: Option<&str>,
    mime: Option<&str>,
    body: Vec<u8>,
) -> Response<Vec<u8>> {
    // Without a valid plugin the policy allows nothing at all.
    let csp = match plugin {
        Some(plugin) => webview::content_security_policy(&origin(), plugin),
        None => "default-src 'none'; sandbox".to_owned(),
    };
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_SECURITY_POLICY, csp)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::REFERRER_POLICY, "no-referrer")
        .header(header::CACHE_CONTROL, "no-store");
    if let Some(mime) = mime {
        builder = builder.header(header::CONTENT_TYPE, mime);
    }
    builder.body(body).unwrap_or_else(|error| {
        tracing::warn!(%error, "a web view response could not be built");
        Response::new(Vec::new())
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

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
        }
    }
}
