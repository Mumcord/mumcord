use std::mem::MaybeUninit;
use std::os::raw::{c_int, c_uint, c_void};
use std::ptr;
use std::sync::Mutex;

use crate::codec::{Codec, VideoDecoder};
use crate::ffi::{self, check};
use crate::frame::{MAX_DECODED_HEIGHT, MAX_DECODED_WIDTH, check_decoded_size};
use crate::{Error, I420Frame, Result};

/// Padding libvpx adds around a frame buffer (border pixels on every side, row alignment,
/// address alignment), generously rounded up.
const FRAME_BUFFER_PADDING: usize = 512;

/// The largest frame buffer libvpx may ask for: an 8-bit I420 picture at the wire limit plus
/// padding. libvpx has no runtime size limit and takes the frame size from the remote bitstream,
/// so without this cap a hostile sender could make every viewer allocate gigabytes.
const MAX_FRAME_BUFFER_LEN: usize = (MAX_DECODED_WIDTH as usize + FRAME_BUFFER_PADDING)
    * (MAX_DECODED_HEIGHT as usize + FRAME_BUFFER_PADDING)
    * 3
    / 2;

/// VP9 decoder, the Rust fallback for webviews without WebCodecs VP9 (FR-2.44).
pub struct Decoder {
    // Boxed: libvpx keeps pointers into the context between calls.
    ctx: Box<ffi::vpx_codec_ctx_t>,
    // Boxed: libvpx keeps a pointer to it as the frame buffer callbacks' private data. Declared
    // after `ctx`, and `drop` destroys the context first, so libvpx never outlives it.
    buffers: Box<FrameBuffers>,
}

// SAFETY: the libvpx context is owned exclusively by this value and only used through `&mut self`;
// libvpx has no thread affinity, so moving it to another thread is fine.
unsafe impl Send for Decoder {}

impl Decoder {
    pub fn new() -> Result<Self> {
        // SAFETY: plain libvpx init on locally owned, zero-initialised structs.
        unsafe {
            let mut ctx = Box::new(MaybeUninit::<ffi::vpx_codec_ctx_t>::zeroed().assume_init());
            let cfg = ffi::vpx_codec_dec_cfg_t {
                threads: 2,
                w: 0,
                h: 0,
            };
            let status = ffi::vpx_codec_dec_init_ver(
                &mut *ctx,
                ffi::vpx_codec_vp9_dx(),
                &cfg,
                0,
                ffi::VPX_DECODER_ABI_VERSION as c_int,
            );
            check(&mut *ctx, status, "decoder init")?;
            let mut decoder = Self {
                ctx,
                buffers: Box::default(),
            };
            let status = ffi::vpx_codec_set_frame_buffer_functions(
                &mut *decoder.ctx,
                Some(get_frame_buffer),
                Some(release_frame_buffer),
                (&mut *decoder.buffers as *mut FrameBuffers).cast::<c_void>(),
            );
            check(&mut *decoder.ctx, status, "set frame buffers")?;
            Ok(decoder)
        }
    }

    /// Decodes one compressed frame. Returns `None` when libvpx has no picture to show for it.
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<I420Frame>> {
        let len = c_uint::try_from(data.len())
            .map_err(|_| Error::InvalidFrame(format!("{} bytes", data.len())))?;
        // SAFETY: libvpx reads `data` only during the call; the returned image stays valid until
        // the next decode call and is copied out before that.
        unsafe {
            let status =
                ffi::vpx_codec_decode(&mut *self.ctx, data.as_ptr(), len, ptr::null_mut(), 0);
            check(&mut *self.ctx, status, "decode")?;

            let mut picture = None;
            let mut iter: ffi::vpx_codec_iter_t = ptr::null();
            loop {
                let img = ffi::vpx_codec_get_frame(&mut *self.ctx, &mut iter);
                if img.is_null() {
                    break;
                }
                picture = Some(copy_image(&*img)?);
            }
            Ok(picture)
        }
    }
}

impl VideoDecoder for Decoder {
    fn codec(&self) -> Codec {
        Codec::Vp9
    }

    fn decode(&mut self, data: &[u8]) -> Result<Option<I420Frame>> {
        Decoder::decode(self, data)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: the context was initialised in `new` and is destroyed exactly once.
        unsafe {
            ffi::vpx_codec_destroy(&mut *self.ctx);
        }
    }
}

/// Copies a decoded libvpx image into a packed [`I420Frame`].
///
/// # Safety
/// `img` must be a valid image returned by libvpx with its planes still alive.
unsafe fn copy_image(img: &ffi::vpx_image_t) -> Result<I420Frame> {
    if img.fmt != ffi::VPX_IMG_FMT_I420 {
        return Err(Error::InvalidFrame(format!(
            "decoder produced pixel format {}, expected I420",
            img.fmt
        )));
    }
    let (w, h) = (img.d_w, img.d_h);
    check_decoded_size(w, h)?;
    let mut data = Vec::with_capacity(I420Frame::buffer_len(w, h));
    let planes = [
        (w as usize, h as usize),
        (w.div_ceil(2) as usize, h.div_ceil(2) as usize),
        (w.div_ceil(2) as usize, h.div_ceil(2) as usize),
    ];
    for (plane, (plane_w, plane_h)) in planes.into_iter().enumerate() {
        let stride = img.stride[plane] as usize;
        for row in 0..plane_h {
            // SAFETY: libvpx guarantees `plane_h` rows of `stride` bytes, each at least `plane_w`.
            let line =
                unsafe { std::slice::from_raw_parts(img.planes[plane].add(row * stride), plane_w) };
            data.extend_from_slice(line);
        }
    }
    I420Frame::new(w, h, data)
}

/// Frame buffers handed to libvpx, kept for reuse until the decoder is dropped. Each slot is
/// `Some` while libvpx holds it.
#[derive(Default)]
struct FrameBuffers {
    // Locked so the callbacks stay sound whichever libvpx thread calls them.
    slots: Mutex<Vec<(Box<[u8]>, bool)>>,
}

impl FrameBuffers {
    /// Hands out a zeroed buffer of at least `min_size` bytes as `(index, pointer, length)`, or
    /// `None` when the size is beyond the wire limit.
    fn get(&self, min_size: usize) -> Option<(usize, *mut u8, usize)> {
        if min_size > MAX_FRAME_BUFFER_LEN {
            return None;
        }
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        let index = match slots.iter().position(|(_, in_use)| !in_use) {
            Some(index) => index,
            None => {
                slots.push((Box::default(), false));
                slots.len() - 1
            }
        };
        let (data, in_use) = &mut slots[index];
        if data.len() < min_size {
            *data = vec![0; min_size].into_boxed_slice();
        } else {
            // libvpx requires zeroed buffers.
            data.fill(0);
        }
        *in_use = true;
        Some((index, data.as_mut_ptr(), data.len()))
    }

    fn release(&self, index: usize) {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, in_use)) = slots.get_mut(index) {
            *in_use = false;
        }
    }
}

/// libvpx's get-frame-buffer callback; refuses sizes beyond the wire limit, which fails the
/// decode before libvpx writes a single pixel.
///
/// # Safety
/// `priv_` must be the decoder's `FrameBuffers` and `fb` a valid frame buffer descriptor.
unsafe extern "C" fn get_frame_buffer(
    priv_: *mut c_void,
    min_size: usize,
    fb: *mut ffi::vpx_codec_frame_buffer_t,
) -> c_int {
    // SAFETY: libvpx passes back the `FrameBuffers` registered in `Decoder::new`, which lives
    // until after the context is destroyed; it is only used through a shared reference.
    let buffers = unsafe { &*priv_.cast::<FrameBuffers>() };
    let Some((index, data, size)) = buffers.get(min_size) else {
        return -1;
    };
    // SAFETY: `fb` is a valid descriptor owned by libvpx for this call. The buffer stays
    // allocated and unused by anyone else until libvpx releases it or the decoder is dropped
    // (after the context is destroyed).
    unsafe {
        (*fb).data = data;
        (*fb).size = size;
        (*fb).priv_ = index as *mut c_void;
    }
    0
}

/// libvpx's release-frame-buffer callback.
///
/// # Safety
/// Same as [`get_frame_buffer`].
unsafe extern "C" fn release_frame_buffer(
    priv_: *mut c_void,
    fb: *mut ffi::vpx_codec_frame_buffer_t,
) -> c_int {
    // SAFETY: see `get_frame_buffer`; `fb` is the descriptor libvpx got from it.
    let (buffers, index) = unsafe { (&*priv_.cast::<FrameBuffers>(), (*fb).priv_ as usize) };
    buffers.release(index);
    0
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn descriptor() -> ffi::vpx_codec_frame_buffer_t {
        ffi::vpx_codec_frame_buffer_t {
            data: ptr::null_mut(),
            size: 0,
            priv_: ptr::null_mut(),
        }
    }

    fn get(
        buffers: &mut FrameBuffers,
        min_size: usize,
        fb: &mut ffi::vpx_codec_frame_buffer_t,
    ) -> c_int {
        // SAFETY: `buffers` and `fb` are valid for the call, as libvpx guarantees.
        unsafe { get_frame_buffer((buffers as *mut FrameBuffers).cast(), min_size, fb) }
    }

    #[test]
    fn frame_buffer_cap_fits_the_largest_allowed_picture() {
        let picture = I420Frame::buffer_len(MAX_DECODED_WIDTH, MAX_DECODED_HEIGHT);
        assert!(MAX_FRAME_BUFFER_LEN > picture);
        // Far below what a 65535x65535 bitstream would ask for.
        assert!(MAX_FRAME_BUFFER_LEN < I420Frame::buffer_len(16384, 16384));
    }

    #[test]
    fn frame_buffers_beyond_the_wire_limit_are_refused() {
        let mut buffers = FrameBuffers::default();
        let mut fb = descriptor();
        assert!(get(&mut buffers, MAX_FRAME_BUFFER_LEN + 1, &mut fb) < 0);
        assert!(get(&mut buffers, usize::MAX, &mut fb) < 0);
        assert!(fb.data.is_null());
        assert!(buffers.slots.lock().unwrap().is_empty());
    }

    #[test]
    fn frame_buffers_are_zeroed_and_reused_after_release() {
        let mut buffers = FrameBuffers::default();
        let mut fb = descriptor();
        assert_eq!(get(&mut buffers, 64, &mut fb), 0);
        assert!(fb.size >= 64);
        // SAFETY: libvpx may write the whole buffer it was given.
        unsafe { ptr::write_bytes(fb.data, 0xAB, fb.size) };
        let first = fb.data;

        let mut second = descriptor();
        assert_eq!(get(&mut buffers, 64, &mut second), 0);
        assert_ne!(second.data, first, "a held buffer is not handed out twice");

        // SAFETY: as in `get`.
        assert_eq!(
            unsafe { release_frame_buffer((&mut buffers as *mut FrameBuffers).cast(), &mut fb) },
            0
        );
        let mut again = descriptor();
        assert_eq!(get(&mut buffers, 32, &mut again), 0);
        assert_eq!(again.data, first);
        // SAFETY: the buffer is `again.size` bytes long and held by this test.
        let bytes = unsafe { std::slice::from_raw_parts(again.data, again.size) };
        assert!(bytes.iter().all(|&b| b == 0));
    }

    #[test]
    fn decoder_opens_with_capped_frame_buffers() {
        Decoder::new().unwrap();
    }
}
