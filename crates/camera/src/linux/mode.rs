//! Capture modes, the 16:9 crop and the choice of mode (FR-8.26).
//!
//! Both Linux paths (portal/PipeWire and V4L2) list what a camera offers as [`Mode`]s and let
//! [`choose`] pick one, so the rule "NV12/YUY2 first, MJPEG only when 720p30 needs it" lives in
//! one place.

use crate::CaptureRequest;

/// Pixel formats Mumcord reads from a camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Nv12,
    /// Packed 4:2:2, `Y0 U Y1 V`.
    Yuy2,
    Mjpeg,
}

impl PixelFormat {
    fn is_raw(self) -> bool {
        self != Self::Mjpeg
    }

    /// Tie-break between formats of equal quality: NV12 needs the least work.
    fn rank(self) -> u8 {
        match self {
            Self::Nv12 => 2,
            Self::Yuy2 => 1,
            Self::Mjpeg => 0,
        }
    }
}

/// Frames per second as a fraction (`num / den`), the way both APIs describe frame rates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rate {
    pub num: u32,
    pub den: u32,
}

impl Rate {
    pub const fn fps(num: u32) -> Self {
        Self { num, den: 1 }
    }

    /// Rounded to whole frames, so 30000/1001 counts as 30.
    pub fn rounded(self) -> u32 {
        if self.den == 0 {
            return 0;
        }
        let (num, den) = (u64::from(self.num), u64::from(self.den));
        u32::try_from((num + den / 2) / den).unwrap_or(u32::MAX)
    }
}

/// One way a camera can deliver pictures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
    pub rate: Rate,
}

/// How a camera picture becomes the delivered frame: a centred 16:9 crop, shrunk by a whole
/// factor when the camera only offers larger pictures than requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    /// Top-left corner of the crop in camera pixels; both even, so chroma samples line up.
    pub x: u32,
    pub y: u32,
    /// Each output pixel averages a `step × step` box of camera pixels.
    pub step: u32,
    /// Delivered frame size; both even, as NV12 needs.
    pub width: u32,
    pub height: u32,
}

impl Geometry {
    /// The crop for a `width × height` camera picture that fits in `max_width × max_height`.
    /// `None` when the picture is too small to give an even-sized frame.
    pub fn new(width: u32, height: u32, max_width: u32, max_height: u32) -> Option<Self> {
        let (width64, height64) = (u64::from(width), u64::from(height));
        // Widest 16:9 area inside the picture: 4:3 cameras lose rows, ultra-wide ones columns.
        let (crop_w, crop_h) = if width64 * 9 >= height64 * 16 {
            ((height64 * 16 / 9) as u32, height)
        } else {
            (width, (width64 * 9 / 16) as u32)
        };
        let max_width = max_width.max(2);
        let max_height = max_height.max(2);
        let step = crop_w
            .div_ceil(max_width)
            .max(crop_h.div_ceil(max_height))
            .max(1);
        let out_w = (crop_w / step) & !1;
        let out_h = (crop_h / step) & !1;
        if out_w == 0 || out_h == 0 {
            return None;
        }
        Some(Self {
            x: ((width - out_w * step) / 2) & !1,
            y: ((height - out_h * step) / 2) & !1,
            step,
            width: out_w,
            height: out_h,
        })
    }
}

/// Ordering key of a mode: smooth frame rate first, then picture size, then frame rate, then
/// the cheaper format, then the smaller camera picture (less to read and crop away).
fn score(mode: &Mode, request: &CaptureRequest) -> Option<(bool, u64, u32, u8, i64)> {
    let geometry = Geometry::new(
        mode.width,
        mode.height,
        request.max_width,
        request.max_height,
    )?;
    let fps = mode.rate.rounded().min(request.max_fps);
    if fps == 0 {
        return None;
    }
    let area = u64::from(geometry.width) * u64::from(geometry.height);
    let source = i64::from(mode.width) * i64::from(mode.height);
    Some((
        fps >= request.max_fps,
        area,
        fps,
        mode.format.rank(),
        -source,
    ))
}

/// Picks the mode to capture with (FR-8.26): the best NV12/YUY2 mode, unless only MJPEG reaches
/// the full request (720p at 30 fps, typical for USB 2 cameras). `None` when nothing is usable.
pub fn choose(modes: &[Mode], request: &CaptureRequest) -> Option<Mode> {
    let best = |raw: bool| {
        modes
            .iter()
            .filter(|mode| mode.format.is_raw() == raw)
            .filter_map(|mode| score(mode, request).map(|key| (key, *mode)))
            .max_by_key(|(key, _)| *key)
    };
    let full_area = u64::from(request.max_width) * u64::from(request.max_height);
    let is_full = |key: &(bool, u64, u32, u8, i64)| key.0 && key.1 >= full_area;
    match (best(true), best(false)) {
        (Some((raw_key, raw)), Some((mjpeg_key, mjpeg))) => {
            if !is_full(&raw_key) && is_full(&mjpeg_key) {
                Some(mjpeg)
            } else {
                Some(raw)
            }
        }
        (Some((_, raw)), None) => Some(raw),
        (None, Some((_, mjpeg))) => Some(mjpeg),
        (None, None) => None,
    }
}

/// Sizes worth trying inside a continuous or stepped size range: the common camera sizes it
/// contains, and its largest size.
pub fn sizes_in_range(
    min: (u32, u32),
    max: (u32, u32),
    step: (u32, u32),
) -> impl Iterator<Item = (u32, u32)> {
    const COMMON: [(u32, u32); 7] = [
        (1280, 720),
        (960, 540),
        (640, 360),
        (1280, 960),
        (960, 720),
        (640, 480),
        (320, 240),
    ];
    let on_grid = move |value: u32, low: u32, high: u32, step: u32| {
        (low..=high).contains(&value) && (step <= 1 || (value - low).is_multiple_of(step))
    };
    COMMON
        .into_iter()
        .chain([max])
        .filter(move |&(w, h)| on_grid(w, min.0, max.0, step.0) && on_grid(h, min.1, max.1, step.1))
}

/// The rate to ask for inside a frame-rate range: the request's rate if the range has it,
/// otherwise the fastest one.
pub fn rate_in_range(min: Rate, max: Rate, request_fps: u32) -> Rate {
    let wanted = Rate::fps(request_fps);
    let at_least = |a: Rate, b: Rate| {
        u64::from(a.num) * u64::from(b.den) >= u64::from(b.num) * u64::from(a.den)
    };
    if at_least(max, wanted) && at_least(wanted, min) {
        wanted
    } else {
        max
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn mode(format: PixelFormat, width: u32, height: u32, fps: u32) -> Mode {
        Mode {
            format,
            width,
            height,
            rate: Rate::fps(fps),
        }
    }

    fn request() -> CaptureRequest {
        CaptureRequest::default()
    }

    #[test]
    fn a_16_9_picture_is_kept_whole() {
        let g = Geometry::new(1280, 720, 1280, 720).unwrap();
        assert_eq!(
            g,
            Geometry {
                x: 0,
                y: 0,
                step: 1,
                width: 1280,
                height: 720
            }
        );
    }

    #[test]
    fn a_4_3_picture_loses_top_and_bottom_rows_equally() {
        let g = Geometry::new(640, 480, 1280, 720).unwrap();
        assert_eq!((g.x, g.y, g.step, g.width, g.height), (0, 60, 1, 640, 360));
        let g = Geometry::new(1280, 960, 1280, 720).unwrap();
        assert_eq!((g.x, g.y, g.width, g.height), (0, 120, 1280, 720));
    }

    #[test]
    fn an_ultra_wide_picture_loses_columns() {
        let g = Geometry::new(2560, 720, 1280, 720).unwrap();
        assert_eq!(
            (g.x, g.y, g.step, g.width, g.height),
            (640, 0, 1, 1280, 720)
        );
    }

    #[test]
    fn a_larger_picture_is_shrunk_by_a_whole_factor() {
        let g = Geometry::new(1920, 1080, 1280, 720).unwrap();
        assert_eq!((g.x, g.y, g.step, g.width, g.height), (0, 0, 2, 960, 540));
        let g = Geometry::new(2560, 1440, 1280, 720).unwrap();
        assert_eq!((g.step, g.width, g.height), (2, 1280, 720));
    }

    #[test]
    fn odd_sizes_give_even_frames_and_an_even_offset() {
        let g = Geometry::new(176, 144, 1280, 720).unwrap();
        assert_eq!((g.width, g.height), (176, 98));
        assert_eq!(g.x % 2, 0);
        assert_eq!(g.y % 2, 0);
        assert!(g.y + g.height <= 144);
        assert_eq!(Geometry::new(1, 1, 1280, 720), None);
    }

    #[test]
    fn raw_720p30_wins_over_mjpeg() {
        let modes = [
            mode(PixelFormat::Mjpeg, 1280, 720, 30),
            mode(PixelFormat::Yuy2, 1280, 720, 30),
            mode(PixelFormat::Yuy2, 640, 480, 30),
        ];
        assert_eq!(choose(&modes, &request()), Some(modes[1]));
    }

    #[test]
    fn mjpeg_is_used_only_when_raw_cannot_reach_720p30() {
        // Typical USB 2 camera: YUY2 720p only at 10 fps.
        let modes = [
            mode(PixelFormat::Yuy2, 1280, 720, 10),
            mode(PixelFormat::Yuy2, 640, 480, 30),
            mode(PixelFormat::Mjpeg, 1280, 720, 30),
        ];
        assert_eq!(choose(&modes, &request()), Some(modes[2]));
    }

    #[test]
    fn mjpeg_that_does_not_reach_720p30_is_not_used() {
        let modes = [
            mode(PixelFormat::Yuy2, 640, 480, 30),
            mode(PixelFormat::Mjpeg, 960, 540, 30),
        ];
        assert_eq!(choose(&modes, &request()), Some(modes[0]));
    }

    #[test]
    fn smooth_rate_beats_size_and_nv12_beats_yuy2() {
        let modes = [
            mode(PixelFormat::Yuy2, 1280, 720, 10),
            mode(PixelFormat::Yuy2, 640, 360, 30),
            mode(PixelFormat::Nv12, 640, 360, 30),
        ];
        assert_eq!(choose(&modes, &request()), Some(modes[2]));
    }

    #[test]
    fn ntsc_rates_count_as_30_and_mjpeg_only_cameras_work() {
        let modes = [Mode {
            format: PixelFormat::Mjpeg,
            width: 1280,
            height: 720,
            rate: Rate {
                num: 30000,
                den: 1001,
            },
        }];
        assert_eq!(choose(&modes, &request()), Some(modes[0]));
        assert_eq!(choose(&[], &request()), None);
    }

    #[test]
    fn a_smaller_request_is_honoured() {
        let small = CaptureRequest {
            max_width: 640,
            max_height: 360,
            max_fps: 15,
        };
        let modes = [
            mode(PixelFormat::Yuy2, 1280, 720, 30),
            mode(PixelFormat::Yuy2, 640, 480, 30),
        ];
        // Both fit after crop (and halving for 720p); the 640×480 one needs less work.
        assert_eq!(choose(&modes, &small), Some(modes[1]));
    }

    #[test]
    fn ranges_offer_common_sizes_and_the_requested_rate() {
        let sizes: Vec<_> = sizes_in_range((320, 240), (1280, 720), (16, 8)).collect();
        assert!(sizes.contains(&(1280, 720)));
        assert!(sizes.contains(&(640, 480)));
        assert!(!sizes.contains(&(1280, 960)));
        assert_eq!(
            rate_in_range(Rate::fps(1), Rate::fps(60), 30),
            Rate::fps(30)
        );
        assert_eq!(
            rate_in_range(Rate::fps(1), Rate::fps(15), 30),
            Rate::fps(15)
        );
    }
}
