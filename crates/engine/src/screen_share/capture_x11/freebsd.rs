//! Native-size X11 conversion without a full-frame intermediate RGB buffer.

use hwcodec::I420Frame;

pub(super) fn bgra_to_i420(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    max_w: u32,
    max_h: u32,
) -> Option<I420Frame> {
    let (w, h) = hwcodec::convert::fit(src_w, src_h, max_w, max_h)?;
    if (w, h) != (src_w, src_h) {
        return hwcodec::convert::bgra_to_i420(src, src_w, src_h, max_w, max_h);
    }
    let (width, height) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
    let pixels = width.checked_mul(height)?;
    if src.len() < pixels.checked_mul(4)? {
        return None;
    }
    let mut data = vec![0; I420Frame::buffer_len(w, h)];
    let (y, chroma) = data.split_at_mut(pixels);
    let (u, v) = chroma.split_at_mut(pixels / 4);
    let stride = width * 4;
    // fit() makes native-size output even. One 2x2 source block supplies four luma
    // samples and both chroma samples, preserving hwcodec's BT.601 rounding exactly.
    for cy in 0..height / 2 {
        let source = cy * 2 * stride;
        let (top, bottom) = src[source..source + 2 * stride].split_at(stride);
        let (y_top, y_bottom) = y[cy * 2 * width..(cy + 1) * 2 * width].split_at_mut(width);
        for cx in 0..width / 2 {
            let (a, b) = (&top[cx * 8..cx * 8 + 8], &bottom[cx * 8..cx * 8 + 8]);
            y_top[cx * 2] = luma(&a[..4]);
            y_top[cx * 2 + 1] = luma(&a[4..]);
            y_bottom[cx * 2] = luma(&b[..4]);
            y_bottom[cx * 2 + 1] = luma(&b[4..]);
            let average = |channel: usize| {
                (i32::from(a[channel])
                    + i32::from(a[channel + 4])
                    + i32::from(b[channel])
                    + i32::from(b[channel + 4])
                    + 2)
                    / 4
            };
            let (r, g, b) = (average(2), average(1), average(0));
            let chroma = cy * (width / 2) + cx;
            u[chroma] = (128 + ((-38 * r - 74 * g + 112 * b + 128) >> 8)) as u8;
            v[chroma] = (128 + ((112 * r - 94 * g - 18 * b + 128) >> 8)) as u8;
        }
    }
    I420Frame::new(w, h, data).ok()
}

fn luma(bgra: &[u8]) -> u8 {
    let (r, g, b) = (i32::from(bgra[2]), i32::from(bgra[1]), i32::from(bgra[0]));
    (16 + ((66 * r + 129 * g + 25 * b + 128) >> 8)) as u8
}

#[cfg(test)]
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
    fn scaled_and_rounded_sizes_keep_reference_sampling() {
        for (w, h, max_w, max_h) in [
            (16, 12, 8, 8),
            (17, 11, 17, 11),
            (7, 5, 7, 5),
            (2, 2, 16, 16),
            (1, 1, 2, 2),
        ] {
            matches_reference(&pattern(w, h), w, h, max_w, max_h);
        }
    }

    #[test]
    fn short_input_invalid_boxes_and_trailing_bytes_match_reference() {
        for len in [0, 1, 8 * 4 * 4 - 1, 8 * 4 * 4, 8 * 4 * 4 + 16] {
            matches_reference(&vec![128; len], 8, 4, 8, 4);
        }
        for (w, h, max_w, max_h) in [(0, 4, 8, 4), (8, 0, 8, 4), (8, 4, 1, 4), (8, 4, 8, 0)] {
            matches_reference(&[], w, h, max_w, max_h);
        }
    }
}
