//! Why the previous run ended when it left no crash record (Linux): a `SIGKILL`, the
//! out-of-memory killer or the kernel's real-time watchdog end the process without giving it a
//! chance to write anything, but the system journal knows. Each run notes who it is next to the
//! marker; the next start asks `journalctl` about exactly that process.
//!
//! Only the reason is derived from the journal. No journal line is copied into the report.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// The file next to the marker that says which process the run was.
pub(super) const RUN: &str = "run";
/// The journal query runs off the start-up path, but must not hang around either.
const QUERY_LIMIT: Duration = Duration::from_secs(3);
/// A kernel line belongs to the end of the run when it was logged at most this long before
/// systemd saw the process exit (in µs); an ended process can take a moment to be torn down.
const BEFORE_EXIT_MICROS: u64 = 5_000_000;
/// The two entries reach the journal on different ways, so their order can be off a little.
const AFTER_EXIT_MICROS: u64 = 1_000_000;

/// What identifies one run in the journal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Run {
    pid: u32,
    /// The boot the run belonged to, 32 hex digits; process ids repeat across boots.
    boot: String,
    started_unix: u64,
    /// The control group, whose last part is the systemd unit when a desktop started the app.
    cgroup: String,
}

impl Run {
    /// This process.
    pub(super) fn current() -> Option<Self> {
        let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
        let cgroup = fs::read_to_string("/proc/self/cgroup")
            .ok()
            .and_then(|text| {
                text.lines()
                    .find_map(|l| l.strip_prefix("0::"))
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        Some(Self {
            pid: std::process::id(),
            boot: boot_id(&boot)?,
            started_unix: super::now_unix(),
            cgroup,
        })
    }

    pub(super) fn save(&self, dir: &Path) {
        let _ = fs::write(dir.join(RUN), self.to_text());
    }

    /// The run that wrote its details into `dir`, i.e. the previous one at start-up.
    pub(super) fn load(dir: &Path) -> Option<Self> {
        Self::parse(&fs::read_to_string(dir.join(RUN)).ok()?)
    }

    fn to_text(&self) -> String {
        format!(
            "pid: {}\nboot: {}\nstarted: {}\ncgroup: {}\n",
            self.pid, self.boot, self.started_unix, self.cgroup
        )
    }

    fn parse(text: &str) -> Option<Self> {
        let field = |name: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(name)?.strip_prefix(": "))
        };
        Some(Self {
            pid: field("pid")?.parse().ok()?,
            boot: boot_id(field("boot")?)?,
            started_unix: field("started")?.parse().ok()?,
            cgroup: field("cgroup").unwrap_or_default().to_owned(),
        })
    }

    /// The systemd unit the app ran as (`app-Mumcord@….service`), if it was its own service.
    /// Only a service has a main process whose exit systemd logs; a scope does not.
    fn unit(&self) -> Option<&str> {
        self.cgroup
            .rsplit('/')
            .next()
            .filter(|unit| unit.ends_with(".service"))
    }
}

/// A boot id as the journal writes it: 32 hex digits without dashes.
fn boot_id(text: &str) -> Option<String> {
    let id = text.trim().replace('-', "");
    (id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())).then_some(id)
}

/// What ended `run`, as the value of the report's `ended by:` line. Never fails: whatever
/// stands in the way becomes `unknown (…)`.
pub(super) fn ended_by(run: Option<&Run>) -> String {
    let Some(run) = run else {
        return unknown("previous run not recorded");
    };
    let journal = match query(run) {
        Ok(journal) => journal,
        Err(why) => return unknown(why),
    };
    match reason(run, &journal) {
        Some(reason) => reason,
        // After a power loss or a hard reset nothing gets logged; the restart is the only hint.
        None if Run::current().is_some_and(|now| now.boot != run.boot) => {
            unknown("no journal entry; the computer was restarted since")
        }
        None => unknown("no journal entry"),
    }
}

fn unknown(why: &str) -> String {
    tracing::debug!(why, "no exit reason for the previous run");
    format!("unknown ({why})")
}

/// Asks the journal for the entries that can be about `run`: those of its unit, the kernel's
/// and systemd-oomd's, of that boot and since the run began. [`reason`] picks out the ones
/// that really are about it.
fn query(run: &Run) -> Result<String, &'static str> {
    let boot = format!("_BOOT_ID={}", run.boot);
    let mut command = Command::new("journalctl");
    command
        .args(["--no-pager", "--quiet", "--output=json"])
        .arg("--output-fields=MESSAGE,_TRANSPORT,SYSLOG_IDENTIFIER,USER_UNIT")
        .arg(format!("--since=@{}", run.started_unix));
    if let Some(unit) = run.unit() {
        command.args([&boot, &format!("USER_UNIT={unit}"), "+"]);
    }
    command
        .args([&boot, "_TRANSPORT=kernel", "+"])
        .args([&boot, "SYSLOG_IDENTIFIER=systemd-oomd"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(|_| "journalctl not available")?;
    let Some(mut stdout) = child.stdout.take() else {
        return Err("journal not readable");
    };
    // The output is read on a thread of its own so that the time limit also holds while
    // journalctl is still writing.
    let (done, output) = mpsc::channel();
    let reader = std::thread::Builder::new()
        .name("crash-journal".to_owned())
        .spawn(move || {
            let mut text = String::new();
            let read = stdout.read_to_string(&mut text).is_ok();
            let _ = done.send(read.then_some(text));
        });
    if reader.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("journal not readable");
    }
    match output.recv_timeout(QUERY_LIMIT) {
        Ok(Some(text)) if child.wait().is_ok_and(|status| status.success()) => Ok(text),
        Ok(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err("journal not readable")
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err("journal query timed out")
        }
    }
}

/// How systemd saw the main process of the unit end.
struct Exit {
    /// `killed`, `dumped` or `exited`.
    code: String,
    /// `9/KILL`, `1/FAILURE`, …
    status: String,
    at_micros: u64,
}

/// A line of the kernel's real-time watchdog (`RT Watchdog Timeout (hard): name[1234]`).
struct Watchdog {
    hard: bool,
    /// The id of the *thread* that overran, which is the process id only for the main thread.
    thread_id: u32,
    at_micros: u64,
}

/// Reads the reason out of `journalctl --output=json` lines, taking only entries about `run`:
/// its unit, its process id or its control group. `None` when there is none.
fn reason(run: &Run, journal: &str) -> Option<String> {
    let unit = run.unit();
    let killed_process = format!("Killed process {} (", run.pid);
    let killed_cgroup = format!("Killed {} due to ", run.cgroup);
    let (mut exit, mut out_of_memory, mut oomd) = (None, false, false);
    let (mut kernel_kills, mut watchdogs) = (Vec::new(), Vec::new());
    for line in journal.lines() {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let text = |name: &str| entry.get(name).and_then(serde_json::Value::as_str);
        let Some(message) = text("MESSAGE") else {
            continue;
        };
        let at_micros = text("__REALTIME_TIMESTAMP")
            .and_then(|t| t.parse().ok())
            .unwrap_or_default();
        if text("_TRANSPORT") == Some("kernel") {
            // "Out of memory: Killed process 1234 (name) …", also "Memory cgroup out of memory: …".
            if message.contains("ut of memory: ") && message.contains(&killed_process) {
                kernel_kills.push(at_micros);
            } else if let Some(watchdog) = parse_watchdog(message, at_micros) {
                watchdogs.push(watchdog);
            }
        } else if text("SYSLOG_IDENTIFIER") == Some("systemd-oomd") {
            oomd |= !run.cgroup.is_empty() && message.starts_with(&killed_cgroup);
        } else if unit.is_some() && text("USER_UNIT") == unit {
            if let Some(found) = parse_exit(message, at_micros) {
                exit = Some(found);
            }
            out_of_memory |= message.contains("killed by the OOM killer");
            oomd |= message.contains("systemd-oomd killed");
        }
    }
    let exit: Option<Exit> = exit;
    // Process and thread ids are handed out again, so a kernel line with the right number can
    // still be about someone else. Where systemd logged the exit, the line must sit next to it.
    let at_exit = |at_micros: u64| {
        exit.as_ref().is_none_or(|exit| {
            at_micros <= exit.at_micros + AFTER_EXIT_MICROS
                && exit.at_micros <= at_micros + BEFORE_EXIT_MICROS
        })
    };
    if out_of_memory || kernel_kills.into_iter().any(at_exit) {
        return Some("out-of-memory killer".to_owned());
    }
    if oomd {
        return Some("systemd-oomd (memory pressure)".to_owned());
    }
    let watchdog = watchdogs.iter().rev().find(|w| match &exit {
        // The kernel names the thread, not the process, so the link is that the process ended
        // by the watchdog's signal at that moment.
        Some(exit) => {
            at_exit(w.at_micros)
                && exit
                    .status
                    .ends_with(if w.hard { "/KILL" } else { "/XCPU" })
        }
        // Without the exit, only the main thread can be recognised.
        None => w.thread_id == run.pid,
    });
    if let Some(watchdog) = watchdog {
        let (signal, limit) = if watchdog.hard {
            ("SIGKILL", "hard")
        } else {
            ("SIGXCPU", "soft")
        };
        return Some(format!(
            "{signal} (kernel real-time watchdog, {limit} limit: a real-time thread used too \
             much CPU time)"
        ));
    }
    exit.map(|exit| describe(&exit))
}

/// `<unit>: Main process exited, code=killed, status=9/KILL`
fn parse_exit(message: &str, at_micros: u64) -> Option<Exit> {
    let (_, rest) = message.split_once(": Main process exited, code=")?;
    let (code, status) = rest.split_once(", status=")?;
    let plain = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '/');
    (plain(code) && plain(status)).then(|| Exit {
        code: code.to_owned(),
        status: status.to_owned(),
        at_micros,
    })
}

/// `RT Watchdog Timeout (hard): name[1234]`, printed when `kernel.print-fatal-signals=1`.
fn parse_watchdog(message: &str, at_micros: u64) -> Option<Watchdog> {
    let rest = message.strip_prefix("RT Watchdog Timeout (")?;
    let (limit, thread) = rest.split_once("): ")?;
    let (_, id) = thread.strip_suffix(']')?.rsplit_once('[')?;
    Some(Watchdog {
        hard: limit == "hard",
        thread_id: id.parse().ok()?,
        at_micros,
    })
}

fn describe(exit: &Exit) -> String {
    let (number, name) = exit.status.split_once('/').unwrap_or((&exit.status, ""));
    match (exit.code.as_str(), name) {
        ("exited", _) => format!("exit code {number} (the app ended itself)"),
        // Nothing inside a process can catch or send itself past SIGKILL: it came from outside.
        (_, "KILL") => "SIGKILL (from outside the app)".to_owned(),
        ("dumped", _) => format!("SIG{name} (core dumped)"),
        _ => format!("SIG{name}"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const UNIT: &str = "app-Mumcord@9f137d0c372b4fb8ba9c71ff5648001e.service";
    const BOOT: &str = "91c75791f31e480d8f91682f7af4bd66";

    fn run() -> Run {
        Run {
            pid: 424_581,
            boot: BOOT.to_owned(),
            started_unix: 1_794_611_584,
            cgroup: format!("/user.slice/user-1000.slice/user@1000.service/app.slice/{UNIT}"),
        }
    }

    /// One journal entry as `journalctl --output=json --output-fields=…` prints it.
    fn entry(micros: u64, fields: &[(&str, &str)]) -> String {
        let mut map = serde_json::Map::new();
        map.insert("__REALTIME_TIMESTAMP".to_owned(), micros.to_string().into());
        map.insert("_BOOT_ID".to_owned(), BOOT.into());
        for (name, value) in fields {
            map.insert((*name).to_owned(), (*value).into());
        }
        serde_json::Value::Object(map).to_string()
    }

    fn unit_line(micros: u64, unit: &str, message: &str) -> String {
        entry(
            micros,
            &[
                ("MESSAGE", &format!("{unit}: {message}")),
                ("SYSLOG_IDENTIFIER", "systemd"),
                ("USER_UNIT", unit),
                ("_TRANSPORT", "journal"),
            ],
        )
    }

    fn kernel_line(micros: u64, message: &str) -> String {
        entry(
            micros,
            &[
                ("MESSAGE", message),
                ("SYSLOG_IDENTIFIER", "kernel"),
                ("_TRANSPORT", "kernel"),
            ],
        )
    }

    const KILLED: &str = "Main process exited, code=killed, status=9/KILL";

    #[test]
    fn a_captured_journal_line_names_the_kill_from_outside() {
        // Copied from `journalctl -o json` on a KDE desktop, cut down to the queried fields.
        let journal = r#"{"__REALTIME_TIMESTAMP":"1794611584382975","_BOOT_ID":"91c75791f31e480d8f91682f7af4bd66","SYSLOG_IDENTIFIER":"systemd","USER_UNIT":"app-Mumcord@9f137d0c372b4fb8ba9c71ff5648001e.service","_TRANSPORT":"journal","MESSAGE":"Started Mumcord."}
{"__CURSOR":"s=f1d3a22784c643fa92144585fc4f90fc;i=46c123","__REALTIME_TIMESTAMP":"1794618749672973","__MONOTONIC_TIMESTAMP":"31427784018","_BOOT_ID":"91c75791f31e480d8f91682f7af4bd66","SYSLOG_IDENTIFIER":"systemd","USER_UNIT":"app-Mumcord@9f137d0c372b4fb8ba9c71ff5648001e.service","_TRANSPORT":"journal","MESSAGE":"app-Mumcord@9f137d0c372b4fb8ba9c71ff5648001e.service: Main process exited, code=killed, status=9/KILL"}
{"__REALTIME_TIMESTAMP":"1794618750099971","_BOOT_ID":"91c75791f31e480d8f91682f7af4bd66","SYSLOG_IDENTIFIER":"systemd","USER_UNIT":"app-Mumcord@9f137d0c372b4fb8ba9c71ff5648001e.service","_TRANSPORT":"journal","MESSAGE":"app-Mumcord@9f137d0c372b4fb8ba9c71ff5648001e.service: Failed with result 'signal'."}
"#;
        assert_eq!(
            reason(&run(), journal).as_deref(),
            Some("SIGKILL (from outside the app)")
        );
    }

    #[test]
    fn other_exits_are_named_by_signal_or_exit_code() {
        let exit = |message| reason(&run(), &unit_line(1, UNIT, message));
        assert_eq!(
            exit("Main process exited, code=dumped, status=6/ABRT").as_deref(),
            Some("SIGABRT (core dumped)")
        );
        assert_eq!(
            exit("Main process exited, code=killed, status=15/TERM").as_deref(),
            Some("SIGTERM")
        );
        assert_eq!(
            exit("Main process exited, code=exited, status=1/FAILURE").as_deref(),
            Some("exit code 1 (the app ended itself)")
        );
        assert_eq!(exit("Consumed 12min 3.876s CPU time."), None);
    }

    #[test]
    fn entries_about_other_processes_are_ignored() {
        let other = "app-Mumcord@fa2ff40f4cb3428b9574d6da4a3e7329.service";
        let journal = [
            unit_line(1, other, KILLED),
            kernel_line(
                2,
                "Out of memory: Killed process 4245 (firefox) total-vm:1kB, UID:1000",
            ),
            kernel_line(3, "RT Watchdog Timeout (hard): pipewire[336619]"),
            entry(
                4,
                &[
                    (
                        "MESSAGE",
                        "Killed /user.slice/user-1000.slice/user@1000.service/app.slice/other.scope \
                         due to memory pressure for /user.slice being 60.00% > 50.00% for > 20s \
                         with reclaim activity",
                    ),
                    ("SYSLOG_IDENTIFIER", "systemd-oomd"),
                ],
            ),
            "not json".to_owned(),
            r#"{"MESSAGE":[1,2,3],"_TRANSPORT":"kernel"}"#.to_owned(),
        ]
        .join("\n");
        assert_eq!(reason(&run(), &journal), None);
    }

    #[test]
    fn the_out_of_memory_killer_is_named() {
        let kernel = kernel_line(
            5,
            "Out of memory: Killed process 424581 (mumcord) total-vm:9kB, anon-rss:8kB, UID:1000",
        );
        let memory_cgroup = kernel_line(
            5,
            "Memory cgroup out of memory: Killed process 424581 (mumcord) total-vm:9kB",
        );
        let systemd = unit_line(
            6,
            UNIT,
            "A process of this unit has been killed by the OOM killer.",
        );
        for journal in [kernel, memory_cgroup, systemd] {
            // The kill also shows up as a plain SIGKILL of the main process; the cause wins.
            let journal = format!("{journal}\n{}", unit_line(7, UNIT, KILLED));
            assert_eq!(
                reason(&run(), &journal).as_deref(),
                Some("out-of-memory killer")
            );
        }
    }

    #[test]
    fn systemd_oomd_is_named() {
        let run = run();
        let oomd = entry(
            5,
            &[
                (
                    "MESSAGE",
                    &format!(
                        "Killed {} due to memory pressure for /user.slice/user-1000.slice being \
                         61.20% > 50.00% for > 20s with reclaim activity",
                        run.cgroup
                    ),
                ),
                ("SYSLOG_IDENTIFIER", "systemd-oomd"),
                ("_TRANSPORT", "journal"),
            ],
        );
        let systemd = unit_line(5, UNIT, "systemd-oomd killed 12 process(es) in this unit.");
        for journal in [oomd, systemd] {
            assert_eq!(
                reason(&run, &journal).as_deref(),
                Some("systemd-oomd (memory pressure)")
            );
        }
    }

    #[test]
    fn the_real_time_watchdog_is_named_for_the_main_thread_or_right_before_the_exit() {
        let hard = "SIGKILL (kernel real-time watchdog, hard limit: a real-time thread used too \
                    much CPU time)";
        // Captured shape: `RT Watchdog Timeout (soft): overrunning_rea[424581]`.
        let own = kernel_line(10, "RT Watchdog Timeout (hard): mumcord[424581]");
        assert_eq!(reason(&run(), &own).as_deref(), Some(hard));

        // An audio thread has its own id; the kill of the process half a second later links it.
        let thread = kernel_line(
            1_000_000,
            "RT Watchdog Timeout (hard): cpal_alsa_out[424600]",
        );
        let journal = format!("{thread}\n{}", unit_line(1_500_000, UNIT, KILLED));
        assert_eq!(reason(&run(), &journal).as_deref(), Some(hard));

        let soft = kernel_line(
            1_000_000,
            "RT Watchdog Timeout (soft): cpal_alsa_out[424600]",
        );
        let journal = format!(
            "{soft}\n{}",
            unit_line(
                1_500_000,
                UNIT,
                "Main process exited, code=killed, status=24/XCPU"
            )
        );
        assert_eq!(
            reason(&run(), &journal).as_deref(),
            Some(
                "SIGXCPU (kernel real-time watchdog, soft limit: a real-time thread used too \
                 much CPU time)"
            )
        );

        // Too long before the exit, or a soft limit before a SIGKILL: someone else's thread.
        let journal = format!("{thread}\n{}", unit_line(9_000_000, UNIT, KILLED));
        assert_eq!(
            reason(&run(), &journal).as_deref(),
            Some("SIGKILL (from outside the app)")
        );
        let journal = format!("{soft}\n{}", unit_line(1_500_000, UNIT, KILLED));
        assert_eq!(
            reason(&run(), &journal).as_deref(),
            Some("SIGKILL (from outside the app)")
        );
    }

    #[test]
    fn a_process_id_handed_out_again_later_is_not_mistaken_for_the_run() {
        // Seen on the capturing machine: hours after the kill another program's thread got
        // the number the app had as process id.
        let exit = unit_line(1_000_000, UNIT, KILLED);
        let later = [
            kernel_line(
                900_000_000,
                "RT Watchdog Timeout (soft): overrunning_rea[424581]",
            ),
            kernel_line(
                900_000_000,
                "RT Watchdog Timeout (hard): overrunning_rea[424581]",
            ),
            kernel_line(
                900_000_000,
                "Out of memory: Killed process 424581 (other) total-vm:9kB",
            ),
        ]
        .join("\n");
        assert_eq!(
            reason(&run(), &format!("{exit}\n{later}")).as_deref(),
            Some("SIGKILL (from outside the app)")
        );
    }

    #[test]
    fn a_run_outside_a_service_has_no_unit() {
        let scope = Run {
            cgroup: "/user.slice/user-1000.slice/user@1000.service/app.slice/app-x-1.scope"
                .to_owned(),
            ..run()
        };
        assert_eq!(scope.unit(), None);
        assert_eq!(run().unit(), Some(UNIT));
        // A unit line cannot be told apart without a unit, a kernel line about the pid can.
        assert_eq!(reason(&scope, &unit_line(1, UNIT, KILLED)), None);
    }

    #[test]
    fn the_run_details_survive_the_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Run::load(dir.path()), None);
        run().save(dir.path());
        assert_eq!(Run::load(dir.path()), Some(run()));
        assert_eq!(
            Run::parse("pid: 1\nboot: not-a-boot-id\nstarted: 2\n"),
            None
        );

        let now = Run::current().unwrap();
        assert_eq!(now.pid, std::process::id());
        assert_eq!(now.boot.len(), 32);
    }

    #[test]
    fn a_run_that_was_not_recorded_is_unknown() {
        assert_eq!(ended_by(None), "unknown (previous run not recorded)");
    }
}
