//! Encode → decode round trips on a synthetic screen-like pattern.

mod common;

use common::{pattern, psnr, scrolling_text};
use mumcord_screenshare::{Decoder, EncodedFrame, Encoder, EncoderConfig, I420Frame};

const FPS: u32 = 30;

fn config(width: u32, height: u32) -> EncoderConfig {
    EncoderConfig {
        width,
        height,
        fps: FPS,
        bitrate_kbps: 1500,
    }
}

fn encode_one(encoder: &mut Encoder, frame: &I420Frame, index: u32, key: bool) -> EncodedFrame {
    let mut out = encoder
        .encode(frame, i64::from(index * 1000 / FPS), key)
        .expect("encode");
    assert_eq!(out.len(), 1, "one packet per frame");
    out.remove(0)
}

#[test]
fn round_trip_keeps_quality() {
    let mut encoder = Encoder::new(config(1280, 720)).expect("encoder");
    let mut decoder = Decoder::new().expect("decoder");
    let mut last_psnr = 0.0;
    for index in 0..30 {
        let frame = pattern(1280, 720, index);
        let encoded = encode_one(&mut encoder, &frame, index, false);
        let decoded = decoder
            .decode(&encoded.data)
            .expect("decode")
            .expect("picture");
        assert_eq!((decoded.width(), decoded.height()), (1280, 720));
        last_psnr = psnr(frame.y(), decoded.y());
    }
    assert!(last_psnr > 30.0, "luma PSNR {last_psnr:.1} dB");
}

#[test]
fn temporal_layers_follow_l1t3() {
    let mut encoder = Encoder::new(config(640, 480)).expect("encoder");
    let layers: Vec<u8> = (0..12)
        .map(|index| {
            encode_one(&mut encoder, &pattern(640, 480, index), index, false).temporal_layer
        })
        .collect();
    assert_eq!(layers, [0, 2, 1, 2, 0, 2, 1, 2, 0, 2, 1, 2]);
}

#[test]
fn lower_layers_decode_without_upper_layers() {
    // A slow viewer at quarter frame rate gets only layer 0, at half frame rate layers 0 and 1.
    for max_layer in [0, 1] {
        let mut encoder = Encoder::new(config(640, 480)).expect("encoder");
        let mut decoder = Decoder::new().expect("decoder");
        let mut decoded_frames = 0;
        for index in 0..16 {
            let encoded = encode_one(&mut encoder, &pattern(640, 480, index), index, false);
            if encoded.temporal_layer <= max_layer {
                let decoded = decoder
                    .decode(&encoded.data)
                    .expect("decode reduced stream");
                assert!(decoded.is_some());
                decoded_frames += 1;
            }
        }
        assert_eq!(decoded_frames, if max_layer == 0 { 4 } else { 8 });
    }
}

#[test]
fn keyframe_on_request() {
    let mut encoder = Encoder::new(config(640, 480)).expect("encoder");
    let mut keys = Vec::new();
    for index in 0..10 {
        let encoded = encode_one(&mut encoder, &pattern(640, 480, index), index, index == 6);
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
    let mut encoder = Encoder::new(config(640, 480)).expect("encoder");
    for index in 0..5 {
        encode_one(&mut encoder, &pattern(640, 480, index), index, false);
    }
    let encoded = encode_one(&mut encoder, &pattern(640, 480, 5), 5, true);
    let mut late_viewer = Decoder::new().expect("decoder");
    let decoded = late_viewer.decode(&encoded.data).expect("decode keyframe");
    assert!(decoded.is_some());
}

#[test]
fn resolution_steps_down_and_up() {
    let mut encoder = Encoder::new(config(1280, 720)).expect("encoder");
    let mut decoder = Decoder::new().expect("decoder");
    let steps = [(1280, 720), (854, 480), (1280, 720)];
    let mut index = 0;
    for (width, height) in steps {
        encoder
            .set_resolution(width, height)
            .expect("set resolution");
        for _ in 0..8 {
            let frame = pattern(width, height, index);
            let encoded = encode_one(&mut encoder, &frame, index, false);
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
fn resolution_grows_beyond_the_opened_size() {
    // A shared window that grows, or a quality step up on an encoder opened small: libvpx
    // corrupts its heap when a running VP9 encoder is grown past the size it was opened at.
    let mut encoder = Encoder::new(config(320, 180)).expect("encoder");
    let mut decoder = Decoder::new().expect("decoder");
    let steps = [(320, 180), (1280, 720), (640, 360), (1920, 1080)];
    let mut index = 0;
    let mut largest = 0;
    for (width, height) in steps {
        encoder
            .set_resolution(width, height)
            .expect("set resolution");
        let grown = width > largest;
        largest = largest.max(width);
        for frame_in_step in 0..8 {
            let frame = pattern(width, height, index);
            let encoded = encode_one(&mut encoder, &frame, index, false);
            if frame_in_step == 0 && grown {
                assert!(encoded.keyframe, "a grown size starts with a keyframe");
            }
            let decoded = decoder
                .decode(&encoded.data)
                .expect("decode")
                .expect("picture");
            assert_eq!((decoded.width(), decoded.height()), (width, height));
            assert!(
                psnr(frame.y(), decoded.y()) > 20.0,
                "picture at {width}x{height}"
            );
            index += 1;
        }
    }
    assert_eq!(encoder.config().width, 1920);
}

#[test]
fn rejects_frame_of_wrong_size() {
    let mut encoder = Encoder::new(config(640, 480)).expect("encoder");
    assert!(encoder.encode(&pattern(320, 240, 0), 0, false).is_err());
}

#[test]
fn bitrate_change_keeps_encoding() {
    let mut encoder = Encoder::new(config(640, 480)).expect("encoder");
    encode_one(&mut encoder, &pattern(640, 480, 0), 0, false);
    encoder.set_bitrate(500).expect("set bitrate");
    encode_one(&mut encoder, &pattern(640, 480, 1), 1, false);
    assert_eq!(encoder.config().bitrate_kbps, 500);
}

#[test]
fn odd_sizes_round_trip() {
    let mut encoder = Encoder::new(config(321, 241)).expect("encoder");
    let mut decoder = Decoder::new().expect("decoder");
    let frame = pattern(321, 241, 0);
    let encoded = encode_one(&mut encoder, &frame, 0, false);
    let decoded = decoder
        .decode(&encoded.data)
        .expect("decode")
        .expect("picture");
    assert_eq!((decoded.width(), decoded.height()), (321, 241));
    assert!(psnr(frame.y(), decoded.y()) > 25.0);
}

#[test]
fn bitrate_stays_near_target() {
    let (width, height, seconds) = (1280, 720, 5);
    let mut encoder = Encoder::new(config(width, height)).expect("encoder");
    let mut bytes = 0;
    for index in 0..FPS * seconds {
        let frame = scrolling_text(width, height, index);
        bytes += encode_one(&mut encoder, &frame, index, false).data.len();
    }
    let kbps = bytes * 8 / 1000 / seconds as usize;
    let target = config(width, height).bitrate_kbps as usize;
    assert!(
        kbps <= target * 6 / 5,
        "{kbps} kbit/s for a target of {target}"
    );
}
