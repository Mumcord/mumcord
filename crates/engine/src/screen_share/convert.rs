//! Picture conversion shared by the capture backends: the OS hands out packed BGRX/BGRA
//! pictures, the encoder takes I420.

use mumcord_screenshare::I420Frame;

/// Size that fits `src` into the box with its aspect ratio kept, both sides even and at least 2.
/// Never enlarges.
pub(crate) fn fit(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> Option<(u32, u32)> {
    if src_w == 0 || src_h == 0 || max_w < 2 || max_h < 2 {
        return None;
    }
    let (mut w, mut h) = (u64::from(src_w), u64::from(src_h));
    let (mw, mh) = (u64::from(max_w), u64::from(max_h));
    if w > mw || h > mh {
        if w * mh > h * mw {
            h = h * mw / w;
            w = mw;
        } else {
            w = w * mh / h;
            h = mh;
        }
    }
    Some((((w & !1).max(2)) as u32, ((h & !1).max(2)) as u32))
}

/// Scales a packed BGRX picture (`src_w * 4` bytes per row) into the box and converts it to
/// I420 (BT.601 limited range). Luma is sampled per output pixel (nearest); chroma is the
/// average of each 2×2 block of output pixels.
pub(crate) fn bgrx_to_i420(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    max_w: u32,
    max_h: u32,
) -> Option<I420Frame> {
    let (w, h) = fit(src_w, src_h, max_w, max_h)?;
    if src.len() < (src_w as usize) * (src_h as usize) * 4 {
        return None;
    }
    let (wu, hu) = (w as usize, h as usize);
    let x_map: Vec<usize> = (0..w)
        .map(|x| (u64::from(x) * u64::from(src_w) / u64::from(w)) as usize)
        .collect();
    let stride = src_w as usize * 4;
    // Scaled RGB first, so luma and chroma both read the picture at its final size.
    let mut rgb = Vec::with_capacity(wu * hu);
    for y in 0..hu {
        let sy = (y as u64 * u64::from(src_h) / u64::from(h)) as usize;
        let row = &src[sy * stride..(sy + 1) * stride];
        rgb.extend(x_map.iter().map(|&sx| {
            let p = &row[sx * 4..sx * 4 + 3];
            [i32::from(p[2]), i32::from(p[1]), i32::from(p[0])]
        }));
    }
    let mut data = Vec::with_capacity(I420Frame::buffer_len(w, h));
    data.extend(
        rgb.iter()
            .map(|&[r, g, b]| (16 + ((66 * r + 129 * g + 25 * b + 128) >> 8)) as u8),
    );
    let block = |cx: usize, cy: usize| -> [i32; 3] {
        let mut sum = [0i32; 3];
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let px = rgb[(cy * 2 + dy) * wu + cx * 2 + dx];
            for (s, v) in sum.iter_mut().zip(px) {
                *s += v;
            }
        }
        sum.map(|s| (s + 2) / 4)
    };
    for plane in 0..2 {
        for cy in 0..hu / 2 {
            for cx in 0..wu / 2 {
                let [r, g, b] = block(cx, cy);
                let value = if plane == 0 {
                    128 + ((-38 * r - 74 * g + 112 * b + 128) >> 8)
                } else {
                    128 + ((112 * r - 94 * g - 18 * b + 128) >> 8)
                };
                data.push(value as u8);
            }
        }
    }
    I420Frame::new(w, h, data).ok()
}

/// Copies a BGRX picture with `stride` bytes per row (rows may be padded) into `out` as tight
/// rows of `width * 4` bytes, ready for [`bgrx_to_i420`]. `false` when `src` is too short.
#[cfg_attr(not(any(target_os = "macos", windows, test)), allow(dead_code))]
pub(crate) fn pack_rows(
    src: &[u8],
    stride: usize,
    width: u32,
    height: u32,
    out: &mut Vec<u8>,
) -> bool {
    let row_len = width as usize * 4;
    let rows = height as usize;
    if stride < row_len || rows == 0 || src.len() < stride * (rows - 1) + row_len {
        return false;
    }
    out.clear();
    out.reserve(row_len * rows);
    for row in src.chunks(stride).take(rows) {
        out.extend_from_slice(&row[..row_len]);
    }
    true
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, bgr: [u8; 3]) -> Vec<u8> {
        (0..w * h)
            .flat_map(|_| [bgr[0], bgr[1], bgr[2], 0])
            .collect()
    }

    #[test]
    fn fit_keeps_aspect_ratio_even_and_never_enlarges() {
        assert_eq!(fit(1920, 1080, 1280, 720), Some((1280, 720)));
        assert_eq!(fit(2560, 1080, 1280, 720), Some((1280, 540)));
        assert_eq!(fit(1000, 1000, 1281, 721), Some((720, 720)));
        assert_eq!(fit(640, 480, 1280, 720), Some((640, 480)));
        assert_eq!(fit(641, 481, 1280, 720), Some((640, 480)));
        assert_eq!(fit(0, 10, 100, 100), None);
        assert_eq!(fit(10, 10, 1, 100), None);
    }

    #[test]
    fn white_and_black_convert_to_limited_range() {
        let white = bgrx_to_i420(&solid(8, 8, [255; 3]), 8, 8, 4, 4).unwrap();
        assert_eq!((white.width(), white.height()), (4, 4));
        assert!(white.data()[..16].iter().all(|&y| y == 235));
        assert!(white.data()[16..].iter().all(|&c| c == 128));
        let black = bgrx_to_i420(&solid(8, 8, [0; 3]), 8, 8, 8, 8).unwrap();
        assert!(black.data()[..64].iter().all(|&y| y == 16));
    }

    #[test]
    fn red_has_high_v_and_low_u() {
        let red = bgrx_to_i420(&solid(4, 4, [0, 0, 255]), 4, 4, 4, 4).unwrap();
        let (u, v) = (red.data()[16], red.data()[20]);
        assert!(u < 128 && v > 200, "u={u} v={v}");
    }

    #[test]
    fn scaling_samples_the_right_half() {
        // Left half black, right half white, scaled down by two.
        let mut src = Vec::new();
        for _ in 0..4 {
            src.extend(solid(4, 1, [0; 3]));
            src.extend(solid(4, 1, [255; 3]));
        }
        let frame = bgrx_to_i420(&src, 8, 4, 4, 2).unwrap();
        assert_eq!((frame.width(), frame.height()), (4, 2));
        assert_eq!(&frame.data()[..4], &[16, 16, 235, 235]);
    }

    #[test]
    fn short_buffers_are_refused() {
        assert!(bgrx_to_i420(&[0; 16], 8, 8, 8, 8).is_none());
    }

    #[test]
    fn padded_rows_are_packed_and_short_ones_refused() {
        // Two rows of one pixel each, padded to 8 bytes.
        let src = [1, 2, 3, 4, 9, 9, 9, 9, 5, 6, 7, 8];
        let mut out = Vec::new();
        assert!(pack_rows(&src, 8, 1, 2, &mut out));
        assert_eq!(out, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(!pack_rows(&src[..11], 8, 1, 2, &mut out));
        assert!(!pack_rows(&src, 3, 1, 2, &mut out));
    }
}
