//! The frame path must not allocate after warm-up (CLAUDE.md real-time rule). A counting global
//! allocator checks it; counts are per thread, so other tests running in parallel do not interfere.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use mumcord_camera::{FramePool, TestCamera, nv12_len};

struct Counting;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: forwards every call unchanged to the system allocator; it only counts allocations.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|count| count.set(count.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.with(|count| count.set(count.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
}

#[test]
fn acquiring_rendering_and_returning_frames_does_not_allocate() {
    let pool = FramePool::new(4, nv12_len(1280, 720));

    let before = allocations();
    for frame in 0..60 {
        let mut a = pool.acquire().expect("buffer");
        let b = pool.acquire().expect("second buffer in flight");
        TestCamera::render(frame, &mut a);
        drop(b);
        drop(a);
    }
    assert_eq!(allocations() - before, 0, "the frame path allocated");
    assert_eq!(pool.available(), 4);
}
