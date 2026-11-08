//! The UI watchdog (FR-9.42). Plugin frames live inside the main web view; on macOS and Linux a
//! busy frame can freeze the whole UI (S0), and a plain reload does not recover it. While any
//! plugin frame is open, a thread pings the UI every second; if no answer comes for 5 s, the
//! plugins with open frames get their web content turned off and the UI is restarted the way
//! S0 found to work per platform. Only the UI's web process is touched: the engine, voice and
//! the audio threads keep running throughout.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Listener, Manager, RunEvent, WebviewWindow};

use super::Plugins;

/// The event the ping script sends back; any payload but the ping's number is ignored.
const PONG_EVENT: &str = "plugins://watchdog-pong";
const TICK: Duration = Duration::from_secs(1);
/// No answer for this long while a frame is open means the UI is frozen.
const TIMEOUT: Duration = Duration::from_secs(5);
/// After a restart the UI needs time to load before missing answers count.
const GRACE: Duration = Duration::from_secs(10);

/// Set while the main window is being recreated, so closing the old one does not quit the app.
static RECREATING: AtomicBool = AtomicBool::new(false);

/// What the watchdog does after a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// No plugin frame is open: nothing to watch.
    Idle,
    /// Send ping number `n` to the UI.
    Ping(u64),
    /// The UI is frozen: turn off the open plugins' web content and restart the UI.
    Recover,
}

/// The watchdog's decisions, free of threads and clocks so tests can drive them.
#[derive(Debug)]
struct Watchdog {
    /// The last moment the UI was known to answer (or nothing was watched).
    last_alive: Instant,
    /// Missing answers do not count before this moment.
    quiet_until: Instant,
    /// The number of the last ping sent.
    sent: u64,
    /// Answers to pings before the last restart are stale.
    first_valid: u64,
    /// Test hook: behave as if the UI never answers.
    ignore_answers: bool,
}

impl Watchdog {
    fn new(now: Instant) -> Self {
        Self {
            last_alive: now,
            quiet_until: now,
            sent: 0,
            first_valid: 1,
            ignore_answers: false,
        }
    }

    fn tick(&mut self, now: Instant, frame_open: bool) -> Step {
        if !frame_open || now < self.quiet_until {
            self.last_alive = now;
            if !frame_open {
                return Step::Idle;
            }
        } else if now.saturating_duration_since(self.last_alive) >= TIMEOUT {
            self.last_alive = now;
            self.quiet_until = now + GRACE;
            self.first_valid = self.sent + 1;
            return Step::Recover;
        }
        self.sent += 1;
        Step::Ping(self.sent)
    }

    fn answer(&mut self, ping: u64, now: Instant) {
        if !self.ignore_answers && (self.first_valid..=self.sent).contains(&ping) {
            self.last_alive = now;
        }
    }
}

/// Starts the watchdog thread. Without it the app runs normally, only unprotected.
pub fn start(app: &AppHandle, plugins: &Arc<Plugins>) {
    let (answers, received) = mpsc::channel();
    app.listen(PONG_EVENT, move |event| {
        if let Ok(ping) = event.payload().parse::<u64>() {
            // The thread only stops with the app.
            let _ = answers.send(ping);
        }
    });
    let app = app.clone();
    let plugins = Arc::clone(plugins);
    let spawned = std::thread::Builder::new()
        .name("ui-watchdog".into())
        .spawn(move || run(&app, &plugins, &received));
    if let Err(error) = spawned {
        tracing::error!(%error, "the UI watchdog could not start");
    }
}

/// Keeps the app alive while the watchdog replaces the main window (pass to `App::run`).
pub fn on_run_event(_app: &AppHandle, event: RunEvent) {
    // `code` is `None` only for the automatic exit after the last window closed; an explicit
    // quit always carries one.
    if let RunEvent::ExitRequested {
        api, code: None, ..
    } = event
        && RECREATING.load(Ordering::SeqCst)
    {
        api.prevent_exit();
    }
}

fn run(app: &AppHandle, plugins: &Plugins, answers: &Receiver<u64>) {
    let mut watchdog = Watchdog::new(Instant::now());
    watchdog.ignore_answers = test_hook_frozen_ui();
    loop {
        let deadline = Instant::now() + TICK;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match answers.recv_timeout(left) {
                Ok(ping) => watchdog.answer(ping, Instant::now()),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        let open = plugins.webviews().with_open_frames();
        match watchdog.tick(Instant::now(), !open.is_empty()) {
            Step::Idle => {}
            Step::Ping(ping) => send_ping(app, ping),
            Step::Recover => recover(app, plugins, &open),
        }
    }
}

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

/// Runs a script in the UI that answers through the event system: if it runs, the UI is alive.
fn send_ping(app: &AppHandle, ping: u64) {
    let Some(window) = main_window(app) else {
        return;
    };
    let script = format!(
        "window.__TAURI_INTERNALS__.invoke(\"plugin:event|emit\",{{event:\"{PONG_EVENT}\",payload:{ping}}})"
    );
    if let Err(error) = window.eval(script) {
        tracing::debug!(%error, "pinging the UI failed");
    }
}

fn recover(app: &AppHandle, plugins: &Plugins, open: &[String]) {
    // The UI is frozen, so which frame caused it cannot be told; usually only one is open.
    tracing::warn!(
        plugins = ?open,
        "the window stopped answering while plugin frames were open; restarting it"
    );
    let service = plugins.service();
    for id in open {
        if let Err(error) = super::lock(service).set_web_disabled(id) {
            tracing::error!(%error, plugin = %id, "turning a plugin's web content off failed");
        }
    }
    restart_ui(app);
}

/// WebKitGTK: the frozen web process is killed and the page loaded in a new one (S0).
#[cfg(target_os = "linux")]
fn restart_ui(app: &AppHandle) {
    let Some(window) = main_window(app) else {
        return;
    };
    if let Err(error) = window.with_webview(|webview| linux::restart(&webview.inner())) {
        tracing::error!(%error, "restarting the window's web process failed");
    }
}

/// WKWebView: the frozen web view cannot be reloaded, so the window is built anew (S0).
#[cfg(target_os = "macos")]
fn restart_ui(app: &AppHandle) {
    const GONE_WAIT: Duration = Duration::from_secs(5);
    const ATTEMPTS: u32 = 3;
    RECREATING.store(true, Ordering::SeqCst);
    if let Some(window) = main_window(app)
        && let Err(error) = window.destroy()
    {
        tracing::error!(%error, "closing the frozen window failed; reloading it instead");
        if let Err(error) = window.reload() {
            tracing::error!(%error, "reloading the frozen window failed");
        }
        RECREATING.store(false, Ordering::SeqCst);
        return;
    }
    // Destroying is asynchronous; the new window needs the label back. The window's own web
    // view keeps the label "main" a little longer than the window, and the plugin frames' web
    // views laid over it go with it, so wait for all of them (a recreate before failed three
    // times with "a webview with label `main` already exists").
    let waited = Instant::now();
    while (main_window(app).is_some() || app.get_webview("main").is_some())
        && waited.elapsed() < GONE_WAIT
    {
        std::thread::sleep(Duration::from_millis(50));
    }
    if app.get_webview("main").is_some() {
        tracing::warn!("the frozen window's web view is still there; recreating anyway");
    }
    for attempt in 1..=ATTEMPTS {
        match crate::create_main_window(app) {
            Ok(()) => break,
            Err(error) => {
                tracing::error!(%error, attempt, "recreating the window failed");
                // Give the old web view more time to let go of its label before the next try.
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
    // The automatic exit after the old window closed may still be on its way.
    std::thread::sleep(Duration::from_secs(1));
    RECREATING.store(false, Ordering::SeqCst);
}

/// WebView2 runs frames in their own process, so a frame cannot freeze the UI (S0); if the UI
/// hangs anyway while a frame is open, a reload is the recovery.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn restart_ui(app: &AppHandle) {
    if let Some(window) = main_window(app)
        && let Err(error) = window.reload()
    {
        tracing::error!(%error, "reloading the window failed");
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::c_void;

    use gtk::glib::object::ObjectType;

    // In libwebkit2gtk-4.1, which wry already links.
    unsafe extern "C" {
        fn webkit_web_view_terminate_web_process(view: *mut c_void);
        fn webkit_web_view_reload(view: *mut c_void);
    }

    pub(super) fn restart(view: &impl ObjectType) {
        // SAFETY: `view` is the live WebKitWebView of the main window, used on the main thread
        // within this call. Terminating leaves it usable; reload starts a new web process.
        unsafe {
            webkit_web_view_terminate_web_process(view.as_ptr().cast());
            webkit_web_view_reload(view.as_ptr().cast());
        }
    }
}

/// Debug builds only: `MUMCORD_TEST_UI_FREEZE` makes the watchdog ignore every answer, as if
/// the UI were frozen, so an E2E test can trigger the recovery with a frame open.
#[cfg(debug_assertions)]
fn test_hook_frozen_ui() -> bool {
    std::env::var_os("MUMCORD_TEST_UI_FREEZE").is_some()
}

#[cfg(not(debug_assertions))]
fn test_hook_frozen_ui() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    /// Ticks once a second from `start` for `seconds`, answering every ping when `answering`;
    /// returns the steps.
    fn run_for(
        watchdog: &mut Watchdog,
        start: Instant,
        seconds: u32,
        frame_open: bool,
        answering: bool,
    ) -> Vec<Step> {
        (1..=seconds)
            .map(|s| {
                let now = start + SECOND * s;
                let step = watchdog.tick(now, frame_open);
                if let (Step::Ping(ping), true) = (step, answering) {
                    watchdog.answer(ping, now);
                }
                step
            })
            .collect()
    }

    #[test]
    fn nothing_is_watched_without_an_open_frame() {
        let start = Instant::now();
        let mut watchdog = Watchdog::new(start);
        let steps = run_for(&mut watchdog, start, 30, false, false);
        assert!(steps.iter().all(|step| *step == Step::Idle));
    }

    #[test]
    fn an_answering_ui_is_pinged_every_second_and_never_restarted() {
        let start = Instant::now();
        let mut watchdog = Watchdog::new(start);
        let steps = run_for(&mut watchdog, start, 30, true, true);
        assert_eq!(steps.len(), 30);
        assert!(steps.iter().all(|step| matches!(step, Step::Ping(_))));
        assert_eq!(steps[29], Step::Ping(30));
    }

    #[test]
    fn a_frozen_ui_is_recovered_after_five_seconds_then_given_time_to_load() {
        let start = Instant::now();
        let mut watchdog = Watchdog::new(start);
        watchdog.ignore_answers = true;
        let steps = run_for(&mut watchdog, start, 5, true, true);
        assert_eq!(steps[..4], [1, 2, 3, 4].map(Step::Ping));
        assert_eq!(steps[4], Step::Recover);

        // Ten quiet seconds for the restarted UI (until 15 s), then five more without answers.
        let restarted = start + SECOND * 5;
        let steps = run_for(&mut watchdog, restarted, 14, true, true);
        assert!(!steps[..13].contains(&Step::Recover), "{steps:?}");
        assert_eq!(steps[13], Step::Recover);
    }

    #[test]
    fn the_five_seconds_count_only_while_a_frame_is_open() {
        let start = Instant::now();
        let mut watchdog = Watchdog::new(start);
        // A long stretch without frames (and without answers) does not count.
        run_for(&mut watchdog, start, 60, false, false);
        let opened = start + SECOND * 60;
        let steps = run_for(&mut watchdog, opened, 4, true, false);
        assert!(!steps.contains(&Step::Recover));
        assert_eq!(watchdog.tick(opened + SECOND * 5, true), Step::Recover);
    }

    #[test]
    fn a_late_answer_keeps_the_ui_alive_but_stale_or_unknown_ones_do_not() {
        let start = Instant::now();
        let mut watchdog = Watchdog::new(start);
        let steps = run_for(&mut watchdog, start, 4, true, false);
        assert_eq!(steps[3], Step::Ping(4));
        // An answer to an earlier ping still proves the UI runs.
        watchdog.answer(2, start + SECOND * 4);
        assert_eq!(watchdog.tick(start + SECOND * 5, true), Step::Ping(5));
        // A made-up ping number does not.
        watchdog.answer(99, start + SECOND * 8);
        assert_eq!(watchdog.tick(start + SECOND * 9, true), Step::Recover);
        // Answers to pings from before the restart are stale.
        run_for(&mut watchdog, start + SECOND * 9, 10, true, false);
        let after = start + SECOND * 19;
        watchdog.answer(5, after);
        let steps = run_for(&mut watchdog, after, 4, true, false);
        assert_eq!(steps[3], Step::Recover, "{steps:?}");
    }
}
