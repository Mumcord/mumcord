//! Real-time scheduling for the threads that feed the audio devices (issue #123).
//!
//! At normal priority, the capture and playback threads wait behind every busy process when the
//! CPU is loaded (a game, a build), so frames go out late and the device plays gaps. Real-time
//! scheduling lets them run as soon as they wake up. It is requested the way desktop audio apps
//! do without root: rtkit over D-Bus on Linux, the MMCSS "Audio" task on Windows, a
//! time-constraint policy on macOS. When the request is refused (no rtkit, a sandbox), the
//! thread keeps its normal priority and audio works as before.
//!
//! On Linux a real-time thread that runs too long without blocking gets SIGXCPU once it passes
//! the soft `RLIMIT_RTTIME` and SIGKILL at the hard one; rtkit requires both to be set. By
//! default SIGXCPU ends the process (issue #139), so this module handles it: the overrunning
//! thread drops back to normal priority and keeps running. The soft limit sits at half the hard
//! one, so only a real overrun triggers that, never an ordinary burst of work. Windows (MMCSS)
//! and macOS (time-constraint policy) have no such kill: MMCSS keeps a share of the CPU for
//! normal threads, and macOS demotes a thread that exceeds its computation time by itself.

use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};

use audio_thread_priority::{
    RtPriorityHandle, demote_current_thread_from_real_time, promote_current_thread_to_real_time,
};

use crate::{FRAME_SAMPLES, SAMPLE_RATE};

/// Real-time threads returned to normal priority for running past their time budget, since the
/// last [`take_demotions`].
static DEMOTIONS: AtomicU32 = AtomicU32::new(0);

/// Why a thread kept its normal priority.
#[derive(Debug, thiserror::Error)]
#[error("real-time scheduling unavailable: {0}")]
pub struct RtError(String);

/// Real-time scheduling of the current thread; dropping it returns the thread to normal
/// priority. Bound to its thread, so it is neither `Send` nor `Sync`.
#[must_use = "the thread returns to normal priority when the guard is dropped"]
pub struct RtGuard {
    handle: Option<RtPriorityHandle>,
    _thread_bound: PhantomData<*const ()>,
}

impl Drop for RtGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take()
            && let Err(e) = demote_current_thread_from_real_time(handle)
        {
            tracing::debug!("returning an audio thread to normal priority failed: {e}");
        }
    }
}

/// Asks for real-time scheduling of the calling thread, which handles 10 ms frames.
///
/// The thread must block regularly (sleep or wait for the next frame): on Linux a real-time
/// thread that runs about 100 ms without blocking is returned to normal priority (see
/// [`take_demotions`]). Never call it from a device
/// callback; the audio host schedules those itself.
pub fn promote_current_thread() -> Result<RtGuard, RtError> {
    guard_overruns();
    // FRAME_SAMPLES (480) always fits; the conversion cannot fail.
    let frames = u32::try_from(FRAME_SAMPLES).unwrap_or(u32::MAX);
    let handle = {
        // Each promotion resets the process-wide limit to one frame (10 ms); widening it again
        // under the same lock keeps two promoting threads from undoing each other's widening.
        #[cfg(target_os = "linux")]
        let _limits = linux::LIMITS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let handle = promote_current_thread_to_real_time(frames, SAMPLE_RATE)
            .map_err(|e| RtError(e.to_string()))?;
        #[cfg(target_os = "linux")]
        linux::widen_soft_limit();
        handle
    };
    Ok(RtGuard {
        handle: Some(handle),
        _thread_bound: PhantomData,
    })
}

/// How many real-time threads were returned to normal priority for running past their time
/// budget since the last call. Audio keeps playing on them, but a loaded CPU can delay their
/// frames again; callers log it from a normal thread (the signal handler cannot).
pub fn take_demotions() -> u32 {
    DEMOTIONS.swap(0, Ordering::Relaxed)
}

/// Makes an overrunning real-time thread drop to normal priority instead of ending the process;
/// needed before any thread of the process goes real-time. Idempotent.
pub(crate) fn guard_overruns() {
    #[cfg(target_os = "linux")]
    {
        static INSTALL: std::sync::Once = std::sync::Once::new();
        INSTALL.call_once(linux::install_sigxcpu_handler);
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::Mutex;
    use std::sync::atomic::Ordering;

    use super::DEMOTIONS;

    pub(super) static LIMITS: Mutex<()> = Mutex::new(());

    /// Runs on the thread that exceeded the soft `RLIMIT_RTTIME`: the kernel directs that
    /// SIGXCPU at the thread itself. Only async-signal-safe calls: raw syscalls and an atomic.
    extern "C" fn on_sigxcpu(_signal: libc::c_int) {
        // SAFETY: plain syscalls on the calling thread (pid 0); `param` outlives the call.
        unsafe {
            let policy = libc::syscall(libc::SYS_sched_getscheduler, 0)
                & !libc::c_long::from(libc::SCHED_RESET_ON_FORK);
            if policy != libc::c_long::from(libc::SCHED_FIFO)
                && policy != libc::c_long::from(libc::SCHED_RR)
            {
                return;
            }
            let param = libc::sched_param { sched_priority: 0 };
            // rtkit sets reset-on-fork, and only a privileged thread may clear it again.
            if libc::syscall(
                libc::SYS_sched_setscheduler,
                0,
                libc::SCHED_OTHER | libc::SCHED_RESET_ON_FORK,
                &raw const param,
            ) == 0
            {
                DEMOTIONS.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub(super) fn install_sigxcpu_handler() {
        // SAFETY: zeroed `sigaction` structs are valid; both calls only read and write them.
        unsafe {
            let mut previous: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(libc::SIGXCPU, std::ptr::null(), &raw mut previous) != 0 {
                tracing::warn!(
                    "reading the SIGXCPU handler failed: {}",
                    std::io::Error::last_os_error()
                );
                return;
            }
            // Someone else (an embedding app) handles SIGXCPU already; leave theirs in place.
            if previous.sa_sigaction != libc::SIG_DFL {
                return;
            }
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_sigxcpu as extern "C" fn(libc::c_int) as libc::sighandler_t;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&raw mut action.sa_mask);
            if libc::sigaction(libc::SIGXCPU, &raw const action, std::ptr::null_mut()) != 0 {
                tracing::warn!(
                    "installing the SIGXCPU handler failed: {}",
                    std::io::Error::last_os_error()
                );
            }
        }
    }

    /// Raises the soft `RLIMIT_RTTIME` (SIGXCPU) to half the hard one (SIGKILL), leaving room
    /// for bursts and time to demote before the kill.
    pub(super) fn widen_soft_limit() {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: getrlimit and setrlimit only read and write `limit`.
        unsafe {
            if libc::getrlimit(libc::RLIMIT_RTTIME, &raw mut limit) != 0 {
                tracing::debug!(
                    "reading RLIMIT_RTTIME failed: {}",
                    std::io::Error::last_os_error()
                );
                return;
            }
            if limit.rlim_max == libc::RLIM_INFINITY || limit.rlim_cur >= limit.rlim_max / 2 {
                return;
            }
            limit.rlim_cur = limit.rlim_max / 2;
            if libc::setrlimit(libc::RLIMIT_RTTIME, &raw const limit) != 0 {
                tracing::debug!(
                    "raising RLIMIT_RTTIME failed: {}",
                    std::io::Error::last_os_error()
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether the machine grants real-time scheduling varies (rtkit, CI sandbox); either way
    /// the call returns, and the guard demotes the thread again on drop.
    #[test]
    fn promotion_is_best_effort() {
        std::thread::spawn(|| match promote_current_thread() {
            Ok(guard) => drop(guard),
            Err(e) => assert!(
                e.to_string()
                    .starts_with("real-time scheduling unavailable")
            ),
        })
        .join()
        .expect("thread");
    }
}
