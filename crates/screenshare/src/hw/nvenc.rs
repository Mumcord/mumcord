//! NVIDIA NVENC AV1 encoder (task N3-nv). For now only the driver is loaded; the encoder itself
//! still reports unavailable.

use super::{HardwareEncoder, dl, ffi};
use crate::{Codec, EncoderConfig, Result, VideoEncoder};

const ENCODER: HardwareEncoder = HardwareEncoder::Nvenc;

pub(super) fn open(_codec: Codec, _config: EncoderConfig) -> Result<Box<dyn VideoEncoder>> {
    let library = dl::open(ENCODER, dl::NVENC)?;
    // SAFETY: the generated loader only looks up the two NVENC entry points with their declared
    // signatures.
    let _api = unsafe { ffi::nvenc::NvEncodeApi::from_library(library) }
        .map_err(|err| ENCODER.unavailable(format!("NVENC driver unusable: {err}")))?;
    Err(ENCODER.unavailable("not implemented yet"))
}
