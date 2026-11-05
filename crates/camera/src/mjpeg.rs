//! MJPEG frames from USB 2 cameras (FR-8.26), decoded with `zune-jpeg`.
//!
//! UVC cameras leave the Huffman tables out of their frames and mark them with an `AVI1` APP0
//! segment (the MJPEG convention); `zune-jpeg` then fills in the standard tables itself. Frames that fail to decode are skipped and counted by
//! [`UnreadableRun`]; a long run of them stops the camera.

use std::io::Cursor;

use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

/// More than this many undecodable frames in a row stop the camera (FR-8.26).
pub const MAX_UNREADABLE_RUN: u32 = 30;

/// Decodes the frames of one capture session into a buffer allocated once per session.
pub struct MjpegDecoder {
    width: u32,
    height: u32,
    /// One `Y Cb Cr` triple per pixel.
    ycbcr: Vec<u8>,
}

impl MjpegDecoder {
    /// A decoder for frames of exactly `width × height`.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            ycbcr: vec![0; width as usize * height as usize * 3],
        }
    }

    /// Decodes one frame to interleaved `Y Cb Cr` (stride `width · 3`). `None` when the frame
    /// is broken or has another size than the session negotiated.
    pub fn decode(&mut self, jpeg: &[u8]) -> Option<&[u8]> {
        let options = DecoderOptions::default()
            .set_max_width(self.width as usize)
            .set_max_height(self.height as usize)
            .set_strict_mode(false)
            .jpeg_set_out_colorspace(ColorSpace::YCbCr);
        let mut decoder = JpegDecoder::new_with_options(Cursor::new(jpeg), options);
        decoder.decode_headers().ok()?;
        let info = decoder.info()?;
        if u32::from(info.width) != self.width || u32::from(info.height) != self.height {
            return None;
        }
        if decoder.output_colorspace() != Some(ColorSpace::YCbCr)
            || decoder.output_buffer_size() != Some(self.ycbcr.len())
        {
            return None;
        }
        decoder.decode_into(&mut self.ycbcr).ok()?;
        Some(&self.ycbcr)
    }
}

/// Counts frames in a row that could not be read.
#[derive(Debug, Default)]
pub struct UnreadableRun {
    run: u32,
}

impl UnreadableRun {
    /// Records one frame; `true` once more than [`MAX_UNREADABLE_RUN`] frames in a row failed.
    pub fn record(&mut self, readable: bool) -> bool {
        self.run = if readable { 0 } else { self.run + 1 };
        self.run > MAX_UNREADABLE_RUN
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// 64×48 UVC-style frame (`AVI1` APP0, 4:2:2, baseline, no Huffman tables): the left
    /// half is grey, the right half white (JPEG is full range: Y 128 and 255, no colour).
    const WEBCAM_FRAME: &[u8] = include_bytes!("../tests/fixtures/mjpeg-64x48-no-dht.jpg");

    #[test]
    fn a_frame_without_huffman_tables_decodes() {
        let mut decoder = MjpegDecoder::new(64, 48);
        let ycbcr = decoder.decode(WEBCAM_FRAME).expect("frame decodes");
        assert_eq!(ycbcr.len(), 64 * 48 * 3);
        let pixel = |x: usize, y: usize| &ycbcr[(y * 64 + x) * 3..(y * 64 + x) * 3 + 3];
        // Lossy: allow a few steps of error away from the edge.
        let near = |a: u8, b: u8| a.abs_diff(b) <= 4;
        let grey = pixel(8, 24);
        assert!(
            near(grey[0], 128) && near(grey[1], 128) && near(grey[2], 128),
            "{grey:?}"
        );
        let white = pixel(56, 24);
        assert!(
            near(white[0], 255) && near(white[1], 128) && near(white[2], 128),
            "{white:?}"
        );
    }

    #[test]
    fn broken_or_resized_frames_are_refused() {
        let mut decoder = MjpegDecoder::new(64, 48);
        assert!(decoder.decode(&WEBCAM_FRAME[..60]).is_none());
        assert!(decoder.decode(b"not a jpeg").is_none());
        let mut other_size = MjpegDecoder::new(32, 24);
        assert!(other_size.decode(WEBCAM_FRAME).is_none());
    }

    #[test]
    fn the_camera_stops_after_more_than_30_unreadable_frames_in_a_row() {
        let mut run = UnreadableRun::default();
        for _ in 0..30 {
            assert!(!run.record(false));
        }
        assert!(!run.record(true), "a good frame resets the run");
        for _ in 0..30 {
            assert!(!run.record(false));
        }
        assert!(run.record(false));
    }
}
