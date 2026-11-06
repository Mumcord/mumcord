//! Crash reports (#568): a Rust panic writes a report right away; a native crash (or a forced
//! quit) is noticed at the next start because the previous run's marker file is still there.
//! Reports are plain text in `crash-reports/` next to `logs/`, never uploaded, at most
//! [`MAX_REPORTS`] kept. They hold the version, OS, cause, backtrace and the last log lines,
//! which are redacted like the log itself; the home folder is shortened to `~`.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::diag;

/// How many reports stay; older ones are deleted.
pub const MAX_REPORTS: usize = 10;
/// Log lines a report carries.
const LOG_LINES: usize = 200;
const MARKER: &str = "running";
const SEEN: &str = "seen";
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
}

struct State {
    dir: PathBuf,
    version: String,
}

static STATE: OnceLock<State> = OnceLock::new();

/// Sets up crash reporting for this run: records a report for an unclean previous run, installs
/// the panic hook and the native signal handler, and leaves a marker until [`clean_exit`].
pub fn install(dir: &Path, version: &str) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let marker = dir.join(MARKER);
    if let Ok(left) = fs::read_to_string(&marker) {
        let kind = match left.trim() {
            "" => CrashKind::Unexpected,
            signal => CrashKind::Signal(signal.to_owned()),
        };
        let previous = previous_log_tail(dir);
        let cause = match &kind {
            CrashKind::Signal(name) => format!("Mumcord was stopped by {name}"),
            _ => "Mumcord closed without shutting down".to_owned(),
        };
        write_report(dir, version, &kind, "-", &cause, "", &previous)?;
    }
    fs::write(&marker, b"")?;
    prune(dir)?;
    let _ = STATE.set(State {
        dir: dir.to_path_buf(),
        version: version.to_owned(),
    });
    install_panic_hook();
    native::install(&marker);
    Ok(())
}

/// The app is closing normally: the next start does not report a crash.
pub fn clean_exit() {
    if let Some(state) = STATE.get() {
        let _ = fs::remove_file(state.dir.join(MARKER));
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
            let _ = write_report(
                &state.dir,
                &state.version,
                &CrashKind::Panic,
                thread.name().unwrap_or("unnamed"),
                &format!("{message}{location}"),
                &backtrace,
                &diag::recent_log_lines(),
            );
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

/// Replaces the home folder with `~`, so user names in paths stay out of reports.
fn shorten_home(text: &str) -> String {
    match std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        Some(home) if !home.is_empty() => text.replace(&*home.to_string_lossy(), "~"),
        _ => text.to_owned(),
    }
}

fn write_report(
    dir: &Path,
    version: &str,
    kind: &CrashKind,
    thread: &str,
    cause: &str,
    backtrace: &str,
    log: &[String],
) -> io::Result<PathBuf> {
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
    text.push_str(&format!("os: {}\n", diag::os_description()));
    text.push_str(&format!("time: {created}\n"));
    text.push_str(&format!("kind: {}\n", kind.as_str()));
    text.push_str(&format!("thread: {thread}\n"));
    text.push_str(&format!("cause: {}\n", cause.replace(['\n', '\r'], " ")));
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
        created_unix: field("time").parse().unwrap_or_default(),
        version: field("version"),
        kind: CrashKind::parse(&field("kind")),
        cause: field("cause"),
    })
}

/// The saved reports, newest first.
pub fn list(dir: &Path) -> Vec<CrashSummary> {
    let mut all: Vec<CrashSummary> = report_files(dir)
        .iter()
        .filter_map(|p| summary(p))
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

/// Reports not shown in the crash notice yet; they count as shown afterwards.
pub fn take_unseen(dir: &Path) -> Vec<CrashSummary> {
    let seen_path = dir.join(SEEN);
    let seen = fs::read_to_string(&seen_path).unwrap_or_default();
    let all = list(dir);
    let unseen: Vec<CrashSummary> = all
        .iter()
        .filter(|s| !seen.lines().any(|l| l == s.id))
        .cloned()
        .collect();
    let ids: Vec<&str> = all.iter().map(|s| s.id.as_str()).collect();
    let _ = fs::write(&seen_path, ids.join("\n"));
    unseen
}

/// The native handler: writes the signal name into the marker with async-signal-safe calls
/// only, then lets the default action end the process.
#[cfg(unix)]
mod native {
    use std::os::fd::IntoRawFd;
    use std::path::Path;
    use std::sync::atomic::{AtomicI32, Ordering};

    static MARKER_FD: AtomicI32 = AtomicI32::new(-1);
    const SIGNALS: [(libc::c_int, &[u8]); 5] = [
        (libc::SIGSEGV, b"SIGSEGV"),
        (libc::SIGILL, b"SIGILL"),
        (libc::SIGBUS, b"SIGBUS"),
        (libc::SIGFPE, b"SIGFPE"),
        (libc::SIGABRT, b"SIGABRT"),
    ];

    pub fn install(marker: &Path) {
        let Ok(file) = std::fs::OpenOptions::new().write(true).open(marker) else {
            return;
        };
        MARKER_FD.store(file.into_raw_fd(), Ordering::Release);
        for (signal, _) in SIGNALS {
            // SAFETY: a plain C handler installed with sigaction; the handler only uses
            // async-signal-safe functions (write, signal, raise).
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = handler as extern "C" fn(libc::c_int) as libc::sighandler_t;
                action.sa_flags = libc::SA_RESETHAND;
                libc::sigemptyset(&mut action.sa_mask);
                libc::sigaction(signal, &action, std::ptr::null_mut());
            }
        }
    }

    extern "C" fn handler(signal: libc::c_int) {
        let fd = MARKER_FD.load(Ordering::Acquire);
        if fd >= 0
            && let Some((_, name)) = SIGNALS.iter().find(|(s, _)| *s == signal)
        {
            // SAFETY: write(2) is async-signal-safe; the buffer is a static string.
            unsafe {
                libc::write(fd, name.as_ptr().cast(), name.len());
            }
        }
        // SA_RESETHAND restored the default action: raising again ends the process as before.
        // SAFETY: raise(3) is async-signal-safe.
        unsafe {
            libc::raise(signal);
        }
    }
}

/// Without a native handler the marker alone tells an unclean exit at the next start.
#[cfg(not(unix))]
mod native {
    pub fn install(_marker: &std::path::Path) {}
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

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
        write_report(
            &dir,
            "0.7.1",
            &kind,
            "-",
            "stopped",
            "",
            &previous_log_tail(&dir),
        )
        .unwrap();

        let reports = list(&dir);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].kind, CrashKind::Signal("SIGSEGV".to_owned()));
        assert_eq!(reports[0].version, "0.7.1");
        assert!(read(&dir, &reports[0].id).unwrap().contains("last line"));
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
    fn unseen_reports_are_shown_once() {
        let dir = tempfile::tempdir().unwrap();
        write_report(
            dir.path(),
            "0.7.1",
            &CrashKind::Panic,
            "main",
            "boom",
            "",
            &[],
        )
        .unwrap();
        assert_eq!(take_unseen(dir.path()).len(), 1);
        assert!(take_unseen(dir.path()).is_empty());
    }

    #[test]
    fn export_zips_the_reports_and_delete_all_empties_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        write_report(
            dir.path(),
            "0.7.1",
            &CrashKind::Panic,
            "main",
            "boom",
            "",
            &[],
        )
        .unwrap();
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
