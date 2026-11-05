//! Pure parts of the macOS backend: picking the camera mode and the frame rate (FR-8.26). Kept
//! free of AVFoundation so they are unit tested on every platform.

use crate::CaptureRequest;
use crate::mode::Geometry;

/// One capture format a camera offers, as AVFoundation reports it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    /// Highest frame rate of the format's frame rate ranges.
    pub max_fps: f64,
}

/// The chosen format and the picture size the video output is asked to deliver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    /// Index into the modes passed to [`choose`].
    pub index: usize,
    /// Size of the pixel buffers the output delivers: the format's own size, or scaled down
    /// (aspect kept) when even its 16:9 crop is larger than the request.
    pub output: (u32, u32),
    /// The 16:9 centre crop of the output pictures; its size is what the pool buffers hold.
    pub geometry: Geometry,
}

/// Rates like 29.97 count as 30.
const FPS_TOLERANCE: f64 = 0.5;

/// Picks the mode that gives the largest 16:9 picture within the request at its frame rate.
///
/// Modes reaching the requested frame rate win over faster-to-read slower ones; among equal
/// pictures the one that needs no scaling and reads the fewest pixels wins (a native 1280×720
/// beats a 1920×1080 scaled down, and beats a 1280×960 cropped). `None` only for an empty list.
pub fn choose(modes: &[Mode], request: CaptureRequest) -> Option<Choice> {
    modes
        .iter()
        .enumerate()
        .filter_map(|(index, mode)| {
            let output = output_size(mode.width, mode.height, request);
            let geometry =
                Geometry::new(output.0, output.1, request.max_width, request.max_height)?;
            let reaches_fps = mode.max_fps + FPS_TOLERANCE >= f64::from(request.max_fps);
            let native = output == (mode.width, mode.height);
            let area = u64::from(geometry.width) * u64::from(geometry.height);
            let read = u64::from(mode.width) * u64::from(mode.height);
            let key = (reaches_fps, area, native, std::cmp::Reverse(read));
            Some((
                key,
                mode.max_fps,
                Choice {
                    index,
                    output,
                    geometry,
                },
            ))
        })
        .max_by(|(a, a_fps, _), (b, b_fps, _)| a.cmp(b).then(a_fps.total_cmp(b_fps)))
        .map(|(_, _, choice)| choice)
}

/// The size to ask the video output for: the mode's size, scaled down with its aspect kept when
/// its 16:9 crop would not fit the request. Even, at least 2×2.
pub fn output_size(width: u32, height: u32, request: CaptureRequest) -> (u32, u32) {
    // Pictures too small for any 16:9 crop are left as they are; `choose` skips them.
    let Some(Geometry {
        width: crop_w,
        height: crop_h,
        ..
    }) = Geometry::new(width, height, u32::MAX, u32::MAX)
    else {
        return (even(width), even(height));
    };
    if crop_w <= request.max_width && crop_h <= request.max_height {
        return (even(width), even(height));
    }
    // Scale factor num/den = the smaller of max_width/crop_w and max_height/crop_h, in integers
    // so 1920×1440 lands on exactly 1280×960.
    let (num, den) = if u64::from(request.max_width) * u64::from(crop_h)
        <= u64::from(request.max_height) * u64::from(crop_w)
    {
        (request.max_width, crop_w)
    } else {
        (request.max_height, crop_h)
    };
    let scaled = |side: u32| even((u64::from(side) * u64::from(num) / u64::from(den)) as u32);
    (scaled(width).max(2), scaled(height).max(2))
}

/// How to set the camera's frame rate within the chosen format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rate {
    /// A frame rate range contains the wanted rate; ask for exactly that.
    Wanted(u32),
    /// No range contains it; use the fastest rate of this range (index into the ranges), the
    /// closest to the wanted one. Only the range's own values are safe to set: AVFoundation
    /// throws on anything outside the format's ranges.
    RangeMax(usize),
}

/// Picks the frame rate for `wanted` fps from a format's `(min_fps, max_fps)` ranges: the wanted
/// rate when a range holds it, otherwise the fastest range at or below it, otherwise the slowest
/// one above it. `None` only for an empty list.
pub fn frame_rate(ranges: &[(f64, f64)], wanted: u32) -> Option<Rate> {
    let target = f64::from(wanted);
    if ranges
        .iter()
        .any(|&(min, max)| min <= target && target <= max)
    {
        return Some(Rate::Wanted(wanted));
    }
    let below = ranges
        .iter()
        .enumerate()
        .filter(|(_, (_, max))| *max <= target + FPS_TOLERANCE)
        .max_by(|(_, a), (_, b)| a.1.total_cmp(&b.1));
    let above = || {
        ranges
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.1.total_cmp(&b.1))
    };
    below.or_else(above).map(|(index, _)| Rate::RangeMax(index))
}

fn even(value: u32) -> u32 {
    value & !1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(width: u32, height: u32, max_fps: f64) -> Mode {
        Mode {
            width,
            height,
            max_fps,
        }
    }

    #[test]
    fn big_modes_are_scaled_down_to_fit() {
        let request = CaptureRequest::default();
        assert_eq!(output_size(1920, 1080, request), (1280, 720));
        assert_eq!(output_size(1280, 720, request), (1280, 720));
        assert_eq!(output_size(1280, 960, request), (1280, 960));
        // 4:3 at 1920×1440: its crop is 1920×1080, so it scales by 2/3 to 1280×960.
        assert_eq!(output_size(1920, 1440, request), (1280, 960));
    }

    #[test]
    fn native_720p30_wins() {
        let modes = [
            mode(1920, 1080, 30.0),
            mode(640, 480, 30.0),
            mode(1280, 720, 30.0),
            mode(1280, 960, 30.0),
        ];
        let choice = choose(&modes, CaptureRequest::default()).expect("a mode");
        assert_eq!(choice.index, 2);
        assert_eq!(choice.output, (1280, 720));
        assert_eq!((choice.geometry.width, choice.geometry.height), (1280, 720));
    }

    #[test]
    fn the_frame_rate_comes_before_the_size() {
        let modes = [mode(1280, 720, 10.0), mode(640, 480, 29.97)];
        let choice = choose(&modes, CaptureRequest::default()).expect("a mode");
        assert_eq!(choice.index, 1);
        assert_eq!((choice.geometry.width, choice.geometry.height), (640, 360));
    }

    #[test]
    fn a_camera_with_only_big_modes_is_scaled() {
        let modes = [mode(3840, 2160, 30.0), mode(1920, 1080, 30.0)];
        let choice = choose(&modes, CaptureRequest::default()).expect("a mode");
        assert_eq!(choice.index, 1);
        assert_eq!(choice.output, (1280, 720));
    }

    #[test]
    fn a_slow_camera_still_gets_its_fastest_mode() {
        let modes = [mode(1280, 720, 10.0), mode(1280, 720, 15.0)];
        assert_eq!(
            choose(&modes, CaptureRequest::default())
                .expect("a mode")
                .index,
            1
        );
        assert_eq!(choose(&[], CaptureRequest::default()), None);
        assert_eq!(choose(&[mode(2, 2, 30.0)], CaptureRequest::default()), None);
    }

    #[test]
    fn frame_rates_stay_inside_the_ranges() {
        assert_eq!(frame_rate(&[(1.0, 30.0)], 30), Some(Rate::Wanted(30)));
        assert_eq!(frame_rate(&[(30.0, 30.0)], 30), Some(Rate::Wanted(30)));
        // 29.97 fps only: its own maximum, never a rate the format does not offer.
        assert_eq!(
            frame_rate(&[(15.0, 15.0), (29.97, 29.97)], 30),
            Some(Rate::RangeMax(1))
        );
        assert_eq!(
            frame_rate(&[(60.0, 60.0), (50.0, 50.0)], 30),
            Some(Rate::RangeMax(1))
        );
        assert_eq!(frame_rate(&[], 30), None);
    }
}
