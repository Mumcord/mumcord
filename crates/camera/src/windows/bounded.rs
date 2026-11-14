//! A stop that cannot hang its caller.
//!
//! Stopping a capture waits for the capture thread, and that thread can sit in a driver call
//! that never returns (a camera that opened but delivers no picture). The wait happens on the
//! engine's camera path and, when the app closes, on the main thread, so it has to end.

use std::sync::mpsc;
use std::time::Duration;

/// Runs `stop` on a thread named `name` and waits at most `limit` for it. Returns whether it
/// finished; if not, the thread is left behind and ends whenever `stop` returns.
pub(super) fn run_bounded(
    name: &str,
    limit: Duration,
    stop: impl FnOnce() + Send + 'static,
) -> bool {
    let (done, finished) = mpsc::sync_channel::<()>(1);
    let spawned = std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            stop();
            // The receiver is gone once the wait timed out; nobody is told then.
            let _ = done.send(());
        });
    match spawned {
        Ok(_) => finished.recv_timeout(limit).is_ok(),
        Err(err) => {
            tracing::error!(%err, "the camera stop thread could not start");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::run_bounded;

    #[test]
    fn a_stop_that_returns_is_waited_for() {
        let (ran, seen) = mpsc::channel();
        let finished = run_bounded("test-stop", Duration::from_secs(5), move || {
            let _ = ran.send(());
        });
        assert!(finished);
        assert!(seen.try_recv().is_ok());
    }

    /// The freeze on Windows: the capture thread never came back, and neither did the caller.
    #[test]
    fn a_stop_that_hangs_gives_up_after_the_limit() {
        let (release, blocked) = mpsc::channel::<()>();
        let started = Instant::now();
        let finished = run_bounded("test-stop", Duration::from_millis(50), move || {
            let _ = blocked.recv();
        });
        assert!(!finished);
        assert!(started.elapsed() < Duration::from_secs(5));
        drop(release);
    }
}
