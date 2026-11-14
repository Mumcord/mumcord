//! Software AV1 self-test (FR-7.56): hwcodec lets SVT-AV1 encode about one second of screen-like
//! frames at the share's size, rate and Optimize for; it passes when an average frame takes
//! clearly less than half its time budget.

use hwcodec::{Av1Options, measure_av1};
use mumcord_store::ScreenOptimize;

/// Average milliseconds per frame (rounded up); `None` when SVT-AV1 did not start or failed.
pub fn measure(height: u32, fps: u32, optimize: ScreenOptimize) -> Option<u32> {
    let options = Av1Options::new(optimize == ScreenOptimize::Text);
    match measure_av1(width_for(height), height, fps, options) {
        Ok(result) => {
            let average = result.average_frame_time.as_secs_f64() * 1000.0;
            let frame_ms = average.ceil() as u32;
            tracing::info!(
                height,
                fps,
                ?optimize,
                frame_ms,
                passed = passes(frame_ms, fps),
                "AV1 self-test measured"
            );
            Some(frame_ms)
        }
        Err(e) => {
            tracing::warn!(height, fps, ?optimize, error = %e, "AV1 self-test did not run");
            None
        }
    }
}

/// Passed if the average frame takes less than half of `1 s / fps`.
pub fn passes(frame_ms: u32, fps: u32) -> bool {
    u64::from(frame_ms) * 2 * u64::from(fps) < 1000
}

fn width_for(height: u32) -> u32 {
    match height {
        1440 => 2560,
        1080 => 1920,
        720 => 1280,
        _ => 854,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn passes_below_half_the_frame_budget() {
        // 30 fps: 33 ms per frame, half is 16.7 ms.
        assert!(passes(16, 30));
        assert!(!passes(17, 30));
        // 15 fps: 66 ms per frame.
        assert!(passes(33, 15));
        assert!(!passes(34, 15));
    }

    #[cfg(feature = "desktop-codecs")]
    #[test]
    fn measures_a_small_size_within_the_budget() {
        let started = Instant::now();
        let ms = measure(480, 15, ScreenOptimize::Balanced).expect("SVT-AV1 runs");
        assert!(ms > 0);
        assert!(started.elapsed() < Duration::from_secs(4));
    }
}
