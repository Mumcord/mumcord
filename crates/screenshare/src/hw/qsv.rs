//! Intel Quick Sync AV1 and VP9 encoder through oneVPL (task N3-qsv). The statically linked
//! oneVPL dispatcher finds Intel's runtime itself. For now only the dispatcher is started; the
//! encoder itself still reports unavailable.

use super::{HardwareEncoder, ffi};
use crate::{Codec, EncoderConfig, Result, VideoEncoder};

const ENCODER: HardwareEncoder = HardwareEncoder::Qsv;

pub(super) fn open(_codec: Codec, _config: EncoderConfig) -> Result<Box<dyn VideoEncoder>> {
    // SAFETY: MFXLoad has no preconditions; the loader is released right away.
    unsafe {
        let loader = ffi::vpl::MFXLoad();
        if loader.is_null() {
            return Err(ENCODER.unavailable("oneVPL dispatcher failed to start"));
        }
        ffi::vpl::MFXUnload(loader);
    }
    Err(ENCODER.unavailable("not implemented yet"))
}
