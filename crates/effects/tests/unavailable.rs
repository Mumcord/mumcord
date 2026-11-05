//! "ONNX unavailable" fallback (FR-8.33, AC-8.28): effects are off with a reason and frames
//! pass unchanged. Its own test binary, because the runtime is loaded once per process.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mumcord_camera::{CameraError, CameraFrame, FramePool, FrameSink, TestCamera, nv12_len};
use mumcord_effects::{
    Background, Effects, EffectsConfig, EffectsError, ModelShape, OnnxSegmenter,
};
use mumcord_store::CameraEffectQuality;

#[derive(Clone, Default)]
struct Collector(Arc<Mutex<Vec<Vec<u8>>>>);

impl FrameSink for Collector {
    fn frame(&mut self, frame: CameraFrame) {
        if let Ok(mut frames) = self.0.lock() {
            frames.push(frame.data.to_vec());
        }
    }

    fn ended(&mut self, _reason: CameraError) {}
}

#[test]
fn without_onnx_runtime_effects_are_off_with_a_reason() {
    // SAFETY: set before any other thread of this test binary reads the environment.
    unsafe { std::env::set_var("MUMCORD_TEST_ONNX", "unavailable") };

    let reason = mumcord_effects::availability().expect_err("unavailable");
    assert!(!reason.is_empty());
    assert!(matches!(
        OnnxSegmenter::new(ModelShape::Square),
        Err(EffectsError::Unavailable(_))
    ));

    let collector = Collector::default();
    let config = EffectsConfig {
        fps: 30,
        quality: CameraEffectQuality::Auto,
        background: Background::Blur { strength: 50 },
        on_battery: Arc::new(|| false),
    };
    let (effects, mut input) =
        Effects::start(config, Box::new(collector.clone())).expect("start effects");
    let pool = FramePool::new(2, nv12_len(1280, 720));
    let mut data = pool.acquire().expect("buffer");
    TestCamera::render(0, &mut data);
    let raw = data.to_vec();
    input.frame(CameraFrame {
        width: 1280,
        height: 720,
        timestamp: Duration::ZERO,
        data,
    });
    let start = Instant::now();
    while collector.0.lock().map(|f| f.is_empty()).unwrap_or(true) {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "frame not passed on"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(collector.0.lock().expect("lock")[0], raw);
    let stats = effects.stats();
    assert_eq!(stats.level, None);
    assert_eq!(stats.unavailable.as_deref(), Some(reason.as_str()));
}
