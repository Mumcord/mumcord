//! Turning a captured picture into the encoder's I420 in one pass (FR-2.42 – FR-2.46).
//!
//! Every capture backend hands over packed BGRX at the source's own size — often far bigger than
//! the quality step being sent (a 6K desktop at 1080p) — while the encoders take I420 at the
//! output size. `hwcodec::convert::bgra_to_i420` scales through a full-size RGB buffer first
//! (24 MiB per frame at 1080p); on a slow CPU that intermediate alone costs more than the encode
//! (measured on an i7-2700K, 6144×3456 into 1920×1080: 19 ms against 9 ms of VP9), which is
//! exactly the frame budget the share is missing. This module reads each source pixel once and
//! writes luma and chroma straight into the output, with the reference's sampling and rounding,
//! so the bytes it produces are the same.

use hwcodec::I420Frame;

/// Scales a packed BGRA picture (`src_w * 4` bytes per row) into the box and converts it to
/// I420 (BT.601 limited range), exactly as [`hwcodec::convert::bgra_to_i420`] does: luma is
/// sampled per output pixel (nearest), chroma is the average of each 2×2 block of output pixels.
/// `None` when there is nothing to scale (a side of 0, a box below 2) or when `src` holds fewer
/// than `src_w * src_h * 4` bytes.
pub(super) fn bgra_to_i420(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    max_w: u32,
    max_h: u32,
) -> Option<I420Frame> {
    let (w, h) = hwcodec::convert::fit(src_w, src_h, max_w, max_h)?;
    let (width, height) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
    let stride = usize::try_from(src_w).ok()?.checked_mul(4)?;
    if src.len() < stride.checked_mul(usize::try_from(src_h).ok()?)? {
        return None;
    }
    let mut data = vec![0; I420Frame::buffer_len(w, h)];
    let (luma, chroma) = data.split_at_mut(width * height);
    let (u, v) = chroma.split_at_mut(chroma.len() / 2);
    let planes = Planes {
        luma,
        u,
        v,
        width,
        height,
    };
    if (w, h) == (src_w, src_h) {
        fill_native(src, stride, planes);
    } else {
        fill_scaled(src, stride, src_w, src_h, planes);
    }
    I420Frame::new(w, h, data).ok()
}

/// The output planes of one picture, at the size it is written in.
struct Planes<'a> {
    luma: &'a mut [u8],
    u: &'a mut [u8],
    v: &'a mut [u8],
    width: usize,
    height: usize,
}

impl Planes<'_> {
    /// Writes the four output pixels of one 2×2 block and its chroma sample. `a`, `b` are the
    /// block's two source pixels of the upper output row, `c`, `d` those of the lower one.
    #[inline(always)]
    fn block(&mut self, cx: usize, cy: usize, a: u32, b: u32, c: u32, d: u32) {
        let row = cy * 2 * self.width;
        let top = row + cx * 2;
        self.luma[top] = luma(a);
        self.luma[top + 1] = luma(b);
        self.luma[top + self.width] = luma(c);
        self.luma[top + self.width + 1] = luma(d);
        let (r, g, b) = average([a, b, c, d]);
        let at = cy * (self.width / 2) + cx;
        self.u[at] = (128 + ((-38 * r - 74 * g + 112 * b + 128) >> 8)) as u8;
        self.v[at] = (128 + ((112 * r - 94 * g - 18 * b + 128) >> 8)) as u8;
    }
}

/// Source and output are the same size (a window that is already the step's size): one 2×2
/// source block supplies four luma samples and both chroma samples, with no column lookup.
fn fill_native(src: &[u8], stride: usize, mut planes: Planes<'_>) {
    let width = planes.width;
    for cy in 0..planes.height / 2 {
        let row = cy * 2 * stride;
        let (top, bottom) = src[row..row + 2 * stride].split_at(stride);
        for cx in 0..width / 2 {
            let (a, b) = (&top[cx * 8..cx * 8 + 8], &bottom[cx * 8..cx * 8 + 8]);
            planes.block(cx, cy, pixel(a, 0), pixel(a, 4), pixel(b, 0), pixel(b, 4));
        }
    }
}

/// The source is bigger than the output: every output column and row takes the nearest source
/// one, as the reference does.
fn fill_scaled(src: &[u8], stride: usize, src_w: u32, src_h: u32, mut planes: Planes<'_>) {
    let (width, height) = (planes.width, planes.height);
    let columns: Vec<usize> = (0..width)
        .map(|x| (x as u64 * u64::from(src_w) / width as u64) as usize * 4)
        .collect();
    let rows: Vec<usize> = (0..height)
        .map(|y| (y as u64 * u64::from(src_h) / height as u64) as usize)
        .collect();
    for cy in 0..height / 2 {
        let top = &src[rows[cy * 2] * stride..(rows[cy * 2] + 1) * stride];
        let bottom = &src[rows[cy * 2 + 1] * stride..(rows[cy * 2 + 1] + 1) * stride];
        for cx in 0..width / 2 {
            let (sx0, sx1) = (columns[cx * 2], columns[cx * 2 + 1]);
            planes.block(
                cx,
                cy,
                pixel(top, sx0),
                pixel(top, sx1),
                pixel(bottom, sx0),
                pixel(bottom, sx1),
            );
        }
    }
}

/// One BGRX pixel as 0x00RRGGBB; the alpha byte is ignored, as the reference does.
#[inline(always)]
fn pixel(row: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([row[at], row[at + 1], row[at + 2], 0])
}

#[inline(always)]
fn luma(p: u32) -> u8 {
    let (r, g, b) = (
        ((p >> 16) & 0xff) as i32,
        ((p >> 8) & 0xff) as i32,
        (p & 0xff) as i32,
    );
    (16 + ((66 * r + 129 * g + 25 * b + 128) >> 8)) as u8
}

/// The average of the block's four source pixels, rounded as the reference rounds it.
#[inline(always)]
fn average(p: [u32; 4]) -> (i32, i32, i32) {
    let mut sum = [0i32; 3];
    for p in p {
        sum[0] += ((p >> 16) & 0xff) as i32;
        sum[1] += ((p >> 8) & 0xff) as i32;
        sum[2] += (p & 0xff) as i32;
    }
    ((sum[0] + 2) / 4, (sum[1] + 2) / 4, (sum[2] + 2) / 4)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn pattern(w: u32, h: u32) -> Vec<u8> {
        let mut state = 0x1234_5678u32;
        (0..w * h * 4)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect()
    }

    /// The bytes are the reference's, or a viewer would see a different picture than the codec
    /// rating assumed (the same guarantee the capture conversion has always had).
    fn matches_reference(src: &[u8], w: u32, h: u32, max_w: u32, max_h: u32) {
        assert_eq!(
            bgra_to_i420(src, w, h, max_w, max_h),
            hwcodec::convert::bgra_to_i420(src, w, h, max_w, max_h),
            "{w}x{h} into {max_w}x{max_h}"
        );
    }

    #[test]
    fn native_size_pixels_and_chroma_rounding_match_reference() {
        for (w, h) in [(2, 2), (4, 6), (8, 2), (32, 18), (1920, 1080)] {
            matches_reference(&pattern(w, h), w, h, w, h);
        }
        for colour in [0, 1, 127, 128, 254, 255] {
            matches_reference(&[colour; 8 * 4 * 4], 8, 4, 8, 4);
        }
    }

    #[test]
    fn scaled_and_rounded_sizes_match_reference() {
        for (w, h, max_w, max_h) in [
            (16, 12, 8, 8),
            (17, 11, 17, 11),
            (7, 5, 7, 5),
            (2, 2, 16, 16),
            (1, 1, 2, 2),
            (3840, 2400, 1920, 1080),
            (6144, 3456, 1280, 720),
            (6144, 3456, 854, 480),
            (2560, 1080, 1280, 720),
            (1000, 1000, 1281, 721),
        ] {
            matches_reference(&pattern(w, h), w, h, max_w, max_h);
        }
    }

    #[test]
    fn short_input_invalid_boxes_and_trailing_bytes_match_reference() {
        for len in [0, 1, 8 * 4 * 4 - 1, 8 * 4 * 4, 8 * 4 * 4 + 16] {
            matches_reference(&vec![128; len], 8, 4, 8, 4);
        }
        for (w, h, max_w, max_h) in [
            (0, 4, 8, 4),
            (8, 0, 8, 4),
            (8, 4, 1, 4),
            (8, 4, 8, 0),
            (8, 4, 8, 4),
        ] {
            matches_reference(&[], w, h, max_w, max_h);
        }
        // A scaled picture whose source is short is refused too, not read past its end.
        matches_reference(&[128; 8 * 4 * 4 - 1], 8, 4, 4, 2);
    }

    #[test]
    fn a_scaled_picture_keeps_its_aspect_ratio_and_never_enlarges() {
        let src = pattern(640, 480);
        let frame = bgra_to_i420(&src, 640, 480, 1280, 720).unwrap();
        assert_eq!((frame.width(), frame.height()), (640, 480));
        let wide = bgra_to_i420(&pattern(2560, 1080), 2560, 1080, 1280, 720).unwrap();
        assert_eq!((wide.width(), wide.height()), (1280, 540));
    }
}
