//! Software AV1 self-test (FR-7.56): SVT-AV1 encodes about one second of screen-like frames at
//! the share's size, rate and Optimize for; it passes when an average frame takes clearly less
//! than half its time budget.

use std::time::{Duration, Instant};

use mumcord_screenshare::{Av1Encoder, Av1Options, EncoderConfig, I420Frame};
use mumcord_store::ScreenOptimize;

/// One test never takes longer than this (spec 07 §9); a slower machine fails it.
pub const BUDGET: Duration = Duration::from_secs(2);

/// Average milliseconds per frame (rounded up); `None` when SVT-AV1 did not start or failed.
pub fn measure(height: u32, fps: u32, optimize: ScreenOptimize) -> Option<u32> {
    let config = EncoderConfig {
        width: width_for(height),
        height,
        fps,
        bitrate_kbps: if height >= 1080 { 4000 } else { 2000 },
    };
    let options = Av1Options {
        screen_content: optimize == ScreenOptimize::Text,
    };
    let mut encoder = match Av1Encoder::new(config, options) {
        Ok(encoder) => encoder,
        Err(e) => {
            tracing::warn!(error = %e, "AV1 self-test encoder did not start");
            return None;
        }
    };
    let mut frame = super::test_frame(config.width, height, 0).into_data();
    let y_len = (config.width * height) as usize;
    let started = Instant::now();
    let mut timed = Duration::ZERO;
    let mut frames = 0u32;
    // The first frame is a keyframe and also warms the encoder up; it is not timed.
    for n in 0..=fps {
        // Scroll the picture a little, like text in a moving window.
        frame[..y_len].rotate_left(config.width as usize * 4);
        let picture = I420Frame::new(config.width, height, frame).ok()?;
        let began = Instant::now();
        let result = encoder.encode(&picture, i64::from(n * 1000 / fps), n == 0);
        let took = began.elapsed();
        frame = picture.into_data();
        if let Err(e) = result {
            tracing::warn!(error = %e, "AV1 self-test frame failed");
            return None;
        }
        if n > 0 {
            timed += took;
            frames += 1;
        }
        if started.elapsed() > BUDGET {
            break;
        }
    }
    let average = timed.as_secs_f64() * 1000.0 / f64::from(frames.max(1));
    Some(average.ceil() as u32)
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
        assert!(started.elapsed() < BUDGET + Duration::from_secs(2));
    }
}
