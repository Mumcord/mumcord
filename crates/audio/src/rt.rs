//! Real-time scheduling for the threads that feed the audio devices (issue #123).
//!
//! At normal priority, the capture and playback threads wait behind every busy process when the
//! CPU is loaded (a game, a build), so frames go out late and the device plays gaps. Real-time
//! scheduling lets them run as soon as they wake up. It is requested the way desktop audio apps
//! do without root: rtkit over D-Bus on Linux, the MMCSS "Audio" task on Windows, a
//! time-constraint policy on macOS. When the request is refused (no rtkit, a sandbox), the
//! thread keeps its normal priority and audio works as before.

use std::marker::PhantomData;

use audio_thread_priority::{
    RtPriorityHandle, demote_current_thread_from_real_time, promote_current_thread_to_real_time,
};

use crate::{FRAME_SAMPLES, SAMPLE_RATE};

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
/// The thread must block regularly (sleep or wait for the next frame): on Linux rtkit stops a
/// process whose real-time thread runs for 200 ms without blocking. Never call it from a device
/// callback; the audio host schedules those itself.
pub fn promote_current_thread() -> Result<RtGuard, RtError> {
    // FRAME_SAMPLES (480) always fits; the conversion cannot fail.
    let frames = u32::try_from(FRAME_SAMPLES).unwrap_or(u32::MAX);
    promote_current_thread_to_real_time(frames, SAMPLE_RATE)
        .map(|handle| RtGuard {
            handle: Some(handle),
            _thread_bound: PhantomData,
        })
        .map_err(|e| RtError(e.to_string()))
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
