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
use std::io::{self, Write as _};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

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
struct DiagLayer {
    shared: Arc<Shared>,
    file: Mutex<RollingFileAppender>,
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
        line.push('\n');
        if let Err(err) = lock(&self.file).write_all(line.as_bytes()) {
            // Stderr is the only place left when the log file itself fails.
            eprintln!("mumcord: writing log file failed: {err}");
        }
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

fn build(
    logs_dir: &Path,
    level: LogLevel,
    max_files: usize,
) -> anyhow::Result<(impl Subscriber + Send + Sync + 'static, Arc<Shared>)> {
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_PREFIX)
        .filename_suffix(LOG_SUFFIX)
        .max_log_files(max_files)
        .build(logs_dir)?;
    let (filter, level_handle) = reload::Layer::new(level.filter());
    let shared = Arc::new(Shared {
        ring: Mutex::new(VecDeque::with_capacity(RING_CAPACITY)),
        level: level_handle,
    });
    let layer = DiagLayer {
        shared: Arc::clone(&shared),
        file: Mutex::new(appender),
    };
    Ok((Registry::default().with(filter).with(layer), shared))
}

/// Starts logging into `logs_dir` (created if missing) as the global tracing subscriber.
/// Keeps at most `max_files` daily files. Call once at app start.
pub fn init(logs_dir: &Path, level: LogLevel, max_files: usize) -> anyhow::Result<()> {
    std::fs::create_dir_all(logs_dir)?;
    let (subscriber, shared) = build(logs_dir, level, max_files)?;
    tracing::subscriber::set_global_default(subscriber)?;
    GLOBAL
        .set(shared)
        .map_err(|_| anyhow::anyhow!("logging already initialised"))
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

fn write_audio_health(out: &mut String, audio: Option<&crate::AudioHealthView>) {
    let _ = writeln!(out, "\nAudio:");
    let Some(a) = audio else {
        let _ = writeln!(out, "  (voice not running)");
        return;
    };
    let r = &a.rating;
    let issue = r.issue.map_or(String::new(), |i| format!(" ({i:?})"));
    let _ = writeln!(
        out,
        "  rating: {:?}{issue} over the last {} s: dropouts {}, late frames {}, \
         concealed {} of {} received frames, processing max {} µs",
        r.rating,
        r.window_secs,
        r.dropouts,
        r.late_frames,
        r.concealed_frames,
        r.received_frames,
        r.processing_max_us
    );
    let _ = writeln!(out, "  real-time threads: {}", a.realtime_threads);
    let _ = writeln!(
        out,
        "  device xruns / stream restarts: {} / {}",
        a.device_xruns, a.stream_restarts
    );
    let _ = writeln!(
        out,
        "  capture overruns / playback underruns: {} / {}",
        a.capture_overruns, a.playback_underruns
    );
    let _ = writeln!(
        out,
        "  late capture frames / late playback wake-ups: {} / {}",
        a.capture_late_frames, a.playback_late_wakeups
    );
    let _ = writeln!(
        out,
        "  concealed frames / late packets: {} / {}",
        a.concealed_frames, a.late_packets
    );
    let _ = writeln!(out, "  jitter buffer: {} ms", a.jitter_delay_ms);
    let _ = writeln!(
        out,
        "  capture time avg / max: {} / {} µs",
        a.capture_time_avg_us, a.capture_time_max_us
    );
    let _ = writeln!(
        out,
        "  mix time avg / max: {} / {} µs",
        a.mix_time_avg_us, a.mix_time_max_us
    );
}

fn write_diagnostic(path: &Path, info: &DiagnosticInfo, lines: &[String]) -> io::Result<()> {
    let mut out = String::new();
    let _ = writeln!(out, "Mumcord diagnostic file");
    let _ = writeln!(out, "App version: {}", info.app_version);
    let _ = writeln!(out, "OS: {}", info.os);
    let _ = writeln!(out, "\nAudio devices:");
    if info.audio_devices.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for d in &info.audio_devices {
        let _ = writeln!(
            out,
            "  {} {}{}",
            if d.is_input { "input: " } else { "output:" },
            scrub_value(&d.name),
            if d.is_default { " (default)" } else { "" }
        );
    }
    let c = &info.connection;
    let _ = writeln!(out, "\nConnection:");
    let _ = writeln!(out, "  connected: {}", c.connected);
    let _ = writeln!(
        out,
        "  transport: {}",
        c.transport.as_deref().unwrap_or("-")
    );
    match c.ping_ms {
        Some(ping) => {
            let _ = writeln!(out, "  ping: {ping:.1} ms");
        }
        None => {
            let _ = writeln!(out, "  ping: -");
        }
    }
    let _ = writeln!(
        out,
        "  packets good/late/lost: {}/{}/{}",
        c.packets_good, c.packets_late, c.packets_lost
    );
    let _ = writeln!(out, "  loss: {:.1} %", c.loss_percent);
    let kbps = |v: Option<u32>| v.map_or_else(|| "-".to_owned(), |v| format!("{v} kbit/s"));
    let _ = writeln!(out, "  send bitrate: {}", kbps(c.send_bitrate_kbps));
    let _ = writeln!(
        out,
        "  server max bandwidth: {}",
        kbps(c.server_max_bandwidth_kbps)
    );
    let _ = writeln!(
        out,
        "  packets sent/received: {}/{}",
        c.packets_sent, c.packets_received
    );
    let _ = writeln!(out, "  reconnects: {}", c.reconnect_count);
    write_audio_health(&mut out, c.audio.as_ref());
    let _ = writeln!(out, "\nRecent log lines:");
    for line in lines {
        let _ = writeln!(out, "{}", scrub_value(line));
    }
    std::fs::write(path, out)
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
        let (sub, shared) = build(dir.path(), level, 7).unwrap();
        tracing::subscriber::with_default(sub, f);
        (dir, shared)
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
        assert!(text.contains("App version: 1.2.3"));
        assert!(text.contains("user_id=4"));
        assert!(text.contains("chat received"));
        assert!(text.contains("line one forged line"));

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
        let dir = tempfile::tempdir().unwrap();
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
        let file = dir.path().join("diag.txt");
        write_diagnostic(&file, &info, &[]).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        for line in [
            "transport: udp",
            "loss: 12.5 %",
            "send bitrate: 40 kbit/s",
            "server max bandwidth: 72 kbit/s",
            "real-time threads: 2",
            "rating: Fair (Network) over the last 120 s: dropouts 0, late frames 0, concealed 20 of 980 received frames",
            "device xruns / stream restarts: 3 / 0",
            "concealed frames / late packets: 5 / 0",
            "jitter buffer: 40 ms",
            "capture time avg / max: 250 / 900 µs",
            "packets sent/received: 7/9",
        ] {
            assert!(text.contains(line), "missing {line}");
        }
    }

    #[test]
    fn diagnostic_file_lists_audio_devices() {
        let dir = tempfile::tempdir().unwrap();
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
        let file = dir.path().join("diag.txt");
        write_diagnostic(&file, &info, &[]).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("input:  MacBook Pro Microphone (default)"));
        assert!(text.contains("output: MacBook Pro Speakers\n"));
        assert!(!text.contains("(none)"));
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
        let _ = build(dir.path(), LogLevel::Info, 3).unwrap();
        let count = std::fs::read_dir(dir.path()).unwrap().count();
        assert!(count <= 3, "{count} files kept");
    }

    #[test]
    fn parses_levels() {
        assert_eq!("debug".parse::<LogLevel>(), Ok(LogLevel::Debug));
        assert!("loud".parse::<LogLevel>().is_err());
    }
}
