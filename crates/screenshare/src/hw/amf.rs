//! AMD AMF AV1 encoder (task N3-amf). For now only the runtime is loaded; the encoder itself
//! still reports unavailable.

use super::{HardwareEncoder, dl, ffi};
use crate::{Codec, EncoderConfig, Result, VideoEncoder};

const ENCODER: HardwareEncoder = HardwareEncoder::Amf;

pub(super) fn open(_codec: Codec, _config: EncoderConfig) -> Result<Box<dyn VideoEncoder>> {
    let library = dl::open(ENCODER, dl::AMF)?;
    // SAFETY: AMFInit_Fn is the AMF header's type for the AMFInit entry point.
    let _init = unsafe { dl::symbol::<ffi::amf::AMFInit_Fn>(ENCODER, &library, "AMFInit") }?;
    Err(ENCODER.unavailable("not implemented yet"))
}
