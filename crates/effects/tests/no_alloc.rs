//! The per-frame path allocates nothing once the pipeline is warmed up.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::time::Duration;

use common::{H, W, frame};
use mumcord_effects::{
    Background, BackgroundImage, FixtureSegmenter, Level, ModelShape, Pipeline, Segmenter,
};

struct Counting;

thread_local! {
    static COUNT: Cell<Option<usize>> = const { Cell::new(None) };
}

// SAFETY: every call is forwarded unchanged to the system allocator; the counter is a
// const-initialised thread-local, which does not allocate.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNT.with(|c| c.set(c.get().map(|n| n + 1)));
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        COUNT.with(|c| c.set(c.get().map(|n| n + 1)));
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn allocations_per_run(pipeline: &mut Pipeline, segmenter: &mut dyn Segmenter) -> usize {
    let mut nv12 = frame();
    for i in 0..3 {
        pipeline
            .process(&mut nv12, Duration::from_millis(33 * i), segmenter)
            .expect("warm-up");
    }
    COUNT.with(|c| c.set(Some(0)));
    for i in 3..13 {
        pipeline
            .process(&mut nv12, Duration::from_millis(33 * i), segmenter)
            .expect("process");
    }
    COUNT.with(|c| c.replace(None)).unwrap_or_default()
}

#[test]
fn processing_frames_does_not_allocate() {
    let image = image::RgbImage::from_pixel(64, 64, image::Rgb([20, 120, 220]));
    let image = Background::Image(Arc::new(BackgroundImage::from_rgb(image)));
    for (background, level, shape) in [
        (
            Background::Blur { strength: 70 },
            Level::Full,
            ModelShape::Square,
        ),
        (
            Background::Blur { strength: 70 },
            Level::Light,
            ModelShape::Landscape,
        ),
        (image.clone(), Level::Full, ModelShape::Square),
        (image, Level::Light, ModelShape::Landscape),
    ] {
        let mut pipeline = Pipeline::new(W as u32, H as u32, background);
        pipeline.set_level(level);
        let mut segmenter = FixtureSegmenter::new(shape, 30);
        let count = allocations_per_run(&mut pipeline, &mut segmenter);
        assert_eq!(count, 0, "{level:?}");
    }
}
