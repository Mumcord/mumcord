use crate::{Error, Result};

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
        let (w, h) = (self.width as usize, self.height as usize);
        let cw = w.div_ceil(2);
        let (y_plane, u_plane, v_plane) = (self.y(), self.u(), self.v());
        let mut out = Vec::with_capacity(w * h * 4);
        for row in 0..h {
            let chroma_row = row / 2 * cw;
            for col in 0..w {
                let c = i32::from(y_plane[row * w + col]) - 16;
                let d = i32::from(u_plane[chroma_row + col / 2]) - 128;
                let e = i32::from(v_plane[chroma_row + col / 2]) - 128;
                let r = (298 * c + 409 * e + 128) >> 8;
                let g = (298 * c - 100 * d - 208 * e + 128) >> 8;
                let b = (298 * c + 516 * d + 128) >> 8;
                out.extend([clamp(r), clamp(g), clamp(b), u8::MAX]);
            }
        }
        out
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
