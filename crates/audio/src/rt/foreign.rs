//! Real-time threads in Mumcord's process that are not audio threads (issue #844).
//!
//! `RLIMIT_RTTIME` counts for every real-time thread of the process, not only for the audio
//! threads it was set for. WebKitGTK up to 2.52 makes some of its own threads SCHED_RR in the
//! app's main process (`ReceiveQueue`), and there is no switch to turn that off. Such a thread
//! is not written for a time budget: when it stays busy past the hard limit, for example inside
//! the kernel while the machine swaps, the kernel ends the whole app with SIGKILL, and no signal
//! handler can prevent that.
//!
//! So a sweeper on a normal thread returns every real-time thread that is not an audio thread to
//! normal scheduling, about once a second. It is the call WebKit itself uses to demote its
//! threads, needs no privilege, and unlike a signal handler it has no deadline to meet. A new
//! foreign real-time thread is exposed for up to one pause; if it overruns in that time, the
//! SIGXCPU handler demotes it, which the sweeper also keeps installed.
//!
//! Which threads are audio threads is decided by thread id, not by name:
//! - Mumcord's own threads enter the registry before they ask for real-time scheduling and
//!   leave it when their [`super::RtGuard`] is dropped.
//! - The audio host's device threads (cpal) are promoted by the host, so they enter the registry
//!   from Mumcord's device callback, the only code of ours that runs on them. With PipeWire the
//!   host asks for the promotion inside that same first callback, so the thread is registered
//!   before it is real-time. With ALSA the host promotes the thread when it starts, before the
//!   first callback; for that gap only, the host's thread names ([`HOST_THREADS`]) are spared
//!   too. Names alone would not do: they are an internal detail of the host that can change
//!   with any update, and then the device threads would silently lose their priority.

use std::fs;
use std::sync::Once;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

/// Far more than the audio threads of a process: two voice threads per connection, two device
/// threads per stream, the microphone test.
const SLOTS: usize = 64;

/// Thread ids of the audio threads; 0 marks a free slot. Atomics, because the device callbacks
/// and the SIGXCPU path must not take a lock.
static AUDIO_THREADS: [AtomicI32; SLOTS] = [const { AtomicI32::new(0) }; SLOTS];

/// The pause between two sweeps: how long a new foreign real-time thread is exposed, against a
/// scan of `/proc/self/task` that costs one syscall per thread.
const SWEEP_INTERVAL: Duration = Duration::from_secs(1);

/// What cpal 0.18 calls its device threads, on PipeWire and on ALSA. Only needed between the
/// start of such a thread and its first callback (see the module docs).
const HOST_THREADS: [&str; 4] = ["pw_in", "pw_out", "cpal_alsa_in", "cpal_alsa_out"];

fn current_thread_id() -> i32 {
    // SAFETY: gettid takes no arguments and cannot fail.
    unsafe { libc::gettid() }
}

/// Marks the calling thread as an audio thread. False when the registry is full. No allocation
/// and no lock, so a device callback may call it.
pub(super) fn register_current_thread() -> bool {
    let thread = current_thread_id();
    if AUDIO_THREADS
        .iter()
        .any(|slot| slot.load(Ordering::Acquire) == thread)
    {
        return true;
    }
    AUDIO_THREADS.iter().any(|slot| {
        slot.compare_exchange(0, thread, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    })
}

pub(super) fn unregister_current_thread() {
    let thread = current_thread_id();
    for slot in &AUDIO_THREADS {
        // Failure means the slot holds another thread.
        let _ = slot.compare_exchange(thread, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}

/// Sweeps once on the calling thread, for the real-time threads that exist already, and then
/// keeps sweeping on a thread of its own for as long as the process runs.
pub(super) fn start() {
    static START: Once = Once::new();
    START.call_once(|| {
        let mut sweeper = Sweeper::default();
        sweeper.sweep();
        let spawned = std::thread::Builder::new()
            .name("mumcord-rt-sweep".to_owned())
            .spawn(move || {
                loop {
                    std::thread::sleep(SWEEP_INTERVAL);
                    sweeper.sweep();
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("starting the sweeper for foreign real-time threads failed: {e}");
        }
    });
}

#[derive(Default)]
struct Sweeper {
    /// Foreign threads already logged, so a thread its owner keeps promoting is logged once.
    logged: Vec<i32>,
    /// Foreign threads returned to normal scheduling in this run.
    demoted: u32,
}

impl Sweeper {
    fn sweep(&mut self) {
        // WebKit installs its SIGXCPU handler whenever it first needs it, possibly long after
        // the window exists, so ours is put back here rather than installed once.
        super::linux::install_sigxcpu_handler();
        forget_ended_threads();
        self.logged.retain(|thread| thread_exists(*thread));

        let tasks = match fs::read_dir("/proc/self/task") {
            Ok(tasks) => tasks,
            Err(e) => {
                tracing::debug!("listing the threads of the process failed: {e}");
                return;
            }
        };
        for task in tasks.flatten() {
            let Some(thread) = task.file_name().to_str().and_then(|n| n.parse().ok()) else {
                continue;
            };
            // Policy first, registry second: a device thread that registers and is promoted
            // between the two checks is seen as registered.
            if !is_realtime(thread) || is_audio_thread(thread) {
                continue;
            }
            let name = thread_name(thread);
            if HOST_THREADS.contains(&name.as_str()) {
                continue;
            }
            self.demote(thread, &name);
        }
    }

    fn demote(&mut self, thread: i32, name: &str) {
        let param = libc::sched_param { sched_priority: 0 };
        // Reset-on-fork stays set: rtkit sets it and only a privileged caller may clear it, and
        // on a thread at normal priority it changes nothing.
        // SAFETY: sched_setscheduler only reads `param`.
        let result = unsafe {
            libc::sched_setscheduler(
                thread,
                libc::SCHED_OTHER | libc::SCHED_RESET_ON_FORK,
                &raw const param,
            )
        };
        let error = (result != 0).then(std::io::Error::last_os_error);
        if self.logged.contains(&thread) {
            return;
        }
        self.logged.push(thread);
        match error {
            None => {
                self.demoted += 1;
                tracing::warn!(
                    thread = name,
                    total = self.demoted,
                    "a real-time thread that is not an audio thread is back at normal priority"
                );
            }
            Some(e) => tracing::warn!(
                thread = name,
                "returning a foreign real-time thread to normal priority failed: {e}"
            ),
        }
    }
}

fn is_audio_thread(thread: i32) -> bool {
    AUDIO_THREADS
        .iter()
        .any(|slot| slot.load(Ordering::Acquire) == thread)
}

/// Frees the slots of device threads that ended; they cannot unregister themselves, and the
/// kernel hands their ids to new threads eventually.
fn forget_ended_threads() {
    for slot in &AUDIO_THREADS {
        let thread = slot.load(Ordering::Acquire);
        if thread != 0 && !thread_exists(thread) {
            // Failure means the slot was reused meanwhile.
            let _ = slot.compare_exchange(thread, 0, Ordering::AcqRel, Ordering::Acquire);
        }
    }
}

fn thread_exists(thread: i32) -> bool {
    fs::metadata(format!("/proc/self/task/{thread}")).is_ok()
}

fn is_realtime(thread: i32) -> bool {
    // SAFETY: queries a thread's policy; no pointers involved. A thread that ended meanwhile
    // gives -1, which is no real-time policy.
    let policy = unsafe { libc::sched_getscheduler(thread) };
    policy >= 0
        && matches!(
            policy & !libc::SCHED_RESET_ON_FORK,
            libc::SCHED_FIFO | libc::SCHED_RR
        )
}

/// The thread's name; empty when the thread ended meanwhile.
fn thread_name(thread: i32) -> String {
    fs::read_to_string(format!("/proc/self/task/{thread}/comm"))
        .map(|name| name.trim_end().to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real-time scheduling the way a foreign library gets it: set directly, no registry.
    fn become_realtime() -> bool {
        let param = libc::sched_param { sched_priority: 5 };
        // SAFETY: only reads `param`; pid 0 is the calling thread.
        unsafe { libc::sched_setscheduler(0, libc::SCHED_RR, &raw const param) == 0 }
    }

    fn on_thread(name: &str, test: fn() -> bool) {
        let ran = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(test)
            .expect("spawn")
            .join()
            .expect("thread");
        if !ran {
            eprintln!("skipped: real-time scheduling unavailable (RLIMIT_RTPRIO is 0)");
        }
    }

    #[test]
    fn registered_thread_stays_realtime_until_it_leaves_the_registry() {
        on_thread("some-thread", || {
            assert!(register_current_thread());
            if !become_realtime() {
                unregister_current_thread();
                return false;
            }
            let thread = current_thread_id();
            Sweeper::default().sweep();
            assert!(is_realtime(thread), "an audio thread keeps its priority");

            unregister_current_thread();
            Sweeper::default().sweep();
            assert!(!is_realtime(thread), "a foreign thread is demoted");
            true
        });
    }

    #[test]
    fn host_device_thread_stays_realtime_before_its_first_callback() {
        on_thread("pw_out", || {
            if !become_realtime() {
                return false;
            }
            Sweeper::default().sweep();
            assert!(is_realtime(current_thread_id()));
            true
        });
    }

    #[test]
    fn slot_of_an_ended_thread_is_freed() {
        let thread = std::thread::spawn(|| {
            assert!(register_current_thread());
            current_thread_id()
        })
        .join()
        .expect("thread");
        assert!(is_audio_thread(thread));
        forget_ended_threads();
        assert!(!is_audio_thread(thread));
    }
}
