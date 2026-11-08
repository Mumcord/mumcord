//! Effects on the test camera's picture: the fixture mask (FR-8.41, the pixel check of AC-8.5),
//! the real model, and the effects thread end to end.
//!
//! Only where a package can carry ONNX Runtime: the real model and the thread both need the
//! library, and FreeBSD has no official build of it (see crates/effects/build.rs).
#![cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mumcord_camera::{CameraError, CameraFrame, FramePool, FrameSink, TestCamera, nv12_len};
use mumcord_effects::{
    Background, EffectStats, Effects, EffectsConfig, FixtureSegmenter, Level, ModelShape,
    OnnxSegmenter, Pipeline, Segmenter,
};
use mumcord_store::CameraEffectQuality;

const W: usize = 1280;
const H: usize = 720;
const BAR: usize = 160;
/// Beyond the reach of the edge refinement (two box filters of 16 px plus upsampling).
const MARGIN: u32 = 48;

fn testcam_frame(n: u64) -> Vec<u8> {
    let mut nv12 = vec![0u8; nv12_len(W as u32, H as u32)];
    TestCamera::render(n, &mut nv12);
    nv12
}

/// Person pixels whose whole `±MARGIN` square lies inside the person.
fn person_interior(frame: u64) -> Vec<(usize, usize)> {
    let inside = |x: u32, y: u32| {
        (y - MARGIN..=(y + MARGIN).min(H as u32 - 1)).all(|row| {
            TestCamera::person_span(row, frame)
                .is_some_and(|(s, e)| s + MARGIN <= x && x + MARGIN < e)
        })
    };
    let mut points = Vec::new();
    for y in (200..H as u32).step_by(9) {
        for x in (0..W as u32).step_by(7) {
            if inside(x, y) {
                points.push((x as usize, y as usize));
            }
        }
    }
    points
}

/// Background pixels next to a bar boundary (where a blur must change them), far from the
/// person.
fn background_edges(frame: u64) -> Vec<(usize, usize)> {
    let mut points = Vec::new();
    for y in (8..H - 8).step_by(11) {
        let span = TestCamera::person_span(y as u32, frame);
        for boundary in (BAR..W).step_by(BAR) {
            for x in [boundary - 2, boundary + 1] {
                let far =
                    span.is_none_or(|(s, e)| (x as u32) + MARGIN < s || (x as u32) > e + MARGIN);
                if far {
                    points.push((x, y));
                }
            }
        }
    }
    points
}

fn mean_luma_change(a: &[u8], b: &[u8], points: &[(usize, usize)]) -> f64 {
    let total: u64 = points
        .iter()
        .map(|&(x, y)| u64::from(a[y * W + x].abs_diff(b[y * W + x])))
        .sum();
    total as f64 / points.len() as f64
}

#[test]
fn fixture_mask_keeps_the_person_and_blurs_the_background_pixel_exactly() {
    let mut pipeline = Pipeline::new(W as u32, H as u32, Background::Blur { strength: 60 });
    let mut segmenter = FixtureSegmenter::new(ModelShape::Square, 30);
    for n in [0u64, 1, 2, 45] {
        let original = testcam_frame(n);
        let mut out = original.clone();
        let timestamp = Duration::from_secs_f64(n as f64 / 30.0);
        pipeline
            .process(&mut out, timestamp, &mut segmenter as &mut dyn Segmenter)
            .expect("process");
        for (x, y) in person_interior(n) {
            assert_eq!(
                out[y * W + x],
                original[y * W + x],
                "frame {n} person at {x},{y}"
            );
            let c = W * H + (y / 2) * W + (x / 2) * 2;
            assert_eq!(
                out[c..c + 2],
                original[c..c + 2],
                "frame {n} chroma at {x},{y}"
            );
        }
        let change = mean_luma_change(&original, &out, &background_edges(n));
        assert!(change > 5.0, "frame {n}: background changed by {change}");
    }
}

#[test]
fn the_real_model_runs_and_changes_the_background() {
    if let Err(reason) = mumcord_effects::availability() {
        panic!("ONNX Runtime should be available in tests (fetched by build.rs): {reason}");
    }
    for (shape, level) in [
        (ModelShape::Square, Level::Full),
        (ModelShape::Landscape, Level::Light),
    ] {
        let mut segmenter = OnnxSegmenter::new(shape).expect("model loads");
        let (mw, mh) = shape.size();
        let rgb = vec![0.5f32; 3 * mw * mh];
        let mut mask = vec![-1.0f32; mw * mh];
        segmenter
            .segment(&rgb, Duration::ZERO, &mut mask)
            .expect("segment");
        assert!(mask.iter().all(|v| (0.0..=1.0).contains(v)), "{shape:?}");

        let mut pipeline = Pipeline::new(W as u32, H as u32, Background::Blur { strength: 60 });
        pipeline.set_level(level);
        let original = testcam_frame(0);
        let mut out = original.clone();
        pipeline
            .process(
                &mut out,
                Duration::ZERO,
                &mut segmenter as &mut dyn Segmenter,
            )
            .expect("process");
        let change = mean_luma_change(&original, &out, &background_edges(0));
        assert!(change > 2.0, "{shape:?}: background changed by {change}");
    }
}

/// Collects what the effects thread passes on.
#[derive(Clone, Default)]
struct Collector {
    frames: Arc<Mutex<Vec<Vec<u8>>>>,
    ended: Arc<Mutex<Option<CameraError>>>,
}

impl FrameSink for Collector {
    fn frame(&mut self, frame: CameraFrame) {
        if let Ok(mut frames) = self.frames.lock() {
            frames.push(frame.data.to_vec());
        }
    }

    fn ended(&mut self, reason: CameraError) {
        if let Ok(mut ended) = self.ended.lock() {
            *ended = Some(reason);
        }
    }
}

/// Feeds test-camera frames at about 30 fps until `wanted` frames came out.
fn feed(input: &mut Box<dyn FrameSink>, collector: &Collector, wanted: usize) {
    let pool = FramePool::new(4, nv12_len(W as u32, H as u32));
    let start = Instant::now();
    let mut n = 0u64;
    while collector.frames.lock().map(|f| f.len()).unwrap_or_default() < wanted {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "effects thread stalled"
        );
        if let Some(mut data) = pool.acquire() {
            TestCamera::render(n, &mut data);
            input.frame(CameraFrame {
                width: W as u32,
                height: H as u32,
                timestamp: start.elapsed(),
                data,
            });
        }
        n += 1;
        std::thread::sleep(Duration::from_millis(33));
    }
}

#[test]
fn the_effects_thread_processes_frames_and_reports_stats() {
    let collector = Collector::default();
    let config = EffectsConfig {
        fps: 30,
        quality: CameraEffectQuality::Best,
        background: Background::Blur { strength: 50 },
        on_battery: Arc::new(|| false),
    };
    let (effects, mut input) =
        Effects::start(config, Box::new(collector.clone())).expect("start effects");
    feed(&mut input, &collector, 10);
    let stats: EffectStats = effects.stats();
    assert_eq!(stats.level, Some(Level::Full));
    assert_eq!(stats.unavailable, None);
    assert!(
        stats.frames >= 10 && stats.median > Duration::ZERO,
        "{stats:?}"
    );

    // Background None passes frames through unchanged and reports no level.
    effects.set_background(Background::None);
    let before = collector.frames.lock().map(|f| f.len()).unwrap_or_default();
    feed(&mut input, &collector, before + 3);
    assert_eq!(effects.stats().level, None);
    let last = collector
        .frames
        .lock()
        .ok()
        .and_then(|f| f.last().cloned())
        .expect("a frame");
    // The sway repeats every 120 frames, so one of these is the raw picture.
    assert!(
        (0..120).any(|n| testcam_frame(n) == last),
        "Background None changed the picture"
    );

    input.ended(CameraError::DeviceGone);
    let start = Instant::now();
    while collector.ended.lock().map(|e| e.is_none()).unwrap_or(true) {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "end not passed on"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(effects);
}
