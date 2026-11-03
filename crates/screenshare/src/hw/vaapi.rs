//! VA-API AV1 and VP9 encoder for Intel and AMD GPUs on Linux (task N3-va). For now only libva
//! is loaded; the encoder itself still reports unavailable.

use super::{HardwareEncoder, dl, ffi};
use crate::{Codec, EncoderConfig, Result, VideoEncoder};

const ENCODER: HardwareEncoder = HardwareEncoder::Vaapi;

pub(super) fn open(_codec: Codec, _config: EncoderConfig) -> Result<Box<dyn VideoEncoder>> {
    let va = dl::open(ENCODER, dl::VA)?;
    let va_drm = dl::open(ENCODER, dl::VA_DRM)?;
    // SAFETY: the generated loaders only look up libva's functions with their declared
    // signatures.
    let _api = unsafe { ffi::va::LibVa::from_library(va) }
        .and_then(|_| unsafe { ffi::va_drm::LibVaDrm::from_library(va_drm) })
        .map_err(|err| ENCODER.unavailable(format!("libva unusable: {err}")))?;
    Err(ENCODER.unavailable("not implemented yet"))
}
