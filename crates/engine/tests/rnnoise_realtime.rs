//! `rnnoise-rs` runs on the real-time capture thread, so a frame must neither allocate nor free
//! memory (same rule as the rest of the chain in `mumcord-codec`). A test binary of its own, because
//! the counting allocator replaces the global one.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use mumcord_audio::dsp::SuppressionBackend;
use mumcord_audio::{FRAME_SAMPLES, Frame};
use mumcord_engine::noise_backend::RnnoiseBackend;

/// Counts allocator calls of the current thread only, so the test harness threads do not count.
struct CountingAlloc;

thread_local! {
    static CALLS: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: delegates to the system allocator; the counter is a const-initialised thread-local
// `Cell`, which never allocates.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        CALLS.with(|c| c.set(c.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        CALLS.with(|c| c.set(c.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        CALLS.with(|c| c.set(c.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

#[test]
fn rnnoise_frames_neither_allocate_nor_free() {
    let mut backend = RnnoiseBackend::new();
    let mut output: Frame = [0.0; FRAME_SAMPLES];
    let mut phase = 0.0f32;
    let mut frame = || -> Frame {
        std::array::from_fn(|_| {
            phase += 0.05;
            0.2 * phase.sin()
        })
    };
    // Warm-up, in case the model sizes anything on its first frames.
    for _ in 0..10 {
        backend.process(&frame(), &mut output);
    }
    let input: Vec<Frame> = (0..200).map(|_| frame()).collect();

    let before = CALLS.with(Cell::get);
    for frame in &input {
        backend.process(frame, &mut output);
    }
    assert_eq!(
        CALLS.with(Cell::get) - before,
        0,
        "rnnoise-rs allocated or freed memory while processing frames"
    );
}
