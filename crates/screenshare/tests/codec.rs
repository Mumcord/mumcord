//! Encode → decode round trips on a synthetic screen-like pattern.

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

/// Gradient background with a moving bar of sharp-edged blocks, like scrolling text.
fn pattern(width: u32, height: u32, index: u32) -> I420Frame {
    let (w, h) = (width as usize, height as usize);
    let mut data = vec![0u8; I420Frame::buffer_len(width, height)];
    let shift = index as usize * 4;
    for y in 0..h {
        for x in 0..w {
            let text = (y / 16).is_multiple_of(3) && ((x + shift) / 8).is_multiple_of(2);
            data[y * w + x] = if text {
                20
            } else {
                (64 + (x * 128) / w + (y * 32) / h) as u8
            };
        }
    }
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    for i in 0..cw * ch {
        data[w * h + i] = 110 + (i % cw * 20 / cw) as u8;
        data[w * h + cw * ch + i] = 140;
    }
    I420Frame::new(width, height, data).expect("pattern has the right size")
}

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let mse = a
        .iter()
        .zip(b)
        .map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2))
        .sum::<f64>()
        / a.len() as f64;
    if mse == 0.0 {
        return f64::INFINITY;
    }
    10.0 * (255.0f64.powi(2) / mse).log10()
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

/// Rows of glyph-like dots scrolling up every frame: everything changes, the hardest case for
/// rate control.
fn scrolling_text(width: u32, height: u32, index: u32) -> I420Frame {
    let (w, h) = (width as usize, height as usize);
    let mut data = vec![128u8; I420Frame::buffer_len(width, height)];
    let scroll = index as usize * 3;
    for y in 0..h {
        for x in 0..w {
            let glyph =
                ((y + scroll) / 18).is_multiple_of(2) && ((x * 7 + y * 3 + scroll) % 11) < 4;
            data[y * w + x] = if glyph { 30 } else { (180 + x * 60 / w) as u8 };
        }
    }
    I420Frame::new(width, height, data).expect("pattern has the right size")
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
