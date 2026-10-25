//! Opt-in diagnostics (`MUMCORD_OVERLAY_DEBUG=1`), written to stderr so they land in the game's
//! own log, e.g. Proton's `steam-<appid>.log` with `PROTON_LOG=1`. A file path would not work
//! inside the Steam Linux Runtime container. Off by default: the layer then stays silent. Never
//! logs names or anything else the user said or typed.
//!
//! The present path only bumps a counter per outcome; one summary line every few seconds says
//! how many frames got the overlay and why the others did not.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static ENABLED: OnceLock<bool> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();
/// Seconds since [`START`] of the last summary line.
static LAST_SUMMARY: AtomicU64 = AtomicU64::new(0);
static COUNTS: [AtomicU64; Outcome::ALL.len()] = [const { AtomicU64::new(0) }; Outcome::ALL.len()];

const SUMMARY_EVERY_SECS: u64 = 5;

pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| {
        std::env::var_os("MUMCORD_OVERLAY_DEBUG").is_some_and(|v| !v.is_empty() && v != "0")
    })
}

/// Writes one diagnostic line when debugging is on.
macro_rules! debug {
    ($($arg:tt)*) => {
        if $crate::log::enabled() {
            eprintln!("[mumcord-overlay] {}", format_args!($($arg)*));
        }
    };
}
pub(crate) use debug;

/// What happened to one present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Drawn,
    /// Mumcord sent nothing to show (not connected, overlay off for the game, or nobody to list).
    NothingToShow,
    SeveralSwapchains,
    UnknownQueue,
    NotGraphicsQueue,
    /// The swapchain was not created through the layer or its renderer could not be built.
    NoRenderer,
    TooManyWaits,
    /// The previous overlay draw into this swapchain image has not finished yet.
    ImageBusy,
    CommandPoolFailed,
    /// No overlay image has been uploaded yet (the slot for it is still in use).
    WaitingForUpload,
    UploadFailed,
    DoesNotFit,
    RecordFailed,
    SubmitFailed,
}

impl Outcome {
    const ALL: [Self; 14] = [
        Self::Drawn,
        Self::NothingToShow,
        Self::SeveralSwapchains,
        Self::UnknownQueue,
        Self::NotGraphicsQueue,
        Self::NoRenderer,
        Self::TooManyWaits,
        Self::ImageBusy,
        Self::CommandPoolFailed,
        Self::WaitingForUpload,
        Self::UploadFailed,
        Self::DoesNotFit,
        Self::RecordFailed,
        Self::SubmitFailed,
    ];
}

/// Counts a present and, at most every few seconds, prints the counts since the last summary.
/// Allocation-free; a no-op while debugging is off.
pub fn present(outcome: Outcome) {
    if !enabled() {
        return;
    }
    COUNTS[outcome as usize].fetch_add(1, Ordering::Relaxed);
    let now = START.get_or_init(Instant::now).elapsed().as_secs();
    let last = LAST_SUMMARY.load(Ordering::Relaxed);
    if now < last + SUMMARY_EVERY_SECS
        || LAST_SUMMARY
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
    {
        return;
    }
    let mut stderr = std::io::stderr().lock();
    let _ = std::io::Write::write_all(&mut stderr, b"[mumcord-overlay] presents:");
    for outcome in Outcome::ALL {
        let count = COUNTS[outcome as usize].swap(0, Ordering::Relaxed);
        if count > 0 {
            let _ = std::io::Write::write_fmt(&mut stderr, format_args!(" {outcome:?}={count}"));
        }
    }
    let _ = std::io::Write::write_all(&mut stderr, b"\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_outcome_has_its_own_counter() {
        for (i, outcome) in Outcome::ALL.iter().enumerate() {
            assert_eq!(*outcome as usize, i);
        }
    }
}
