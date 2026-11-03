//! SVT-AV1 → dav1d round trips, and both codecs driven through the shared traits.

mod common;

use common::{pattern, psnr, scrolling_text};
use mumcord_screenshare::{
    AV1_TEMPORAL_LAYERS, Av1Decoder, Av1Encoder, Av1Options, Codec, Decoder, EncodedFrame, Encoder,
    EncoderConfig, VideoDecoder, VideoEncoder,
};

fn config(width: u32, height: u32, fps: u32) -> EncoderConfig {
    EncoderConfig {
        width,
        height,
        fps,
        bitrate_kbps: 1500,
    }
}

fn encoder(width: u32, height: u32, fps: u32) -> Av1Encoder {
    Av1Encoder::new(config(width, height, fps), Av1Options::default()).expect("encoder")
}

fn encode_one(encoder: &mut dyn VideoEncoder, index: u32, key: bool) -> EncodedFrame {
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
    for screen_content in [false, true] {
        let mut encoder =
            Av1Encoder::new(config(1280, 720, 30), Av1Options { screen_content }).expect("encoder");
        let mut decoder = Av1Decoder::new().expect("decoder");
        let mut last_psnr = 0.0;
        for index in 0..30 {
            let frame = pattern(1280, 720, index);
            let encoded = encoder
                .encode(&frame, i64::from(index * 1000 / 30), false)
                .expect("encode")
                .remove(0);
            assert_eq!(encoded.pts, i64::from(index * 1000 / 30));
            let decoded = decoder
                .decode(&encoded.data)
                .expect("decode")
                .expect("picture");
            assert_eq!((decoded.width(), decoded.height()), (1280, 720));
            last_psnr = psnr(frame.y(), decoded.y());
        }
        assert!(
            last_psnr > 30.0,
            "screen content {screen_content}: luma PSNR {last_psnr:.1} dB"
        );
    }
}

fn layers(fps: u32, frames: u32) -> Vec<u8> {
    let mut encoder = encoder(640, 480, fps);
    (0..frames)
        .map(|index| encode_one(&mut encoder, index, false).temporal_layer)
        .collect()
}

#[test]
fn three_temporal_layers_up_to_30_fps() {
    let layers = layers(30, 12);
    assert_eq!(layers, [0, 2, 1, 2, 0, 2, 1, 2, 0, 2, 1, 2]);
}

#[test]
fn three_temporal_layers_at_60_fps() {
    // SVT-AV1's low-delay mode has no fourth layer (see AV1_TEMPORAL_LAYERS).
    assert_eq!(encoder(640, 480, 60).temporal_layers(), AV1_TEMPORAL_LAYERS);
    let layers = layers(60, 8);
    assert_eq!(layers, [0, 2, 1, 2, 0, 2, 1, 2]);
}

/// A slow viewer keeps only the layers up to `max_layer`; every frame it gets must decode.
fn decodes_up_to_layer(fps: u32, max_layer: u8) -> usize {
    let mut encoder = encoder(640, 480, fps);
    let mut decoder = Av1Decoder::new().expect("decoder");
    let mut decoded_frames = 0;
    for index in 0..32 {
        let encoded = encode_one(&mut encoder, index, false);
        if encoded.temporal_layer <= max_layer {
            let decoded = decoder
                .decode(&encoded.data)
                .expect("decode reduced stream");
            assert!(
                decoded.is_some(),
                "{fps} fps, up to layer {max_layer}: frame {index}"
            );
            decoded_frames += 1;
        }
    }
    decoded_frames
}

#[test]
fn lower_layers_decode_without_upper_layers() {
    assert_eq!(decodes_up_to_layer(30, 0), 8);
    assert_eq!(decodes_up_to_layer(30, 1), 16);
    assert_eq!(decodes_up_to_layer(60, 0), 8);
}

#[test]
fn keyframe_on_request() {
    let mut encoder = encoder(640, 480, 30);
    let mut keys = Vec::new();
    for index in 0..10 {
        let encoded = encode_one(&mut encoder, index, index == 6);
        keys.push(encoded.keyframe);
        if encoded.keyframe {
            assert_eq!(encoded.temporal_layer, 0, "keyframes are base layer");
        }
    }
    let expected: Vec<bool> = (0..10).map(|i| i == 0 || i == 6).collect();
    assert_eq!(keys, expected);
}

#[test]
fn keyframe_lets_a_new_viewer_join() {
    let mut encoder = encoder(640, 480, 30);
    for index in 0..5 {
        encode_one(&mut encoder, index, false);
    }
    let keyframe = encode_one(&mut encoder, 5, true);
    let mut late_viewer = Av1Decoder::new().expect("decoder");
    let decoded = late_viewer.decode(&keyframe.data).expect("decode keyframe");
    assert!(decoded.is_some());
    for index in 6..10 {
        let encoded = encode_one(&mut encoder, index, false);
        assert!(late_viewer.decode(&encoded.data).expect("decode").is_some());
    }
}

#[test]
fn resolution_steps_down_and_up() {
    let mut encoder = encoder(1280, 720, 30);
    let mut decoder = Av1Decoder::new().expect("decoder");
    let steps = [(1280, 720), (854, 480), (1920, 1080), (1280, 720)];
    let mut index = 0;
    for (step, (width, height)) in steps.into_iter().enumerate() {
        encoder
            .set_resolution(width, height)
            .expect("set resolution");
        for frame in 0..8 {
            let encoded = encode_one(&mut encoder, index, false);
            if step > 0 && frame == 0 {
                assert!(encoded.keyframe, "a new size starts with a keyframe");
            }
            let decoded = decoder
                .decode(&encoded.data)
                .expect("decode")
                .expect("picture");
            assert_eq!((decoded.width(), decoded.height()), (width, height));
            index += 1;
        }
    }
    assert_eq!(encoder.config().width, 1280);
}

#[test]
fn rejects_bad_sizes() {
    let mut encoder = encoder(640, 480, 30);
    assert!(encoder.encode(&pattern(320, 240, 0), 0, false).is_err());
    assert!(encoder.set_resolution(0, 480).is_err());
    assert_eq!(
        encoder.config().width,
        640,
        "a rejected size keeps the old one"
    );
}

#[test]
fn bitrate_and_fps_change_keep_encoding() {
    let mut encoder = encoder(640, 480, 30);
    let mut decoder = Av1Decoder::new().expect("decoder");
    let mut index = 0;
    let mut step = |encoder: &mut Av1Encoder, decoder: &mut Av1Decoder| {
        for _ in 0..8 {
            let encoded = encode_one(encoder, index, false);
            assert!(decoder.decode(&encoded.data).expect("decode").is_some());
            index += 1;
        }
    };
    step(&mut encoder, &mut decoder);
    encoder.set_bitrate(500).expect("set bitrate");
    step(&mut encoder, &mut decoder);
    encoder.set_fps(15).expect("set fps");
    step(&mut encoder, &mut decoder);
    encoder.set_fps(60).expect("set fps");
    step(&mut encoder, &mut decoder);
    assert_eq!(encoder.config().bitrate_kbps, 500);
    assert_eq!(encoder.config().fps, 60);
}

#[test]
fn odd_sizes_round_trip() {
    let mut encoder = encoder(321, 241, 30);
    let mut decoder = Av1Decoder::new().expect("decoder");
    let frame = pattern(321, 241, 0);
    let encoded = encoder.encode(&frame, 0, false).expect("encode").remove(0);
    let decoded = decoder
        .decode(&encoded.data)
        .expect("decode")
        .expect("picture");
    assert_eq!((decoded.width(), decoded.height()), (321, 241));
    assert!(psnr(frame.y(), decoded.y()) > 25.0);
}

#[test]
fn bitrate_stays_near_target() {
    for (fps, screen_content) in [(30, false), (30, true), (60, false)] {
        let (width, height, seconds) = (1280, 720, 5);
        let mut encoder =
            Av1Encoder::new(config(width, height, fps), Av1Options { screen_content })
                .expect("encoder");
        let mut bytes = 0;
        for index in 0..fps * seconds {
            let frame = scrolling_text(width, height, index);
            let pts = i64::from(index) * 1000 / i64::from(fps);
            bytes += encoder.encode(&frame, pts, false).expect("encode")[0]
                .data
                .len();
        }
        let kbps = bytes * 8 / 1000 / seconds as usize;
        let target = config(width, height, fps).bitrate_kbps as usize;
        assert!(
            kbps <= target * 6 / 5,
            "{fps} fps, screen content {screen_content}: {kbps} kbit/s for a target of {target}"
        );
    }
}

#[test]
fn rejects_garbage() {
    let mut decoder = Av1Decoder::new().expect("decoder");
    assert!(decoder.decode(&[]).is_err());
    assert!(decoder.decode(&[0xff; 64]).is_err());
}

#[test]
fn both_codecs_work_through_the_traits() {
    let encoders: Vec<(Box<dyn VideoEncoder>, Box<dyn VideoDecoder>)> = vec![
        (
            Box::new(Encoder::new(config(640, 480, 30)).expect("vp9 encoder")),
            Box::new(Decoder::new().expect("vp9 decoder")),
        ),
        (
            Box::new(encoder(640, 480, 30)),
            Box::new(Av1Decoder::new().expect("av1 decoder")),
        ),
    ];
    let mut codecs = Vec::new();
    for (mut encoder, mut decoder) in encoders {
        assert_eq!(encoder.codec(), decoder.codec());
        assert_eq!(encoder.temporal_layers(), 3);
        codecs.push(encoder.codec());
        let mut index = 0;
        for change in 0..3 {
            match change {
                1 => encoder.set_bitrate(800).expect("set bitrate"),
                2 => encoder.set_fps(15).expect("set fps"),
                _ => {}
            }
            for _ in 0..4 {
                let encoded = encode_one(encoder.as_mut(), index, index == 6);
                assert_eq!(encoded.keyframe, index == 0 || index == 6);
                assert!(decoder.decode(&encoded.data).expect("decode").is_some());
                index += 1;
            }
        }
        assert_eq!(encoder.config().fps, 15);
        assert_eq!(encoder.config().bitrate_kbps, 800);
    }
    assert_eq!(codecs, [Codec::Vp9, Codec::Av1]);
}

#[test]
fn live_bitrate_change_takes_effect() {
    let (width, height, fps) = (1280, 720, 30);
    let mut encoder = Av1Encoder::new(
        EncoderConfig {
            bitrate_kbps: 3000,
            ..config(width, height, fps)
        },
        Av1Options::default(),
    )
    .expect("encoder");
    let encode_seconds = |encoder: &mut Av1Encoder, from: u32, seconds: u32| {
        let mut bytes = 0;
        for index in from..from + fps * seconds {
            let frame = scrolling_text(width, height, index);
            let pts = i64::from(index) * 1000 / i64::from(fps);
            bytes += encoder.encode(&frame, pts, false).expect("encode")[0]
                .data
                .len();
        }
        bytes * 8 / 1000 / seconds as usize
    };
    let before = encode_seconds(&mut encoder, 0, 2);
    encoder.set_bitrate(600).expect("set bitrate");
    encode_seconds(&mut encoder, fps * 2, 1);
    let after = encode_seconds(&mut encoder, fps * 3, 2);
    assert!(before > 2000, "{before} kbit/s before the change");
    assert!(after <= 600 * 6 / 5, "{after} kbit/s after lowering to 600");
}

#[test]
fn switching_screen_content_tools_starts_with_a_keyframe() {
    let mut encoder = encoder(640, 480, 30);
    let mut decoder = Av1Decoder::new().expect("decoder");
    for index in 0..6 {
        let encoded = encode_one(&mut encoder, index, false);
        assert!(decoder.decode(&encoded.data).expect("decode").is_some());
    }
    encoder
        .set_options(Av1Options {
            screen_content: true,
        })
        .expect("set options");
    assert!(encoder.options().screen_content);
    for index in 6..10 {
        let encoded = encode_one(&mut encoder, index, false);
        assert_eq!(encoded.keyframe, index == 6);
        assert!(decoder.decode(&encoded.data).expect("decode").is_some());
    }
}
