//! Direct hardware encoders (FR-7 codec), each behind [`VideoEncoder`]: NVENC (NVIDIA), VA-API
//! (Linux, Intel and AMD), AMF (AMD) and Quick Sync through oneVPL (Intel).
//!
//! They are only compiled with the `hw-encode` feature and when the build found the vendor
//! headers (`HWENC_SDK_DIR`, see `build.rs`; never on macOS). The GPU drivers are loaded at
//! runtime ([`dl`]), so a missing driver, an unsupported GPU or a build without the feature or the SDK all end in the same
//! [`Error::HardwareUnavailable`], and the caller falls back to the software encoders.

#[cfg(hwenc)]
mod amf;
#[cfg(hwenc)]
mod dl;
#[cfg(hwenc)]
mod nvenc;
// Used by the vendor encoders that don't report temporal layer ids themselves.
#[cfg(hwenc)]
mod obu;
#[cfg(hwenc)]
mod qsv;
#[cfg(all(hwenc, target_os = "linux"))]
mod vaapi;

use std::fmt;

use crate::{Codec, EncoderConfig, Error, Result, VideoEncoder};

/// Generated vendor bindings, one module per SDK.
#[cfg(hwenc)]
#[allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    dead_code,
    unused_imports,
    unnecessary_transmutes,
    improper_ctypes,
    unsafe_op_in_unsafe_fn,
    clippy::all
)]
mod ffi {
    pub mod nvenc {
        include!(concat!(env!("OUT_DIR"), "/nvenc.rs"));
    }
    pub mod amf {
        include!(concat!(env!("OUT_DIR"), "/amf.rs"));
    }
    pub mod vpl {
        include!(concat!(env!("OUT_DIR"), "/vpl.rs"));
    }
    #[cfg(target_os = "linux")]
    pub mod va {
        include!(concat!(env!("OUT_DIR"), "/va.rs"));
    }
    #[cfg(target_os = "linux")]
    pub mod va_drm {
        include!(concat!(env!("OUT_DIR"), "/va_drm.rs"));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HardwareEncoder {
    Nvenc,
    Vaapi,
    Amf,
    Qsv,
}

impl HardwareEncoder {
    pub const ALL: [Self; 4] = [Self::Nvenc, Self::Vaapi, Self::Amf, Self::Qsv];

    pub(crate) fn unavailable(self, reason: impl Into<String>) -> Error {
        Error::HardwareUnavailable {
            encoder: self,
            reason: reason.into(),
        }
    }
}

impl fmt::Display for HardwareEncoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Nvenc => "NVENC",
            Self::Vaapi => "VA-API",
            Self::Amf => "AMF",
            Self::Qsv => "Quick Sync",
        })
    }
}

/// Opens `encoder` for `codec`, or fails with [`Error::HardwareUnavailable`] when this build,
/// system or GPU can't provide it.
pub fn open_hardware_encoder(
    encoder: HardwareEncoder,
    codec: Codec,
    config: EncoderConfig,
) -> Result<Box<dyn VideoEncoder>> {
    #[cfg(hwenc)]
    match encoder {
        HardwareEncoder::Nvenc => nvenc::open(codec, config),
        #[cfg(target_os = "linux")]
        HardwareEncoder::Vaapi => vaapi::open(codec, config),
        #[cfg(not(target_os = "linux"))]
        HardwareEncoder::Vaapi => Err(encoder.unavailable("VA-API only exists on Linux")),
        HardwareEncoder::Amf => amf::open(codec, config),
        HardwareEncoder::Qsv => qsv::open(codec, config),
    }
    #[cfg(not(hwenc))]
    {
        let _ = (codec, config);
        Err(encoder.unavailable("this build has no hardware encoder support"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Without the GPU (as in CI), every encoder reports itself unavailable instead of failing to
    // load or crashing. Implemented encoders may open on a machine that has the hardware.
    #[test]
    fn every_hardware_encoder_reports_unavailable() {
        let config = EncoderConfig {
            width: 1280,
            height: 720,
            fps: 30,
            bitrate_kbps: 2000,
        };
        for encoder in HardwareEncoder::ALL {
            for codec in [Codec::Av1, Codec::Vp9] {
                match open_hardware_encoder(encoder, codec, config) {
                    Err(Error::HardwareUnavailable { encoder: e, .. }) => assert_eq!(e, encoder),
                    Err(err) => panic!("{encoder} {codec:?}: unexpected error {err}"),
                    Ok(_) if (encoder, codec) == (HardwareEncoder::Nvenc, Codec::Av1) => {}
                    Ok(_) if encoder == HardwareEncoder::Vaapi => {}
                    Ok(_) if (encoder, codec) == (HardwareEncoder::Amf, Codec::Av1) => {}
                    Ok(_) if encoder == HardwareEncoder::Qsv => {}
                    Ok(_) => panic!("{encoder} {codec:?}: not implemented yet"),
                }
            }
        }
    }
}
