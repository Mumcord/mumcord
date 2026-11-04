//! Pure parts of the macOS backend: picking the camera mode, the 16:9 centre crop and copying
//! the NV12 planes out of a pixel buffer (FR-8.26). Kept free of AVFoundation so they are unit
//! tested on every platform.

use crate::CaptureRequest;

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
    /// Size of the frames after the 16:9 centre crop, what the pool buffers hold.
    pub frame: (u32, u32),
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
            let frame = crop_16_9(output.0, output.1, request);
            if frame.0 == 0 || frame.1 == 0 {
                return None;
            }
            let reaches_fps = mode.max_fps + FPS_TOLERANCE >= f64::from(request.max_fps);
            let native = output == (mode.width, mode.height);
            let area = u64::from(frame.0) * u64::from(frame.1);
            let read = u64::from(mode.width) * u64::from(mode.height);
            let key = (reaches_fps, area, native, std::cmp::Reverse(read));
            Some((
                key,
                mode.max_fps,
                Choice {
                    index,
                    output,
                    frame,
                },
            ))
        })
        .max_by(|(a, a_fps, _), (b, b_fps, _)| a.cmp(b).then(a_fps.total_cmp(b_fps)))
        .map(|(_, _, choice)| choice)
}

/// The size to ask the video output for: the mode's size, scaled down with its aspect kept when
/// its 16:9 crop would not fit the request. Even, at least 2×2.
pub fn output_size(width: u32, height: u32, request: CaptureRequest) -> (u32, u32) {
    let (crop_w, crop_h) = crop_16_9(width, height, unlimited());
    // Pictures too small for any 16:9 crop are left as they are; `choose` skips them.
    if crop_w == 0 || crop_h == 0 || (crop_w <= request.max_width && crop_h <= request.max_height) {
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

/// The centred 16:9 part of a `width × height` picture, clamped to the request: (width, height)
/// of the crop, both even. Pictures wider than 16:9 lose their sides, taller ones top and bottom.
pub fn crop_16_9(width: u32, height: u32, request: CaptureRequest) -> (u32, u32) {
    let (width, height) = (u64::from(even(width)), u64::from(even(height)));
    let (mut w, mut h) = if width * 9 > height * 16 {
        (height * 16 / 9, height)
    } else {
        (width, width * 9 / 16)
    };
    w = w.min(u64::from(request.max_width));
    h = h.min(u64::from(request.max_height));
    (even(w as u32), even(h as u32))
}

/// Where the crop of a `width × height` picture starts: (x, y), both even so the chroma samples
/// line up.
pub fn crop_origin(width: u32, height: u32, crop: (u32, u32)) -> (u32, u32) {
    let x = width.saturating_sub(crop.0) / 2;
    let y = height.saturating_sub(crop.1) / 2;
    (x & !1, y & !1)
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

/// One NV12 picture as the OS hands it over: a luma plane and an interleaved chroma plane at
/// half height, each with its own row stride.
pub struct Planes<'a> {
    pub width: u32,
    pub height: u32,
    pub luma: &'a [u8],
    pub luma_stride: usize,
    pub chroma: &'a [u8],
    pub chroma_stride: usize,
}

/// Copies the centred `crop` of `planes` into `out` as tight NV12 (no row padding). Returns false
/// without touching `out` when the planes are too small for their size or `out` does not hold
/// exactly the cropped picture. Runs on the capture thread: no allocation.
pub fn copy_cropped(planes: &Planes<'_>, crop: (u32, u32), out: &mut [u8]) -> bool {
    let (crop_w, crop_h) = (crop.0 as usize, crop.1 as usize);
    let (width, height) = (planes.width as usize, planes.height as usize);
    if crop_w == 0
        || crop_h == 0
        || crop_w > width
        || crop_h > height
        || out.len() != crate::nv12_len(crop.0, crop.1)
        || planes.luma_stride < width
        || planes.chroma_stride < width
        || planes.luma.len() < planes.luma_stride * (height - 1) + width
        || planes.chroma.len() < planes.chroma_stride * (height / 2 - 1) + width
    {
        return false;
    }
    let (x, y) = crop_origin(planes.width, planes.height, crop);
    let (x, y) = (x as usize, y as usize);
    let (luma_out, chroma_out) = out.split_at_mut(crop_w * crop_h);
    for (row, dst) in luma_out.chunks_exact_mut(crop_w).enumerate() {
        let start = (y + row) * planes.luma_stride + x;
        dst.copy_from_slice(&planes.luma[start..start + crop_w]);
    }
    for (row, dst) in chroma_out.chunks_exact_mut(crop_w).enumerate() {
        // Chroma rows cover two luma rows; `x` is even, so it is also the byte offset of the
        // first U/V pair.
        let start = (y / 2 + row) * planes.chroma_stride + x;
        dst.copy_from_slice(&planes.chroma[start..start + crop_w]);
    }
    true
}

fn unlimited() -> CaptureRequest {
    CaptureRequest {
        max_width: u32::MAX,
        max_height: u32::MAX,
        max_fps: u32::MAX,
    }
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
    fn a_4_3_picture_loses_top_and_bottom() {
        let request = CaptureRequest::default();
        assert_eq!(crop_16_9(640, 480, request), (640, 360));
        assert_eq!(crop_16_9(1280, 960, request), (1280, 720));
        assert_eq!(crop_origin(640, 480, (640, 360)), (0, 60));
    }

    #[test]
    fn a_wider_picture_loses_its_sides() {
        let request = CaptureRequest::default();
        assert_eq!(crop_16_9(1000, 360, request), (640, 360));
        assert_eq!(crop_origin(1000, 360, (640, 360)), (180, 0));
    }

    #[test]
    fn crops_are_even_and_clamped_to_the_request() {
        let request = CaptureRequest::default();
        assert_eq!(crop_16_9(1280, 720, request), (1280, 720));
        assert_eq!(crop_16_9(1920, 1080, request), (1280, 720));
        // 352×288 (CIF): 16:9 of 352 wide is 198 rows.
        assert_eq!(crop_16_9(352, 288, request), (352, 198));
        let (w, h) = crop_16_9(353, 289, request);
        assert_eq!((w % 2, h % 2), (0, 0));
        assert_eq!(crop_origin(352, 288, (352, 198)), (0, 44));
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
        assert_eq!(choice.frame, (1280, 720));
    }

    #[test]
    fn the_frame_rate_comes_before_the_size() {
        let modes = [mode(1280, 720, 10.0), mode(640, 480, 29.97)];
        let choice = choose(&modes, CaptureRequest::default()).expect("a mode");
        assert_eq!(choice.index, 1);
        assert_eq!(choice.frame, (640, 360));
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

    /// An 8×6 NV12 picture with padded rows: luma value = 10 × row + column, chroma U = 100 +
    /// pair, V = 200 + chroma row.
    fn padded_picture() -> (Vec<u8>, Vec<u8>) {
        let stride = 12;
        let mut luma = vec![0xEE; stride * 6];
        for row in 0..6 {
            for col in 0..8 {
                luma[row * stride + col] = (10 * row + col) as u8;
            }
        }
        let mut chroma = vec![0xEE; stride * 3];
        for row in 0..3 {
            for pair in 0..4 {
                chroma[row * stride + 2 * pair] = (100 + pair) as u8;
                chroma[row * stride + 2 * pair + 1] = (200 + row) as u8;
            }
        }
        (luma, chroma)
    }

    #[test]
    fn copies_the_centred_crop_without_padding() {
        let (luma, chroma) = padded_picture();
        let planes = Planes {
            width: 8,
            height: 6,
            luma: &luma,
            luma_stride: 12,
            chroma: &chroma,
            chroma_stride: 12,
        };
        // Crop 4×2 from (2, 2).
        let mut out = vec![0; crate::nv12_len(4, 2)];
        assert!(copy_cropped(&planes, (4, 2), &mut out));
        assert_eq!(&out[..8], &[22, 23, 24, 25, 32, 33, 34, 35]);
        assert_eq!(&out[8..], &[101, 201, 102, 201]);
    }

    #[test]
    fn refuses_mismatched_buffers() {
        let (luma, chroma) = padded_picture();
        let planes = Planes {
            width: 8,
            height: 6,
            luma: &luma,
            luma_stride: 12,
            chroma: &chroma[..20],
            chroma_stride: 12,
        };
        let mut out = vec![7; crate::nv12_len(4, 2)];
        assert!(!copy_cropped(&planes, (4, 2), &mut out));
        assert!(out.iter().all(|&b| b == 7));
        let planes = Planes {
            chroma: &chroma,
            ..planes
        };
        let mut wrong = vec![0; crate::nv12_len(4, 2) + 1];
        assert!(!copy_cropped(&planes, (4, 2), &mut wrong));
        assert!(!copy_cropped(&planes, (10, 2), &mut out));
    }
}
