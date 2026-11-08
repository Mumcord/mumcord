//! Crash reports (#568): a Rust panic writes a report right away; a native crash (or a forced
//! quit) is noticed at the next start because the previous run's marker file is still there.
//! Reports are plain text in `crash-reports/` next to `logs/`, never uploaded, at most
//! [`MAX_REPORTS`] kept. They hold the version, OS, cause, backtrace and the last log lines,
//! which are redacted like the log itself; the home folder is shortened to `~`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::diag;

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
}

impl CrashKind {
    fn as_str(&self) -> String {
        match self {
            Self::Panic => "panic".to_owned(),
            Self::Signal(name) => format!("signal {name}"),
            Self::Unexpected => "unexpected exit".to_owned(),
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "panic" => Self::Panic,
            "unexpected exit" => Self::Unexpected,
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
    if let Some(state) = STATE.get() {
        let _ = fs::write(state.dir.join(CONTEXT), context_text(running));
    }
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
    let previous_context = fs::read_to_string(dir.join(CONTEXT)).unwrap_or_default();
    let _ = fs::remove_file(dir.join(CONTEXT));
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
                        _ => "Mumcord closed without shutting down".to_owned(),
                    };
                    let report = Report {
                        kind,
                        thread: &thread,
                        cause: &cause,
                        backtrace: &stack,
                        log: &previous,
                        context: &previous_context,
                        uptime_secs: None,
                    };
                    let _ = write_report(&dir, &version, &report);
                    let _ = prune(&dir);
                }
            });
    }
    fs::write(&marker, b"")?;
    prune(dir)?;
    let _ = STATE.set(State {
        dir: dir.to_path_buf(),
        version: version.to_owned(),
        started: Instant::now(),
    });
    install_panic_hook();
    native::install(&marker, &dir.join(MAPS));
    Ok(())
}

/// The app is closing normally: the next start does not report a crash.
pub fn clean_exit() {
    if let Some(state) = STATE.get() {
        let _ = fs::remove_file(state.dir.join(MARKER));
        let _ = fs::remove_file(state.dir.join(CONTEXT));
        let _ = fs::remove_file(state.dir.join(MAPS));
    }
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(state) = STATE.get() {
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
pub fn take_unseen(dir: &Path) -> Vec<CrashSummary> {
    let seen_path = dir.join(SEEN);
    let seen = fs::read_to_string(&seen_path).unwrap_or_default();
    let all = list(dir);
    let unseen: Vec<CrashSummary> = all
        .iter()
        .filter(|s| !seen.lines().any(|l| l == s.id))
        .filter(|s| s.kind != CrashKind::Unexpected)
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
    use std::sync::atomic::{AtomicI32, Ordering};

    pub const MAX_FRAMES: usize = 64;
    static MARKER_FD: AtomicI32 = AtomicI32::new(-1);
    static MAPS_FD: AtomicI32 = AtomicI32::new(-1);
    static MARKER_PATH: std::sync::OnceLock<std::ffi::CString> = std::sync::OnceLock::new();
    /// Asked to quit (the system shuts down, the session ends, Ctrl+C): not a crash.
    const QUIT_SIGNALS: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP];

    /// A quit request: the run ends on purpose, so its marker goes and the next start reports
    /// nothing; then the default action ends the process as before.
    extern "C" fn quit(signal: libc::c_int) {
        if let Some(path) = MARKER_PATH.get() {
            // SAFETY: unlink(2) is async-signal-safe; the path was set before the handler.
            unsafe {
                libc::unlink(path.as_ptr());
            }
        }
        // SAFETY: raise(3) is async-signal-safe; SA_RESETHAND restored the default action.
        unsafe {
            libc::raise(signal);
        }
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
        for signal in QUIT_SIGNALS {
            // SAFETY: as below; the handler only calls unlink and raise.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = quit as extern "C" fn(libc::c_int) as libc::sighandler_t;
                action.sa_flags = libc::SA_RESETHAND;
                libc::sigemptyset(&mut action.sa_mask);
                libc::sigaction(signal, &action, std::ptr::null_mut());
            }
        }
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

    fn put(fd: libc::c_int, bytes: &[u8]) {
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
            let mut frames = [0usize; MAX_FRAMES];
            let mut count = 0;
            // SAFETY: no other thread unwinds through this one while it is stopped in a fatal
            // signal; the closure only stores into a stack array.
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
            copy_maps();
        }
        // SA_RESETHAND restored the default action: raising again ends the process as before.
        // SAFETY: raise(3) is async-signal-safe.
        unsafe {
            libc::raise(signal);
        }
    }

    /// `thread <name>` of the crashing thread. Linux names threads through prctl, a plain
    /// system call; elsewhere the line is left out.
    fn thread_name(fd: libc::c_int) {
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

    /// Copies `/proc/self/maps` (which library sits where) for the symbol lookup at the next
    /// start, using only open/read/write/close.
    fn copy_maps() {
        let out = MAPS_FD.load(Ordering::Acquire);
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

/// Windows (issue #629): an unhandled-exception filter writes the exception, the thread, the stack
/// and the loaded modules into the marker and the modules file, in the same marker format as the
/// Unix handler, so the next start names the module and offset of every frame. The filter runs
/// once, writes only into memory reserved at start and through handles opened at start, and then
/// lets Windows end the process as before.
#[cfg(windows)]
mod native {
    use std::os::windows::io::IntoRawHandle;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

    use windows::Win32::Foundation::{HANDLE, HMODULE};
    use windows::Win32::Storage::FileSystem::{
        FILE_BEGIN, SetEndOfFile, SetFilePointerEx, WriteFile,
    };
    use windows::Win32::System::Diagnostics::Debug::{
        EXCEPTION_POINTERS, RtlCaptureStackBackTrace, SetUnhandledExceptionFilter,
    };
    use windows::Win32::System::ProcessStatus::{
        K32EnumProcessModules, K32GetModuleBaseNameW, K32GetModuleInformation, MODULEINFO,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentThreadId};

    const MAX_FRAMES: usize = 62;
    const MAX_MODULES: usize = 512;
    /// Lets Windows go on and end the process.
    const CONTINUE_SEARCH: i32 = 0;

    static MARKER: AtomicIsize = AtomicIsize::new(0);
    static MODULES: AtomicIsize = AtomicIsize::new(0);
    static ENTERED: AtomicBool = AtomicBool::new(false);

    /// Text built without allocating, in memory that exists before any crash.
    struct Text {
        bytes: [u8; 64 * 1024],
        len: usize,
    }

    static mut TEXT: Text = Text {
        bytes: [0; 64 * 1024],
        len: 0,
    };

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

    pub fn install(marker: &Path, maps: &Path) {
        let Ok(file) = std::fs::OpenOptions::new().write(true).open(marker) else {
            return;
        };
        MARKER.store(file.into_raw_handle() as isize, Ordering::Release);
        if let Ok(file) = std::fs::File::create(maps) {
            MODULES.store(file.into_raw_handle() as isize, Ordering::Release);
        }
        // SAFETY: installs a plain filter function; it only reads the exception record and calls
        // functions that do not allocate on the Rust heap.
        unsafe {
            SetUnhandledExceptionFilter(Some(filter));
        }
    }

    /// The name of an exception code; Rust's own `abort` and C++ `throw` included.
    pub(super) fn exception_name(code: u32) -> &'static str {
        match code {
            0xC000_0005 => "EXCEPTION_ACCESS_VIOLATION",
            0xC000_00FD => "EXCEPTION_STACK_OVERFLOW",
            0xC000_0409 => "STATUS_STACK_BUFFER_OVERRUN",
            0xC000_001D => "EXCEPTION_ILLEGAL_INSTRUCTION",
            0xC000_0094 => "EXCEPTION_INT_DIVIDE_BY_ZERO",
            0xC000_0006 => "EXCEPTION_IN_PAGE_ERROR",
            0xC000_0374 => "STATUS_HEAP_CORRUPTION",
            0xC000_0096 => "EXCEPTION_PRIV_INSTRUCTION",
            0x8000_0003 => "EXCEPTION_BREAKPOINT",
            0xE06D_7363 => "C++ exception",
            _ => "EXCEPTION",
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

    unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
        // A second exception while writing ends here; Windows still ends the process.
        if ENTERED.swap(true, Ordering::AcqRel) {
            return CONTINUE_SEARCH;
        }
        // SAFETY: only this one call of the filter reaches here (ENTERED), so the static text has
        // a single user; the record comes from Windows and is valid during the call.
        let text = unsafe { &mut *std::ptr::addr_of_mut!(TEXT) };
        text.len = 0;
        let record = unsafe { info.as_ref().and_then(|i| i.ExceptionRecord.as_ref()) };
        let code = record.map_or(0, |r| r.ExceptionCode.0 as u32);
        text.push(exception_name(code).as_bytes());
        text.push(b" (");
        text.hex(code as usize);
        text.push(b")\nthread id ");
        // SAFETY: plain query of the current thread.
        text.decimal(unsafe { GetCurrentThreadId() });
        text.push(b"\n");
        if let Some(record) = record {
            // For an access violation the second parameter is the address that was touched.
            if code == 0xC000_0005 && record.NumberParameters >= 2 {
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
        let mut modules = [HMODULE::default(); MAX_MODULES];
        let mut needed = 0u32;
        // SAFETY: the process's own module list into an array on the stack.
        let listed = unsafe {
            K32EnumProcessModules(
                GetCurrentProcess(),
                modules.as_mut_ptr(),
                std::mem::size_of_val(&modules) as u32,
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
        CONTINUE_SEARCH
    }
}

/// Without a native handler the marker alone tells an unclean exit at the next start.
#[cfg(not(any(unix, windows)))]
mod native {
    pub fn install(_marker: &std::path::Path, _maps: &std::path::Path) {}
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
            },
        )
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
