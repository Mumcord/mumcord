use crate::{Error, Result};

/// Widest picture a decoder hands out: the screen-share wire format's limit (8K UHD). Defined
/// here because the engine depends on this crate, not the other way round.
pub const MAX_DECODED_WIDTH: u32 = 7680;
/// Tallest picture a decoder hands out, see [`MAX_DECODED_WIDTH`].
pub const MAX_DECODED_HEIGHT: u32 = 4320;

/// Rejects a decoded picture size beyond the wire limits. The size comes from a remote
/// bitstream, not from the checked frame header, so a hostile sender could otherwise make every
/// viewer allocate gigabytes.
// Only the decoders call this.
#[cfg(any(test, feature = "vp9", feature = "av1-decode"))]
pub(crate) fn check_decoded_size(width: u32, height: u32) -> Result<()> {
    if width > MAX_DECODED_WIDTH || height > MAX_DECODED_HEIGHT {
        return Err(Error::InvalidFrame(format!(
            "decoded picture {width}x{height} exceeds {MAX_DECODED_WIDTH}x{MAX_DECODED_HEIGHT}"
        )));
    }
    Ok(())
}

/// One I420 picture: Y plane, then U, then V, without row padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct I420Frame {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl I420Frame {
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidFrame(format!("size {width}x{height}")));
        }
        let expected = Self::buffer_len(width, height);
        if data.len() != expected {
            return Err(Error::InvalidFrame(format!(
                "{width}x{height} needs {expected} bytes, got {}",
                data.len()
            )));
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    /// Bytes an I420 picture of this size takes.
    pub fn buffer_len(width: u32, height: u32) -> usize {
        let (w, h) = (width as usize, height as usize);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        w * h + 2 * cw * ch
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn into_data(self) -> Vec<u8> {
        self.data
    }

    pub fn y(&self) -> &[u8] {
        &self.data[..self.luma_len()]
    }

    pub fn u(&self) -> &[u8] {
        let start = self.luma_len();
        &self.data[start..start + self.chroma_len()]
    }

    pub fn v(&self) -> &[u8] {
        &self.data[self.luma_len() + self.chroma_len()..]
    }

    /// The picture as packed RGBA (opaque), for viewers whose webview cannot decode VP9 itself.
    /// BT.601 limited range, matching the capture side.
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.append_rgba(&mut out);
        out
    }

    /// Appends packed RGBA directly to a transport buffer, avoiding a second picture-sized
    /// allocation and copy when the caller prefixes the picture with a frame header.
    pub fn append_rgba(&self, out: &mut Vec<u8>) {
        let (w, h) = (self.width as usize, self.height as usize);
        let cw = w.div_ceil(2);
        let (y_plane, u_plane, v_plane) = (self.y(), self.u(), self.v());
        let start = out.len();
        out.resize(start + w * h * 4, 0);
        for (row, (luma, rgba)) in y_plane
            .chunks_exact(w)
            .zip(out[start..].chunks_exact_mut(w * 4))
            .enumerate()
        {
            let chroma_row = row / 2 * cw;
            let u = &u_plane[chroma_row..chroma_row + cw];
            let v = &v_plane[chroma_row..chroma_row + cw];
            // Two adjacent pixels share chroma, including the single last pixel of odd widths.
            for ((luma_pair, pixels), (&u, &v)) in
                luma.chunks(2).zip(rgba.chunks_mut(8)).zip(u.iter().zip(v))
            {
                let d = i32::from(u) - 128;
                let e = i32::from(v) - 128;
                let r_chroma = 409 * e + 128;
                let g_chroma = -100 * d - 208 * e + 128;
                let b_chroma = 516 * d + 128;
                for (&y, pixel) in luma_pair.iter().zip(pixels.as_chunks_mut::<4>().0) {
                    let c = 298 * (i32::from(y) - 16);
                    pixel.copy_from_slice(&[
                        clamp((c + r_chroma) >> 8),
                        clamp((c + g_chroma) >> 8),
                        clamp((c + b_chroma) >> 8),
                        u8::MAX,
                    ]);
                }
            }
        }
    }

    fn luma_len(&self) -> usize {
        self.width as usize * self.height as usize
    }

    fn chroma_len(&self) -> usize {
        (self.width as usize).div_ceil(2) * (self.height as usize).div_ceil(2)
    }
}

fn clamp(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn filled(width: u32, height: u32, y: u8, u: u8, v: u8) -> I420Frame {
        let luma = (width * height) as usize;
        let chroma = (I420Frame::buffer_len(width, height) - luma) / 2;
        let mut data = vec![y; luma];
        data.extend(std::iter::repeat_n(u, chroma));
        data.extend(std::iter::repeat_n(v, chroma));
        I420Frame::new(width, height, data).unwrap()
    }

    #[test]
    fn decoded_size_up_to_the_wire_limit_is_accepted() {
        check_decoded_size(1, 1).unwrap();
        check_decoded_size(1920, 1080).unwrap();
        check_decoded_size(MAX_DECODED_WIDTH, MAX_DECODED_HEIGHT).unwrap();
    }

    #[test]
    fn decoded_size_beyond_the_wire_limit_is_rejected() {
        for (w, h) in [
            (MAX_DECODED_WIDTH + 1, 1),
            (1, MAX_DECODED_HEIGHT + 1),
            (65535, 65535),
            (u32::MAX, u32::MAX),
        ] {
            assert!(
                matches!(check_decoded_size(w, h), Err(Error::InvalidFrame(_))),
                "{w}x{h}"
            );
        }
    }

    #[test]
    fn rgba_matches_bt601_on_varied_planes_and_sizes() {
        for width in 1..=9 {
            for height in 1..=9 {
                let data = (0..I420Frame::buffer_len(width, height))
                    .map(|index| (index * 113 + 17) as u8)
                    .collect();
                let frame = I420Frame::new(width, height, data).unwrap();
                let w = width as usize;
                let mut expected = Vec::new();
                for row in 0..height as usize {
                    for col in 0..w {
                        let chroma = row / 2 * w.div_ceil(2) + col / 2;
                        let c = i32::from(frame.y()[row * w + col]) - 16;
                        let d = i32::from(frame.u()[chroma]) - 128;
                        let e = i32::from(frame.v()[chroma]) - 128;
                        expected.extend([
                            clamp((298 * c + 409 * e + 128) >> 8),
                            clamp((298 * c - 100 * d - 208 * e + 128) >> 8),
                            clamp((298 * c + 516 * d + 128) >> 8),
                            255,
                        ]);
                    }
                }
                assert_eq!(frame.to_rgba(), expected, "{width}x{height}");
                let mut prefixed = vec![1, 2, 3, 4, 5];
                frame.append_rgba(&mut prefixed);
                frame.append_rgba(&mut prefixed);
                assert_eq!(&prefixed[..5], &[1, 2, 3, 4, 5]);
                assert_eq!(&prefixed[5..], expected.repeat(2));
            }
        }
    }

    #[test]
    fn rgba_covers_every_pixel_of_odd_sizes() {
        let rgba = filled(3, 3, 126, 128, 128).to_rgba();
        assert_eq!(rgba.len(), 3 * 3 * 4);
        assert!(rgba.chunks(4).all(|p| p == [128, 128, 128, 255]));
    }

    #[test]
    fn rgba_maps_limited_range_to_full_range() {
        assert_eq!(filled(2, 2, 16, 128, 128).to_rgba()[..4], [0, 0, 0, 255]);
        assert_eq!(
            filled(2, 2, 235, 128, 128).to_rgba()[..4],
            [255, 255, 255, 255]
        );
        // Pure red as the capture side encodes it.
        let red = filled(2, 2, 82, 90, 240).to_rgba();
        assert!(red[0] > 250 && red[1] < 5 && red[2] < 5, "{:?}", &red[..4]);
    }
}
