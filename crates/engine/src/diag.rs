//! Logging and diagnostics (FR-1.40, spec section 9).
//!
//! # Logging rules for every module (privacy, GDPR)
//!
//! Log IDs and counts, never content. Chat text, user names, channel names, server
//! addresses, passwords, server passwords, certificates, keys and audio must never be
//! logged, not even at `debug` level.
//!
//! - Use structured fields: `info!(channel_id, user_count, "synced")`, not
//!   `info!("synced {name}")`.
//! - Fields whose name contains a sensitive word (`password`, `secret`, `token`, `key`,
//!   `cert`, `pem`, `text`, `msg`, `body`, `comment`, `name`, `user`, `host`, `addr`, ...)
//!   are replaced by `[redacted]` by the redaction layer, unless the name ends in `id`,
//!   `ids`, `count`, `len`, `size`, `bytes` or `session`. So `user_id` is kept, `username` is not.
//! - Any value that contains a PEM block is redacted whatever its field name.
//! - The redaction layer cannot see inside a formatted message: never interpolate
//!   sensitive data into the message string itself.
//! - Never log from the real-time audio thread.

use std::collections::VecDeque;
use std::fmt::{self, Write as _};
use std::io::{self, BufWriter, Write as _};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bovnar::Value;
use serde::{Deserialize, Serialize};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{Layer, Registry, reload};

const REDACTED: &str = "[redacted]";
const RING_CAPACITY: usize = 500;
const LOG_PREFIX: &str = "mumcord";
const LOG_SUFFIX: &str = "log";
/// Log lines are written in batches (spec 12 FR-12.1): the buffer is flushed this often.
const FLUSH_INTERVAL: Duration = Duration::from_secs(5);
/// Buffer capacity for the log file, so many small lines coalesce into one write.
const LOG_BUF_CAPACITY: usize = 64 * 1024;

/// Words in a field name that mark its value as personal or secret.
const SENSITIVE_WORDS: &[&str] = &[
    "password",
    "passwd",
    "pass",
    "pwd",
    "secret",
    "token",
    "key",
    "keys",
    "cert",
    "certificate",
    "pem",
    "p12",
    "pkcs12",
    "text",
    "msg",
    "body",
    "chat",
    "content",
    "comment",
    "description",
    "name",
    "nick",
    "user",
    "username",
    "host",
    "hostname",
    "addr",
    "address",
    "ip",
    "email",
    "path",
];
/// Last name part that marks a field as a plain identifier or quantity, which is safe.
const SAFE_SUFFIXES: &[&str] = &["id", "ids", "count", "len", "size", "bytes", "session"];

/// Log verbosity, matching the `logLevel` setting.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

impl LogLevel {
    fn filter(self) -> LevelFilter {
        match self {
            Self::Error => LevelFilter::ERROR,
            Self::Warn => LevelFilter::WARN,
            Self::Info => LevelFilter::INFO,
            Self::Debug => LevelFilter::DEBUG,
        }
    }
}

impl FromStr for LogLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "error" => Ok(Self::Error),
            "warn" => Ok(Self::Warn),
            "info" => Ok(Self::Info),
            "debug" => Ok(Self::Debug),
            other => Err(format!("unknown log level: {other}")),
        }
    }
}

/// An audio device as shown in the diagnostic file.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DiagAudioDevice {
    pub name: String,
    pub is_input: bool,
    pub is_default: bool,
}

/// Connection statistics of the current session; counters only, no addresses or names.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct DiagConnectionStats {
    pub connected: bool,
    /// `udp` or `tcp`.
    pub transport: Option<String>,
    pub ping_ms: Option<f32>,
    pub packets_good: u64,
    pub packets_late: u64,
    pub packets_lost: u64,
    /// Recent packet loss, 0–100.
    pub loss_percent: f32,
    /// Current encoder target of the bitrate controller.
    pub send_bitrate_kbps: Option<u32>,
    pub server_max_bandwidth_kbps: Option<u32>,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub reconnect_count: u32,
    /// Glitch counters of the local audio path; `None` without a running voice pipeline.
    #[serde(default)]
    pub audio: Option<crate::AudioHealthView>,
}

/// Everything the diagnostic file contains besides the log lines.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DiagnosticInfo {
    pub app_version: String,
    pub os: String,
    pub audio_devices: Vec<DiagAudioDevice>,
    pub connection: DiagConnectionStats,
}

impl DiagnosticInfo {
    /// Version and OS only; the caller adds devices and statistics.
    pub fn new(app_version: &str) -> Self {
        Self {
            app_version: app_version.to_owned(),
            os: os_description(),
            audio_devices: Vec::new(),
            connection: DiagConnectionStats::default(),
        }
    }
}

/// OS family and CPU architecture, e.g. `linux x86_64`.
pub fn os_description() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

type InfoProvider = Box<dyn Fn() -> DiagnosticInfo + Send + Sync>;

static INFO_PROVIDER: OnceLock<InfoProvider> = OnceLock::new();
static GLOBAL: OnceLock<Arc<Shared>> = OnceLock::new();

/// Registers the function that collects devices and statistics when the user saves a
/// diagnostic file. Only the first registration counts.
pub fn set_info_provider(provider: impl Fn() -> DiagnosticInfo + Send + Sync + 'static) {
    if INFO_PROVIDER.set(Box::new(provider)).is_err() {
        tracing::warn!("diagnostic info provider already set");
    }
}

/// Current diagnostic info: from the registered provider, else version and OS only.
pub fn collect_info(app_version: &str) -> DiagnosticInfo {
    match INFO_PROVIDER.get() {
        Some(provider) => provider(),
        None => DiagnosticInfo::new(app_version),
    }
}

struct Shared {
    sink: Arc<Mutex<Sink>>,
    ring: Mutex<VecDeque<String>>,
    level: reload::Handle<LevelFilter, Registry>,
}

impl Shared {
    fn recent_lines(&self) -> Vec<String> {
        lock(&self.ring).iter().cloned().collect()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Layer that redacts, formats, writes to the rotating file and keeps the in-memory ring.
/// Where formatted lines go. `Pending` holds the lines logged before the log directory and
/// level are known, so nothing from start-up is lost and nothing is written to disk before the
/// profile has been checked. `File` writes through a buffer (spec 12 FR-12.1): lines collect in
/// memory and are flushed every [`FLUSH_INTERVAL`], on clean shutdown and on a panic.
enum Sink {
    Pending(Vec<String>),
    File(BufWriter<RollingFileAppender>),
    /// Logging to disk is off (spec 12 FR-12.2): the memory ring still records, nothing is
    /// written.
    Off,
}

impl Sink {
    fn write_line(&mut self, line: &str) {
        match self {
            Self::Pending(lines) => {
                if lines.len() < RING_CAPACITY {
                    lines.push(line.to_owned());
                }
            }
            Self::File(file) => {
                if let Err(err) = file.write_all(format!("{line}\n").as_bytes()) {
                    // Stderr is the only place left when the log file itself fails.
                    eprintln!("mumcord: writing log file failed: {err}");
                }
            }
            Self::Off => {}
        }
    }

    /// Pushes everything buffered to the file. A no-op for `Pending` and `Off`.
    fn flush(&mut self) {
        if let Self::File(file) = self
            && let Err(err) = file.flush()
        {
            eprintln!("mumcord: flushing the log file failed: {err}");
        }
    }
}

struct DiagLayer {
    shared: Arc<Shared>,
    file: Arc<Mutex<Sink>>,
}

impl<S: Subscriber> Layer<S> for DiagLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let mut line = format!("{} {:<5} {}:", timestamp(), meta.level(), meta.target());
        event.record(&mut RedactingVisitor { line: &mut line });

        {
            let mut ring = lock(&self.shared.ring);
            if ring.len() == RING_CAPACITY {
                ring.pop_front();
            }
            ring.push_back(line.clone());
        }
        lock(&self.file).write_line(&line);
    }
}

struct RedactingVisitor<'a> {
    line: &'a mut String,
}

impl RedactingVisitor<'_> {
    fn push(&mut self, field: &Field, value: &str) {
        let name = field.name();
        let _ = if name == "message" {
            write!(self.line, " {}", scrub_value(value))
        } else if is_sensitive_name(name) {
            write!(self.line, " {name}={REDACTED}")
        } else {
            write!(self.line, " {name}={}", scrub_value(value))
        };
    }
}

impl Visit for RedactingVisitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.push(field, value);
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.push(field, &format!("{value:?}"));
    }
}

fn is_sensitive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let parts: Vec<&str> = lower.split(['_', '.']).collect();
    if parts.last().is_some_and(|p| SAFE_SUFFIXES.contains(p)) {
        return false;
    }
    parts.iter().any(|p| SENSITIVE_WORDS.contains(p))
}

/// Drops PEM blocks (also inside otherwise harmless text) and flattens line breaks so a
/// value cannot forge extra log lines.
fn scrub_value(value: &str) -> String {
    if value.contains("-----BEGIN") || value.contains("PRIVATE KEY") {
        return REDACTED.to_owned();
    }
    value.replace(['\n', '\r'], " ")
}

fn timestamp() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", d.as_secs(), d.subsec_millis())
}

fn open_appender(logs_dir: &Path, max_files: usize) -> anyhow::Result<RollingFileAppender> {
    Ok(RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_PREFIX)
        .filename_suffix(LOG_SUFFIX)
        .max_log_files(max_files)
        .build(logs_dir)?)
}

fn build_with(
    sink: Sink,
    level: LogLevel,
) -> (impl Subscriber + Send + Sync + 'static, Arc<Shared>) {
    let (filter, level_handle) = reload::Layer::new(level.filter());
    let sink = Arc::new(Mutex::new(sink));
    let shared = Arc::new(Shared {
        sink: Arc::clone(&sink),
        ring: Mutex::new(VecDeque::with_capacity(RING_CAPACITY)),
        level: level_handle,
    });
    let layer = DiagLayer {
        shared: Arc::clone(&shared),
        file: sink,
    };
    (Registry::default().with(filter).with(layer), shared)
}

/// Starts logging into memory only, as the global tracing subscriber, so lines logged before
/// the log directory and level are known are kept. Writes nothing to disk; call [`attach_file`]
/// once the profile has been checked. Call once at app start.
pub fn init_early() -> anyhow::Result<()> {
    let (subscriber, shared) = build_with(Sink::Pending(Vec::new()), LogLevel::Info);
    tracing::subscriber::set_global_default(subscriber)?;
    GLOBAL
        .set(shared)
        .map_err(|_| anyhow::anyhow!("logging already initialised"))
}

/// Opens the log file in `logs_dir` (created if missing), writes the lines kept since
/// [`init_early`] and sets the level. Keeps at most `max_files` daily files.
pub fn attach_file(logs_dir: &Path, level: LogLevel, max_files: usize) -> anyhow::Result<()> {
    set_disk_logging(true, logs_dir, level, max_files)
}

/// Turns writing the log file on or off at runtime (spec 12 FR-12.2). The in-memory ring and
/// the level filter stay active either way, so "copy log" and the diagnostic file keep working.
pub fn set_disk_logging(
    enabled: bool,
    logs_dir: &Path,
    level: LogLevel,
    max_files: usize,
) -> anyhow::Result<()> {
    let shared = GLOBAL
        .get()
        .ok_or_else(|| anyhow::anyhow!("logging not initialised"))?;
    if enabled {
        if matches!(&*lock(&shared.sink), Sink::Pending(_) | Sink::Off) {
            std::fs::create_dir_all(logs_dir)?;
            attach(shared, logs_dir, max_files)?;
        }
    } else {
        let mut sink = lock(&shared.sink);
        sink.flush();
        *sink = Sink::Off;
    }
    set_log_level(level)
}

fn attach(shared: &Shared, logs_dir: &Path, max_files: usize) -> anyhow::Result<()> {
    let appender = open_appender(logs_dir, max_files)?;
    let mut writer = BufWriter::with_capacity(LOG_BUF_CAPACITY, appender);
    let mut sink = lock(&shared.sink);
    if let Sink::Pending(lines) = &*sink {
        for line in lines {
            writer.write_all(format!("{line}\n").as_bytes())?;
        }
        // Start-up lines are few and should reach the file right away.
        writer.flush()?;
    }
    *sink = Sink::File(writer);
    ensure_flush_thread(shared);
    Ok(())
}

/// Starts the periodic log flush (spec 12 FR-12.1). Runs once, detached, for the process life.
fn ensure_flush_thread(shared: &Shared) {
    static STARTED: OnceLock<()> = OnceLock::new();
    let sink = Arc::clone(&shared.sink);
    STARTED.get_or_init(|| {
        std::thread::Builder::new()
            .name("mumcord-log-flush".to_owned())
            .spawn(move || {
                loop {
                    std::thread::sleep(FLUSH_INTERVAL);
                    lock(&sink).flush();
                }
            })
            .expect("spawn log flush thread");
    });
}

/// Writes buffered log lines to the file now (spec 12 FR-12.1): called on clean shutdown and
/// from the panic hook. The in-memory ring feeds the panic report and "copy log", so those
/// never depend on this flush.
pub fn flush_log() {
    if let Some(shared) = GLOBAL.get() {
        lock(&shared.sink).flush();
    }
}

/// Changes the log level at runtime (settings `logLevel`).
pub fn set_log_level(level: LogLevel) -> anyhow::Result<()> {
    let shared = GLOBAL
        .get()
        .ok_or_else(|| anyhow::anyhow!("logging not initialised"))?;
    shared.level.reload(level.filter())?;
    Ok(())
}

/// Recent log lines, oldest first (already redacted).
pub fn recent_log_lines() -> Vec<String> {
    GLOBAL.get().map(|s| s.recent_lines()).unwrap_or_default()
}

/// Writes the diagnostic file: app version, OS, audio devices, connection statistics and
/// recent log lines. Contains nothing personal.
pub fn save_diagnostic_file(path: &Path, info: &DiagnosticInfo) -> io::Result<()> {
    write_diagnostic(path, info, &recent_log_lines())
}

/// Bovnar strings cannot hold ASCII control characters; the scrubbed log lines and device names
/// keep their text with spaces in their place.
fn text(s: &str) -> Value {
    Value::String(
        s.chars()
            .map(|c| if c.is_ascii_control() { ' ' } else { c })
            .collect(),
    )
}

fn int(n: impl Into<i128>) -> Value {
    Value::Int(n.into())
}

fn entries<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::Struct(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn audio_health(audio: Option<&crate::AudioHealthView>) -> Value {
    let Some(a) = audio else {
        return Value::Null;
    };
    let r = &a.rating;
    entries([
        (
            "rating",
            entries([
                ("rating", text(&format!("{:?}", r.rating))),
                (
                    "issue",
                    r.issue.map_or(Value::Null, |i| text(&format!("{i:?}"))),
                ),
                ("window_secs", int(r.window_secs)),
                ("dropouts", int(r.dropouts)),
                ("late_frames", int(r.late_frames)),
                ("concealed_frames", int(r.concealed_frames)),
                ("received_frames", int(r.received_frames)),
                ("processing_max_us", int(r.processing_max_us)),
            ]),
        ),
        ("realtime_threads", int(a.realtime_threads)),
        ("device_xruns", int(a.device_xruns)),
        ("stream_restarts", int(a.stream_restarts)),
        ("capture_overruns", int(a.capture_overruns)),
        ("playback_underruns", int(a.playback_underruns)),
        ("capture_late_frames", int(a.capture_late_frames)),
        ("playback_late_wakeups", int(a.playback_late_wakeups)),
        ("concealed_frames", int(a.concealed_frames)),
        ("late_packets", int(a.late_packets)),
        ("jitter_delay_ms", int(a.jitter_delay_ms)),
        ("capture_time_avg_us", int(a.capture_time_avg_us)),
        ("capture_time_max_us", int(a.capture_time_max_us)),
        ("mix_time_avg_us", int(a.mix_time_avg_us)),
        ("mix_time_max_us", int(a.mix_time_max_us)),
    ])
}

/// Writes the diagnostic file as a Bovnar document (FR-9.100), indented so people can still read
/// it. Format version 1.
fn write_diagnostic(path: &Path, info: &DiagnosticInfo, lines: &[String]) -> io::Result<()> {
    let c = &info.connection;
    let devices = info
        .audio_devices
        .iter()
        .map(|d| {
            entries([
                ("name", text(&scrub_value(&d.name))),
                ("input", Value::Bool(d.is_input)),
                ("default", Value::Bool(d.is_default)),
            ])
        })
        .collect();
    let opt_int = |v: Option<u32>| v.map_or(Value::Null, int);
    let doc = entries([
        ("v", int(1u8)),
        ("app_version", text(&info.app_version)),
        ("os", text(&info.os)),
        ("audio_devices", Value::Array(devices)),
        (
            "connection",
            entries([
                ("connected", Value::Bool(c.connected)),
                (
                    "transport",
                    c.transport.as_deref().map_or(Value::Null, text),
                ),
                (
                    "ping_ms",
                    c.ping_ms.map_or(Value::Null, |p| Value::Float(p.into())),
                ),
                ("packets_good", int(c.packets_good)),
                ("packets_late", int(c.packets_late)),
                ("packets_lost", int(c.packets_lost)),
                ("loss_percent", Value::Float(c.loss_percent.into())),
                ("send_bitrate_kbps", opt_int(c.send_bitrate_kbps)),
                (
                    "server_max_bandwidth_kbps",
                    opt_int(c.server_max_bandwidth_kbps),
                ),
                ("packets_sent", int(c.packets_sent)),
                ("packets_received", int(c.packets_received)),
                ("reconnects", int(c.reconnect_count)),
            ]),
        ),
        ("audio", audio_health(c.audio.as_ref())),
        (
            "log",
            Value::Array(lines.iter().map(|l| text(&scrub_value(l))).collect()),
        ),
    ]);
    let bytes = bovnar::to_string(&doc, true).map_err(io::Error::other)?;
    std::fs::write(path, bytes)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const PASSWORD: &str = "hunter2-fake-password";
    const MESSAGE: &str = "meet me at the secret base";
    const USER: &str = "AliceTheTester";
    const PEM: &str = "-----BEGIN CERTIFICATE-----\nMIIFAKEFAKEFAKE\n-----END CERTIFICATE-----";

    fn capture(level: LogLevel, f: impl FnOnce()) -> (tempfile::TempDir, Arc<Shared>) {
        let dir = tempfile::tempdir().unwrap();
        let (sub, shared) = build_with(Sink::Pending(Vec::new()), level);
        attach(&shared, dir.path(), 7).unwrap();
        tracing::subscriber::with_default(sub, f);
        // Buffered writes land on disk only after a flush (spec 12 FR-12.1).
        lock(&shared.sink).flush();
        (dir, shared)
    }

    #[test]
    fn early_lines_reach_the_log_file_once_it_is_attached() {
        let dir = tempfile::tempdir().unwrap();
        let (sub, shared) = build_with(Sink::Pending(Vec::new()), LogLevel::Info);
        tracing::subscriber::with_default(sub, || {
            tracing::info!("CPU without AVX2, self-test skipped");
        });
        let log_files = |d: &Path| std::fs::read_dir(d).unwrap().count();
        assert_eq!(log_files(dir.path()), 0, "nothing is written before attach");

        attach(&shared, dir.path(), 7).unwrap();
        let on_disk: String = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| std::fs::read_to_string(e.path()).unwrap())
            .collect();
        assert!(on_disk.contains("CPU without AVX2, self-test skipped"));
    }

    #[test]
    fn lines_wait_in_the_buffer_until_a_flush() {
        // FR-12.1: writing is batched; a line is not on disk until the buffer is flushed.
        let dir = tempfile::tempdir().unwrap();
        let (sub, shared) = build_with(Sink::Pending(Vec::new()), LogLevel::Info);
        attach(&shared, dir.path(), 7).unwrap();
        tracing::subscriber::with_default(sub, || {
            tracing::info!("buffered");
        });
        let on_disk = |d: &Path| {
            std::fs::read_dir(d)
                .unwrap()
                .filter_map(Result::ok)
                .map(|e| std::fs::read_to_string(e.path()).unwrap())
                .collect::<String>()
        };
        assert!(!on_disk(dir.path()).contains("buffered"));
        lock(&shared.sink).flush();
        assert!(on_disk(dir.path()).contains("buffered"));
    }

    /// Writes the diagnostic file and parses it back.
    fn written(info: &DiagnosticInfo, lines: &[String]) -> bovnar::DomDoc {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("diag.bovnar");
        write_diagnostic(&file, info, lines).unwrap();
        let bytes = std::fs::read(&file).unwrap();
        assert_eq!(bovnar::json::detect(&bytes), bovnar::json::Format::Bovnar);
        bovnar::DomDoc::parse(&bytes).unwrap()
    }

    #[test]
    fn diagnostic_file_contains_no_sensitive_values() {
        let (dir, shared) = capture(LogLevel::Debug, || {
            tracing::info!(password = PASSWORD, "connecting");
            tracing::info!(server_password = %PASSWORD, "connecting");
            tracing::info!(text = MESSAGE, "chat received");
            tracing::info!(username = USER, user_id = 4, "user joined");
            tracing::info!(client_cert = PEM, "loaded identity");
            tracing::info!(blob = PEM, "unusual field name");
            tracing::info!("line one\nforged line");
        });
        let info = DiagnosticInfo::new("1.2.3");
        let file = dir.path().join("diag.txt");
        write_diagnostic(&file, &info, &shared.recent_lines()).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();

        for secret in [PASSWORD, MESSAGE, USER, "MIIFAKE", "BEGIN CERTIFICATE"] {
            assert!(!text.contains(secret), "leaked {secret}");
        }
        let doc = bovnar::DomDoc::parse(text.as_bytes()).unwrap();
        assert_eq!(doc.get("v").and_then(bovnar::DomNode::as_u8), Some(1));
        assert_eq!(
            doc.get("app_version").and_then(bovnar::DomNode::as_str),
            Some("1.2.3")
        );
        let log: Vec<&str> = (0..doc.get("log").unwrap().len())
            .filter_map(|i| doc.lookup("log").unwrap().at(i)?.as_str())
            .collect();
        assert!(log.iter().any(|l| l.contains("user_id=4")));
        assert!(log.iter().any(|l| l.contains("chat received")));
        assert!(log.iter().any(|l| l.contains("line one forged line")));

        let on_disk: String = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with(LOG_PREFIX))
            .map(|e| std::fs::read_to_string(e.path()).unwrap())
            .collect();
        assert!(on_disk.contains("chat received"));
        for secret in [PASSWORD, MESSAGE, USER, "MIIFAKE"] {
            assert!(!on_disk.contains(secret), "leaked {secret} to log file");
        }
    }

    #[test]
    fn diagnostic_file_lists_link_figures() {
        let mut info = DiagnosticInfo::new("1.2.3");
        info.connection = DiagConnectionStats {
            connected: true,
            transport: Some("udp".to_owned()),
            loss_percent: 12.5,
            send_bitrate_kbps: Some(40),
            server_max_bandwidth_kbps: Some(72),
            packets_sent: 7,
            packets_received: 9,
            audio: Some(crate::AudioHealthView {
                device_xruns: 3,
                concealed_frames: 5,
                jitter_delay_ms: 40,
                capture_time_avg_us: 250,
                capture_time_max_us: 900,
                realtime_threads: 2,
                rating: crate::audio_rating::AudioRatingView {
                    rating: crate::audio_rating::AudioRating::Fair,
                    issue: Some(crate::audio_rating::AudioIssue::Network),
                    window_secs: 120,
                    concealed_frames: 20,
                    received_frames: 980,
                    ..Default::default()
                },
                ..crate::AudioHealthView::default()
            }),
            ..DiagConnectionStats::default()
        };
        let doc = written(&info, &[]);
        let str_at = |path: &str| doc.lookup(path).and_then(bovnar::DomNode::as_str);
        let u64_at = |path: &str| doc.lookup(path).and_then(bovnar::DomNode::as_u64);
        assert_eq!(str_at("connection.transport"), Some("udp"));
        assert_eq!(
            doc.lookup("connection.loss_percent")
                .and_then(bovnar::DomNode::as_float),
            Some(12.5)
        );
        assert!(doc.lookup("connection.ping_ms").unwrap().is_null());
        for (path, value) in [
            ("connection.send_bitrate_kbps", 40),
            ("connection.server_max_bandwidth_kbps", 72),
            ("connection.packets_sent", 7),
            ("connection.packets_received", 9),
            ("audio.realtime_threads", 2),
            ("audio.rating.window_secs", 120),
            ("audio.rating.concealed_frames", 20),
            ("audio.rating.received_frames", 980),
            ("audio.device_xruns", 3),
            ("audio.concealed_frames", 5),
            ("audio.jitter_delay_ms", 40),
            ("audio.capture_time_max_us", 900),
        ] {
            assert_eq!(u64_at(path), Some(value), "{path}");
        }
        assert_eq!(str_at("audio.rating.rating"), Some("Fair"));
        assert_eq!(str_at("audio.rating.issue"), Some("Network"));
    }

    #[test]
    fn diagnostic_file_lists_audio_devices() {
        let mut info = DiagnosticInfo::new("1.2.3");
        info.audio_devices = vec![
            DiagAudioDevice {
                name: "MacBook Pro Microphone".to_owned(),
                is_input: true,
                is_default: true,
            },
            DiagAudioDevice {
                name: "MacBook Pro Speakers".to_owned(),
                is_input: false,
                is_default: false,
            },
        ];
        let doc = written(&info, &[]);
        let devices = doc.get("audio_devices").unwrap();
        assert_eq!(devices.len(), 2);
        let mic = devices.at(0).unwrap();
        assert_eq!(
            mic.get("name").and_then(bovnar::DomNode::as_str),
            Some("MacBook Pro Microphone")
        );
        assert_eq!(
            mic.get("input").and_then(bovnar::DomNode::as_bool),
            Some(true)
        );
        assert_eq!(
            mic.get("default").and_then(bovnar::DomNode::as_bool),
            Some(true)
        );
        let speakers = devices.at(1).unwrap();
        assert_eq!(
            speakers.get("input").and_then(bovnar::DomNode::as_bool),
            Some(false)
        );
        assert!(doc.get("audio").unwrap().is_null());
    }

    #[test]
    fn diagnostic_file_survives_control_characters() {
        let mut info = DiagnosticInfo::new("1.2.3");
        info.audio_devices = vec![DiagAudioDevice {
            name: "Mic\u{1b}[31m".to_owned(),
            is_input: true,
            is_default: false,
        }];
        let doc = written(&info, &["bell\u{7}line".to_owned()]);
        assert_eq!(
            doc.lookup("log")
                .unwrap()
                .at(0)
                .and_then(bovnar::DomNode::as_str),
            Some("bell line")
        );
        assert_eq!(
            doc.lookup("audio_devices")
                .unwrap()
                .at(0)
                .and_then(|d| d.get("name"))
                .and_then(bovnar::DomNode::as_str),
            Some("Mic [31m")
        );
    }

    #[test]
    fn level_filters_events() {
        let (_dir, shared) = capture(LogLevel::Warn, || {
            tracing::info!("quiet");
            tracing::warn!("loud");
        });
        let lines = shared.recent_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("loud"));
    }

    #[test]
    fn ring_is_bounded() {
        let (_dir, shared) = capture(LogLevel::Info, || {
            for i in 0..RING_CAPACITY + 10 {
                tracing::info!(n = i, "tick");
            }
        });
        let lines = shared.recent_lines();
        assert_eq!(lines.len(), RING_CAPACITY);
        assert!(lines[0].contains("n=10"));
    }

    #[test]
    fn old_log_files_are_pruned() {
        let dir = tempfile::tempdir().unwrap();
        for day in 1..=9 {
            std::fs::write(
                dir.path().join(format!("mumcord.2020-01-0{day}.log")),
                "old",
            )
            .unwrap();
        }
        let _ = open_appender(dir.path(), 3).unwrap();
        let count = std::fs::read_dir(dir.path()).unwrap().count();
        assert!(count <= 3, "{count} files kept");
    }

    #[test]
    fn parses_levels() {
        assert_eq!("debug".parse::<LogLevel>(), Ok(LogLevel::Debug));
        assert!("loud".parse::<LogLevel>().is_err());
    }
}
