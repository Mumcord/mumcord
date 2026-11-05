//! `http` (FR-6.37, FR-9.28): Mumcord makes HTTP requests on the plugin's behalf. Official
//! plugins (`net.http`) reach a host and port the user typed into a `server-address` field of
//! that plugin, or one of their declared origins. Other plugins (`net.origins`) reach only their
//! declared origins, at public addresses, and every such request is counted per day (FR-9.30).
//!
//! The host call only validates and returns an id; the request runs on one shared background
//! thread with a small tokio runtime, and the result goes into the plugin's event queue as
//! `http-response`. Nothing about a request's content (URL, headers, bodies) is ever logged.

use std::collections::{BTreeMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant};

use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Url, redirect};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use wasmtime::component::{HasSelf, Linker};

use mumcord_store::Store;

use crate::api::files::Picked;
use crate::api::secrets::PluginSecrets;
use crate::net_guard::{self, Blocked, Counters, NetHooks};
use crate::origins::Origin;
use crate::runtime::bindings::mumcord::plugin::http::{
    Body, FormValue, Header, Host, HttpError, HttpRequest, HttpResponse, Method, RequestId,
    add_to_linker,
};
use crate::runtime::wit::events::{Event, HttpResult};
use crate::runtime::wit::types::Error;
use crate::runtime::{EventQueue, HostApi, HostContext, PluginState};

/// Largest request body, and the largest sum of text fields in a form.
pub const MAX_BODY_BYTES: usize = 64 * 1024;
/// Largest response body.
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// Whole-request timeout, and the one for requests that upload a file.
pub const TIMEOUT: Duration = Duration::from_secs(10);
pub const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// Requests per second per plugin (a bucket that starts full).
pub const REQUESTS_PER_SECOND: f64 = 5.0;
/// Addresses remembered per plugin; entering more forgets the oldest.
pub const MAX_ADDRESSES: usize = 16;
const MAX_HEADERS: usize = 32;
/// Where the allowed addresses of all plugins are kept, inside the plugins folder.
const ADDRESSES_FILE: &str = "net-hosts.json";
/// Headers Mumcord sets itself; a plugin naming one is refused.
const RESERVED_HEADERS: &[&str] = &["host", "content-length", "transfer-encoding", "connection"];

pub(crate) struct HttpApi;

impl HostApi for HttpApi {
    fn add_to_linker(linker: &mut Linker<PluginState>) -> wasmtime::Result<()> {
        add_to_linker::<PluginState, HasSelf<PluginState>>(linker, |s| s)
    }
}

impl Host for PluginState {
    fn request(&mut self, req: HttpRequest) -> Result<RequestId, Error> {
        self.http_request(req).map_err(|refused| match refused {
            Refused::Api(error) => error,
            // A 1.0 manifest cannot declare origins (FR-9.23), so a 1.0 plugin only gets here
            // without `net.http`, and 1.0 has no `origin-not-allowed`.
            Refused::OriginNotAllowed => Error::NotPermitted,
        })
    }
}

/// Why `http.request` refused a request before it started.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    Api(Error),
    /// The URL is not one of the plugin's declared origins (FR-9.28).
    OriginNotAllowed,
}

impl From<Error> for Refused {
    fn from(error: Error) -> Self {
        Self::Api(error)
    }
}

impl PluginState {
    /// `http.request` for every API minor; each minor maps [`Refused`] to its own error type.
    pub(crate) fn http_request(&mut self, req: HttpRequest) -> Result<RequestId, Refused> {
        let official = self.ctx.grants.allows(&self.id, "net.http");
        if !official {
            self.ctx.grants.check(&self.id, "net.origins")?;
        }
        self.http.request(&self.ctx, req, official)
    }
}

/// Why a typed address cannot be allowed.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum AddressError {
    #[error("This is not a valid server address")]
    Invalid,
}

/// A host and port the user typed. `port: None` means the user gave neither a port nor a
/// scheme, so the default port of the request's scheme (80 or 443) is allowed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Address {
    host: String,
    port: Option<u16>,
}

/// The `http` service shared by all plugins; lives in [`crate::HostContext`].
pub struct Http {
    addresses: Mutex<Addresses>,
    /// The checked `net.origins` of each loaded plugin, set by the service on load.
    origins: Mutex<BTreeMap<String, Arc<[Origin]>>>,
    counters: Arc<Counters>,
    hooks: NetHooks,
    worker: OnceLock<Result<Worker, String>>,
    timeout: Duration,
    upload_timeout: Duration,
}

#[derive(Debug, Default)]
struct Addresses {
    file: Option<PathBuf>,
    by_plugin: BTreeMap<String, VecDeque<Address>>,
}

/// The background thread that runs requests.
struct Worker {
    jobs: mpsc::UnboundedSender<Job>,
    tls: rustls::ClientConfig,
}

type Job = Pin<Box<dyn Future<Output = ()> + Send>>;

impl std::fmt::Debug for Http {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Http").finish_non_exhaustive()
    }
}

impl Default for Http {
    /// Keeps allowed addresses in memory only (tests, `--no-plugins`).
    fn default() -> Self {
        Self::with_addresses(Addresses::default(), None)
    }
}

impl Http {
    /// Loads the allowed addresses from `<plugins_dir>/net-hosts.json`; changes are saved there.
    /// Requests to declared origins are counted in `store` (FR-9.30).
    pub fn new(plugins_dir: &Path, store: Option<Arc<Mutex<Store>>>) -> Self {
        let file = plugins_dir.join(ADDRESSES_FILE);
        let by_plugin = match std::fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                tracing::warn!(%error, "the allowed plugin addresses are unreadable; starting empty");
                BTreeMap::new()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => {
                tracing::warn!(%error, "can't read the allowed plugin addresses; starting empty");
                BTreeMap::new()
            }
        };
        Self::with_addresses(
            Addresses {
                file: Some(file),
                by_plugin,
            },
            store,
        )
    }

    fn with_addresses(addresses: Addresses, store: Option<Arc<Mutex<Store>>>) -> Self {
        Self {
            addresses: Mutex::new(addresses),
            origins: Mutex::new(BTreeMap::new()),
            counters: Arc::new(Counters::new(store)),
            hooks: NetHooks::from_env(),
            worker: OnceLock::new(),
            timeout: TIMEOUT,
            upload_timeout: UPLOAD_TIMEOUT,
        }
    }

    /// Allows `plugin` to reach the address the user confirmed in one of its `server-address`
    /// text inputs ('host', 'host:port' or a URL). The UI glue calls this; plugins never can.
    pub fn allow(&self, plugin: &str, text: &str) -> Result<(), AddressError> {
        let address = parse_address(text)?;
        let mut addresses = self.lock();
        let list = addresses.by_plugin.entry(plugin.to_owned()).or_default();
        list.retain(|a| *a != address);
        if list.len() == MAX_ADDRESSES {
            list.pop_front();
        }
        list.push_back(address);
        addresses.save();
        Ok(())
    }

    /// Forgets every address allowed for `plugin` (reset data, remove).
    pub fn forget(&self, plugin: &str) {
        let mut addresses = self.lock();
        if addresses.by_plugin.remove(plugin).is_some() {
            addresses.save();
        }
    }

    /// Sets the declared origins of `plugin` (already checked by `Manifest::validate_origins`);
    /// the service calls this whenever it loads the plugin.
    pub fn set_origins(&self, plugin: &str, origins: Vec<Origin>) {
        self.origins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(plugin.to_owned(), origins.into());
    }

    /// The declared origin of `plugin` that `url` belongs to.
    fn origin_for(&self, plugin: &str, url: &Url) -> Option<Origin> {
        let origins = self.origins.lock().unwrap_or_else(PoisonError::into_inner);
        net_guard::match_origin(origins.get(plugin)?, url).cloned()
    }

    /// The request state of one plugin instance that delivers results into `queue`.
    pub(crate) fn set(&self, plugin: Arc<str>, queue: Weak<EventQueue>) -> HttpSet {
        HttpSet {
            plugin,
            queue,
            tokens: REQUESTS_PER_SECOND,
            refilled: Instant::now(),
            last_id: 0,
            uploading: Arc::new(AtomicBool::new(false)),
        }
    }

    fn is_allowed(&self, plugin: &str, url: &Url) -> bool {
        let (Some(host), Some(port)) = (url.host_str(), url.port_or_known_default()) else {
            return false;
        };
        let default = if url.scheme() == "https" { 443 } else { 80 };
        self.lock().by_plugin.get(plugin).is_some_and(|list| {
            list.iter()
                .any(|a| a.host == host && a.port.unwrap_or(default) == port)
        })
    }

    fn worker(&self) -> Result<&Worker, String> {
        self.worker
            .get_or_init(|| Worker::start(&self.hooks))
            .as_ref()
            .map_err(Clone::clone)
    }

    fn lock(&self) -> MutexGuard<'_, Addresses> {
        self.addresses
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl Addresses {
    fn save(&self) {
        let Some(file) = &self.file else {
            return;
        };
        if let Err(error) = write_atomically(file, &self.by_plugin) {
            tracing::warn!(%error, "can't save the allowed plugin addresses");
        }
    }
}

fn write_atomically(file: &Path, value: &impl Serialize) -> std::io::Result<()> {
    let dir = file.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer(&mut tmp, value)?;
    tmp.persist(file).map_err(|e| e.error)?;
    Ok(())
}

/// Turns what the user typed into an exact host and port.
fn parse_address(text: &str) -> Result<Address, AddressError> {
    let text = text.trim();
    let parse = |t: &str| Url::parse(t).map_err(|_| AddressError::Invalid);
    let (url, port) = if let Some((scheme, _)) = text.split_once("://") {
        if scheme != "http" && scheme != "https" {
            return Err(AddressError::Invalid);
        }
        let url = parse(text)?;
        let port = url.port_or_known_default();
        (url, port)
    } else {
        // The URL parser drops a port equal to the scheme default, so a port typed without a
        // scheme is read under both schemes.
        let http = parse(&format!("http://{text}"))?;
        let https = parse(&format!("https://{text}"))?;
        let port = http.port().or(https.port());
        (http, port)
    };
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AddressError::Invalid);
    }
    let host = url.host_str().ok_or(AddressError::Invalid)?;
    if host.is_empty() {
        return Err(AddressError::Invalid);
    }
    Ok(Address {
        host: host.to_owned(),
        port,
    })
}

impl Worker {
    fn start(hooks: &NetHooks) -> Result<Self, String> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = rustls_platform_verifier::Verifier::new_with_extra_roots(
            test_roots(hooks)?,
            Arc::clone(&provider),
        )
        .map_err(|e| e.to_string())?;
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let (jobs, mut rx) = mpsc::unbounded_channel::<Job>();
        // The thread ends (and drops running requests) when `Http` and with it `jobs` go away.
        std::thread::Builder::new()
            .name("plugin-http".into())
            .spawn(move || {
                runtime.block_on(async move {
                    while let Some(job) = rx.recv().await {
                        tokio::spawn(job);
                    }
                });
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { jobs, tls })
    }
}

/// The E2E fixture CA (debug builds only, see [`crate::test_hooks`]) on top of the system roots.
fn test_roots(hooks: &NetHooks) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, String> {
    use rustls::pki_types::pem::PemObject;
    let Some(file) = &hooks.test_ca else {
        return Ok(Vec::new());
    };
    rustls::pki_types::CertificateDer::pem_file_iter(file)
        .and_then(|certs| certs.collect::<Result<Vec<_>, _>>())
        .map_err(|e| format!("the test CA is unreadable: {e}"))
}

/// The `http` state of one plugin instance: rate limit and request ids.
pub(crate) struct HttpSet {
    plugin: Arc<str>,
    queue: Weak<EventQueue>,
    tokens: f64,
    refilled: Instant,
    last_id: RequestId,
    /// One file upload at a time per plugin keeps memory bounded (each may hold 50 MiB).
    uploading: Arc<AtomicBool>,
}

/// A request that passed the checks that need no I/O.
struct Prepared {
    method: reqwest::Method,
    url: Url,
    route: Route,
    headers: Vec<(HeaderName, PreparedValue)>,
    body: PreparedBody,
    timeout: Duration,
}

/// Why a request may go to its host.
enum Route {
    /// The user typed this host and port into the plugin's form (official plugins).
    Address,
    /// A declared origin: public addresses only, counted per day.
    Origin(Origin),
}

/// Why `prepare` did not produce a request.
enum NotPrepared {
    /// Reported later as the request's `http-response`.
    Failed(HttpError),
    /// Refused right away with `origin-not-allowed`; `host` is counted as blocked.
    OriginNotAllowed { host: String },
}

impl From<HttpError> for NotPrepared {
    fn from(error: HttpError) -> Self {
        Self::Failed(error)
    }
}

enum PreparedValue {
    Text(HeaderValue),
    Secret(String),
}

enum PreparedBody {
    Empty,
    Bytes(Vec<u8>),
    File(Picked),
    Form(Vec<(String, PreparedField)>),
}

enum PreparedField {
    Text(String),
    File(Picked),
}

impl HttpSet {
    fn request(
        &mut self,
        ctx: &HostContext,
        req: HttpRequest,
        official: bool,
    ) -> Result<RequestId, Refused> {
        if body_bytes(&req.body) > MAX_BODY_BYTES {
            return Err(Error::TooLarge.into());
        }
        self.take_token()?;
        let uploads = has_file(&req.body);
        if uploads && self.uploading.swap(true, Ordering::AcqRel) {
            return Err(Error::RateLimited.into());
        }
        self.last_id = self.last_id.wrapping_add(1);
        let id = self.last_id;
        let upload_guard = uploads.then(|| UploadGuard(Arc::clone(&self.uploading)));

        let timeout = if uploads {
            ctx.http.upload_timeout
        } else {
            ctx.http.timeout
        };
        let prepared = match prepare(ctx, &self.plugin, req, timeout, official) {
            Ok(prepared) => prepared,
            Err(NotPrepared::Failed(error)) => {
                deliver(&self.queue, id, Err(error));
                return Ok(id);
            }
            Err(NotPrepared::OriginNotAllowed { host }) => {
                ctx.http.count(&self.plugin, host, true);
                return Err(Refused::OriginNotAllowed);
            }
        };
        let worker = match ctx.http.worker() {
            Ok(worker) => worker,
            Err(error) => {
                tracing::error!(%error, "the plugin HTTP worker can't start");
                deliver(
                    &self.queue,
                    id,
                    Err(HttpError::Connect(
                        "Mumcord can't make requests right now".into(),
                    )),
                );
                return Ok(id);
            }
        };
        let queue = Weak::clone(&self.queue);
        let plugin = Arc::clone(&self.plugin);
        let net = Net {
            secrets: ctx.secrets.clone(),
            tls: worker.tls.clone(),
            hooks: ctx.http.hooks.clone(),
            counters: Arc::clone(&ctx.http.counters),
        };
        let job = async move {
            let timeout = prepared.timeout;
            let outcome = match tokio::time::timeout(timeout, execute(prepared, &plugin, net)).await
            {
                Ok(outcome) => outcome,
                Err(_) => Err(HttpError::Timeout),
            };
            drop(upload_guard);
            if let Err(error) = &outcome {
                tracing::debug!(plugin = %plugin, error = error_kind(error), "plugin request failed");
            }
            deliver(&queue, id, outcome);
        };
        if worker.jobs.send(Box::pin(job)).is_err() {
            tracing::error!("the plugin HTTP worker has stopped");
            deliver(
                &self.queue,
                id,
                Err(HttpError::Connect(
                    "Mumcord can't make requests right now".into(),
                )),
            );
        }
        Ok(id)
    }

    fn take_token(&mut self) -> Result<(), Error> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.refilled).as_secs_f64();
        self.tokens = (self.tokens + elapsed * REQUESTS_PER_SECOND).min(REQUESTS_PER_SECOND);
        self.refilled = now;
        if self.tokens < 1.0 {
            return Err(Error::RateLimited);
        }
        self.tokens -= 1.0;
        Ok(())
    }
}

/// Clears the "upload running" flag when the request ends, however it ends.
struct UploadGuard(Arc<AtomicBool>);

impl Drop for UploadGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl Http {
    /// Counts a request to a declared origin (or a blocked one) on the worker, so the plugin's
    /// thread never waits for the database.
    fn count(&self, plugin: &str, host: String, blocked: bool) {
        let Ok(worker) = self.worker() else {
            return;
        };
        let counters = Arc::clone(&self.counters);
        let plugin = plugin.to_owned();
        let job = async move { count(counters, plugin, host, blocked) };
        if worker.jobs.send(Box::pin(job)).is_err() {
            tracing::warn!("the plugin HTTP worker has stopped; a request was not counted");
        }
    }
}

/// Must run inside the worker's runtime.
fn count(counters: Arc<Counters>, plugin: String, host: String, blocked: bool) {
    tokio::task::spawn_blocking(move || counters.record(&plugin, &host, blocked));
}

fn deliver(queue: &Weak<EventQueue>, id: RequestId, outcome: Result<HttpResponse, HttpError>) {
    // A plugin that stopped or restarted in the meantime no longer wants the answer.
    if let Some(queue) = queue.upgrade() {
        queue.push(Event::HttpResponse(HttpResult { id, outcome }));
    }
}

/// Request bytes that count against [`MAX_BODY_BYTES`]; picked files are counted separately.
fn body_bytes(body: &Body) -> usize {
    match body {
        Body::Empty | Body::File(_) => 0,
        Body::Bytes(bytes) => bytes.len(),
        Body::Form(fields) => fields
            .iter()
            .map(|f| match &f.value {
                FormValue::Text(text) => f.name.len() + text.len(),
                FormValue::File(_) => f.name.len(),
            })
            .sum(),
    }
}

fn has_file(body: &Body) -> bool {
    match body {
        Body::File(_) => true,
        Body::Form(fields) => fields.iter().any(|f| matches!(f.value, FormValue::File(_))),
        Body::Empty | Body::Bytes(_) => false,
    }
}

fn prepare(
    ctx: &HostContext,
    plugin: &str,
    req: HttpRequest,
    timeout: Duration,
    official: bool,
) -> Result<Prepared, NotPrepared> {
    let url = Url::parse(&req.url).map_err(|_| HttpError::InvalidRequest)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(HttpError::InvalidRequest.into());
    }
    // A typed address wins for official plugins: the user chose it, and it may be on the LAN.
    let route = if official && ctx.http.is_allowed(plugin, &url) {
        Route::Address
    } else if let Some(origin) = ctx.http.origin_for(plugin, &url) {
        Route::Origin(origin)
    } else if official {
        return Err(HttpError::HostNotAllowed.into());
    } else {
        let host = url.host_str().unwrap_or_default().to_owned();
        return Err(NotPrepared::OriginNotAllowed { host });
    };
    if req.headers.len() > MAX_HEADERS {
        return Err(HttpError::InvalidRequest.into());
    }
    let headers = req
        .headers
        .into_iter()
        .map(prepare_header)
        .collect::<Result<Vec<_>, _>>()?;
    let picked = |handle| {
        ctx.files
            .get(plugin, handle)
            .ok_or(HttpError::InvalidRequest)
    };
    let body = match req.body {
        Body::Empty => PreparedBody::Empty,
        Body::Bytes(bytes) => PreparedBody::Bytes(bytes),
        Body::File(handle) => PreparedBody::File(picked(handle)?),
        Body::Form(fields) => {
            let mut files = 0;
            let mut out = Vec::with_capacity(fields.len());
            for field in fields {
                if field.name.is_empty() || field.name.contains(['"', '\r', '\n']) {
                    return Err(HttpError::InvalidRequest.into());
                }
                let value = match field.value {
                    FormValue::Text(text) => PreparedField::Text(text),
                    FormValue::File(handle) => {
                        files += 1;
                        PreparedField::File(picked(handle)?)
                    }
                };
                out.push((field.name, value));
            }
            if files > 1 {
                return Err(HttpError::InvalidRequest.into());
            }
            PreparedBody::Form(out)
        }
    };
    Ok(Prepared {
        method: match req.method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
            Method::Put => reqwest::Method::PUT,
            Method::Patch => reqwest::Method::PATCH,
            Method::Delete => reqwest::Method::DELETE,
        },
        url,
        route,
        headers,
        body,
        timeout,
    })
}

fn prepare_header(header: Header) -> Result<(HeaderName, PreparedValue), HttpError> {
    let name =
        HeaderName::from_bytes(header.name.as_bytes()).map_err(|_| HttpError::InvalidRequest)?;
    if RESERVED_HEADERS.contains(&name.as_str()) {
        return Err(HttpError::InvalidRequest);
    }
    let value = match header.value {
        crate::runtime::wit::http::HeaderValue::Text(text) => PreparedValue::Text(
            HeaderValue::from_str(&text).map_err(|_| HttpError::InvalidRequest)?,
        ),
        crate::runtime::wit::http::HeaderValue::Secret(name) => PreparedValue::Secret(name),
    };
    Ok((name, value))
}

/// What a running request needs from the shared `Http` service.
struct Net {
    secrets: Option<Arc<PluginSecrets>>,
    tls: rustls::ClientConfig,
    hooks: NetHooks,
    counters: Arc<Counters>,
}

async fn execute(p: Prepared, plugin: &str, net: Net) -> Result<HttpResponse, HttpError> {
    let host = p
        .url
        .host_str()
        .ok_or(HttpError::InvalidRequest)?
        .to_owned();
    let port = p
        .url
        .port_or_known_default()
        .ok_or(HttpError::InvalidRequest)?;
    // Resolve once and connect only to the checked addresses, so a DNS answer can't change
    // between the check and the connection.
    let addrs = match &p.route {
        Route::Address => {
            let addrs = resolve(&p.url, port).await?;
            if p.url.scheme() == "http" && !addrs.iter().all(|a| is_local(a.ip())) {
                return Err(HttpError::HostNotAllowed);
            }
            addrs
        }
        Route::Origin(origin) => {
            let checked = net_guard::resolve_checked(origin, &net.hooks).await;
            let blocked = matches!(checked, Err(Blocked::Address));
            count(
                Arc::clone(&net.counters),
                plugin.to_owned(),
                host.clone(),
                blocked,
            );
            match checked {
                Ok(addrs) => addrs,
                Err(Blocked::Address) => return Err(HttpError::HostNotAllowed),
                Err(Blocked::NotFound) => {
                    return Err(HttpError::Connect("Can't find this address".into()));
                }
            }
        }
    };

    let mut headers = HeaderMap::new();
    for (name, value) in p.headers {
        let value = match value {
            PreparedValue::Text(value) => value,
            PreparedValue::Secret(secret) => {
                secret_header(net.secrets.as_ref(), plugin, secret).await?
            }
        };
        headers.append(name, value);
    }
    let body = match p.body {
        PreparedBody::Empty => None,
        PreparedBody::Bytes(bytes) => Some(bytes),
        PreparedBody::File(file) => Some(read_file(&file).await?),
        PreparedBody::Form(fields) => {
            let (content_type, bytes) = multipart(fields).await?;
            headers.insert(CONTENT_TYPE, content_type);
            Some(bytes)
        }
    };

    let mut builder = reqwest::Client::builder()
        .tls_backend_preconfigured(net.tls)
        .no_proxy()
        .referer(false)
        .redirect(redirect::Policy::none());
    if p.url.domain().is_some() {
        builder = builder.resolve_to_addrs(&host, &addrs);
    }
    let client = builder
        .build()
        .map_err(|_| HttpError::Connect("Mumcord can't make requests right now".into()))?;
    let mut request = client.request(p.method, p.url).headers(headers);
    if let Some(body) = body {
        request = request.body(body);
    }
    let mut response = request.send().await.map_err(map_error)?;
    let status = response.status();
    if status.is_redirection() && status != reqwest::StatusCode::NOT_MODIFIED {
        return Err(HttpError::Redirect);
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_RESPONSE_BYTES as u64)
    {
        return Err(HttpError::ResponseTooLarge);
    }
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_error)? {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(HttpError::ResponseTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(HttpResponse {
        status: status.as_u16(),
        headers,
        body,
    })
}

async fn resolve(url: &Url, port: u16) -> Result<Vec<SocketAddr>, HttpError> {
    let not_found = || HttpError::Connect("Can't find this address".into());
    let host = url.host_str().ok_or(HttpError::InvalidRequest)?;
    let literal = host.trim_start_matches('[').trim_end_matches(']');
    let addrs: Vec<SocketAddr> = match literal.parse::<IpAddr>() {
        Ok(ip) => vec![SocketAddr::new(ip, port)],
        Err(_) => tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| not_found())?
            .collect(),
    };
    if addrs.is_empty() {
        return Err(not_found());
    }
    Ok(addrs)
}

/// Loopback, private and link-local addresses: the only ones plain `http` may reach.
fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return is_local(IpAddr::V4(v4));
            }
            ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()
        }
    }
}

async fn secret_header(
    secrets: Option<&Arc<PluginSecrets>>,
    plugin: &str,
    name: String,
) -> Result<HeaderValue, HttpError> {
    let secrets = Arc::clone(secrets.ok_or(HttpError::InvalidRequest)?);
    let plugin = plugin.to_owned();
    // The keychain can be slow; keep it off the runtime thread.
    let value = tokio::task::spawn_blocking(move || secrets.get(&plugin, &name))
        .await
        .map_err(|_| HttpError::InvalidRequest)?
        .map_err(|_| HttpError::InvalidRequest)?
        .ok_or(HttpError::InvalidRequest)?;
    let mut value = HeaderValue::from_str(&value).map_err(|_| HttpError::InvalidRequest)?;
    value.set_sensitive(true);
    Ok(value)
}

async fn read_file(file: &Picked) -> Result<Vec<u8>, HttpError> {
    file.read()
        .await
        .map_err(|_| HttpError::Connect("Can't read the picked file".into()))
}

/// Builds a `multipart/form-data` body by hand: at most one file and a few text fields don't
/// need a streaming encoder.
async fn multipart(
    fields: Vec<(String, PreparedField)>,
) -> Result<(HeaderValue, Vec<u8>), HttpError> {
    let mut parts = Vec::with_capacity(fields.len());
    for (name, field) in fields {
        let (disposition, bytes) = match field {
            PreparedField::Text(text) => (
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n"),
                text.into_bytes(),
            ),
            PreparedField::File(file) => {
                let filename = file.name().replace(['"', '\r', '\n'], "_");
                (
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n"
                    ),
                    read_file(&file).await?,
                )
            }
        };
        parts.push((disposition, bytes));
    }
    // A boundary must not occur inside any part.
    let boundary = (0u32..)
        .map(|n| format!("mumcord-form-boundary-{n}"))
        .find(|b| !parts.iter().any(|(_, bytes)| contains(bytes, b.as_bytes())))
        .unwrap_or_default();
    let mut body = Vec::new();
    for (disposition, bytes) in parts {
        body.extend_from_slice(format!("--{boundary}\r\n{disposition}\r\n").as_bytes());
        body.extend_from_slice(&bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let content_type = HeaderValue::from_str(&format!("multipart/form-data; boundary={boundary}"))
        .map_err(|_| HttpError::InvalidRequest)?;
    Ok((content_type, body))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn map_error(error: reqwest::Error) -> HttpError {
    if error.is_timeout() {
        return HttpError::Timeout;
    }
    if let Some(tls) = find_tls_error(&error) {
        return HttpError::Tls(tls_text(tls).into());
    }
    if error.is_connect() {
        return HttpError::Connect("Can't connect to the server".into());
    }
    HttpError::Connect("The connection was interrupted".into())
}

fn find_tls_error(error: &reqwest::Error) -> Option<&rustls::Error> {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = current {
        if let Some(tls) = error.downcast_ref::<rustls::Error>() {
            return Some(tls);
        }
        // hyper wraps the rustls error in nested io::Errors whose `source` skips the payload.
        current = match error.downcast_ref::<std::io::Error>() {
            Some(io) => io
                .get_ref()
                .map(|inner| inner as &(dyn std::error::Error + 'static)),
            None => error.source(),
        };
    }
    None
}

fn tls_text(error: &rustls::Error) -> &'static str {
    use rustls::CertificateError as C;
    match error {
        rustls::Error::InvalidCertificate(cert) => match cert {
            C::Expired | C::ExpiredContext { .. } => "The certificate has expired",
            C::NotValidYet | C::NotValidYetContext { .. } => "The certificate is not valid yet",
            C::NotValidForName | C::NotValidForNameContext { .. } => {
                "The certificate is not valid for this address"
            }
            C::Revoked => "The certificate was revoked",
            _ => "The certificate is not trusted",
        },
        _ => "The secure connection failed",
    }
}

/// The error kind for logs; never the content of the request.
fn error_kind(error: &HttpError) -> &'static str {
    match error {
        HttpError::HostNotAllowed => "host-not-allowed",
        HttpError::InvalidRequest => "invalid-request",
        HttpError::Redirect => "redirect",
        HttpError::Timeout => "timeout",
        HttpError::Tls(_) => "tls",
        HttpError::Connect(_) => "connect",
        HttpError::ResponseTooLarge => "response-too-large",
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, Ipv6Addr, TcpListener, TcpStream};
    use std::sync::mpsc as std_mpsc;

    use super::*;
    use crate::api::files::Files;
    use crate::api::v1_1::Down;
    use crate::diag::Diagnostics;
    use crate::runtime::Next;
    use crate::runtime::wit::http::{FormField, HeaderValue as WitHeaderValue};

    const PLUGIN: &str = "mumbot-music";

    struct Harness {
        ctx: HostContext,
        queue: Arc<EventQueue>,
        set: HttpSet,
        /// Whether the plugin has `net.http` (official) rather than only `net.origins`.
        official: bool,
    }

    fn harness_with(http: Http) -> Harness {
        let secrets = PluginSecrets::in_memory();
        secrets
            .set(PLUGIN, "bot/key", "s3cret")
            .expect("test value");
        let ctx = HostContext {
            http,
            files: Files::default(),
            secrets: Some(Arc::new(secrets)),
            ..HostContext::default()
        };
        let queue = Arc::new(EventQueue::new(Arc::new(Diagnostics::default())));
        let set = ctx.http.set(PLUGIN.into(), Arc::downgrade(&queue));
        Harness {
            ctx,
            queue,
            set,
            official: true,
        }
    }

    fn harness() -> Harness {
        harness_with(Http::default())
    }

    impl Harness {
        fn start(&mut self, req: HttpRequest) -> Result<RequestId, Refused> {
            self.set.request(&self.ctx, req, self.official)
        }

        fn send(&mut self, req: HttpRequest) -> Result<HttpResponse, HttpError> {
            let id = self.start(req).expect("accepted");
            match self.queue.pop() {
                Next::Event(event, _) => match event.down() {
                    Some(Event::HttpResponse(result)) => {
                        assert_eq!(result.id, id);
                        result.outcome
                    }
                    _ => panic!("expected an http-response event"),
                },
                _ => panic!("expected an http-response event"),
            }
        }
    }

    fn get(url: &str) -> HttpRequest {
        HttpRequest {
            method: Method::Get,
            url: url.to_owned(),
            headers: Vec::new(),
            body: Body::Empty,
        }
    }

    /// Answers `count` connections with `response` and hands back each raw request.
    fn server(
        count: usize,
        delay: Duration,
        response: Vec<u8>,
    ) -> (u16, std_mpsc::Receiver<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test value");
        let port = listener.local_addr().expect("test value").port();
        let (tx, rx) = std_mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(count) {
                let mut stream = stream.expect("test value");
                let request = read_request(&mut stream);
                tx.send(request).ok();
                std::thread::sleep(delay);
                stream.write_all(&response).ok();
            }
        });
        (port, rx)
    }

    fn read_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut data = Vec::new();
        let mut buf = [0; 65536];
        loop {
            let n = stream.read(&mut buf).expect("test value");
            if n == 0 {
                return data;
            }
            data.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&data).to_lowercase();
            if let Some(end) = text.find("\r\n\r\n") {
                let len = text
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length: "))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if data.len() >= end + 4 + len {
                    return data;
                }
            }
        }
    }

    fn ok(body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[test]
    fn addresses_are_parsed_to_exact_host_and_port() {
        let a = |host: &str, port| Address {
            host: host.to_owned(),
            port,
        };
        assert_eq!(
            parse_address("127.0.0.1:8081"),
            Ok(a("127.0.0.1", Some(8081)))
        );
        assert_eq!(parse_address(" Bot.Example "), Ok(a("bot.example", None)));
        assert_eq!(
            parse_address("bot.example:80"),
            Ok(a("bot.example", Some(80)))
        );
        assert_eq!(
            parse_address("bot.example:443"),
            Ok(a("bot.example", Some(443)))
        );
        assert_eq!(
            parse_address("https://bot.example"),
            Ok(a("bot.example", Some(443)))
        );
        assert_eq!(
            parse_address("http://bot.example:8081/queue"),
            Ok(a("bot.example", Some(8081)))
        );
        assert_eq!(parse_address("[::1]:8081"), Ok(a("[::1]", Some(8081))));
        for bad in [
            "",
            "ftp://bot.example",
            "user:pw@bot.example",
            "bot example:99999",
        ] {
            assert_eq!(parse_address(bad), Err(AddressError::Invalid), "{bad}");
        }
    }

    #[test]
    fn only_typed_hosts_and_ports_are_allowed() {
        let http = Http::default();
        http.allow(PLUGIN, "127.0.0.1:8081").expect("test value");
        http.allow(PLUGIN, "bot.example").expect("test value");
        let allowed = |url: &str| http.is_allowed(PLUGIN, &Url::parse(url).expect("test value"));
        assert!(allowed("http://127.0.0.1:8081/queue"));
        assert!(!allowed("http://127.0.0.1:8082/queue"));
        assert!(!allowed("http://127.0.0.2:8081/queue"));
        assert!(allowed("https://bot.example/x"));
        assert!(allowed("http://bot.example/x"));
        assert!(!allowed("https://bot.example:8443/x"));
        assert!(!http.is_allowed("dice", &Url::parse("http://127.0.0.1:8081").expect("url")));
        http.forget(PLUGIN);
        assert!(!allowed("http://127.0.0.1:8081/queue"));
    }

    #[test]
    fn addresses_survive_a_restart_and_keep_the_newest() {
        let dir = tempfile::tempdir().expect("test value");
        let http = Http::new(dir.path(), None);
        for port in 0..=MAX_ADDRESSES {
            http.allow(PLUGIN, &format!("127.0.0.1:{}", 9000 + port))
                .expect("test value");
        }
        http.allow("dice", "10.0.0.1:1").expect("test value");
        drop(http);
        let http = Http::new(dir.path(), None);
        let allowed = |plugin, url: &str| http.is_allowed(plugin, &Url::parse(url).expect("url"));
        assert!(!allowed(PLUGIN, "http://127.0.0.1:9000"));
        assert!(allowed(PLUGIN, "http://127.0.0.1:9001"));
        assert!(allowed(
            PLUGIN,
            &format!("http://127.0.0.1:{}", 9000 + MAX_ADDRESSES)
        ));
        http.forget("dice");
        drop(http);
        assert!(
            !Http::new(dir.path(), None)
                .is_allowed("dice", &Url::parse("http://10.0.0.1:1").expect("url"))
        );
    }

    #[test]
    fn plain_http_is_only_for_local_addresses() {
        for ip in [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)),
            IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            IpAddr::V4(Ipv4Addr::new(169, 254, 0, 1)),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            "fd00::1".parse().expect("test value"),
            "fe80::1".parse().expect("test value"),
            "::ffff:192.168.0.1".parse().expect("test value"),
        ] {
            assert!(is_local(ip), "{ip}");
        }
        for ip in [
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1)),
            "2001:db8::1".parse().expect("test value"),
            "::ffff:8.8.8.8".parse().expect("test value"),
        ] {
            assert!(!is_local(ip), "{ip}");
        }
    }

    #[test]
    fn public_http_is_refused_before_connecting() {
        let mut h = harness();
        // 192.0.2.0/24 is reserved for documentation; nothing is ever sent there.
        h.ctx
            .http
            .allow(PLUGIN, "http://192.0.2.1:8081")
            .expect("test value");
        assert!(matches!(
            h.send(get("http://192.0.2.1:8081/queue")),
            Err(HttpError::HostNotAllowed)
        ));
    }

    #[test]
    fn a_request_reaches_the_allowed_server_with_the_secret_inserted() {
        let (port, requests) = server(1, Duration::ZERO, ok("queue"));
        let mut h = harness();
        h.ctx
            .http
            .allow(PLUGIN, &format!("127.0.0.1:{port}"))
            .expect("test value");
        let mut req = get(&format!("http://127.0.0.1:{port}/queue"));
        req.headers = vec![
            Header {
                name: "X-API-Key".into(),
                value: WitHeaderValue::Secret("bot/key".into()),
            },
            Header {
                name: "Accept".into(),
                value: WitHeaderValue::Text("text/plain".into()),
            },
        ];
        let response = h.send(req).expect("test value");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"queue");
        assert!(
            response
                .headers
                .iter()
                .any(|(n, v)| n == "content-type" && v == "text/plain")
        );
        let raw = String::from_utf8(requests.recv().expect("test value")).expect("utf-8");
        assert!(raw.starts_with("GET /queue HTTP/1.1\r\n"));
        assert!(raw.to_lowercase().contains("x-api-key: s3cret\r\n"));
    }

    #[test]
    fn other_ports_and_hosts_are_refused() {
        let mut h = harness();
        h.ctx
            .http
            .allow(PLUGIN, "127.0.0.1:8081")
            .expect("test value");
        assert!(matches!(
            h.send(get("http://127.0.0.1:8082/")),
            Err(HttpError::HostNotAllowed)
        ));
        assert!(matches!(
            h.send(get("http://localhost:8081/")),
            Err(HttpError::HostNotAllowed)
        ));
    }

    #[test]
    fn malformed_requests_are_refused() {
        let mut h = harness();
        h.ctx
            .http
            .allow(PLUGIN, "127.0.0.1:8081")
            .expect("test value");
        let url = "http://127.0.0.1:8081/";
        let header = |name: &str, value| {
            let mut req = get(url);
            req.headers = vec![Header {
                name: name.into(),
                value,
            }];
            req
        };
        for req in [
            get("not a url"),
            get("ftp://127.0.0.1:8081/"),
            get("http://user:pw@127.0.0.1:8081/"),
            header("Host", WitHeaderValue::Text("evil".into())),
            header("Bad Name", WitHeaderValue::Text("x".into())),
            header("X-Line", WitHeaderValue::Text("a\r\nb".into())),
            header("X-Key", WitHeaderValue::Secret("unknown".into())),
            HttpRequest {
                body: Body::File(42),
                ..get(url)
            },
        ] {
            // A fresh plugin instance per request keeps the rate limit out of the way. Unknown
            // secrets are found only when the request runs and nothing listens on the port, so
            // either refusal is fine, but never a response.
            let mut fresh = harness();
            fresh
                .ctx
                .http
                .allow(PLUGIN, "127.0.0.1:8081")
                .expect("test value");
            let outcome = fresh.send(req);
            assert!(
                matches!(
                    outcome,
                    Err(HttpError::InvalidRequest | HttpError::Connect(_))
                ),
                "{outcome:?}"
            );
        }
        // A file handle of another plugin is unknown to this one.
        let dir = tempfile::tempdir().expect("test value");
        let path = dir.path().join("a.mp3");
        std::fs::write(&path, b"x").expect("test value");
        let foreign = h.ctx.files.register("dice", &path).expect("test value");
        assert!(matches!(
            h.send(HttpRequest {
                body: Body::File(foreign),
                ..get(url)
            }),
            Err(HttpError::InvalidRequest)
        ));
    }

    #[test]
    fn unknown_secrets_are_refused_before_sending() {
        let (port, requests) = server(1, Duration::ZERO, ok(""));
        let mut h = harness();
        h.ctx
            .http
            .allow(PLUGIN, &format!("127.0.0.1:{port}"))
            .expect("test value");
        let mut req = get(&format!("http://127.0.0.1:{port}/"));
        req.headers = vec![Header {
            name: "X-Key".into(),
            value: WitHeaderValue::Secret("unknown".into()),
        }];
        assert!(matches!(h.send(req), Err(HttpError::InvalidRequest)));
        assert!(requests.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn redirects_are_not_followed() {
        let (port, _requests) = server(
            1,
            Duration::ZERO,
            b"HTTP/1.1 302 Found\r\nlocation: http://127.0.0.1:1/\r\ncontent-length: 0\r\n\r\n"
                .to_vec(),
        );
        let mut h = harness();
        h.ctx
            .http
            .allow(PLUGIN, &format!("127.0.0.1:{port}"))
            .expect("test value");
        assert!(matches!(
            h.send(get(&format!("http://127.0.0.1:{port}/"))),
            Err(HttpError::Redirect)
        ));
    }

    #[test]
    fn responses_over_one_mib_are_refused() {
        let big = "x".repeat(MAX_RESPONSE_BYTES + 1);
        let chunked = format!(
            "HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n{:x}\r\n{big}\r\n0\r\n\r\n",
            big.len()
        );
        let (port, _requests) = server(1, Duration::ZERO, ok(&big));
        let (chunked_port, _chunked) = server(1, Duration::ZERO, chunked.into_bytes());
        let mut h = harness();
        for port in [port, chunked_port] {
            h.ctx
                .http
                .allow(PLUGIN, &format!("127.0.0.1:{port}"))
                .expect("test value");
            assert!(matches!(
                h.send(get(&format!("http://127.0.0.1:{port}/"))),
                Err(HttpError::ResponseTooLarge)
            ));
        }
    }

    #[test]
    fn slow_servers_time_out() {
        let (port, _requests) = server(1, Duration::from_secs(2), ok(""));
        let http = Http {
            timeout: Duration::from_millis(200),
            ..Http::default()
        };
        let mut h = harness_with(http);
        h.ctx
            .http
            .allow(PLUGIN, &format!("127.0.0.1:{port}"))
            .expect("test value");
        assert!(matches!(
            h.send(get(&format!("http://127.0.0.1:{port}/"))),
            Err(HttpError::Timeout)
        ));
    }

    #[test]
    fn bodies_and_rates_are_capped() {
        let mut h = harness();
        let big = HttpRequest {
            method: Method::Post,
            body: Body::Bytes(vec![0; MAX_BODY_BYTES + 1]),
            ..get("http://127.0.0.1:1/")
        };
        assert_eq!(h.start(big), Err(Refused::Api(Error::TooLarge)));
        for _ in 0..5 {
            h.start(get("http://127.0.0.1:1/")).expect("test value");
        }
        assert_eq!(
            h.start(get("http://127.0.0.1:1/")),
            Err(Refused::Api(Error::RateLimited))
        );
    }

    #[test]
    fn a_picked_file_is_uploaded_as_multipart() {
        let (port, requests) = server(1, Duration::ZERO, ok("added"));
        let mut h = harness();
        h.ctx
            .http
            .allow(PLUGIN, &format!("127.0.0.1:{port}"))
            .expect("test value");
        let dir = tempfile::tempdir().expect("test value");
        let path = dir.path().join("song.mp3");
        std::fs::write(&path, b"ID3-song-bytes").expect("test value");
        let handle = h.ctx.files.register(PLUGIN, &path).expect("test value");
        let req = HttpRequest {
            method: Method::Post,
            url: format!("http://127.0.0.1:{port}/queue/upload"),
            headers: Vec::new(),
            body: Body::Form(vec![
                FormField {
                    name: "position".into(),
                    value: FormValue::Text("end".into()),
                },
                FormField {
                    name: "file".into(),
                    value: FormValue::File(handle),
                },
            ]),
        };
        assert_eq!(h.send(req).expect("test value").body, b"added");
        let raw = String::from_utf8(requests.recv().expect("test value")).expect("utf-8");
        assert!(
            raw.contains("content-type: multipart/form-data; boundary=mumcord-form-boundary-0")
        );
        assert!(raw.contains("name=\"position\"\r\n\r\nend\r\n"));
        assert!(raw.contains("name=\"file\"; filename=\"song.mp3\"\r\nContent-Type: application/octet-stream\r\n\r\nID3-song-bytes\r\n"));
        assert!(raw.ends_with("--mumcord-form-boundary-0--\r\n"));
    }

    #[test]
    fn a_form_may_carry_only_one_file() {
        let mut h = harness();
        h.ctx
            .http
            .allow(PLUGIN, "127.0.0.1:8081")
            .expect("test value");
        let dir = tempfile::tempdir().expect("test value");
        let path = dir.path().join("a.mp3");
        std::fs::write(&path, b"x").expect("test value");
        let handle = h.ctx.files.register(PLUGIN, &path).expect("test value");
        let field = |name: &str| FormField {
            name: name.into(),
            value: FormValue::File(handle),
        };
        let req = HttpRequest {
            method: Method::Post,
            body: Body::Form(vec![field("a"), field("b")]),
            ..get("http://127.0.0.1:8081/")
        };
        assert!(matches!(h.send(req), Err(HttpError::InvalidRequest)));
    }

    #[test]
    fn untrusted_certificates_are_reported() {
        let generated =
            rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).expect("cert");
        let key =
            rustls::pki_types::PrivateKeyDer::Pkcs8(generated.signing_key.serialize_der().into());
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("test value")
        .with_no_client_auth()
        .with_single_cert(vec![generated.cert.der().clone()], key)
        .expect("test value");
        let config = Arc::new(config);
        let listener = TcpListener::bind("127.0.0.1:0").expect("test value");
        let port = listener.local_addr().expect("test value").port();
        std::thread::spawn(move || {
            if let Some(Ok(mut stream)) = listener.incoming().next() {
                let mut conn = rustls::ServerConnection::new(config).expect("test value");
                while conn.complete_io(&mut stream).is_ok() && conn.is_handshaking() {}
            }
        });
        let mut h = harness();
        h.ctx
            .http
            .allow(PLUGIN, &format!("localhost:{port}"))
            .expect("test value");
        let outcome = h.send(get(&format!("https://localhost:{port}/")));
        assert!(
            matches!(&outcome, Err(HttpError::Tls(text)) if text == "The certificate is not trusted"),
            "{outcome:?}"
        );
    }

    /// The E2E fixture setup in small: `fixture.test` resolves to 127.0.0.1 and that address
    /// counts as public, as with `MUMCORD_TEST_RESOLVE` and `MUMCORD_TEST_PUBLIC_ADDR`.
    fn fixture_hooks(test_ca: Option<PathBuf>) -> NetHooks {
        NetHooks {
            test_ca,
            public_addr: Some(Ipv4Addr::LOCALHOST.into()),
            resolve: |name| (name == "fixture.test").then_some(Ipv4Addr::LOCALHOST.into()),
        }
    }

    /// A harness for a third-party plugin with only `net.origins` and `origins` declared.
    fn origins_harness(
        hooks: NetHooks,
        store: Option<Arc<Mutex<Store>>>,
        origins: &[&str],
    ) -> Harness {
        let http = Http {
            hooks,
            ..Http::with_addresses(Addresses::default(), store)
        };
        let declared = origins
            .iter()
            .map(|o| crate::origins::validate(o, false).expect("valid origin"))
            .collect();
        http.set_origins(PLUGIN, declared);
        let mut h = harness_with(http);
        h.official = false;
        h
    }

    /// A TLS server for `name` whose certificate a fresh CA signed; answers one request with
    /// `response`. Returns the port and the CA as a PEM file.
    fn tls_server(name: &str, response: Vec<u8>) -> (u16, tempfile::NamedTempFile) {
        let ca_key = rcgen::KeyPair::generate().expect("key");
        let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).expect("params");
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        // rcgen gives every certificate the same default subject; Windows then takes the leaf
        // for a self-issued root and never builds the chain to the CA.
        ca_params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "Mumcord test CA");
        let ca = rcgen::CertifiedIssuer::self_signed(ca_params, ca_key).expect("ca");
        let leaf_key = rcgen::KeyPair::generate().expect("key");
        let mut leaf_params = rcgen::CertificateParams::new(vec![name.to_owned()]).expect("params");
        leaf_params
            .distinguished_name
            .push(rcgen::DnType::CommonName, name);
        let leaf = leaf_params.signed_by(&leaf_key, &ca).expect("leaf");
        let mut ca_file = tempfile::NamedTempFile::new().expect("temp file");
        ca_file.write_all(ca.pem().as_bytes()).expect("write ca");

        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(leaf_key.serialize_der().into());
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("test value")
        .with_no_client_auth()
        .with_single_cert(vec![leaf.der().clone(), ca.der().clone()], key)
        .expect("test value");
        let config = Arc::new(config);
        let listener = TcpListener::bind("127.0.0.1:0").expect("test value");
        let port = listener.local_addr().expect("test value").port();
        std::thread::spawn(move || {
            if let Some(Ok(mut tcp)) = listener.incoming().next() {
                let mut conn = rustls::ServerConnection::new(config).expect("test value");
                let mut tls = rustls::Stream::new(&mut conn, &mut tcp);
                let mut buf = [0; 4096];
                if matches!(tls.read(&mut buf), Ok(n) if n > 0) {
                    tls.write_all(&response).ok();
                    tls.flush().ok();
                }
            }
        });
        (port, ca_file)
    }

    fn counters(store: &Arc<Mutex<Store>>) -> (i64, i64, Option<String>) {
        // Counting runs on the worker after the answer; give it a moment.
        for _ in 0..100 {
            let rows = store
                .lock()
                .expect("lock")
                .plugin_origin_counters(PLUGIN)
                .expect("counters");
            if let Some(row) = rows.first() {
                return (row.requests, row.blocked, row.last_host.clone());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        (0, 0, None)
    }

    #[test]
    fn a_declared_origin_is_reached_and_counted() {
        let (port, ca) = tls_server("fixture.test", ok("hello"));
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Arc::new(Mutex::new(Store::open(dir.path()).expect("store")));
        let origin = format!("https://fixture.test:{port}");
        let mut h = origins_harness(
            fixture_hooks(Some(ca.path().to_owned())),
            Some(Arc::clone(&store)),
            &[&origin],
        );
        let response = h.send(get(&format!("{origin}/api"))).expect("response");
        assert_eq!((response.status, response.body), (200, b"hello".to_vec()));
        assert_eq!(counters(&store), (1, 0, Some("fixture.test".into())));
    }

    #[test]
    fn undeclared_hosts_and_ports_are_refused_and_counted_as_blocked() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Arc::new(Mutex::new(Store::open(dir.path()).expect("store")));
        let mut h = origins_harness(
            fixture_hooks(None),
            Some(Arc::clone(&store)),
            &["https://fixture.test"],
        );
        for url in [
            "https://other.test/",
            "https://fixture.test:8443/",
            "http://fixture.test/",
        ] {
            assert_eq!(h.start(get(url)), Err(Refused::OriginNotAllowed), "{url}");
        }
        let (requests, blocked, _) = counters(&store);
        assert!(requests >= 1 && requests == blocked, "{requests} {blocked}");
    }

    #[test]
    fn a_plugin_without_origins_gets_origin_not_allowed() {
        let mut h = origins_harness(fixture_hooks(None), None, &[]);
        assert_eq!(
            h.start(get("https://fixture.test/")),
            Err(Refused::OriginNotAllowed)
        );
    }

    #[test]
    fn a_declared_name_resolving_to_a_local_address_is_blocked() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Arc::new(Mutex::new(Store::open(dir.path()).expect("store")));
        // Without the public-address hook, 127.0.0.1 is local: the DNS rebinding case.
        let hooks = NetHooks {
            public_addr: None,
            ..fixture_hooks(None)
        };
        let mut h = origins_harness(hooks, Some(Arc::clone(&store)), &["https://fixture.test"]);
        assert!(matches!(
            h.send(get("https://fixture.test/")),
            Err(HttpError::HostNotAllowed)
        ));
        assert_eq!(counters(&store), (1, 1, Some("fixture.test".into())));
    }

    #[test]
    fn redirects_from_a_declared_origin_are_not_followed() {
        let (port, ca) = tls_server(
            "fixture.test",
            b"HTTP/1.1 302 Found\r\nlocation: https://other.test/\r\ncontent-length: 0\r\n\r\n"
                .to_vec(),
        );
        let origin = format!("https://fixture.test:{port}");
        let mut h = origins_harness(fixture_hooks(Some(ca.path().to_owned())), None, &[&origin]);
        assert!(matches!(
            h.send(get(&format!("{origin}/"))),
            Err(HttpError::Redirect)
        ));
    }

    #[test]
    fn the_certificate_must_match_the_declared_name() {
        let (port, ca) = tls_server("other.test", ok("hello"));
        let origin = format!("https://fixture.test:{port}");
        let mut h = origins_harness(fixture_hooks(Some(ca.path().to_owned())), None, &[&origin]);
        let outcome = h.send(get(&format!("{origin}/")));
        assert!(matches!(&outcome, Err(HttpError::Tls(_))), "{outcome:?}");
    }

    #[test]
    fn the_fixture_certificate_is_untrusted_without_the_test_ca() {
        let (port, _ca) = tls_server("fixture.test", ok("hello"));
        let origin = format!("https://fixture.test:{port}");
        let mut h = origins_harness(fixture_hooks(None), None, &[&origin]);
        let outcome = h.send(get(&format!("{origin}/")));
        assert!(
            matches!(&outcome, Err(HttpError::Tls(text)) if text == "The certificate is not trusted"),
            "{outcome:?}"
        );
    }

    #[test]
    fn official_plugins_may_also_use_declared_origins() {
        let (port, ca) = tls_server("fixture.test", ok("hi"));
        let origin = format!("https://fixture.test:{port}");
        let mut h = origins_harness(fixture_hooks(Some(ca.path().to_owned())), None, &[&origin]);
        h.official = true;
        assert_eq!(
            h.send(get(&format!("{origin}/"))).expect("response").status,
            200
        );
        assert!(matches!(
            h.send(get("https://other.test/")),
            Err(HttpError::HostNotAllowed)
        ));
    }
}
