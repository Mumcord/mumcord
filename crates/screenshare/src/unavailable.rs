//! Stand-ins for the codecs this build leaves out (Cargo features `vp9`, `av1-encode`,
//! `av1-decode`). They keep the crate's API the same in every build, so callers need no feature
//! checks of their own: opening a missing codec fails with [`Error::Codec`], exactly like a
//! codec library that refuses to start. No stand-in can ever be constructed, which is why the
//! methods that take `self` have nothing to do.

use std::convert::Infallible;

use crate::{Error, Result};

fn unavailable(library: &'static str) -> Error {
    Error::Codec {
        library,
        operation: "open",
        message: "this codec is not built into this app".into(),
    }
}

/// Implements the encoder API of a stand-in around its `Infallible` field.
#[cfg(not(all(feature = "vp9", feature = "av1-encode")))]
macro_rules! encoder_stand_in {
    ($name:ident, $codec:expr) => {
        impl $name {
            pub fn config(&self) -> crate::EncoderConfig {
                match self.0 {}
            }

            pub fn encode(
                &mut self,
                _frame: &crate::I420Frame,
                _pts: i64,
                _force_keyframe: bool,
            ) -> Result<Vec<crate::EncodedFrame>> {
                match self.0 {}
            }

            pub fn set_resolution(&mut self, _width: u32, _height: u32) -> Result<()> {
                match self.0 {}
            }

            pub fn set_bitrate(&mut self, _bitrate_kbps: u32) -> Result<()> {
                match self.0 {}
            }

            pub fn set_fps(&mut self, _fps: u32) -> Result<()> {
                match self.0 {}
            }
        }

        impl crate::VideoEncoder for $name {
            fn codec(&self) -> crate::Codec {
                $codec
            }

            fn config(&self) -> crate::EncoderConfig {
                match self.0 {}
            }

            fn temporal_layers(&self) -> u8 {
                match self.0 {}
            }

            fn encode(
                &mut self,
                _frame: &crate::I420Frame,
                _pts: i64,
                _force_keyframe: bool,
            ) -> Result<Vec<crate::EncodedFrame>> {
                match self.0 {}
            }

            fn set_resolution(&mut self, _width: u32, _height: u32) -> Result<()> {
                match self.0 {}
            }

            fn set_bitrate(&mut self, _bitrate_kbps: u32) -> Result<()> {
                match self.0 {}
            }

            fn set_fps(&mut self, _fps: u32) -> Result<()> {
                match self.0 {}
            }
        }
    };
}

/// Implements the decoder API of a stand-in around its `Infallible` field.
#[cfg(not(all(feature = "vp9", feature = "av1-decode")))]
macro_rules! decoder_stand_in {
    ($name:ident, $codec:expr, $library:literal) => {
        impl $name {
            pub fn new() -> Result<Self> {
                Err(unavailable($library))
            }

            pub fn decode(&mut self, _data: &[u8]) -> Result<Option<crate::I420Frame>> {
                match self.0 {}
            }
        }

        impl crate::VideoDecoder for $name {
            fn codec(&self) -> crate::Codec {
                $codec
            }

            fn decode(&mut self, _data: &[u8]) -> Result<Option<crate::I420Frame>> {
                match self.0 {}
            }
        }
    };
}

/// VP9 encoder of a build without the `vp9` feature: it never opens.
#[cfg(not(feature = "vp9"))]
#[derive(Debug)]
pub struct Encoder(Infallible);

#[cfg(not(feature = "vp9"))]
impl Encoder {
    pub fn new(_config: crate::EncoderConfig) -> Result<Self> {
        Err(unavailable("libvpx"))
    }

    pub fn natural(_config: crate::EncoderConfig) -> Result<Self> {
        Err(unavailable("libvpx"))
    }
}

#[cfg(not(feature = "vp9"))]
encoder_stand_in!(Encoder, crate::Codec::Vp9);

/// VP9 decoder of a build without the `vp9` feature: it never opens.
#[cfg(not(feature = "vp9"))]
#[derive(Debug)]
pub struct Decoder(Infallible);

#[cfg(not(feature = "vp9"))]
decoder_stand_in!(Decoder, crate::Codec::Vp9, "libvpx");

/// AV1 encoder of a build without the `av1-encode` feature: it never opens.
#[cfg(not(feature = "av1-encode"))]
#[derive(Debug)]
pub struct Av1Encoder(Infallible);

#[cfg(not(feature = "av1-encode"))]
impl Av1Encoder {
    pub fn new(_config: crate::EncoderConfig, _options: crate::Av1Options) -> Result<Self> {
        Err(unavailable("SVT-AV1"))
    }

    pub fn options(&self) -> crate::Av1Options {
        match self.0 {}
    }

    pub fn set_options(&mut self, _options: crate::Av1Options) -> Result<()> {
        match self.0 {}
    }
}

#[cfg(not(feature = "av1-encode"))]
encoder_stand_in!(Av1Encoder, crate::Codec::Av1);

/// AV1 decoder of a build without the `av1-decode` feature: it never opens.
#[cfg(not(feature = "av1-decode"))]
#[derive(Debug)]
pub struct Av1Decoder(Infallible);

#[cfg(not(feature = "av1-decode"))]
decoder_stand_in!(Av1Decoder, crate::Codec::Av1, "dav1d");

#[cfg(test)]
mod tests {
    use crate::Error;

    #[cfg(any(not(feature = "vp9"), not(feature = "av1-encode")))]
    const CONFIG: crate::EncoderConfig = crate::EncoderConfig {
        width: 1280,
        height: 720,
        fps: 30,
        bitrate_kbps: 2000,
    };

    fn assert_not_built<T: std::fmt::Debug>(opened: crate::Result<T>, expected: &str) {
        match opened {
            Err(Error::Codec {
                library, operation, ..
            }) => {
                assert_eq!(library, expected);
                assert_eq!(operation, "open");
            }
            other => panic!("{expected}: expected Error::Codec, got {other:?}"),
        }
    }

    #[cfg(not(feature = "vp9"))]
    #[test]
    fn vp9_does_not_open_without_its_feature() {
        assert_not_built(crate::Encoder::new(CONFIG), "libvpx");
        assert_not_built(crate::Encoder::natural(CONFIG), "libvpx");
        assert_not_built(crate::Decoder::new(), "libvpx");
    }

    #[cfg(not(feature = "av1-encode"))]
    #[test]
    fn av1_encoder_does_not_open_without_its_feature() {
        assert_not_built(
            crate::Av1Encoder::new(CONFIG, crate::Av1Options::default()),
            "SVT-AV1",
        );
    }

    #[cfg(not(feature = "av1-decode"))]
    #[test]
    fn av1_decoder_does_not_open_without_its_feature() {
        assert_not_built(crate::Av1Decoder::new(), "dav1d");
    }
}
