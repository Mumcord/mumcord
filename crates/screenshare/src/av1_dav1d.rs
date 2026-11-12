use std::ptr;

use crate::codec::{Codec, VideoDecoder};
use crate::frame::{MAX_DECODED_HEIGHT, MAX_DECODED_WIDTH, check_decoded_size};
use crate::{Error, I420Frame, Result};

mod ffi {
    #![allow(
        non_upper_case_globals,
        non_camel_case_types,
        non_snake_case,
        dead_code,
        unsafe_op_in_unsafe_fn,
        clippy::all
    )]

    include!(concat!(env!("OUT_DIR"), "/dav1d.rs"));
}

/// Decoder threads: a screen share is one stream at a time and should not take the CPU from the
/// call it runs next to.
const THREADS: i32 = 2;

/// AV1 decoder on dav1d, the Rust fallback for webviews without WebCodecs AV1 (FR-7.59).
pub struct Av1Decoder {
    ctx: *mut ffi::Dav1dContext,
}

// SAFETY: the dav1d context is owned exclusively by this value and only used through
// `&mut self`; dav1d has no thread affinity, so moving it to another thread is fine.
unsafe impl Send for Av1Decoder {}

impl Av1Decoder {
    pub fn new() -> Result<Self> {
        let settings = decoder_settings();
        let mut ctx = ptr::null_mut();
        // SAFETY: `settings` is fully initialised; the context pointer is only kept when
        // dav1d_open succeeds.
        check(
            unsafe { ffi::dav1d_open(&mut ctx, &settings) },
            "decoder init",
        )?;
        Ok(Self { ctx })
    }

    /// Decodes one compressed frame (a temporal unit). Returns `None` when dav1d has no picture
    /// to show for it.
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<I420Frame>> {
        if data.is_empty() {
            return Err(Error::InvalidFrame("empty frame".into()));
        }
        // SAFETY: dav1d_data_create allocates a buffer of `data.len()` bytes that the copy fills;
        // dav1d takes ownership of it on a successful send and the remainder is unreferenced
        // below. Pictures are copied out and unreferenced before returning.
        unsafe {
            let mut input = std::mem::zeroed::<ffi::Dav1dData>();
            let buf = ffi::dav1d_data_create(&mut input, data.len());
            if buf.is_null() {
                return Err(codec_error("allocate input", "out of memory".into()));
            }
            ptr::copy_nonoverlapping(data.as_ptr(), buf, data.len());

            let mut picture = None;
            let result = loop {
                let status = ffi::dav1d_send_data(self.ctx, &mut input);
                if status == again() {
                    // dav1d wants its output drained before it takes (the rest of) the data.
                    match self.next_picture() {
                        Ok(Some(frame)) => picture = Some(frame),
                        Ok(None) => break Err(codec_error("decode", "decoder stalled".into())),
                        Err(err) => break Err(err),
                    }
                } else if let Err(err) = check(status, "decode") {
                    break Err(err);
                } else if input.sz == 0 {
                    break Ok(());
                }
            };
            if input.sz > 0 {
                ffi::dav1d_data_unref(&mut input);
            }
            result?;
            while let Some(frame) = self.next_picture()? {
                picture = Some(frame);
            }
            Ok(picture)
        }
    }

    /// Takes the next decoded picture out of dav1d, if one is ready.
    ///
    /// # Safety
    /// `self.ctx` must be an open decoder (it always is between `new` and `drop`).
    unsafe fn next_picture(&mut self) -> Result<Option<I420Frame>> {
        // SAFETY: a zeroed picture is the empty value dav1d expects; a returned picture is
        // unreferenced exactly once after its planes are copied.
        unsafe {
            let mut pic = std::mem::zeroed::<ffi::Dav1dPicture>();
            let status = ffi::dav1d_get_picture(self.ctx, &mut pic);
            if status == again() {
                return Ok(None);
            }
            check(status, "get picture")?;
            let frame = copy_picture(&pic);
            ffi::dav1d_picture_unref(&mut pic);
            frame.map(Some)
        }
    }
}

impl VideoDecoder for Av1Decoder {
    fn codec(&self) -> Codec {
        Codec::Av1
    }

    fn decode(&mut self, data: &[u8]) -> Result<Option<I420Frame>> {
        Av1Decoder::decode(self, data)
    }
}

impl Drop for Av1Decoder {
    fn drop(&mut self) {
        // SAFETY: the context was opened in `new` and is closed exactly once.
        unsafe { ffi::dav1d_close(&mut self.ctx) }
    }
}

fn decoder_settings() -> ffi::Dav1dSettings {
    // SAFETY: an all-zero Dav1dSettings is valid plain data (integers, a null callback and a null
    // pointer), and dav1d_default_settings overwrites every field before it is read.
    let mut settings = unsafe { std::mem::zeroed::<ffi::Dav1dSettings>() };
    // SAFETY: `settings` is a valid, exclusively borrowed struct for dav1d to fill.
    unsafe { ffi::dav1d_default_settings(&mut settings) };
    settings.n_threads = THREADS;
    // One frame in, one picture out: no frame-threading delay on a live stream.
    settings.max_frame_delay = 1;
    // dav1d refuses larger frames before allocating for them; its default 0 means unlimited, and
    // the frame size comes from the remote bitstream.
    settings.frame_size_limit = MAX_DECODED_WIDTH * MAX_DECODED_HEIGHT;
    // Decode errors come back as status codes; dav1d's own log would only repeat them on stderr.
    settings.logger.callback = None;
    settings
}

/// dav1d's "try again" status, `DAV1D_ERR(EAGAIN)`: negative errno, whose value differs per OS.
fn again() -> i32 {
    -(ffi::EAGAIN as i32)
}

fn check(status: i32, operation: &'static str) -> Result<()> {
    if status >= 0 {
        return Ok(());
    }
    Err(codec_error(operation, format!("error {}", -status)))
}

fn codec_error(operation: &'static str, message: String) -> Error {
    Error::Codec {
        library: "dav1d",
        operation,
        message,
    }
}

/// Copies a decoded dav1d picture into a packed [`I420Frame`].
///
/// # Safety
/// `pic` must be a picture returned by dav1d that is still referenced.
unsafe fn copy_picture(pic: &ffi::Dav1dPicture) -> Result<I420Frame> {
    if pic.p.layout != ffi::DAV1D_PIXEL_LAYOUT_I420 || pic.p.bpc != 8 {
        return Err(Error::InvalidFrame(format!(
            "decoder produced layout {} at {} bits, expected 8-bit I420",
            pic.p.layout, pic.p.bpc
        )));
    }
    // A negative size turns into a huge one and is rejected with it.
    let (w, h) = (pic.p.w as u32, pic.p.h as u32);
    check_decoded_size(w, h)?;
    let mut data = Vec::with_capacity(I420Frame::buffer_len(w, h));
    let chroma = (w.div_ceil(2) as usize, h.div_ceil(2) as usize);
    let planes = [
        (w as usize, h as usize, pic.stride[0]),
        (chroma.0, chroma.1, pic.stride[1]),
        (chroma.0, chroma.1, pic.stride[1]),
    ];
    for (plane, (plane_w, plane_h, stride)) in planes.into_iter().enumerate() {
        let base = pic.data[plane] as *const u8;
        for row in 0..plane_h {
            // SAFETY: dav1d guarantees `plane_h` rows of `stride` bytes, each at least `plane_w`.
            let line =
                unsafe { std::slice::from_raw_parts(base.offset(row as isize * stride), plane_w) };
            data.extend_from_slice(line);
        }
    }
    I420Frame::new(w, h, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_refuses_frames_beyond_the_wire_limit() {
        let settings = decoder_settings();
        assert_eq!(
            settings.frame_size_limit,
            MAX_DECODED_WIDTH * MAX_DECODED_HEIGHT
        );
        assert_ne!(settings.frame_size_limit, 0, "0 means unlimited");
    }
}
