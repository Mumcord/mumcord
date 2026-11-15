//! Opens the software codecs a build may leave out (Cargo features `vp9` and `desktop-codecs`).
//!
//! hwcodec has no encoder type for a codec whose feature is off, so this is the only module that
//! names those types; the rest of the engine opens encoders here and needs no feature checks.
//! Opening a codec this build leaves out fails with [`hwcodec::Error::Codec`], exactly like a
//! codec library that refuses to start. Decoders need no wrapper: `hwcodec::open_software_decoder`
//! exists in every build.

use std::time::Duration;

use hwcodec::{Av1Options, EncoderConfig, Result, VideoEncoder};

#[cfg(not(all(feature = "vp9", feature = "desktop-codecs")))]
fn not_built(library: &'static str) -> hwcodec::Error {
    hwcodec::Error::Codec {
        library,
        operation: "open",
        message: "this codec is not built into this app".into(),
    }
}

/// VP9 on libvpx, tuned for screen content.
#[cfg(feature = "vp9")]
pub fn open_vp9(config: EncoderConfig) -> Result<Box<dyn VideoEncoder>> {
    Ok(Box::new(hwcodec::Vp9Encoder::new(config)?))
}

#[cfg(not(feature = "vp9"))]
pub fn open_vp9(_config: EncoderConfig) -> Result<Box<dyn VideoEncoder>> {
    Err(not_built("libvpx"))
}

/// VP9 on libvpx, tuned for natural pictures (camera, video).
#[cfg(feature = "vp9")]
pub fn open_vp9_natural(config: EncoderConfig) -> Result<Box<dyn VideoEncoder>> {
    Ok(Box::new(hwcodec::Vp9Encoder::natural(config)?))
}

#[cfg(not(feature = "vp9"))]
pub fn open_vp9_natural(_config: EncoderConfig) -> Result<Box<dyn VideoEncoder>> {
    Err(not_built("libvpx"))
}

/// AV1 on SVT-AV1.
#[cfg(feature = "desktop-codecs")]
pub fn open_av1(config: EncoderConfig, options: Av1Options) -> Result<Box<dyn VideoEncoder>> {
    Ok(Box::new(hwcodec::Av1Encoder::new(config, options)?))
}

#[cfg(not(feature = "desktop-codecs"))]
pub fn open_av1(_config: EncoderConfig, _options: Av1Options) -> Result<Box<dyn VideoEncoder>> {
    Err(not_built("SVT-AV1"))
}

/// hwcodec's software AV1 speed test: the average encode time of one frame.
#[cfg(feature = "desktop-codecs")]
pub fn measure_av1(width: u32, height: u32, fps: u32, options: Av1Options) -> Result<Duration> {
    Ok(hwcodec::measure_av1(width, height, fps, options)?.average_frame_time)
}

#[cfg(not(feature = "desktop-codecs"))]
pub fn measure_av1(_width: u32, _height: u32, _fps: u32, _options: Av1Options) -> Result<Duration> {
    Err(not_built("SVT-AV1"))
}

#[cfg(test)]
#[cfg(not(all(feature = "vp9", feature = "desktop-codecs")))]
mod tests {
    use super::*;

    fn assert_not_built<T>(opened: Result<T>, expected: &str) {
        match opened {
            Err(hwcodec::Error::Codec {
                library,
                operation,
                message,
            }) => {
                assert_eq!(library, expected);
                assert_eq!(operation, "open");
                assert_eq!(message, "this codec is not built into this app");
            }
            Err(other) => panic!("{expected}: expected Error::Codec, got {other:?}"),
            Ok(_) => panic!("{expected}: opened although its feature is off"),
        }
    }

    #[cfg(not(feature = "vp9"))]
    #[test]
    fn vp9_does_not_open_without_its_feature() {
        let config = EncoderConfig::new(1280, 720, 30, 2000);
        assert_not_built(open_vp9(config), "libvpx");
        assert_not_built(open_vp9_natural(config), "libvpx");
    }

    #[cfg(not(feature = "desktop-codecs"))]
    #[test]
    fn av1_does_not_open_without_its_feature() {
        let config = EncoderConfig::new(1280, 720, 30, 2000);
        assert_not_built(open_av1(config, Av1Options::default()), "SVT-AV1");
        assert_not_built(measure_av1(1280, 720, 30, Av1Options::default()), "SVT-AV1");
    }
}
