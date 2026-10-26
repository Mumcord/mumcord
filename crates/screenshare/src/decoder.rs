use std::mem::MaybeUninit;
use std::os::raw::{c_int, c_uint};
use std::ptr;

use crate::ffi::{self, check};
use crate::{Error, I420Frame, Result};

/// VP9 decoder, the Rust fallback for webviews without WebCodecs VP9 (FR-2.44).
pub struct Decoder {
    // Boxed: libvpx keeps pointers into the context between calls.
    ctx: Box<ffi::vpx_codec_ctx_t>,
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
            Ok(Self { ctx })
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
