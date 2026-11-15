//! The stacks of all threads of the running process (Linux), for the freeze report.
//!
//! A thread's stack can only be walked from that thread, so the collector sends each thread a
//! signal ([`signal`]) whose handler records the thread's return addresses, the same way the
//! fatal handler in [`super::native`] does: the unwinder plus atomics, no allocation, no locks,
//! no logging. Threads are asked one after the other, so one pre-allocated slot is enough and a
//! real-time audio thread is interrupted once, for the length of one stack walk.
//!
//! This covers every thread, the web view's and the audio stack's included. The only ones left
//! out are threads that block all signals (the graphics driver's workers and GLib's `gmain` do);
//! they are listed as such.

use std::fs;
use std::io;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use super::Symbolizer;

/// Return addresses kept per thread.
const MAX_FRAMES: usize = 64;
/// Room for the handler's own frames, which sit above the interrupted code and are dropped.
const HANDLER_FRAMES: usize = 8;
/// How long one thread gets to answer. A thread answers within microseconds unless it is stuck
/// in the kernel, and the collector must not hang on it.
const ANSWER_TIMEOUT: Duration = Duration::from_millis(200);

/// [`CLAIM`] when no thread is asked.
const IDLE: i32 = 0;
/// [`CLAIM`] while the asked thread copies its frames into [`FRAMES`].
const WRITING: i32 = -1;
/// [`CLAIM`] once [`FRAMES`] and [`COUNT`] hold the asked thread's stack.
const DONE: i32 = -2;

/// The id of the thread whose stack is wanted, or one of the states above. Only the thread
/// named here may fill the slot, so an answer that comes after its timeout is dropped.
static CLAIM: AtomicI32 = AtomicI32::new(IDLE);
static COUNT: AtomicUsize = AtomicUsize::new(0);
static FRAMES: [AtomicUsize; MAX_FRAMES] = [const { AtomicUsize::new(0) }; MAX_FRAMES];
/// One capture at a time: they share the slot.
static CAPTURING: Mutex<()> = Mutex::new(());
/// Whether the handler could be installed.
static READY: OnceLock<bool> = OnceLock::new();

/// One thread at the moment of the capture.
pub(super) struct ThreadStack {
    /// The name the thread gave itself (at most 15 bytes on Linux); `main` for the main thread.
    pub name: String,
    pub stack: Stack,
}

pub(super) enum Stack {
    /// Return addresses, innermost first.
    Frames(Vec<usize>),
    /// The thread did not run the handler in time (stuck in the kernel, or it ended meanwhile).
    NoAnswer,
    /// The thread blocks the signal, so it cannot be asked.
    SignalBlocked,
}

/// The signal that asks a thread for its stack. Mumcord handles the fatal signals, the quit
/// signals and SIGXCPU; glibc keeps the first two real-time signals for itself; WebKit suspends
/// threads with SIGUSR1; GTK, GStreamer, ALSA and PipeWire handle none. A real-time signal from
/// the middle of the range is free, and [`install`] still checks that nobody took it.
fn signal() -> libc::c_int {
    libc::SIGRTMIN() + 20
}

fn thread_id() -> libc::pid_t {
    // SAFETY: gettid(2) takes no arguments, cannot fail and is async-signal-safe.
    let tid = unsafe { libc::syscall(libc::SYS_gettid) };
    libc::pid_t::try_from(tid).unwrap_or_default()
}

fn install() -> bool {
    // SAFETY: zeroed `sigaction` structs are valid; the calls only read and write them. The
    // handler only uses atomics, gettid and the unwinder (see `record`). SA_RESTART keeps the
    // interrupted system calls of the other threads going where the kernel allows it. No
    // SA_ONSTACK: the alternate stack of a Rust thread is too small for a stack walk.
    unsafe {
        let mut previous: libc::sigaction = std::mem::zeroed();
        if libc::sigaction(signal(), std::ptr::null(), &raw mut previous) != 0
            || previous.sa_sigaction != libc::SIG_DFL
        {
            return false;
        }
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = record
            as extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void)
            as libc::sighandler_t;
        action.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
        libc::sigemptyset(&raw mut action.sa_mask);
        libc::sigaction(signal(), &raw const action, std::ptr::null_mut()) == 0
    }
}

/// Where the thread was when the signal arrived. The unwinder reports this address for the
/// frame below the handler's own, which is how those are told apart.
fn interrupted_pc(ctx: *mut libc::c_void) -> Option<usize> {
    let ctx = ctx.cast::<libc::ucontext_t>();
    if ctx.is_null() {
        return None;
    }
    // SAFETY: the kernel passes a valid ucontext to SA_SIGINFO handlers.
    #[cfg(target_arch = "x86_64")]
    let pc = unsafe { (*ctx).uc_mcontext.gregs[libc::REG_RIP as usize] };
    // SAFETY: as above.
    #[cfg(target_arch = "aarch64")]
    let pc = unsafe { (*ctx).uc_mcontext.pc };
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    return usize::try_from(pc).ok();
    // Elsewhere the handler's frames stay at the top of the stack.
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    None
}

/// The handler: if this thread is the one asked, walk its stack and hand the frames over.
extern "C" fn record(_signal: libc::c_int, _info: *mut libc::siginfo_t, ctx: *mut libc::c_void) {
    // The thread goes on after this handler, so the code it interrupted must find its errno
    // untouched.
    // SAFETY: the errno location is valid for the calling thread.
    let errno = unsafe { *libc::__errno_location() };
    let tid = thread_id();
    // A signal that arrives after its timeout finds another id here and does nothing.
    if CLAIM.load(Ordering::Acquire) == tid {
        let mut frames = [0usize; MAX_FRAMES + HANDLER_FRAMES];
        let mut count = 0;
        let mut start = None;
        let pc = interrupted_pc(ctx);
        // SAFETY: the unwinder only reads this thread's own stack, which stands still while the
        // handler runs; the closure only stores into a stack array.
        unsafe {
            backtrace::trace_unsynchronized(|frame| {
                let ip = frame.ip() as usize;
                if start.is_none() && Some(ip) == pc {
                    start = Some(count);
                }
                frames[count] = ip;
                count += 1;
                count < frames.len()
            });
        }
        let frames = &frames[start.unwrap_or(0)..count];
        let kept = frames.len().min(MAX_FRAMES);
        if CLAIM
            .compare_exchange(tid, WRITING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            for (slot, &ip) in FRAMES.iter().zip(&frames[..kept]) {
                slot.store(ip, Ordering::Relaxed);
            }
            COUNT.store(kept, Ordering::Relaxed);
            CLAIM.store(DONE, Ordering::Release);
        }
    }
    // SAFETY: as above.
    unsafe {
        *libc::__errno_location() = errno;
    }
}

/// Whether the thread blocks [`signal`]. Such a thread would never answer, and the signal would
/// stay queued for it.
fn blocks_signal(tid: libc::pid_t) -> bool {
    fs::read_to_string(format!("/proc/self/task/{tid}/status"))
        .ok()
        .and_then(|status| {
            let mask = status.lines().find_map(|l| l.strip_prefix("SigBlk:"))?;
            u64::from_str_radix(mask.trim(), 16).ok()
        })
        .is_some_and(|mask| mask & (1 << (signal() - 1)) != 0)
}

fn stack_of(pid: libc::pid_t, tid: libc::pid_t) -> Stack {
    if blocks_signal(tid) {
        return Stack::SignalBlocked;
    }
    CLAIM.store(tid, Ordering::Release);
    // SAFETY: tgkill(2) only sends a signal; with this process as the thread group it cannot
    // reach another process even if the thread id was reused.
    let sent = unsafe { libc::syscall(libc::SYS_tgkill, pid, tid, signal()) } == 0;
    let deadline = Instant::now() + ANSWER_TIMEOUT;
    while CLAIM.load(Ordering::Acquire) != DONE {
        // Taking the claim back keeps a late answer out of the slot. It fails while the thread
        // is copying its frames ([`WRITING`]), which is a few stores away from done.
        if (!sent || Instant::now() >= deadline)
            && CLAIM
                .compare_exchange(tid, IDLE, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            return Stack::NoAnswer;
        }
        std::thread::sleep(Duration::from_micros(200));
    }
    let count = COUNT.load(Ordering::Relaxed);
    let frames = FRAMES[..count]
        .iter()
        .map(|ip| ip.load(Ordering::Relaxed))
        .collect();
    CLAIM.store(IDLE, Ordering::Release);
    Stack::Frames(frames)
}

/// The stacks of all threads but the calling one, main thread first. Never call it from a
/// signal handler or a real-time thread: it reads `/proc` and waits.
pub(super) fn capture() -> io::Result<Vec<ThreadStack>> {
    if !*READY.get_or_init(install) {
        return Err(io::Error::other(
            "the signal for reading thread stacks is in use",
        ));
    }
    let _one = CAPTURING.lock().unwrap_or_else(PoisonError::into_inner);
    // SAFETY: getpid(2) takes no arguments and cannot fail.
    let pid = unsafe { libc::getpid() };
    let own = thread_id();
    let mut tids: Vec<libc::pid_t> = fs::read_dir("/proc/self/task")?
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .filter(|&tid| tid != own)
        .collect();
    // The main thread's id is the process id.
    tids.sort_by_key(|&tid| (tid != pid, tid));
    Ok(tids
        .into_iter()
        .filter_map(|tid| {
            // A thread that ended since the listing has no name any more and is left out.
            let name = fs::read_to_string(format!("/proc/self/task/{tid}/comm")).ok()?;
            let name = if tid == pid { "main" } else { name.trim() };
            Some(ThreadStack {
                name: name.to_owned(),
                stack: stack_of(pid, tid),
            })
        })
        .collect())
}

/// The report's backtrace section: one block per thread in the given order, each address named
/// as `library symbol+offset`. Threads with the same name and the same stack (idle workers of
/// one pool) share a block.
pub(super) fn render(threads: &[ThreadStack], maps: &str) -> String {
    let mut symbols = Symbolizer::new(maps);
    let mut blocks: Vec<(&str, String, usize)> = Vec::new();
    for thread in threads {
        let body = match &thread.stack {
            Stack::Frames(ips) if ips.is_empty() => "  no frames\n".to_owned(),
            Stack::Frames(ips) => symbols
                .lines(ips)
                .iter()
                .map(|line| format!("{line}\n"))
                .collect(),
            Stack::NoAnswer => "  did not answer in time\n".to_owned(),
            Stack::SignalBlocked => "  blocks signals, not captured\n".to_owned(),
        };
        match blocks
            .iter_mut()
            .find(|(name, same, _)| *name == thread.name && *same == body)
        {
            Some((_, _, count)) => *count += 1,
            None => blocks.push((&thread.name, body, 1)),
        }
    }
    let mut text = format!(
        "{} threads, main thread first (code locations only)\n",
        threads.len()
    );
    for (name, body, count) in blocks {
        if count > 1 {
            text.push_str(&format!("thread {name} (x{count}):\n"));
        } else {
            text.push_str(&format!("thread {name}:\n"));
        }
        text.push_str(&body);
    }
    text.truncate(text.trim_end().len());
    text
}
