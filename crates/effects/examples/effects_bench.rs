//! Per-frame effect time on 720p test-camera frames with the real models (FR-8.37).
//!
//! Run with `cargo run --release -p mumcord-effects --example effects_bench [photo]`. Prints the
//! median and 90th percentile per configuration, and the model's share. Without a photo it uses
//! the test camera's drawing, which the model does not see as a person, so the edge work is
//! missing; pass a 16:9 photo of a person for realistic numbers.

use std::sync::Arc;
use std::time::{Duration, Instant};

use mumcord_camera::{TestCamera, nv12_len};
use mumcord_effects::{
    Background, BackgroundImage, Level, ModelShape, OnnxSegmenter, Pipeline, Segmenter,
};

const FRAMES: usize = 300;
const WARMUP: usize = 20;

/// Wraps a segmenter and adds up the time spent in it.
struct Timed<'a> {
    inner: &'a mut OnnxSegmenter,
    spent: Duration,
}

impl Segmenter for Timed<'_> {
    fn shape(&self) -> ModelShape {
        self.inner.shape()
    }

    fn segment(
        &mut self,
        rgb: &[f32],
        timestamp: Duration,
        mask: &mut [f32],
    ) -> Result<(), mumcord_effects::EffectsError> {
        let start = Instant::now();
        let result = self.inner.segment(rgb, timestamp, mask);
        self.spent += start.elapsed();
        result
    }
}

fn percentile(times: &mut [Duration], p: f64) -> f64 {
    times.sort_unstable();
    let i = ((times.len() - 1) as f64 * p).round() as usize;
    times[i].as_secs_f64() * 1000.0
}

/// A photo scaled to the frame and converted to NV12 (BT.601 limited range).
fn photo_frame(path: &str, w: u32, h: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let rgb = image::open(path)?
        .resize_exact(w, h, image::imageops::FilterType::Triangle)
        .into_rgb8();
    let (wu, hu) = (w as usize, h as usize);
    let mut nv12 = vec![0u8; nv12_len(w, h)];
    let yuv = |p: &image::Rgb<u8>| {
        let [r, g, b] = p.0.map(f32::from);
        (
            16.0 + (65.481 * r + 128.553 * g + 24.966 * b) / 255.0,
            128.0 + (-37.797 * r - 74.203 * g + 112.0 * b) / 255.0,
            128.0 + (112.0 * r - 93.786 * g - 18.214 * b) / 255.0,
        )
    };
    for (i, p) in rgb.pixels().enumerate() {
        nv12[i] = yuv(p).0.round() as u8;
    }
    for cy in 0..hu / 2 {
        for cx in 0..wu / 2 {
            let (_, u, v) = yuv(rgb.get_pixel(cx as u32 * 2, cy as u32 * 2));
            nv12[wu * hu + cy * wu + cx * 2] = u.round() as u8;
            nv12[wu * hu + cy * wu + cx * 2 + 1] = v.round() as u8;
        }
    }
    Ok(nv12)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    mumcord_effects::availability().map_err(|reason| format!("ONNX Runtime: {reason}"))?;
    let (w, h) = (1280u32, 720u32);
    let frames: Vec<Vec<u8>> = match std::env::args().nth(1) {
        Some(path) => vec![photo_frame(&path, w, h)?],
        None => (0..120)
            .map(|n| {
                let mut nv12 = vec![0u8; nv12_len(w, h)];
                TestCamera::render(n, &mut nv12);
                nv12
            })
            .collect(),
    };
    let image = Background::Image(Arc::new(BackgroundImage::from_rgb(
        image::RgbImage::from_fn(1920, 1080, |x, y| {
            image::Rgb([(x / 8) as u8, (y / 5) as u8, ((x + y) / 12) as u8])
        }),
    )));
    let mut square = OnnxSegmenter::new(ModelShape::Square)?;
    let mut landscape = OnnxSegmenter::new(ModelShape::Landscape)?;
    let cases = [
        ("Best, blur", Level::Full, Background::Blur { strength: 50 }),
        ("Best, image", Level::Full, image.clone()),
        (
            "Light, blur",
            Level::Light,
            Background::Blur { strength: 50 },
        ),
        ("Light, image", Level::Light, image),
    ];
    println!("{FRAMES} frames of 1280x720 per case (after {WARMUP} warm-up frames)");
    for (name, level, background) in cases {
        let segmenter = match level {
            Level::Full => &mut square,
            Level::Light => &mut landscape,
        };
        let mut timed = Timed {
            inner: segmenter,
            spent: Duration::ZERO,
        };
        let mut pipeline = Pipeline::new(w, h, background);
        pipeline.set_level(level);
        let mut frame = frames[0].clone();
        let mut times = Vec::with_capacity(FRAMES);
        for i in 0..WARMUP + FRAMES {
            if i == WARMUP {
                timed.spent = Duration::ZERO;
            }
            frame.copy_from_slice(&frames[i % frames.len()]);
            // 30 fps timestamps, so Light runs its model on every second frame.
            let timestamp = Duration::from_secs_f64(i as f64 / 30.0);
            let start = Instant::now();
            pipeline.process(&mut frame, timestamp, &mut timed)?;
            if i >= WARMUP {
                times.push(start.elapsed());
            }
        }
        let model_ms = timed.spent.as_secs_f64() * 1000.0 / FRAMES as f64;
        let median = percentile(&mut times, 0.5);
        let p90 = percentile(&mut times, 0.9);
        println!(
            "{name:<13} median {median:6.2} ms   p90 {p90:6.2} ms   model {model_ms:5.2} ms/frame"
        );
    }
    Ok(())
}
