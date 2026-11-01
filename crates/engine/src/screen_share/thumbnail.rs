//! The live picture the sharer's control bar shows (issue #163, FR-3.127): the last keyframe the
//! encoder took, scaled down and re-encoded as a JPEG the UI can show as it is.

use image::{ImageBuffer, Rgb, codecs::jpeg::JpegEncoder};
use mumcord_screenshare::I420Frame;

use crate::people::base64;

/// Longest side of the picture in the bar.
pub const MAX_SIDE: u32 = 320;

/// The live picture of the own share as the UI shows it (issue #163, FR-3.127).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareThumbnail {
    pub width: u32,
    pub height: u32,
    /// The scaled-down keyframe as a `data:` URL.
    pub data_url: String,
}

/// The frame as `ShareThumbnail`, or `None` for an empty or malformed picture.
pub fn thumbnail(frame: &I420Frame) -> Option<ShareThumbnail> {
    let w = usize::try_from(frame.width()).ok()?;
    let h = usize::try_from(frame.height()).ok()?;
    let max = usize::try_from(MAX_SIDE).ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    // Nearest sampling: one output pixel per `step` input pixels, so the picture keeps its shape
    // without a resampler here in the share's own path.
    let step = (w.max(h).div_ceil(max)).max(1);
    let (dw, dh) = (w.div_ceil(step), h.div_ceil(step));
    let (cw, ch) = (w / 2, h / 2);
    if cw == 0 || ch == 0 {
        return None;
    }
    let (luma, cb, cr) = (frame.y(), frame.u(), frame.v());
    if cb.len() != cw * ch || cr.len() != cw * ch {
        return None;
    }
    let mut out: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(dw as u32, dh as u32, Rgb([0, 0, 0]));
    for oy in 0..dh {
        for ox in 0..dw {
            let sy = oy * step;
            let sx = ox * step;
            let y = i64::from(luma[sy * w + sx]);
            let cx = (sx / 2).min(cw - 1);
            let cy = (sy / 2).min(ch - 1);
            let b = i64::from(cb[cy * cw + cx]) - 128;
            let r = i64::from(cr[cy * cw + cx]) - 128;
            // BT.601, the coefficients the capture backends use too.
            let red = ((y as f64 + 1.402 * r as f64).clamp(0.0, 255.0)) as u8;
            let green = ((y as f64 - 0.344 * b as f64 - 0.714 * r as f64).clamp(0.0, 255.0)) as u8;
            let blue = ((y as f64 + 1.772 * b as f64).clamp(0.0, 255.0)) as u8;
            out.put_pixel(ox as u32, oy as u32, Rgb([red, green, blue]));
        }
    }
    let mut bytes = Vec::new();
    out.write_with_encoder(JpegEncoder::new_with_quality(&mut bytes, 80))
        .ok()?;
    Some(ShareThumbnail {
        width: dw as u32,
        height: dh as u32,
        data_url: format!("data:image/jpeg;base64,{}", base64(&bytes)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumbnail_is_a_jpeg_data_url_of_the_scaled_size() {
        // A 4×2 picture: the whole frame is one blue-ish block.
        let mut data = Vec::new();
        data.extend(std::iter::repeat_n(120u8, 8)); // luma
        data.extend(std::iter::repeat_n(128u8, 2)); // cb
        data.extend(std::iter::repeat_n(200u8, 2)); // cr
        let frame = I420Frame::new(4, 2, data).expect("frame");
        let url = thumbnail(&frame).expect("thumbnail").data_url;
        assert!(url.starts_with("data:image/jpeg;base64,"), "{url}");
    }

    #[test]
    fn an_empty_frame_gives_nothing() {
        assert_eq!(
            thumbnail(&I420Frame::new(1, 1, vec![0, 0, 0]).expect("frame")),
            None
        );
    }
}
