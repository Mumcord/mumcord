//! Camera pictures to tight NV12, with the 16:9 crop and the whole-factor shrink of
//! [`Geometry`] (FR-8.26).
//!
//! Runs on the capture thread for every frame, so it only reads the source and writes into the
//! caller's pooled buffer: no allocation.

use crate::mode::Geometry;

/// One camera picture as it arrives. Strides are bytes per row; rows may be padded.
#[derive(Clone, Copy)]
pub enum Picture<'a> {
    Nv12 {
        y: &'a [u8],
        y_stride: usize,
        uv: &'a [u8],
        uv_stride: usize,
    },
    /// Packed 4:2:2, `Y0 U Y1 V`.
    Yuy2 { data: &'a [u8], stride: usize },
    /// One `Y Cb Cr` triple per pixel, as the MJPEG decoder writes it.
    Ycbcr { data: &'a [u8], stride: usize },
}

/// Whether `data` holds `rows` rows of at least `row_len` bytes at `stride`.
fn fits(data: &[u8], stride: usize, row_len: usize, rows: usize) -> bool {
    stride >= row_len && (rows == 0 || data.len() >= stride * (rows - 1) + row_len)
}

/// Writes the cropped and shrunk `width × height` picture into `out` as NV12 (`g.width ×
/// g.height`). `false` when the picture is shorter than its size says or `out` has the wrong
/// size; `out` is then left unspecified.
pub fn to_nv12(
    picture: Picture<'_>,
    width: u32,
    height: u32,
    g: &Geometry,
    out: &mut [u8],
) -> bool {
    let (w, h) = (width as usize, height as usize);
    let (out_w, out_h) = (g.width as usize, g.height as usize);
    if out.len() != out_w * out_h * 3 / 2
        || (g.x + g.width * g.step) as usize > w
        || (g.y + g.height * g.step) as usize > h
    {
        return false;
    }
    match picture {
        Picture::Nv12 {
            y,
            y_stride,
            uv,
            uv_stride,
        } => {
            if !fits(y, y_stride, w, h) || !fits(uv, uv_stride, w.div_ceil(2) * 2, h.div_ceil(2)) {
                return false;
            }
            convert(
                g,
                out,
                |px, row| y[row * y_stride + px],
                |px, row| {
                    let at = (row / 2) * uv_stride + (px / 2) * 2;
                    (uv[at], uv[at + 1])
                },
            );
        }
        Picture::Yuy2 { data, stride } => {
            if !fits(data, stride, w.div_ceil(2) * 4, h) {
                return false;
            }
            convert(
                g,
                out,
                |px, row| data[row * stride + px * 2],
                |px, row| {
                    let at = row * stride + (px / 2) * 4;
                    (data[at + 1], data[at + 3])
                },
            );
        }
        Picture::Ycbcr { data, stride } => {
            if !fits(data, stride, w * 3, h) {
                return false;
            }
            convert(
                g,
                out,
                |px, row| data[row * stride + px * 3],
                |px, row| {
                    let at = row * stride + px * 3;
                    (data[at + 1], data[at + 2])
                },
            );
        }
    }
    true
}

/// Fills `out` from per-pixel accessors (camera coordinates). Every luma sample averages a
/// `step × step` box, every chroma sample the `2·step × 2·step` box of its four output pixels.
fn convert(
    g: &Geometry,
    out: &mut [u8],
    luma: impl Fn(usize, usize) -> u8,
    chroma: impl Fn(usize, usize) -> (u8, u8),
) {
    let (x0, y0, step) = (g.x as usize, g.y as usize, g.step as usize);
    let (out_w, out_h) = (g.width as usize, g.height as usize);
    let (luma_out, chroma_out) = out.split_at_mut(out_w * out_h);

    let luma_box = (step * step) as u32;
    for (oy, row) in luma_out.chunks_exact_mut(out_w).enumerate() {
        let top = y0 + oy * step;
        for (ox, px) in row.iter_mut().enumerate() {
            let left = x0 + ox * step;
            let mut sum = 0u32;
            for y in top..top + step {
                for x in left..left + step {
                    sum += u32::from(luma(x, y));
                }
            }
            *px = average(sum, luma_box);
        }
    }

    let chroma_step = step * 2;
    let chroma_box = (chroma_step * chroma_step) as u32;
    for (cy, row) in chroma_out.chunks_exact_mut(out_w).enumerate() {
        let top = y0 + cy * chroma_step;
        for (cx, pair) in row.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let left = x0 + cx * chroma_step;
            let (mut u, mut v) = (0u32, 0u32);
            for y in top..top + chroma_step {
                for x in left..left + chroma_step {
                    let (cu, cv) = chroma(x, y);
                    u += u32::from(cu);
                    v += u32::from(cv);
                }
            }
            *pair = [average(u, chroma_box), average(v, chroma_box)];
        }
    }
}

fn average(sum: u32, count: u32) -> u8 {
    u8::try_from((sum + count / 2) / count).unwrap_or(u8::MAX)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::nv12_len;

    /// A 8×6 test card: luma `10·x + y`, U `100 + x`, V `200 - y` (per camera pixel).
    const W: u32 = 8;
    const H: u32 = 6;

    fn luma(x: usize, y: usize) -> u8 {
        (10 * x + y) as u8
    }

    fn yuy2(stride: usize) -> Vec<u8> {
        let mut data = vec![0xEE; stride * H as usize];
        for y in 0..H as usize {
            for pair in 0..W as usize / 2 {
                let at = y * stride + pair * 4;
                data[at] = luma(pair * 2, y);
                data[at + 1] = 100 + (pair * 2) as u8;
                data[at + 2] = luma(pair * 2 + 1, y);
                data[at + 3] = 200 - y as u8;
            }
        }
        data
    }

    fn whole(width: u32, height: u32) -> Geometry {
        Geometry {
            x: 0,
            y: 0,
            step: 1,
            width,
            height,
        }
    }

    #[test]
    fn yuy2_with_padded_rows_becomes_tight_nv12() {
        let stride = 40;
        let data = yuy2(stride);
        let mut out = vec![0; nv12_len(W, H)];
        let picture = Picture::Yuy2 {
            data: &data,
            stride,
        };
        assert!(to_nv12(picture, W, H, &whole(W, H), &mut out));

        let (y_plane, uv_plane) = out.split_at(48);
        for y in 0..6 {
            for x in 0..8 {
                assert_eq!(y_plane[y * 8 + x], luma(x, y), "luma at {x},{y}");
            }
        }
        // First chroma pair covers camera rows 0-1, columns 0-1: U 100, V (200+199)/2 rounded.
        assert_eq!(&uv_plane[..2], &[100, 200]);
        // Last pair of the last chroma row: columns 6-7, rows 4-5.
        assert_eq!(&uv_plane[22..24], &[106, 196]);
    }

    #[test]
    fn nv12_and_ycbcr_crop_to_the_same_result() {
        let g = Geometry {
            x: 2,
            y: 2,
            step: 1,
            width: 4,
            height: 2,
        };
        let mut y_plane = vec![0; 8 * 6];
        let mut uv_plane = vec![0; 8 * 3];
        let mut ycbcr = vec![0; 8 * 6 * 3];
        for y in 0..6 {
            for x in 0..8 {
                y_plane[y * 8 + x] = luma(x, y);
                ycbcr[(y * 8 + x) * 3] = luma(x, y);
                ycbcr[(y * 8 + x) * 3 + 1] = 50 + (x / 2) as u8;
                ycbcr[(y * 8 + x) * 3 + 2] = 60 + (y / 2) as u8;
            }
        }
        for cy in 0..3 {
            for cx in 0..4 {
                uv_plane[cy * 8 + cx * 2] = 50 + cx as u8;
                uv_plane[cy * 8 + cx * 2 + 1] = 60 + cy as u8;
            }
        }
        let mut from_nv12 = vec![0; nv12_len(4, 2)];
        let mut from_ycbcr = vec![0; nv12_len(4, 2)];
        let nv12 = Picture::Nv12 {
            y: &y_plane,
            y_stride: 8,
            uv: &uv_plane,
            uv_stride: 8,
        };
        assert!(to_nv12(nv12, W, H, &g, &mut from_nv12));
        let packed = Picture::Ycbcr {
            data: &ycbcr,
            stride: 24,
        };
        assert!(to_nv12(packed, W, H, &g, &mut from_ycbcr));
        assert_eq!(from_nv12, from_ycbcr);
        assert_eq!(from_nv12, [22, 32, 42, 52, 23, 33, 43, 53, 51, 61, 52, 61]);
    }

    #[test]
    fn shrinking_averages_boxes() {
        let data = yuy2(16);
        let g = Geometry {
            x: 0,
            y: 0,
            step: 2,
            width: 4,
            height: 2,
        };
        let mut out = vec![0; nv12_len(4, 2)];
        assert!(to_nv12(
            Picture::Yuy2 {
                data: &data,
                stride: 16
            },
            W,
            H,
            &g,
            &mut out
        ));
        // Box of columns 0-1 and rows 0-1: (0 + 10 + 1 + 11) / 4 = 5.5, rounded up.
        assert_eq!(out[0], 6);
        // Chroma box columns 0-3, rows 0-3: U (100·2 + 102·2)/4 = 101, V (200+199+198+197)/4.
        assert_eq!(&out[8..10], &[101, 199]);
    }

    #[test]
    fn short_or_mismatched_buffers_are_refused() {
        let data = yuy2(16);
        let mut out = vec![0; nv12_len(W, H)];
        let short = Picture::Yuy2 {
            data: &data[..data.len() - 1],
            stride: 16,
        };
        assert!(!to_nv12(short, W, H, &whole(W, H), &mut out));
        let narrow = Picture::Yuy2 {
            data: &data,
            stride: 8,
        };
        assert!(!to_nv12(narrow, W, H, &whole(W, H), &mut out));
        let mut wrong_out = vec![0; 10];
        let fine = Picture::Yuy2 {
            data: &data,
            stride: 16,
        };
        assert!(!to_nv12(fine, W, H, &whole(W, H), &mut wrong_out));
        assert!(!to_nv12(fine, W, H, &whole(W + 2, H), &mut out));
    }
}
