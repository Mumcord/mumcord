//! Generated libvpx bindings (see `build.rs`) and the error helper shared by encoder and decoder.

#![allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    dead_code,
    unsafe_op_in_unsafe_fn,
    clippy::all
)]

include!(concat!(env!("OUT_DIR"), "/vpx.rs"));

use std::ffi::CStr;

/// Turns a libvpx status into a crate error, with libvpx's own detail text when it has one.
pub(crate) fn check(
    ctx: *mut vpx_codec_ctx_t,
    status: vpx_codec_err_t,
    operation: &'static str,
) -> crate::Result<()> {
    if status == VPX_CODEC_OK {
        return Ok(());
    }
    // SAFETY: libvpx returns static or context-owned NUL-terminated strings (or null for no detail).
    let message = unsafe {
        let base = CStr::from_ptr(vpx_codec_err_to_string(status)).to_string_lossy();
        let detail = vpx_codec_error_detail(ctx);
        if detail.is_null() {
            base.into_owned()
        } else {
            format!("{base}: {}", CStr::from_ptr(detail).to_string_lossy())
        }
    };
    Err(crate::Error::Codec { operation, message })
}
