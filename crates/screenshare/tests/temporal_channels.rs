//! P0.4 spike for D3: every encoded frame can be routed to channel `1 + temporal_layer`.
//!
//! The media profile (spec addendum FR-7.90) carries video temporal layers on channels 1–4.
//! This test checks the encoder side of that mapping for the software encoders the screen share
//! and the camera use (both go through the same [`VideoEncoder`] trait):
//!
//! - every emitted frame reports a layer below the encoder's layer count and at most 3, so
//!   `1 + layer` is always one of channels 1–4;
//! - a keyframe is always layer 0 / channel 1, wherever in the pattern it is forced, so every
//!   viewer receives it (a viewer with any layer choice reads channel 1);
//! - dropping the channels above a viewer's layer leaves a stream that still decodes;
//! - the layer arrives with the frame itself, so the writer needs no extra state to pick the
//!   channel.
//!
//! The hardware paths (NVENC, VA-API, AMF, Quick Sync) cannot open without their GPUs; their
//! temporal layer and keyframe handling is covered by the mock-driver tests in `src/hw/`, which
//! run with `HWENC_SDK_DIR` set, and VA-API additionally by `tests/vaapi.rs` on a machine whose
//! driver encodes.
//!
//! SVT-AV1 refuses to open on a CPU without AVX2, so the AV1 branch of the software tests is
//! skipped there; run them on an AVX2 machine (CI) to cover it.
//!
//! Run:
//! `cargo test -p mumcord-screenshare --test temporal_channels`
//! (hardware mocks: `HWENC_SDK_DIR=<sdk> cargo test -p mumcord-screenshare --lib`)

#[allow(dead_code)] // the other integration tests use the rest of the shared helpers
mod common;

use common::pattern;
use mumcord_screenshare::{
    AV1_TEMPORAL_LAYERS, Av1Decoder, Av1Encoder, Av1Options, Decoder, EncodedFrame, Encoder,
    EncoderConfig, TEMPORAL_LAYERS, VideoDecoder, VideoEncoder, av1_cpu_supported,
};

/// Video channels 1–4 carry layers 0–3 (FR-7.90).
const MAX_CHANNELS: u8 = 4;
const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Path {
    Vp9,
    Av1,
}

impl Path {
    const BOTH: [Self; 2] = [Self::Vp9, Self::Av1];

    fn label(self) -> &'static str {
        match self {
            Self::Vp9 => "VP9 (libvpx)",
            Self::Av1 => "AV1 (SVT-AV1)",
        }
    }

    fn expected_layers(self) -> u8 {
        match self {
            Self::Vp9 => TEMPORAL_LAYERS,
            Self::Av1 => AV1_TEMPORAL_LAYERS,
        }
    }

    fn config(self, fps: u32) -> EncoderConfig {
        EncoderConfig {
            width: WIDTH,
            height: HEIGHT,
            fps,
            bitrate_kbps: 500,
        }
    }

    fn open(self, fps: u32) -> Box<dyn VideoEncoder> {
        match self {
            Self::Vp9 => Box::new(Encoder::new(self.config(fps)).expect("VP9 encoder")),
            Self::Av1 => Box::new(
                Av1Encoder::new(self.config(fps), Av1Options::default()).expect("AV1 encoder"),
            ),
        }
    }

    fn decoder(self) -> Box<dyn VideoDecoder> {
        match self {
            Self::Vp9 => Box::new(Decoder::new().expect("VP9 decoder")),
            Self::Av1 => Box::new(Av1Decoder::new().expect("AV1 decoder")),
        }
    }
}

fn channel(frame: &EncodedFrame) -> u8 {
    1 + frame.temporal_layer
}

/// The software paths this machine can actually open. SVT-AV1 refuses to start on a CPU
/// without AVX2, and that is an environment limit, not a D3 result.
fn software_paths() -> impl Iterator<Item = Path> {
    let av1 = av1_cpu_supported();
    if !av1 {
        eprintln!("skipping AV1 (SVT-AV1): this CPU has no AVX2");
    }
    Path::BOTH
        .into_iter()
        .filter(move |path| *path != Path::Av1 || av1)
}

/// The D3 invariant for one emitted frame.
fn check_frame(path: Path, count: u8, frame: &EncodedFrame, context: &str) {
    assert!(
        (1..=MAX_CHANNELS).contains(&count),
        "{} {context}: {count} temporal layers do not fit channels 1-{MAX_CHANNELS}",
        path.label()
    );
    assert!(
        frame.temporal_layer < count,
        "{} {context}: layer {} with {count} layers",
        path.label(),
        frame.temporal_layer
    );
    assert!(
        (1..=MAX_CHANNELS).contains(&channel(frame)),
        "{} {context}: layer {} maps outside channels 1-{MAX_CHANNELS}",
        path.label(),
        frame.temporal_layer
    );
    if frame.keyframe {
        assert_eq!(
            frame.temporal_layer,
            0,
            "{} {context}: keyframe on layer {} (channel {})",
            path.label(),
            frame.temporal_layer,
            channel(frame)
        );
    }
}

fn encode(encoder: &mut dyn VideoEncoder, index: u32, keyframe: bool) -> Vec<EncodedFrame> {
    let EncoderConfig {
        width, height, fps, ..
    } = encoder.config();
    encoder
        .encode(
            &pattern(width, height, index),
            i64::from(index) * 1000 / i64::from(fps),
            keyframe,
        )
        .expect("encode")
}

#[test]
fn software_paths_report_channels_1_to_4() {
    for path in software_paths() {
        assert_eq!(path.open(30).temporal_layers(), path.expected_layers());
        for fps in [15, 30, 60] {
            let mut encoder = path.open(fps);
            let count = encoder.temporal_layers();
            assert!(
                (1..=MAX_CHANNELS).contains(&count),
                "{} {fps} fps: {count} layers",
                path.label()
            );
            if path == Path::Vp9 {
                assert_eq!(count, 3, "VP9 L1T3 stays at three layers at {fps} fps");
            }

            let mut channels = Vec::new();
            let mut keyframes = 0;
            for index in 0..16 {
                for frame in encode(encoder.as_mut(), index, false) {
                    check_frame(path, count, &frame, &format!("{fps} fps frame {index}"));
                    channels.push(channel(&frame));
                    keyframes += usize::from(frame.keyframe);
                }
            }
            assert!(keyframes >= 1, "{} {fps} fps: no keyframe", path.label());
            assert!(
                channels.iter().all(|c| (1..=MAX_CHANNELS).contains(c)),
                "{} {fps} fps: channels {channels:?}",
                path.label()
            );
            if count > 1 {
                assert!(
                    channels.iter().any(|c| *c > 1),
                    "{} {fps} fps: only channel 1 used, layers not routed: {channels:?}",
                    path.label()
                );
            }
        }
    }
}

#[test]
fn a_forced_keyframe_is_channel_1_at_every_pattern_position() {
    // A three-layer pattern repeats every four frames (0-2-1-2); forcing a keyframe where a
    // higher layer would fall must still put it on layer 0. Eight positions cover the pattern
    // twice, and would catch a four-layer pattern too.
    for path in software_paths() {
        for key_at in 0..8 {
            let mut encoder = path.open(30);
            let count = encoder.temporal_layers();
            for index in 0..=key_at {
                let force = index == key_at;
                for frame in encode(encoder.as_mut(), index, force) {
                    check_frame(
                        path,
                        count,
                        &frame,
                        &format!("keyframe at {key_at}, frame {index}"),
                    );
                }
            }
            // The pattern after the keyframe keeps the layers inside the channels as well.
            for index in key_at + 1..=key_at + 4 {
                for frame in encode(encoder.as_mut(), index, false) {
                    check_frame(
                        path,
                        count,
                        &frame,
                        &format!("keyframe at {key_at}, frame {index}"),
                    );
                }
            }
        }
    }
}

#[test]
fn a_resolution_change_starts_on_channel_1() {
    // The engine forces a keyframe when the size changes because not every decoder follows a
    // size change on an inter frame (media.rs `reconfigure`); that keyframe must be layer 0.
    for path in software_paths() {
        let mut encoder = path.open(30);
        let count = encoder.temporal_layers();
        for index in 0..5 {
            for frame in encode(encoder.as_mut(), index, false) {
                check_frame(
                    path,
                    count,
                    &frame,
                    &format!("before resize, frame {index}"),
                );
            }
        }
        encoder
            .set_resolution(WIDTH / 2, HEIGHT / 2)
            .expect("resize");
        for index in 5..10 {
            let force = index == 5;
            for frame in encode(encoder.as_mut(), index, force) {
                check_frame(path, count, &frame, &format!("after resize, frame {index}"));
                if index == 5 {
                    assert!(frame.keyframe, "{}: no keyframe after resize", path.label());
                    assert_eq!(
                        channel(&frame),
                        1,
                        "{}: resize keyframe not on channel 1",
                        path.label()
                    );
                }
            }
        }
    }
}

#[test]
fn dropping_channels_above_a_layer_still_decodes() {
    // What a viewer with layer choice N sees: channels 1..=N+1 of the same encoded stream.
    for path in software_paths() {
        let mut encoder = path.open(30);
        let count = encoder.temporal_layers();
        let frames: Vec<EncodedFrame> = (0..16)
            .flat_map(|index| encode(encoder.as_mut(), index, false))
            .inspect(|frame| check_frame(path, count, frame, "decode run"))
            .collect();
        for max_layer in 0..count {
            let top_channel = 1 + max_layer;
            let mut decoder = path.decoder();
            let mut kept = 0;
            for frame in &frames {
                if channel(frame) > top_channel {
                    continue;
                }
                kept += 1;
                let decoded = decoder.decode(&frame.data).unwrap_or_else(|e| {
                    panic!(
                        "{} layer {max_layer}: frame on channel {} did not decode: {e}",
                        path.label(),
                        channel(frame)
                    )
                });
                assert!(
                    decoded.is_some(),
                    "{} layer {max_layer}: no picture for channel {}",
                    path.label(),
                    channel(frame)
                );
            }
            assert!(kept > 0);
        }
    }
}

/// The camera opens the natural-video variants of the same encoders (spec 08, FR-8.43); their
/// layer reporting must be the same.
#[test]
fn camera_encoders_report_the_same_channels() {
    let config = EncoderConfig {
        width: WIDTH,
        height: HEIGHT,
        fps: 30,
        bitrate_kbps: 500,
    };
    let mut natural = Encoder::natural(config).expect("natural VP9 encoder");
    let count = natural.temporal_layers();
    for index in 0..8 {
        for out in natural
            .encode(&pattern(WIDTH, HEIGHT, index), i64::from(index) * 33, false)
            .expect("encode")
        {
            check_frame(Path::Vp9, count, &out, &format!("camera frame {index}"));
        }
    }
}
