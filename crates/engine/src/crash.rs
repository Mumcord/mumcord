//! Crash reports (#568): a Rust panic writes a report right away; a native crash (or a forced
//! quit) is noticed at the next start because the previous run's marker file is still there.
//! Reports are plain text in `crash-reports/` next to `logs/`, never uploaded, at most
//! [`MAX_REPORTS`] kept. They hold the version, OS, cause, backtrace and the last log lines,
//! which are redacted like the log itself; the home folder is shortened to `~`.
//! On Linux a real-time thread that overruns its time budget leaves a report too, although the
//! app keeps running (see `report_overrun`).

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::diag;

#[cfg(target_os = "linux")]
mod exit_reason;

/// How many reports stay; older ones are deleted.
pub const MAX_REPORTS: usize = 10;
/// Log lines a report carries.
const LOG_LINES: usize = 200;
const MARKER: &str = "running";
const SEEN: &str = "seen";
/// Ids of reports the user uploaded.
const UPLOADED: &str = "uploaded";
/// Where the Upload button sends one report (#573); the server keeps it for 90 days.
pub const UPLOAD_URL: &str = "https://crash.mumcord.app/api/v1/reports";
/// What was running, kept on disk too so the report of a native crash can show it next start.
const CONTEXT: &str = "context";
/// The loaded libraries at a fatal signal, for naming the native stack at the next start.
const MAPS: &str = "maps";
const PREFIX: &str = "crash-";
const SUFFIX: &str = ".txt";

/// What ended the run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CrashKind {
    Panic,
    /// A signal such as SIGSEGV, caught by the native handler.
    Signal(String),
    /// The previous run left its marker without a signal: killed, power loss or a crash the
    /// handler could not see.
    Unexpected,
    /// Not a crash: a real-time thread ran past its time budget (SIGXCPU) and the app kept
    /// running. The report says where that thread was busy.
    Overrun,
}

impl CrashKind {
    fn as_str(&self) -> String {
        match self {
            Self::Panic => "panic".to_owned(),
            Self::Signal(name) => format!("signal {name}"),
            Self::Unexpected => "unexpected exit".to_owned(),
            Self::Overrun => "real-time overrun".to_owned(),
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "panic" => Self::Panic,
            "unexpected exit" => Self::Unexpected,
            "real-time overrun" => Self::Overrun,
            other => Self::Signal(other.trim_start_matches("signal ").to_owned()),
        }
    }
}

/// One saved report, as the crash list shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrashSummary {
    /// The file name without folder; also the id the UI passes back.
    pub id: String,
    pub created_unix: u64,
    pub version: String,
    pub kind: CrashKind,
    /// First line of the cause, e.g. the panic message.
    pub cause: String,
    /// Sent with the Upload button.
    pub uploaded: bool,
}

struct State {
    dir: PathBuf,
    version: String,
    started: Instant,
}

static STATE: OnceLock<State> = OnceLock::new();
/// Gathered once in the background at start, so a panic report needs no tools of its own.
static SYSTEM: OnceLock<Vec<String>> = OnceLock::new();

/// What is running right now (camera, screen share, install type, …), see [`set_context`].
static RUNNING: Mutex<BTreeMap<&'static str, String>> = Mutex::new(BTreeMap::new());
/// Last time the context file was written, so rapid changes write at most once per second
/// (spec 12 FR-12.12). The in-memory [`RUNNING`] map stays exact for the panic hook.
static LAST_CONTEXT_WRITE: Mutex<Option<Instant>> = Mutex::new(None);

/// Records what is going on, e.g. `set_context("camera", "on, NV12 1280x720@15")`, so a crash
/// report can say what was running. Values must hold nothing personal (no names, URLs, paths).
pub fn set_context(key: &'static str, value: impl Into<String>) {
    let value = value.into();
    if let Ok(mut running) = RUNNING.lock() {
        running.insert(key, value);
        persist_context(&running);
    }
}

pub fn clear_context(key: &'static str) {
    if let Ok(mut running) = RUNNING.lock()
        && running.remove(key).is_some()
    {
        persist_context(&running);
    }
}

fn persist_context(running: &BTreeMap<&'static str, String>) {
    let Some(state) = STATE.get() else {
        return;
    };
    // Debounce: at most one file write per second (spec 12 FR-12.12).
    let now = Instant::now();
    let mut last = LAST_CONTEXT_WRITE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if last.is_some_and(|at| now.duration_since(at) < Duration::from_secs(1)) {
        return;
    }
    *last = Some(now);
    let _ = fs::write(state.dir.join(CONTEXT), context_text(running));
}

fn context_text(running: &BTreeMap<&'static str, String>) -> String {
    running
        .iter()
        .map(|(k, v)| format!("{k}: {}\n", v.replace(['\n', '\r'], " ")))
        .collect()
}

/// The context lines for a report. The panic hook must not block: a panic while the lock is
/// held leaves the section out.
fn context_now() -> String {
    RUNNING
        .try_lock()
        .map(|r| context_text(&r))
        .unwrap_or_default()
}

fn system_lines() -> &'static [String] {
    SYSTEM.get().map(Vec::as_slice).unwrap_or_default()
}

/// Sets up crash reporting for this run: records a report for an unclean previous run, installs
/// the panic hook and the native signal handler, and leaves a marker until [`clean_exit`].
pub fn install(dir: &Path, version: &str) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let marker = dir.join(MARKER);
    let left = fs::read_to_string(&marker).ok();
    let native = left.as_deref().and_then(parse_marker);
    let unclean = left.map(|_| match &native {
        Some(crash) => CrashKind::Signal(crash.signal.clone()),
        None => CrashKind::Unexpected,
    });
    let previous = unclean.as_ref().map(|_| previous_log_tail(dir));
    let thread = native
        .as_ref()
        .and_then(|crash| crash.thread.clone())
        .unwrap_or_else(|| "-".to_owned());
    // An overrun the previous run recorded but could not report any more (it was ended first).
    #[cfg(target_os = "linux")]
    let previous_overrun = overrun::take_record(dir).map(|record| (record, previous_log_tail(dir)));
    let previous_context = fs::read_to_string(dir.join(CONTEXT)).unwrap_or_default();
    let _ = fs::remove_file(dir.join(CONTEXT));
    // Read before this run notes its own details in the same file.
    #[cfg(target_os = "linux")]
    let previous_run = exit_reason::Run::load(dir);
    // The libraries still sit where the crashed run had them on disk, so the stack can be
    // named now; the mapping file is reused for this run's handler.
    let stack = native
        .as_ref()
        .map(|crash| {
            let maps = fs::read_to_string(dir.join(MAPS)).unwrap_or_default();
            let mut lines = Vec::new();
            if let Some(fault) = crash.fault {
                lines.push(format!("faulting address: 0x{fault:x}"));
            }
            lines.extend(symbolize(&crash.ips, &maps));
            lines.join("\n")
        })
        .unwrap_or_default();
    {
        // The system details take a few tool calls; the report of an unclean previous run waits
        // for them, the start does not.
        let (dir, version) = (dir.to_path_buf(), version.to_owned());
        let _ = std::thread::Builder::new()
            .name("crash-sysinfo".to_owned())
            .spawn(move || {
                let _ = SYSTEM.set(crate::sysinfo::collect());
                if let (Some(kind), Some(previous)) = (unclean, previous) {
                    let cause = match &kind {
                        CrashKind::Signal(name) => format!("Mumcord was stopped by {name}"),
                        _ => unexpected_cause(cfg!(windows)).to_owned(),
                    };
                    // A run that left no record was ended in a way the app could not see; on
                    // Linux the system journal often knows how.
                    #[cfg(target_os = "linux")]
                    let ended_by = (kind == CrashKind::Unexpected)
                        .then(|| exit_reason::ended_by(previous_run.as_ref()));
                    #[cfg(not(target_os = "linux"))]
                    let ended_by: Option<String> = None;
                    let report = Report {
                        kind,
                        thread: &thread,
                        cause: &cause,
                        backtrace: &stack,
                        log: &previous,
                        context: &previous_context,
                        uptime_secs: None,
                        ended_by: ended_by.as_deref(),
                    };
                    let _ = write_report(&dir, &version, &report);
                    let _ = prune(&dir);
                }
                #[cfg(target_os = "linux")]
                if let Some((record, log)) = previous_overrun {
                    let cause = "A real-time thread ran past its time budget in the previous run, \
                                 which ended before this report was written";
                    let _ = overrun::write(
                        &dir,
                        &version,
                        &record,
                        cause,
                        &log,
                        &previous_context,
                        None,
                    );
                }
            });
    }
    fs::write(&marker, b"")?;
    #[cfg(target_os = "linux")]
    if let Some(run) = exit_reason::Run::current() {
        run.save(dir);
    }
    prune(dir)?;
    let _ = STATE.set(State {
        dir: dir.to_path_buf(),
        version: version.to_owned(),
        started: Instant::now(),
    });
    install_panic_hook();
    native::install(&marker, &dir.join(MAPS));
    #[cfg(target_os = "linux")]
    overrun::install(dir);
    Ok(())
}

/// Writes the report of a real-time overrun the signal handler recorded (see the `overrun` module), at
/// most once per run; later overruns are only counted and logged. Costs one atomic read when
/// there is nothing to report, and the writing happens on its own thread, so it can be polled.
#[cfg(target_os = "linux")]
pub fn report_overrun() {
    let Some(state) = STATE.get() else {
        return;
    };
    if !overrun::claim() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("overrun-report".to_owned())
        .spawn(|| {
            let Some(record) = overrun::take_record(&state.dir) else {
                return;
            };
            let cause = "A real-time thread ran past its time budget; Mumcord kept running \
                         (not a crash)";
            let uptime = Some(state.started.elapsed().as_secs());
            let log = diag::recent_log_lines();
            if let Err(e) = overrun::write(
                &state.dir,
                &state.version,
                &record,
                cause,
                &log,
                &context_now(),
                uptime,
            ) {
                tracing::warn!(error = %e, "writing the real-time overrun report failed");
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "no thread for the real-time overrun report");
    }
}

/// The cause of a run that left its marker but no native record. On Windows every exception the
/// handler can see is recorded (issue #699), so a missing record itself says how the run ended.
fn unexpected_cause(windows: bool) -> &'static str {
    if windows {
        "Mumcord closed without shutting down (no exception recorded: likely a fast-fail such as \
         a Rust abort or a stack-cookie check, or the process was ended from outside)"
    } else {
        "Mumcord closed without shutting down"
    }
}

/// Runs `quit` once, on its own thread, when the system asks the app to end (Unix: SIGTERM,
/// SIGINT or SIGHUP, i.e. shutdown, logout, `kill` or Ctrl+C), instead of the process ending at
/// once: `quit` is expected to shut down as a normal quit does and then exit (#748). A second
/// such signal ends the process immediately. Either way the run does not count as a crash.
/// Call it once.
pub fn on_quit_signal(quit: impl FnOnce() + Send + 'static) {
    if let Err(e) = native::on_quit(quit) {
        tracing::warn!(error = %e, "no clean quit when the system ends the app");
    }
}

/// The app is closing normally: the next start does not report a crash.
pub fn clean_exit() {
    if let Some(state) = STATE.get() {
        let _ = fs::remove_file(state.dir.join(MARKER));
        let _ = fs::remove_file(state.dir.join(CONTEXT));
        let _ = fs::remove_file(state.dir.join(MAPS));
        #[cfg(target_os = "linux")]
        {
            let _ = fs::remove_file(state.dir.join(exit_reason::RUN));
            overrun::remove_record(&state.dir);
        }
    }
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(state) = STATE.get() {
            // Push buffered log lines to the file too (spec 12 FR-12.1); the report below
            // already uses the exact in-memory ring.
            crate::diag::flush_log();
            let thread = std::thread::current();
            let message = match info.payload().downcast_ref::<&str>() {
                Some(s) => (*s).to_owned(),
                None => info
                    .payload()
                    .downcast_ref::<String>()
                    .cloned()
                    .unwrap_or_else(|| "panic".to_owned()),
            };
            let location = info
                .location()
                .map(|l| format!(" at {}:{}", l.file(), l.line()))
                .unwrap_or_default();
            let backtrace = std::backtrace::Backtrace::force_capture().to_string();
            let cause = format!("{message}{location}");
            let log = diag::recent_log_lines();
            let context = context_now();
            let report = Report {
                kind: CrashKind::Panic,
                thread: thread.name().unwrap_or("unnamed"),
                cause: &cause,
                backtrace: &backtrace,
                log: &log,
                context: &context,
                uptime_secs: Some(state.started.elapsed().as_secs()),
                ended_by: None,
            };
            let _ = write_report(&state.dir, &state.version, &report);
        }
        previous(info);
    }));
}

/// The last lines of the newest log file (the previous run's, since this run has not written
/// much yet).
fn previous_log_tail(crash_dir: &Path) -> Vec<String> {
    let Some(logs) = crash_dir.parent().map(|p| p.join("logs")) else {
        return Vec::new();
    };
    let newest = fs::read_dir(logs).ok().and_then(|entries| {
        entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".log"))
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
    });
    let Some(text) = newest.and_then(|e| fs::read_to_string(e.path()).ok()) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(LOG_LINES)..]
        .iter()
        .map(|l| (*l).to_owned())
        .collect()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// `2026-11-06 21:51 UTC` from seconds since 1970 (the civil-from-days algorithm by Howard
/// Hinnant), so a report reads the same in every time zone.
fn utc(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or_default();
    let (hour, minute) = (secs % 86_400 / 3600, secs % 3600 / 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
}

/// Replaces the home folder with `~`, so user names in paths stay out of reports.
fn shorten_home(text: &str) -> String {
    match std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        Some(home) if !home.is_empty() => text.replace(&*home.to_string_lossy(), "~"),
        _ => text.to_owned(),
    }
}

/// What goes into one report besides the version and the system details.
struct Report<'a> {
    kind: CrashKind,
    thread: &'a str,
    cause: &'a str,
    backtrace: &'a str,
    log: &'a [String],
    /// What was running, one `key: value` per line.
    context: &'a str,
    uptime_secs: Option<u64>,
    /// What ended a run that left no record, as far as the system journal knows (Linux).
    ended_by: Option<&'a str>,
}

fn write_report(dir: &Path, version: &str, report: &Report<'_>) -> io::Result<PathBuf> {
    let Report {
        kind,
        thread,
        cause,
        backtrace,
        log,
        context,
        uptime_secs,
        ended_by,
    } = report;
    let created = now_unix();
    let mut path = dir.join(format!("{PREFIX}{created}{SUFFIX}"));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{PREFIX}{created}-{n}{SUFFIX}"));
        n += 1;
    }
    let mut text = String::new();
    text.push_str("Mumcord crash report\n");
    text.push_str(&format!("version: {version}\n"));
    text.push_str(&format!("time: {created} ({})\n", utc(created)));
    text.push_str(&format!("kind: {}\n", kind.as_str()));
    text.push_str(&format!("thread: {thread}\n"));
    text.push_str(&format!("cause: {}\n", cause.replace(['\n', '\r'], " ")));
    if let Some(secs) = *uptime_secs {
        text.push_str(&format!(
            "uptime: {}h {:02}m {:02}s\n",
            secs / 3600,
            secs / 60 % 60,
            secs % 60
        ));
    }
    if let Some(ended_by) = ended_by {
        text.push_str(&format!("ended by: {ended_by}\n"));
    }
    if !context.is_empty() {
        text.push_str("\nrunning:\n");
        text.push_str(context);
    }
    if !system_lines().is_empty() {
        text.push_str("\nsystem:\n");
        for line in system_lines() {
            text.push_str(line);
            text.push('\n');
        }
    }
    if !backtrace.is_empty() {
        text.push_str("\nbacktrace:\n");
        text.push_str(backtrace);
        text.push('\n');
    }
    text.push_str("\nlast log lines:\n");
    let start = log.len().saturating_sub(LOG_LINES);
    for line in &log[start..] {
        text.push_str(line);
        text.push('\n');
    }
    let mut file = fs::File::create(&path)?;
    file.write_all(shorten_home(&text).as_bytes())?;
    Ok(path)
}

fn report_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| is_report_name(p))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

fn is_report_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with(PREFIX) && n.ends_with(SUFFIX))
}

fn prune(dir: &Path) -> io::Result<()> {
    let files = report_files(dir);
    let excess = files.len().saturating_sub(MAX_REPORTS);
    for old in &files[..excess] {
        fs::remove_file(old)?;
    }
    Ok(())
}

fn summary(path: &Path) -> Option<CrashSummary> {
    let text = fs::read_to_string(path).ok()?;
    let field = |name: &str| {
        text.lines()
            .take_while(|l| !l.is_empty())
            .find_map(|l| l.strip_prefix(&format!("{name}: ")))
            .unwrap_or_default()
            .to_owned()
    };
    Some(CrashSummary {
        id: path.file_name()?.to_str()?.to_owned(),
        created_unix: field("time")
            .split_whitespace()
            .next()
            .and_then(|t| t.parse().ok())
            .unwrap_or_default(),
        version: field("version"),
        kind: CrashKind::parse(&field("kind")),
        cause: field("cause"),
        uploaded: false,
    })
}

/// The saved reports, newest first.
pub fn list(dir: &Path) -> Vec<CrashSummary> {
    let uploaded = fs::read_to_string(dir.join(UPLOADED)).unwrap_or_default();
    let mut all: Vec<CrashSummary> = report_files(dir)
        .iter()
        .filter_map(|p| summary(p))
        .map(|mut s| {
            s.uploaded = uploaded.lines().any(|l| l == s.id);
            s
        })
        .collect();
    all.sort_by(|a, b| b.created_unix.cmp(&a.created_unix).then(b.id.cmp(&a.id)));
    all
}

/// A report's path, refusing anything that is not a report file in `dir`.
fn report_path(dir: &Path, id: &str) -> io::Result<PathBuf> {
    let path = dir.join(id);
    if id.contains(['/', '\\']) || !is_report_name(&path) || !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no such crash report",
        ));
    }
    Ok(path)
}

/// The full text of one report.
pub fn read(dir: &Path, id: &str) -> io::Result<String> {
    fs::read_to_string(report_path(dir, id)?)
}

pub fn delete(dir: &Path, id: &str) -> io::Result<()> {
    fs::remove_file(report_path(dir, id)?)
}

pub fn delete_all(dir: &Path) -> io::Result<()> {
    for path in report_files(dir) {
        fs::remove_file(path)?;
    }
    let _ = fs::remove_file(dir.join(SEEN));
    Ok(())
}

/// What the Upload button sends: the report unchanged, plus the user's comment as a last
/// `comment:` section when there is one.
pub fn upload_body(dir: &Path, id: &str, comment: &str) -> io::Result<String> {
    let mut body = read(dir, id)?;
    let comment = comment.trim();
    if !comment.is_empty() {
        if !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str("\ncomment:\n");
        body.push_str(&shorten_home(comment));
        body.push('\n');
    }
    Ok(body)
}

/// Sends one report after the user confirmed it (#573). Only this function uploads, and only
/// the report it is given; nothing is sent in the background.
pub async fn upload(dir: &Path, id: &str, comment: &str) -> Result<(), String> {
    let body = upload_body(dir, id, comment).map_err(|e| e.to_string())?;
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let tls = {
        use rustls_platform_verifier::BuilderVerifierExt;
        rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .and_then(|b| b.with_platform_verifier())
            .map_err(|e| e.to_string())?
            .with_no_client_auth()
    };
    let client = reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(UPLOAD_URL)
        .header(reqwest::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("the crash server answered {}", response.status()));
    }
    let path = dir.join(UPLOADED);
    let mut ids = fs::read_to_string(&path).unwrap_or_default();
    if !ids.lines().any(|l| l == id) {
        ids.push_str(id);
        ids.push('\n');
        fs::write(path, ids).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Writes the given reports (all when `ids` is empty) into one zip file.
pub fn export(dir: &Path, ids: &[String], dest: &Path) -> io::Result<()> {
    let paths: Vec<PathBuf> = if ids.is_empty() {
        report_files(dir)
    } else {
        ids.iter()
            .map(|id| report_path(dir, id))
            .collect::<io::Result<_>>()?
    };
    let mut zip = zip::ZipWriter::new(fs::File::create(dest)?);
    let options = zip::write::SimpleFileOptions::default();
    for path in paths {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("crash.txt");
        zip.start_file(name, options).map_err(io::Error::other)?;
        zip.write_all(&fs::read(&path)?)?;
    }
    zip.finish().map_err(io::Error::other)?;
    Ok(())
}

/// Crashes the notice has not told about yet; all reports count as shown afterwards. An
/// unexpected exit without a signal (killed, power loss) is listed in Settings but brings no
/// notice: it is mostly not Mumcord's fault, and a notice after every forced quit would mislead.
/// Neither does a real-time overrun: the app kept running.
pub fn take_unseen(dir: &Path) -> Vec<CrashSummary> {
    let seen_path = dir.join(SEEN);
    let seen = fs::read_to_string(&seen_path).unwrap_or_default();
    let all = list(dir);
    let unseen: Vec<CrashSummary> = all
        .iter()
        .filter(|s| !seen.lines().any(|l| l == s.id))
        .filter(|s| !matches!(s.kind, CrashKind::Unexpected | CrashKind::Overrun))
        .cloned()
        .collect();
    let ids: Vec<&str> = all.iter().map(|s| s.id.as_str()).collect();
    let _ = fs::write(&seen_path, ids.join("\n"));
    unseen
}

/// The native handler (Unix). At a fatal signal it writes, with async-signal-safe calls only
/// (no allocation, no locks), into the marker: the signal name, the faulting address and up to
/// [`native::MAX_FRAMES`] return addresses; and it copies `/proc/self/maps` next to it. The next
/// start turns the addresses into library and symbol names ([`symbolize`]). Then the default
/// action ends the process as before.
#[cfg(unix)]
mod native {
    use std::os::fd::IntoRawFd;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    pub const MAX_FRAMES: usize = 64;
    static MARKER_FD: AtomicI32 = AtomicI32::new(-1);
    static MAPS_FD: AtomicI32 = AtomicI32::new(-1);
    static MARKER_PATH: std::sync::OnceLock<std::ffi::CString> = std::sync::OnceLock::new();
    /// Asked to quit (the system shuts down, the session ends, Ctrl+C): not a crash.
    const QUIT_SIGNALS: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP];

    /// Write end of the pipe that wakes the quit thread; -1 until [`on_quit`] ran.
    static QUIT_FD: AtomicI32 = AtomicI32::new(-1);
    /// A quit signal was already handed to the quit thread.
    static QUITTING: AtomicBool = AtomicBool::new(false);

    /// A quit request: the run ends on purpose, so its marker goes and the next start reports
    /// nothing. The first one wakes the quit thread, which runs the normal quit path (plugins
    /// get their `stop`, the connections close; #748). A second one, or one before anybody
    /// listens, ends the process through the default action, so a hanging quit can be forced.
    extern "C" fn quit(signal: libc::c_int) {
        // The process goes on after this handler, so the code it interrupted must find its
        // errno untouched.
        // SAFETY: the errno location is valid for the calling thread.
        let errno = unsafe { *errno_location() };
        if let Some(path) = MARKER_PATH.get() {
            // SAFETY: unlink(2) is async-signal-safe; the path was set before the handler.
            unsafe {
                libc::unlink(path.as_ptr());
            }
        }
        let fd = QUIT_FD.load(Ordering::Acquire);
        if fd >= 0 && !QUITTING.swap(true, Ordering::AcqRel) {
            put(fd, &[0]);
            // SAFETY: as above.
            unsafe {
                *errno_location() = errno;
            }
            return;
        }
        // SAFETY: raise(3) is async-signal-safe; SA_RESETHAND restored the default action.
        unsafe {
            libc::raise(signal);
        }
    }

    pub(super) fn errno_location() -> *mut libc::c_int {
        // SAFETY: these only return the address of the calling thread's errno.
        unsafe {
            #[cfg(target_os = "linux")]
            return libc::__errno_location();
            #[cfg(target_os = "android")]
            return libc::__errno();
            // The BSDs spell it `__error` like macOS does.
            #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
            return libc::__error();
        }
    }

    fn install_quit_handlers() {
        for signal in QUIT_SIGNALS {
            // SAFETY: a plain C handler installed with sigaction; it only uses async-signal-safe
            // calls (unlink, write, raise) and atomics.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = quit as extern "C" fn(libc::c_int) as libc::sighandler_t;
                action.sa_flags = libc::SA_RESETHAND;
                libc::sigemptyset(&mut action.sa_mask);
                libc::sigaction(signal, &action, std::ptr::null_mut());
            }
        }
    }

    /// Runs `run` on its own thread at the first quit signal. A signal handler may not do more
    /// than a few system calls, so it only writes a byte into a pipe this thread waits on.
    pub fn on_quit(run: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
        let mut fds: [libc::c_int; 2] = [-1; 2];
        // SAFETY: pipe(2) fills the two descriptors; fcntl only sets a flag on them.
        unsafe {
            if libc::pipe(fds.as_mut_ptr()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            for fd in fds {
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
            }
        }
        let [read, write] = fds;
        std::thread::Builder::new()
            .name("quit-signal".to_owned())
            .spawn(move || {
                let mut byte = 0u8;
                loop {
                    // SAFETY: reads at most one byte into a one-byte buffer.
                    let got = unsafe { libc::read(read, std::ptr::from_mut(&mut byte).cast(), 1) };
                    if got == 1 {
                        run();
                        return;
                    }
                    let interrupted = got < 0
                        && std::io::Error::last_os_error().kind()
                            == std::io::ErrorKind::Interrupted;
                    if !interrupted {
                        return;
                    }
                }
            })?;
        QUIT_FD.store(write, Ordering::Release);
        // Again, in case crash reporting could not be set up and left them out.
        install_quit_handlers();
        Ok(())
    }

    const SIGNALS: [(libc::c_int, &[u8]); 5] = [
        (libc::SIGSEGV, b"SIGSEGV"),
        (libc::SIGILL, b"SIGILL"),
        (libc::SIGBUS, b"SIGBUS"),
        (libc::SIGFPE, b"SIGFPE"),
        (libc::SIGABRT, b"SIGABRT"),
    ];

    pub fn install(marker: &Path, maps: &Path) {
        let Ok(file) = std::fs::OpenOptions::new().write(true).open(marker) else {
            return;
        };
        if let Ok(path) = std::ffi::CString::new(marker.as_os_str().as_encoded_bytes()) {
            let _ = MARKER_PATH.set(path);
        }
        install_quit_handlers();
        MARKER_FD.store(file.into_raw_fd(), Ordering::Release);
        if let Ok(file) = std::fs::File::create(maps) {
            MAPS_FD.store(file.into_raw_fd(), Ordering::Release);
        }
        for (signal, _) in SIGNALS {
            // SAFETY: a plain C handler installed with sigaction; it only uses async-signal-safe
            // calls (write, open, read, close, raise) plus the unwinder, which crash reporters
            // commonly call from such handlers. SA_ONSTACK uses the alternate stack the Rust
            // runtime gives each of its threads, so a stack overflow is reported as well.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = handler
                    as extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void)
                    as libc::sighandler_t;
                action.sa_flags = libc::SA_RESETHAND | libc::SA_SIGINFO | libc::SA_ONSTACK;
                libc::sigemptyset(&mut action.sa_mask);
                libc::sigaction(signal, &action, std::ptr::null_mut());
            }
        }
    }

    pub(super) fn put(fd: libc::c_int, bytes: &[u8]) {
        // SAFETY: write(2) is async-signal-safe; the buffer is valid for its length.
        unsafe {
            libc::write(fd, bytes.as_ptr().cast(), bytes.len());
        }
    }

    /// `prefix 0x<hex>\n` without allocating.
    fn put_hex(fd: libc::c_int, prefix: &[u8], value: usize) {
        let mut buf = [0u8; 64];
        let mut n = 0;
        for &b in prefix.iter().chain(b" 0x") {
            buf[n] = b;
            n += 1;
        }
        let digits = b"0123456789abcdef";
        let mut started = false;
        for shift in (0..usize::BITS).step_by(4).rev() {
            let d = (value >> shift) & 0xf;
            if d != 0 || started || shift == 0 {
                started = true;
                buf[n] = digits[d];
                n += 1;
            }
        }
        buf[n] = b'\n';
        put(fd, &buf[..=n]);
    }

    extern "C" fn handler(
        signal: libc::c_int,
        info: *mut libc::siginfo_t,
        _ctx: *mut libc::c_void,
    ) {
        let fd = MARKER_FD.load(Ordering::Acquire);
        if fd >= 0
            && let Some((_, name)) = SIGNALS.iter().find(|(s, _)| *s == signal)
        {
            put(fd, name);
            put(fd, b"\n");
            thread_name(fd);
            if !info.is_null() {
                // SAFETY: the kernel passes a valid siginfo for SA_SIGINFO handlers.
                let address = unsafe { (*info).si_addr() } as usize;
                put_hex(fd, b"fault", address);
            }
            put_stack(fd);
            copy_maps(MAPS_FD.load(Ordering::Acquire));
        }
        // SA_RESETHAND restored the default action: raising again ends the process as before.
        // SAFETY: raise(3) is async-signal-safe.
        unsafe {
            libc::raise(signal);
        }
    }

    /// One `ip 0x<hex>` line per return address of the calling thread, innermost first.
    pub(super) fn put_stack(fd: libc::c_int) {
        let mut frames = [0usize; MAX_FRAMES];
        let mut count = 0;
        // SAFETY: the unwinder only reads this thread's own stack, which stands still while its
        // signal handler runs; the closure only stores into a stack array.
        unsafe {
            backtrace::trace_unsynchronized(|frame| {
                frames[count] = frame.ip() as usize;
                count += 1;
                count < MAX_FRAMES
            });
        }
        for &ip in &frames[..count] {
            put_hex(fd, b"ip", ip);
        }
    }

    /// `thread <name>` of the crashing thread. Linux names threads through prctl, a plain
    /// system call; elsewhere the line is left out.
    pub(super) fn thread_name(fd: libc::c_int) {
        #[cfg(target_os = "linux")]
        {
            let mut name = [0u8; 16];
            // SAFETY: PR_GET_NAME writes at most 16 bytes into the buffer.
            let ok = unsafe { libc::prctl(libc::PR_GET_NAME, name.as_mut_ptr()) } == 0;
            if ok {
                let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
                put(fd, b"thread ");
                put(fd, &name[..len]);
                put(fd, b"\n");
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = fd;
    }

    /// Copies `/proc/self/maps` (which library sits where) into `out` for the symbol lookup
    /// later, using only open/read/write/close.
    pub(super) fn copy_maps(out: libc::c_int) {
        if out < 0 {
            return;
        }
        // SAFETY: open/read/close are async-signal-safe; the path is a static C string.
        unsafe {
            let input = libc::open(c"/proc/self/maps".as_ptr(), libc::O_RDONLY);
            if input < 0 {
                return;
            }
            let mut buf = [0u8; 4096];
            loop {
                let n = libc::read(input, buf.as_mut_ptr().cast(), buf.len());
                if n <= 0 {
                    break;
                }
                put(out, &buf[..n as usize]);
            }
            libc::close(input);
        }
    }
}

/// Real-time overruns (Linux). A real-time thread that runs too long without blocking gets
/// SIGXCPU, and the whole process SIGKILL a little later if it stays real-time; the audio crate
/// demotes the thread at SIGXCPU (`mumcord_audio::rt`). That warning is the only moment the
/// thread can say where it is busy, so [`capture`] runs inside the signal handler and writes
/// the thread name and its stack into a file opened beforehand, the way the fatal handler fills
/// the marker. A normal thread turns the record into a report ([`super::report_overrun`]); if
/// the run is ended first, the next start does.
#[cfg(target_os = "linux")]
mod overrun {
    use std::fs;
    use std::io;
    use std::os::fd::IntoRawFd;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicI32, AtomicU8, Ordering};

    use super::{CrashKind, NativeCrash, Report, native};

    /// What [`capture`] wrote, in the marker's format.
    const RECORD: &str = "overrun";
    /// The loaded libraries at that moment, for naming the stack.
    const MAPS: &str = "overrun-maps";

    static RECORD_FD: AtomicI32 = AtomicI32::new(-1);
    static MAPS_FD: AtomicI32 = AtomicI32::new(-1);

    const IDLE: u8 = 0;
    const CAPTURING: u8 = 1;
    const CAPTURED: u8 = 2;
    const REPORTED: u8 = 3;
    /// Only the first overrun of a run is recorded and reported, so a thread that keeps
    /// overrunning cannot fill the disk.
    static STAGE: AtomicU8 = AtomicU8::new(IDLE);

    /// Opens the record files and asks the audio crate to call [`capture`] at an overrun.
    pub(super) fn install(dir: &Path) {
        let (Ok(record), Ok(maps)) = (
            fs::File::create(dir.join(RECORD)),
            fs::File::create(dir.join(MAPS)),
        ) else {
            return;
        };
        RECORD_FD.store(record.into_raw_fd(), Ordering::Release);
        MAPS_FD.store(maps.into_raw_fd(), Ordering::Release);
        mumcord_audio::rt::set_overrun_hook(capture);
    }

    /// Runs inside the SIGXCPU handler, on the thread that overran. Async-signal-safe calls
    /// only (write, prctl, open, read, close and atomics) plus the unwinder, like the fatal
    /// handler.
    pub(super) extern "C" fn capture() {
        let fd = RECORD_FD.load(Ordering::Acquire);
        if fd < 0
            || STAGE
                .compare_exchange(IDLE, CAPTURING, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }
        // The thread goes on after the handler, so the code it interrupted must find its errno
        // untouched.
        // SAFETY: the errno location is valid for the calling thread.
        let errno = unsafe { *native::errno_location() };
        native::put(fd, b"SIGXCPU\n");
        native::thread_name(fd);
        native::put_stack(fd);
        native::copy_maps(MAPS_FD.load(Ordering::Acquire));
        // SAFETY: as above.
        unsafe {
            *native::errno_location() = errno;
        }
        STAGE.store(CAPTURED, Ordering::Release);
    }

    /// Whether a captured overrun waits for its report; true only once per run.
    pub(super) fn claim() -> bool {
        STAGE
            .compare_exchange(CAPTURED, REPORTED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(super) fn remove_record(dir: &Path) {
        let _ = fs::remove_file(dir.join(RECORD));
        let _ = fs::remove_file(dir.join(MAPS));
    }

    /// The record in `dir` with its library mappings, taken off the disk so that it is
    /// reported once.
    pub(super) fn take_record(dir: &Path) -> Option<(NativeCrash, String)> {
        let record = fs::read_to_string(dir.join(RECORD)).ok();
        let maps = fs::read_to_string(dir.join(MAPS)).unwrap_or_default();
        remove_record(dir);
        Some((super::parse_marker(&record?)?, maps))
    }

    pub(super) fn write(
        dir: &Path,
        version: &str,
        (record, maps): &(NativeCrash, String),
        cause: &str,
        log: &[String],
        context: &str,
        uptime_secs: Option<u64>,
    ) -> io::Result<PathBuf> {
        let report = Report {
            kind: CrashKind::Overrun,
            thread: record.thread.as_deref().unwrap_or("-"),
            cause,
            backtrace: &super::symbolize(&record.ips, maps).join("\n"),
            log,
            context,
            uptime_secs,
        };
        let path = super::write_report(dir, version, &report)?;
        super::prune(dir)?;
        Ok(path)
    }
}

/// Windows exception codes and how the handler treats them. Outside the Windows build only the
/// tests use them.
#[cfg(any(windows, test))]
mod codes {
    pub const ACCESS_VIOLATION: u32 = 0xC000_0005;
    const STACK_OVERFLOW: u32 = 0xC000_00FD;
    const STACK_BUFFER_OVERRUN: u32 = 0xC000_0409;
    const HEAP_CORRUPTION: u32 = 0xC000_0374;
    const FAIL_FAST: u32 = 0xC000_0602;

    /// The name of an exception code; Rust's own `abort` and C++ `throw` included.
    pub fn exception_name(code: u32) -> &'static str {
        match code {
            ACCESS_VIOLATION => "EXCEPTION_ACCESS_VIOLATION",
            STACK_OVERFLOW => "EXCEPTION_STACK_OVERFLOW",
            STACK_BUFFER_OVERRUN => "STATUS_STACK_BUFFER_OVERRUN",
            0xC000_001D => "EXCEPTION_ILLEGAL_INSTRUCTION",
            0xC000_0094 => "EXCEPTION_INT_DIVIDE_BY_ZERO",
            0xC000_0006 => "EXCEPTION_IN_PAGE_ERROR",
            HEAP_CORRUPTION => "STATUS_HEAP_CORRUPTION",
            FAIL_FAST => "STATUS_FAIL_FAST_EXCEPTION",
            0xC000_0096 => "EXCEPTION_PRIV_INSTRUCTION",
            0x8000_0003 => "EXCEPTION_BREAKPOINT",
            0xE06D_7363 => "C++ exception",
            _ => "EXCEPTION",
        }
    }

    /// Codes nobody recovers from, recorded as soon as they are raised (issue #699): a stack
    /// overflow leaves too little room to wait for the filter, and Windows never hands heap
    /// corruption or a raised fail-fast to the unhandled-exception filter at all.
    pub fn fatal_at_first_chance(code: u32) -> bool {
        matches!(
            code,
            STACK_OVERFLOW | STACK_BUFFER_OVERRUN | HEAP_CORRUPTION | FAIL_FAST
        )
    }

    /// An error (the two top bits set), as opposed to a warning or an information such as a
    /// debugger message; only errors can end the process.
    pub fn is_error(code: u32) -> bool {
        code >> 30 == 0b11
    }
}

/// Windows (issues #629, #699): the exception, the thread, the stack and the loaded modules go into
/// the marker and the modules file, in the same marker format as the Unix handler, so the next
/// start names the module and offset of every frame. Three ways in, all writing the same record
/// once per run, only into memory reserved at start and through handles opened at start:
///
/// - the unhandled-exception filter, for every crash nobody catches;
/// - a vectored handler, which Windows calls before anything else: it records the codes the filter
///   never sees (stack overflow, heap corruption) and puts the filter back when a library loaded
///   later (ONNX Runtime, camera drivers, Media Foundation) replaced it;
/// - a `SIGABRT` handler, because the C runtime's `abort()` (also behind `std::terminate`, e.g. an
///   uncaught C++ exception in a library) would otherwise end the process with a fast-fail that
///   no handler sees.
///
/// Afterwards Windows ends the process as before. What stays out of reach is a fast-fail (Rust's
/// `abort`, a stack-cookie check): the next start then says that no exception was recorded.
#[cfg(windows)]
mod native {
    use std::os::windows::io::IntoRawHandle;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};

    use windows::Win32::Foundation::{HANDLE, HMODULE};
    use windows::Win32::Storage::FileSystem::{
        FILE_BEGIN, SetEndOfFile, SetFilePointerEx, WriteFile,
    };
    use windows::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, EXCEPTION_POINTERS, EXCEPTION_RECORD,
        RtlCaptureStackBackTrace, SetUnhandledExceptionFilter,
    };
    use windows::Win32::System::ProcessStatus::{
        K32EnumProcessModules, K32GetModuleBaseNameW, K32GetModuleInformation, MODULEINFO,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentThreadId, SetThreadStackGuarantee,
    };

    use super::codes;

    const MAX_FRAMES: usize = 62;
    const MAX_MODULES: usize = 512;
    /// Lets Windows go on and end the process.
    const CONTINUE_SEARCH: i32 = 0;
    /// Room the main thread keeps for the handler after a stack overflow. Rust already keeps
    /// 20 KiB on every thread it starts; the main thread runs the UI and the deepest calls, so it
    /// gets a little more.
    const STACK_GUARANTEE: u32 = 32 * 1024;
    const SIGABRT: i32 = 22;
    /// `_CALL_REPORTFAULT`: without it `abort()` exits after the `SIGABRT` handler instead of
    /// fast-failing.
    const CALL_REPORTFAULT: u32 = 0x2;

    // The C runtime Rust links against; DLLs built with the shared C runtime (ONNX Runtime) use
    // the same one, so their `abort()` reaches this handler too.
    unsafe extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
        fn _set_abort_behavior(flags: u32, mask: u32) -> u32;
    }

    type Filter = unsafe extern "system" fn(*const EXCEPTION_POINTERS) -> i32;

    static MARKER: AtomicIsize = AtomicIsize::new(0);
    static MODULES: AtomicIsize = AtomicIsize::new(0);
    static ENTERED: AtomicBool = AtomicBool::new(false);
    /// A filter a library installed over ours; ours runs first and then hands the crash to it.
    static REPLACED: AtomicUsize = AtomicUsize::new(0);
    /// Set while the replaced filter runs: when it calls back into ours, that call ends at once.
    static HANDED_OVER: AtomicBool = AtomicBool::new(false);

    /// Text built without allocating, in memory that exists before any crash.
    struct Text {
        bytes: [u8; 64 * 1024],
        len: usize,
    }

    static mut TEXT: Text = Text {
        bytes: [0; 64 * 1024],
        len: 0,
    };
    /// The module list lives here, not on the stack, which may be nearly used up after a stack
    /// overflow.
    static mut MODULE_LIST: [HMODULE; MAX_MODULES] = [HMODULE(std::ptr::null_mut()); MAX_MODULES];

    impl Text {
        fn push(&mut self, part: &[u8]) {
            let end = (self.len + part.len()).min(self.bytes.len());
            let n = end - self.len;
            self.bytes[self.len..end].copy_from_slice(&part[..n]);
            self.len = end;
        }

        fn hex(&mut self, value: usize) {
            self.push(b"0x");
            let mut digits = [0u8; 16];
            let mut i = digits.len();
            let mut v = value;
            loop {
                i -= 1;
                digits[i] = b"0123456789abcdef"[v & 0xf];
                v >>= 4;
                if v == 0 {
                    break;
                }
            }
            self.push(&digits[i..]);
        }

        fn decimal(&mut self, value: u32) {
            let mut digits = [0u8; 10];
            let mut i = digits.len();
            let mut v = value;
            loop {
                i -= 1;
                digits[i] = b'0' + (v % 10) as u8;
                v /= 10;
                if v == 0 {
                    break;
                }
            }
            self.push(&digits[i..]);
        }

        /// A module name; characters outside ASCII become `?`, which is enough to recognise it.
        fn wide(&mut self, text: &[u16]) {
            for &c in text {
                self.push(&[if (1..0x80).contains(&c) {
                    c as u8
                } else {
                    b'?'
                }]);
            }
        }
    }

    /// Windows has no quit signals; the end of the session reaches the app as a window message.
    pub fn on_quit(_run: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
        Ok(())
    }

    pub fn install(marker: &Path, maps: &Path) {
        let Ok(file) = std::fs::OpenOptions::new().write(true).open(marker) else {
            return;
        };
        MARKER.store(file.into_raw_handle() as isize, Ordering::Release);
        if let Ok(file) = std::fs::File::create(maps) {
            MODULES.store(file.into_raw_handle() as isize, Ordering::Release);
        }
        let mut guarantee = STACK_GUARANTEE;
        // SAFETY: plain functions are installed; they only read the exception record and call
        // functions that do not allocate on the Rust heap. `install` runs on the main thread, so
        // the stack guarantee applies to it.
        unsafe {
            SetUnhandledExceptionFilter(Some(filter));
            // First in line, so a library's own vectored handler cannot end the process before
            // the record is written.
            AddVectoredExceptionHandler(1, Some(vectored));
            _set_abort_behavior(0, CALL_REPORTFAULT);
            signal(SIGABRT, aborted as extern "C" fn(i32) as usize);
            let _ = SetThreadStackGuarantee(&mut guarantee);
        }
    }

    fn write_all(handle: isize, bytes: &[u8]) {
        if handle == 0 {
            return;
        }
        let handle = HANDLE(handle as *mut core::ffi::c_void);
        // SAFETY: a handle opened at start for this file; the bytes live in static memory.
        unsafe {
            let _ = SetFilePointerEx(handle, 0, None, FILE_BEGIN);
            let _ = WriteFile(handle, Some(bytes), None, None);
            let _ = SetEndOfFile(handle);
        }
    }

    /// Writes the record: `name (code)` (or just `name` without a code), the thread, the faulting
    /// address, the stack and the modules. Only the first call of a run writes; a second one (the
    /// filter after the vectored handler, another thread crashing too) leaves it as it is.
    fn write_record(name: &[u8], code: Option<u32>, record: Option<&EXCEPTION_RECORD>) {
        if ENTERED.swap(true, Ordering::AcqRel) {
            return;
        }
        // SAFETY: only this one call reaches here (ENTERED), so the static memory has a single
        // user.
        let text = unsafe { &mut *std::ptr::addr_of_mut!(TEXT) };
        let modules = unsafe { &mut *std::ptr::addr_of_mut!(MODULE_LIST) };
        text.len = 0;
        text.push(name);
        if let Some(code) = code {
            text.push(b" (");
            text.hex(code as usize);
            text.push(b")");
        }
        text.push(b"\nthread id ");
        // SAFETY: plain query of the current thread.
        text.decimal(unsafe { GetCurrentThreadId() });
        text.push(b"\n");
        if let Some(record) = record {
            // For an access violation the second parameter is the address that was touched.
            if code == Some(codes::ACCESS_VIOLATION) && record.NumberParameters >= 2 {
                text.push(b"fault ");
                text.hex(record.ExceptionInformation[1]);
                text.push(b"\n");
            }
            text.push(b"ip ");
            text.hex(record.ExceptionAddress as usize);
            text.push(b"\n");
        }
        let mut frames = [std::ptr::null_mut(); MAX_FRAMES];
        // SAFETY: fills the array on the stack; does not allocate.
        let count = unsafe { RtlCaptureStackBackTrace(0, &mut frames, None) } as usize;
        for frame in &frames[..count.min(MAX_FRAMES)] {
            text.push(b"ip ");
            text.hex(*frame as usize);
            text.push(b"\n");
        }
        write_all(MARKER.load(Ordering::Acquire), &text.bytes[..text.len]);

        text.len = 0;
        let mut needed = 0u32;
        // SAFETY: the process's own module list into static memory.
        let listed = unsafe {
            K32EnumProcessModules(
                GetCurrentProcess(),
                modules.as_mut_ptr(),
                std::mem::size_of_val(modules) as u32,
                &mut needed,
            )
        };
        if listed.as_bool() {
            let count = (needed as usize / std::mem::size_of::<HMODULE>()).min(MAX_MODULES);
            for module in &modules[..count] {
                let mut info = MODULEINFO::default();
                let mut name = [0u16; 260];
                // SAFETY: queries about a module of this process, into memory on the stack.
                let known = unsafe {
                    K32GetModuleInformation(
                        GetCurrentProcess(),
                        *module,
                        &mut info,
                        std::mem::size_of::<MODULEINFO>() as u32,
                    )
                };
                let length =
                    unsafe { K32GetModuleBaseNameW(GetCurrentProcess(), Some(*module), &mut name) };
                if !known.as_bool() {
                    continue;
                }
                text.push(b"module ");
                text.hex(info.lpBaseOfDll as usize);
                text.push(b" ");
                text.hex(info.SizeOfImage as usize);
                text.push(b" ");
                text.wide(&name[..(length as usize).min(name.len())]);
                text.push(b"\n");
            }
        }
        write_all(MODULES.load(Ordering::Acquire), &text.bytes[..text.len]);
    }

    /// The exception record Windows passes to both handlers.
    ///
    /// # Safety
    /// `info` comes from Windows and is valid during the handler call.
    unsafe fn exception<'a>(info: *const EXCEPTION_POINTERS) -> Option<&'a EXCEPTION_RECORD> {
        // SAFETY: see above.
        unsafe { info.as_ref().and_then(|i| i.ExceptionRecord.as_ref()) }
    }

    fn record_exception(record: Option<&EXCEPTION_RECORD>) {
        let code = record.map_or(0, |r| r.ExceptionCode.0 as u32);
        write_record(codes::exception_name(code).as_bytes(), Some(code), record);
    }

    unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
        // SAFETY: Windows passes valid exception pointers for the duration of the call.
        record_exception(unsafe { exception(info) });
        let replaced = REPLACED.load(Ordering::Acquire);
        if replaced == 0 || HANDED_OVER.swap(true, Ordering::AcqRel) {
            return CONTINUE_SEARCH;
        }
        // SAFETY: the value was stored from a filter function pointer in `keep_filter`; the
        // library that installed it is still loaded, since it is part of this process.
        let replaced = unsafe { std::mem::transmute::<usize, Filter>(replaced) };
        // SAFETY: the same arguments Windows passed to this filter.
        unsafe { replaced(info) }
    }

    /// Puts our filter back at the top if a library replaced it, and keeps theirs to hand over
    /// to. Cheap enough for every exception: one pointer swap.
    fn keep_filter() {
        // SAFETY: swaps the process-wide filter; no other effect.
        let current = unsafe { SetUnhandledExceptionFilter(Some(filter)) };
        if let Some(current) = current
            && current as usize != filter as Filter as usize
        {
            REPLACED.store(current as usize, Ordering::Release);
        }
    }

    unsafe extern "system" fn vectored(info: *mut EXCEPTION_POINTERS) -> i32 {
        // SAFETY: Windows passes valid exception pointers for the duration of the call.
        let record = unsafe { exception(info) };
        let code = record.map_or(0, |r| r.ExceptionCode.0 as u32);
        if codes::fatal_at_first_chance(code) {
            record_exception(record);
        } else if codes::is_error(code) {
            // Most errors are caught somewhere and the run goes on; only the filter knows at the
            // end, so it must still be ours then.
            keep_filter();
        }
        CONTINUE_SEARCH
    }

    /// The C runtime's `abort()` raised `SIGABRT`; after this returns it exits the process.
    extern "C" fn aborted(_signal: i32) {
        write_record(b"SIGABRT (abort or std::terminate)", None, None);
    }
}

/// Without a native handler the marker alone tells an unclean exit at the next start.
#[cfg(not(any(unix, windows)))]
mod native {
    pub fn install(_marker: &std::path::Path, _maps: &std::path::Path) {}

    pub fn on_quit(_run: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
        Ok(())
    }
}

/// A module line the Windows handler wrote: `module <base> <size> <name>`.
struct Module<'a> {
    start: usize,
    end: usize,
    name: &'a str,
}

fn modules(text: &str) -> Vec<Module<'_>> {
    let hex = |s: &str| usize::from_str_radix(s.trim_start_matches("0x"), 16).ok();
    text.lines()
        .filter_map(|line| {
            let mut fields = line.strip_prefix("module ")?.splitn(3, ' ');
            let start = hex(fields.next()?)?;
            let size = hex(fields.next()?)?;
            Some(Module {
                start,
                end: start + size,
                name: fields.next()?.trim(),
            })
        })
        .collect()
}

/// One mapping line of `/proc/self/maps`: address range, file offset and path.
struct Mapping<'a> {
    start: usize,
    end: usize,
    offset: usize,
    path: &'a str,
}

fn mappings(maps: &str) -> Vec<Mapping<'_>> {
    maps.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (start, end) = fields.next()?.split_once('-')?;
            let _perms = fields.next()?;
            let offset = fields.next()?;
            let _dev = fields.next()?;
            let _inode = fields.next()?;
            let path = fields.next().filter(|p| p.starts_with('/'))?;
            Some(Mapping {
                start: usize::from_str_radix(start, 16).ok()?,
                end: usize::from_str_radix(end, 16).ok()?,
                offset: usize::from_str_radix(offset, 16).ok()?,
                path,
            })
        })
        .collect()
}

/// The nearest symbol at or below `file_offset` in an ELF file, with the distance to it.
fn symbol_at(data: &[u8], file_offset: usize) -> Option<(String, u64)> {
    use object::{Object, ObjectSegment, ObjectSymbol};
    let file = object::File::parse(data).ok()?;
    let offset = file_offset as u64;
    let address = file.segments().find_map(|segment| {
        let (seg_offset, size) = segment.file_range();
        (seg_offset..seg_offset + size)
            .contains(&offset)
            .then(|| segment.address() + (offset - seg_offset))
    })?;
    let best = file
        .symbols()
        .chain(file.dynamic_symbols())
        .filter(|s| s.is_definition() && s.address() <= address && s.address() != 0)
        .filter(|s| s.size() == 0 || address < s.address() + s.size())
        .max_by_key(|s| s.address())?;
    let name = best.name().ok()?;
    Some((
        format!("{:#}", rustc_demangle::demangle(name)),
        address - best.address(),
    ))
}

/// Turns the addresses the native handler wrote into `library symbol+offset` lines. Addresses
/// outside any file (JIT code, the stack) stay as they are.
fn symbolize(ips: &[usize], maps: &str) -> Vec<String> {
    let windows = modules(maps);
    let maps = mappings(maps);
    let mut files: BTreeMap<&str, Option<Vec<u8>>> = BTreeMap::new();
    ips.iter()
        .enumerate()
        .map(|(i, &ip)| {
            let Some(map) = maps.iter().find(|m| m.start <= ip && ip < m.end) else {
                // Windows names a frame by its module and the offset in it (issue #629).
                return match windows.iter().find(|m| m.start <= ip && ip < m.end) {
                    Some(module) => format!("{i:>3}: {} +0x{:x}", module.name, ip - module.start),
                    None => format!("{i:>3}: 0x{ip:x}"),
                };
            };
            let file_offset = ip - map.start + map.offset;
            let library = Path::new(map.path)
                .file_name()
                .map_or(map.path.to_owned(), |n| n.to_string_lossy().into_owned());
            let data = files
                .entry(map.path)
                .or_insert_with(|| fs::read(map.path).ok());
            match data.as_deref().and_then(|d| symbol_at(d, file_offset)) {
                Some((name, delta)) => format!("{i:>3}: {library} {name}+0x{delta:x}"),
                None => format!("{i:>3}: {library} +0x{file_offset:x}"),
            }
        })
        .collect()
}

/// The marker a fatal signal left: the signal name, the faulting address and the stack.
struct NativeCrash {
    signal: String,
    thread: Option<String>,
    fault: Option<usize>,
    ips: Vec<usize>,
}

fn parse_marker(text: &str) -> Option<NativeCrash> {
    let mut lines = text.lines();
    let signal = lines.next()?.trim().to_owned();
    if signal.is_empty() {
        return None;
    }
    let hex = |s: &str| usize::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok();
    let mut fault = None;
    let mut thread = None;
    let mut ips = Vec::new();
    for line in lines {
        if let Some(v) = line.strip_prefix("thread ") {
            thread = Some(v.trim().to_owned());
        } else if let Some(v) = line.strip_prefix("fault ") {
            fault = hex(v);
        } else if let Some(v) = line.strip_prefix("ip ") {
            ips.extend(hex(v));
        }
    }
    Some(NativeCrash {
        signal,
        thread,
        fault,
        ips,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    #[test]
    fn a_windows_marker_names_each_frame_by_module_and_offset() {
        let marker = "EXCEPTION_ACCESS_VIOLATION (0xc0000005)\nthread id 4242\nfault 0x10\n\
                      ip 0x7ff800001234\nip 0x140005000\nip 0x1\n";
        let crash = parse_marker(marker).unwrap();
        assert_eq!(crash.signal, "EXCEPTION_ACCESS_VIOLATION (0xc0000005)");
        assert_eq!(crash.thread.as_deref(), Some("id 4242"));
        assert_eq!(crash.fault, Some(0x10));
        let modules = "module 0x7ff800000000 0x200000 onnxruntime.dll\n\
                       module 0x140000000 0x800000 mumcord.exe\n";
        assert_eq!(
            symbolize(&crash.ips, modules),
            vec![
                "  0: onnxruntime.dll +0x1234".to_owned(),
                "  1: mumcord.exe +0x5000".to_owned(),
                "  2: 0x1".to_owned(),
            ]
        );
    }

    use super::*;

    #[test]
    fn windows_says_why_a_run_left_no_record() {
        assert!(unexpected_cause(true).contains("no exception recorded: likely a fast-fail"));
        assert_eq!(
            unexpected_cause(false),
            "Mumcord closed without shutting down"
        );
    }

    #[test]
    fn windows_codes_are_named_and_sorted_by_when_they_are_recorded() {
        assert_eq!(
            codes::exception_name(0xC000_00FD),
            "EXCEPTION_STACK_OVERFLOW"
        );
        assert_eq!(
            codes::exception_name(0xC000_0602),
            "STATUS_FAIL_FAST_EXCEPTION"
        );
        assert_eq!(codes::exception_name(0x1234), "EXCEPTION");
        // Recorded at once: nobody recovers, and the filter would come too late or never.
        for code in [0xC000_00FD, 0xC000_0409, 0xC000_0374, 0xC000_0602] {
            assert!(codes::fatal_at_first_chance(code), "{code:x}");
        }
        // Often caught and survived (an access violation in a probing driver, a C++ throw):
        // left to the filter.
        assert!(!codes::fatal_at_first_chance(0xC000_0005));
        assert!(!codes::fatal_at_first_chance(0xE06D_7363));
        assert!(codes::is_error(0xC000_0005));
        assert!(codes::is_error(0xE06D_7363));
        // A debugger message or a breakpoint is no error.
        assert!(!codes::is_error(0x4001_0006));
        assert!(!codes::is_error(0x8000_0003));
    }

    fn test_report(
        dir: &Path,
        kind: CrashKind,
        cause: &str,
        log: &[String],
    ) -> io::Result<PathBuf> {
        write_report(
            dir,
            "0.7.1",
            &Report {
                kind,
                thread: "main",
                cause,
                backtrace: "",
                log,
                context: "camera: on\n",
                uptime_secs: Some(42),
                ended_by: None,
            },
        )
    }

    #[test]
    fn what_ended_the_run_is_one_more_header_line() {
        let dir = tempfile::tempdir().unwrap();
        let report = Report {
            kind: CrashKind::Unexpected,
            thread: "-",
            cause: "closed",
            backtrace: "",
            log: &[],
            context: "",
            uptime_secs: None,
            ended_by: Some("SIGKILL (from outside the app)"),
        };
        let path = write_report(dir.path(), "0.9.0", &report).unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("cause: closed\nended by: SIGKILL (from outside the app)\n\n"));
        // The list still reads the report as before.
        let listed = list(dir.path());
        assert_eq!(listed[0].kind, CrashKind::Unexpected);
        assert_eq!(listed[0].cause, "closed");
    }

    #[test]
    fn a_marker_left_behind_becomes_a_report_and_a_clean_run_does_not() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("crash-reports");
        fs::create_dir_all(root.path().join("logs")).unwrap();
        fs::write(
            root.path().join("logs/mumcord.2026-11-06.log"),
            "a\nb\nlast line\n",
        )
        .unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(MARKER), "SIGSEGV").unwrap();

        // `install` would also set process-wide hooks, so the start-up check is tested directly.
        let left = fs::read_to_string(dir.join(MARKER)).unwrap();
        let kind = CrashKind::Signal(left);
        test_report(&dir, kind, "stopped", &previous_log_tail(&dir)).unwrap();

        let reports = list(&dir);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].kind, CrashKind::Signal("SIGSEGV".to_owned()));
        assert_eq!(reports[0].version, "0.7.1");
        let text = read(&dir, &reports[0].id).unwrap();
        assert!(text.contains("last line"));
        assert!(text.contains("running:\ncamera: on\n"));
        assert!(text.contains("uptime: 0h 00m 42s"));
    }

    #[test]
    fn context_lines_are_one_per_key_without_line_breaks() {
        let mut running = BTreeMap::new();
        running.insert("screen share", "sending, AV1\n1080p".to_owned());
        running.insert("camera", "on".to_owned());
        assert_eq!(
            context_text(&running),
            "camera: on\nscreen share: sending, AV1 1080p\n"
        );
    }

    #[test]
    fn context_file_is_written_at_most_once_per_second() {
        let dir = tempfile::tempdir().unwrap();
        let _ = STATE.set(State {
            dir: dir.path().to_path_buf(),
            version: "0.7.2".to_owned(),
            started: Instant::now(),
        });
        let on_disk = || fs::read_to_string(dir.path().join(CONTEXT)).unwrap();

        // The first write always happens.
        set_context("camera", "on");
        assert_eq!(on_disk(), "camera: on\n");

        // A change right after the write is not written again.
        set_context("camera", "off");
        assert_eq!(on_disk(), "camera: on\n");

        // After a second has passed, the next change reaches the file.
        *LAST_CONTEXT_WRITE.lock().unwrap() = Some(Instant::now() - Duration::from_secs(2));
        set_context("camera", "off");
        assert_eq!(on_disk(), "camera: off\n");
    }

    #[test]
    fn the_upload_is_the_report_plus_an_optional_comment() {
        let dir = tempfile::tempdir().unwrap();
        let path = test_report(dir.path(), CrashKind::Panic, "boom", &[]).unwrap();
        let id = path.file_name().unwrap().to_str().unwrap();
        let report = read(dir.path(), id).unwrap();
        assert_eq!(upload_body(dir.path(), id, "  ").unwrap(), report);
        let body = upload_body(dir.path(), id, "watched a VP9 share in full screen").unwrap();
        assert!(body.starts_with(&report));
        assert!(body.ends_with("\ncomment:\nwatched a VP9 share in full screen\n"));
    }

    #[test]
    fn times_are_written_in_utc() {
        assert_eq!(utc(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc(1_794_001_864), "2026-11-06 21:51 UTC");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00 UTC");
    }

    #[cfg(unix)]
    #[test]
    fn a_quit_signal_runs_the_quit_path_instead_of_ending_the_process() {
        let (tx, rx) = std::sync::mpsc::channel();
        on_quit_signal(move || {
            let _ = tx.send(());
        });
        // SAFETY: raises a signal whose handler was just installed.
        unsafe {
            libc::raise(libc::SIGTERM);
        }
        // Still alive, and the quit path was started.
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .expect("the quit path runs");
    }

    #[test]
    fn the_signal_marker_is_parsed() {
        let crash =
            parse_marker("SIGSEGV\nthread screen-decode\nfault 0x10\nip 0x7f00aa\nip 0x7f00bb\n")
                .unwrap();
        assert_eq!(crash.signal, "SIGSEGV");
        assert_eq!(crash.thread.as_deref(), Some("screen-decode"));
        assert_eq!(crash.fault, Some(0x10));
        assert_eq!(crash.ips, vec![0x7f00aa, 0x7f00bb]);
        assert!(parse_marker("").is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_address_in_libc_gets_a_symbol_name() {
        // This process's own libc: `getpid` must resolve to a name in it.
        let maps = fs::read_to_string("/proc/self/maps").unwrap();
        let getpid = libc::getpid as unsafe extern "C" fn() -> libc::pid_t as usize;
        let lines = symbolize(&[getpid, 0x10], &maps);
        assert!(lines[0].contains("libc"), "{lines:?}");
        assert!(lines[0].contains("getpid"), "{lines:?}");
        assert_eq!(lines[1], "  1: 0x10");
    }

    #[test]
    fn only_the_newest_reports_stay() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..MAX_REPORTS + 3 {
            fs::write(
                dir.path().join(format!("{PREFIX}{:010}{SUFFIX}", i)),
                "time: 1\n",
            )
            .unwrap();
        }
        prune(dir.path()).unwrap();
        let left = report_files(dir.path());
        assert_eq!(left.len(), MAX_REPORTS);
        assert!(left[0].ends_with(format!("{PREFIX}{:010}{SUFFIX}", 3)));
    }

    #[test]
    fn ids_outside_the_folder_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path(), "../secret.txt").is_err());
        assert!(delete(dir.path(), "running").is_err());
    }

    #[test]
    fn an_unexpected_exit_is_listed_but_brings_no_notice() {
        let dir = tempfile::tempdir().unwrap();
        test_report(dir.path(), CrashKind::Unexpected, "closed", &[]).unwrap();
        assert!(take_unseen(dir.path()).is_empty());
        assert_eq!(list(dir.path()).len(), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_overrun_record_becomes_a_non_fatal_report_without_a_notice() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("overrun"),
            "SIGXCPU\nthread voice-capture\nip 0x10\nip 0x20\n",
        )
        .unwrap();
        fs::write(dir.path().join("overrun-maps"), "").unwrap();

        let record = overrun::take_record(dir.path()).unwrap();
        let cause = "ran past its time budget; Mumcord kept running";
        overrun::write(dir.path(), "0.9.0", &record, cause, &[], "", Some(42)).unwrap();

        let reports = list(dir.path());
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].kind, CrashKind::Overrun);
        assert_eq!(reports[0].cause, cause);
        let text = read(dir.path(), &reports[0].id).unwrap();
        assert!(text.contains("kind: real-time overrun\nthread: voice-capture\n"));
        assert!(text.contains("backtrace:\n  0: 0x10\n  1: 0x20\n"));
        // The record is gone, so neither this run nor the next start reports it again.
        assert!(overrun::take_record(dir.path()).is_none());
        assert!(take_unseen(dir.path()).is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn only_the_first_overrun_of_a_run_is_recorded_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        overrun::install(dir.path());
        assert!(!overrun::claim(), "nothing captured yet");

        // What the SIGXCPU handler calls, here on the test's own thread.
        overrun::capture();
        overrun::capture();

        assert!(overrun::claim());
        assert!(!overrun::claim(), "one report per run");
        let text = fs::read_to_string(dir.path().join("overrun")).unwrap();
        assert_eq!(text.matches("SIGXCPU").count(), 1);
        let (record, maps) = overrun::take_record(dir.path()).unwrap();
        assert!(!record.ips.is_empty());
        let stack = symbolize(&record.ips, &maps).join("\n");
        assert!(stack.contains("overrun::capture"), "{stack}");
    }

    #[test]
    fn unseen_reports_are_shown_once() {
        let dir = tempfile::tempdir().unwrap();
        test_report(dir.path(), CrashKind::Panic, "boom", &[]).unwrap();
        assert_eq!(take_unseen(dir.path()).len(), 1);
        assert!(take_unseen(dir.path()).is_empty());
    }

    #[test]
    fn export_zips_the_reports_and_delete_all_empties_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        test_report(dir.path(), CrashKind::Panic, "boom", &[]).unwrap();
        let dest = dir.path().join("out.zip");
        export(dir.path(), &[], &dest).unwrap();
        let zip = zip::ZipArchive::new(fs::File::open(&dest).unwrap()).unwrap();
        assert_eq!(zip.len(), 1);
        delete_all(dir.path()).unwrap();
        assert!(list(dir.path()).is_empty());
    }

    #[test]
    fn the_home_folder_is_shortened() {
        if let Some(home) = std::env::var_os("HOME") {
            let text = format!("{}/x", home.to_string_lossy());
            assert_eq!(shorten_home(&text), "~/x");
        }
    }
}
