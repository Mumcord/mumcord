//! VA-API hardware encoder round trips (AV1 decoded with dav1d, VP9 with libvpx).
//!
//! They need a GPU whose VA-API driver encodes the codec. Without one (CI, a build without
//! `HWENC_SDK_DIR`, an NVIDIA GPU, AMD for VP9) each test returns early after noting it; set
//! `MUMCORD_REQUIRE_VAAPI=av1` (or `av1,vp9`) to make that a failure on a machine that has the
//! encoder.

mod common;

use common::{pattern, psnr, scrolling_text};
use mumcord_screenshare::{
    Av1Decoder, Codec, Decoder, EncodedFrame, EncoderConfig, Error, HardwareEncoder, I420Frame,
    VideoDecoder, VideoEncoder, open_hardware_encoder,
};

const CODECS: [Codec; 2] = [Codec::Av1, Codec::Vp9];

fn config(width: u32, height: u32, fps: u32) -> EncoderConfig {
    EncoderConfig {
        width,
        height,
        fps,
        bitrate_kbps: 3000,
    }
}

fn required(codec: Codec) -> bool {
    let name = match codec {
        Codec::Av1 => "av1",
        Codec::Vp9 => "vp9",
    };
    std::env::var("MUMCORD_REQUIRE_VAAPI").is_ok_and(|list| list.split(',').any(|c| c == name))
}

/// The VA-API encoder for `codec`, or `None` when this machine has none.
fn encoder(codec: Codec, config: EncoderConfig) -> Option<Box<dyn VideoEncoder>> {
    match open_hardware_encoder(HardwareEncoder::Vaapi, codec, config) {
        Ok(encoder) => Some(encoder),
        Err(Error::HardwareUnavailable { reason, .. }) if !required(codec) => {
            eprintln!("VA-API {codec:?} encoder unavailable, test skipped: {reason}");
            None
        }
        Err(err) => panic!("VA-API {codec:?}: {err}"),
    }
}

fn decoder(codec: Codec) -> Box<dyn VideoDecoder> {
    match codec {
        Codec::Av1 => Box::new(Av1Decoder::new().expect("dav1d")),
        Codec::Vp9 => Box::new(Decoder::new().expect("libvpx")),
    }
}

fn encode(encoder: &mut dyn VideoEncoder, index: u32, key: bool) -> EncodedFrame {
    let EncoderConfig {
        width, height, fps, ..
    } = encoder.config();
    let mut out = encoder
        .encode(
            &pattern(width, height, index),
            i64::from(index * 1000 / fps),
            key,
        )
        .expect("encode");
    assert_eq!(out.len(), 1, "one packet per frame");
    out.remove(0)
}

#[test]
fn round_trip_keeps_quality() {
    for codec in CODECS {
        let Some(mut encoder) = encoder(codec, config(1280, 720, 30)) else {
            continue;
        };
        assert_eq!(encoder.codec(), codec);
        let mut decoder = decoder(codec);
        let mut worst = f64::INFINITY;
        for index in 0..60 {
            let encoded = encode(encoder.as_mut(), index, false);
            assert_eq!(encoded.keyframe, index == 0, "{codec:?} frame {index}");
            assert_eq!(encoded.pts, i64::from(index * 1000 / 30));
            let decoded = decoder
                .decode(&encoded.data)
                .expect("decode")
                .expect("picture");
            assert_eq!((decoded.width(), decoded.height()), (1280, 720));
            let frame = pattern(1280, 720, index);
            worst = worst.min(psnr(frame.y(), decoded.y()));
        }
        eprintln!("VA-API {codec:?} 720p30 @ 3 Mbit/s: worst luma PSNR {worst:.1} dB");
        assert!(worst > 32.0, "{codec:?}: luma PSNR {worst:.1} dB");
    }
}

fn layers(codec: Codec, fps: u32, frames: u32) -> Option<(u8, Vec<u8>)> {
    let mut encoder = encoder(codec, config(640, 480, fps))?;
    let layers = (0..frames)
        .map(|index| encode(encoder.as_mut(), index, false).temporal_layer)
        .collect();
    Some((encoder.temporal_layers(), layers))
}

#[test]
fn three_temporal_layers_below_60_fps() {
    for codec in CODECS {
        if let Some((count, layers)) = layers(codec, 30, 12) {
            assert_eq!(count, 3);
            assert_eq!(layers, [0, 2, 1, 2, 0, 2, 1, 2, 0, 2, 1, 2]);
        }
    }
}

#[test]
fn four_temporal_layers_at_60_fps() {
    for codec in CODECS {
        if let Some((count, layers)) = layers(codec, 60, 16) {
            assert_eq!(count, 4);
            assert_eq!(layers, [0, 3, 2, 3, 1, 3, 2, 3, 0, 3, 2, 3, 1, 3, 2, 3]);
        }
    }
}

/// Encodes 64 frames and decodes only those up to `max_layer`; every kept frame must decode to
/// the right picture. Returns how many were kept.
fn decodes_up_to_layer(codec: Codec, fps: u32, max_layer: u8) -> Option<usize> {
    let mut encoder = encoder(codec, config(1280, 720, fps))?;
    let mut decoder = decoder(codec);
    let mut kept = 0;
    for index in 0..64 {
        let encoded = encode(encoder.as_mut(), index, false);
        if encoded.temporal_layer > max_layer {
            continue;
        }
        kept += 1;
        let decoded = decoder
            .decode(&encoded.data)
            .expect("decode")
            .expect("picture");
        let quality = psnr(pattern(1280, 720, index).y(), decoded.y());
        assert!(
            quality > 30.0,
            "{codec:?} {fps} fps up to layer {max_layer}, frame {index}: {quality:.1} dB"
        );
    }
    Some(kept)
}

#[test]
fn lower_layers_decode_without_upper_layers() {
    for codec in CODECS {
        if decodes_up_to_layer(codec, 30, 0).is_none() {
            continue;
        }
        assert_eq!(decodes_up_to_layer(codec, 30, 0), Some(16));
        assert_eq!(decodes_up_to_layer(codec, 30, 1), Some(32));
        assert_eq!(decodes_up_to_layer(codec, 60, 0), Some(8));
        assert_eq!(decodes_up_to_layer(codec, 60, 1), Some(16));
        assert_eq!(decodes_up_to_layer(codec, 60, 2), Some(32));
    }
}

#[test]
fn keyframe_lets_a_new_viewer_join() {
    for codec in CODECS {
        let Some(mut encoder) = encoder(codec, config(640, 480, 30)) else {
            continue;
        };
        for index in 0..10 {
            let encoded = encode(encoder.as_mut(), index, false);
            assert!(!encoded.keyframe || index == 0);
        }
        let key = encode(encoder.as_mut(), 10, true);
        assert!(key.keyframe);
        assert_eq!(key.temporal_layer, 0, "key frames are layer 0");
        let mut late_viewer = decoder(codec);
        assert!(late_viewer.decode(&key.data).expect("decode").is_some());
        for index in 11..20 {
            let encoded = encode(encoder.as_mut(), index, false);
            let decoded = late_viewer
                .decode(&encoded.data)
                .expect("decode")
                .expect("picture");
            assert!(psnr(pattern(640, 480, index).y(), decoded.y()) > 30.0);
        }
    }
}

#[test]
fn resolution_steps_down_and_up() {
    for codec in CODECS {
        let Some(mut encoder) = encoder(codec, config(1280, 720, 30)) else {
            continue;
        };
        let mut decoder = decoder(codec);
        let mut index = 0;
        for (width, height) in [(1280, 720), (640, 360), (1920, 1080), (1280, 720)] {
            encoder.set_resolution(width, height).expect("resize");
            for step in 0..5 {
                let encoded = encode(encoder.as_mut(), index, false);
                if step == 0 {
                    assert!(encoded.keyframe, "a new size starts with a key frame");
                }
                let decoded = decoder
                    .decode(&encoded.data)
                    .expect("decode")
                    .expect("picture");
                assert_eq!((decoded.width(), decoded.height()), (width, height));
                index += 1;
            }
        }
        assert!(encoder.set_resolution(8, 8).is_err());
        assert!(encoder.set_resolution(8192, 4320).is_err());
        assert!(
            encoder.encode(&pattern(640, 360, 0), 0, false).is_err(),
            "wrong frame size"
        );
    }
}

/// Luma of the top-left `width`×`height` of `frame`.
fn crop_y(frame: &I420Frame, width: u32, height: u32) -> Vec<u8> {
    let stride = frame.width() as usize;
    (0..height as usize)
        .flat_map(|row| &frame.y()[row * stride..][..width as usize])
        .copied()
        .collect()
}

// AMD's driver encodes widths rounded up to 8 and heights to 2 and doesn't signal the smaller
// render size, so a viewer gets up to 7 extra columns; they repeat the picture's edge.
#[test]
fn unaligned_sizes_round_trip() {
    for codec in CODECS {
        for (width, height) in [(321, 241), (642, 362), (1918, 1078)] {
            let Some(mut encoder) = encoder(codec, config(width, height, 30)) else {
                break;
            };
            let mut decoder = decoder(codec);
            let frame = pattern(width, height, 0);
            let encoded = encoder.encode(&frame, 0, false).expect("encode").remove(0);
            let decoded = decoder
                .decode(&encoded.data)
                .expect("decode")
                .expect("picture");
            let (dw, dh) = (decoded.width(), decoded.height());
            assert!(
                (width..width + 16).contains(&dw) && (height..height + 16).contains(&dh),
                "{codec:?} {width}x{height} decoded as {dw}x{dh}"
            );
            let quality = psnr(frame.y(), &crop_y(&decoded, width, height));
            assert!(
                quality > 30.0,
                "{codec:?} {width}x{height}: {quality:.1} dB"
            );
            if dw > width {
                let padding = crop_y(&decoded, dw, height);
                let edge =
                    |row: usize, col: u32| i32::from(padding[row * dw as usize + col as usize]);
                for row in (0..height as usize).step_by(7) {
                    let diff = (edge(row, dw - 1) - edge(row, width - 1)).abs();
                    assert!(
                        diff < 24,
                        "{codec:?} {width}x{height} row {row}: padding differs by {diff}"
                    );
                }
            }
        }
    }
}

/// Average bitrate of seconds 2–4 of scrolling text, the hardest case for rate control.
fn measured_kbps(encoder: &mut dyn VideoEncoder, first: u32) -> f64 {
    let EncoderConfig {
        width, height, fps, ..
    } = encoder.config();
    let mut bytes = 0;
    for index in first..first + 4 * fps {
        let encoded = encoder
            .encode(
                &scrolling_text(width, height, index),
                i64::from(index * 1000 / fps),
                false,
            )
            .expect("encode")
            .remove(0);
        if index >= first + fps {
            bytes += encoded.data.len();
        }
    }
    (bytes * 8) as f64 / 3.0 / 1000.0
}

#[test]
fn bitrate_follows_live_changes() {
    for codec in CODECS {
        let config = EncoderConfig {
            bitrate_kbps: 2000,
            ..config(1280, 720, 30)
        };
        let Some(mut encoder) = encoder(codec, config) else {
            continue;
        };
        let high = measured_kbps(encoder.as_mut(), 0);
        encoder.set_bitrate(800).expect("bitrate");
        let low = measured_kbps(encoder.as_mut(), 1000);
        eprintln!("VA-API {codec:?}: 2000 kbit/s target → {high:.0}, 800 → {low:.0}");
        assert!(
            (1400.0..2600.0).contains(&high),
            "{codec:?}: {high:.0} kbit/s"
        );
        assert!((500.0..1100.0).contains(&low), "{codec:?}: {low:.0} kbit/s");
    }
}

#[test]
fn fps_change_switches_the_layer_count() {
    for codec in CODECS {
        let Some(mut encoder) = encoder(codec, config(640, 480, 30)) else {
            continue;
        };
        let mut decoder = decoder(codec);
        let mut layers = Vec::new();
        for index in 0..14 {
            if index == 6 {
                encoder.set_fps(60).expect("fps");
            }
            let encoded = encode(encoder.as_mut(), index, false);
            assert!(decoder.decode(&encoded.data).expect("decode").is_some());
            layers.push(encoded.temporal_layer);
        }
        // The 3-layer pattern finishes; the next layer 0 starts the 4-layer one.
        assert_eq!(layers, [0, 2, 1, 2, 0, 2, 1, 2, 0, 3, 2, 3, 1, 3]);
        assert_eq!(encoder.temporal_layers(), 4);
        assert_eq!(encoder.config().fps, 60);
    }
}
