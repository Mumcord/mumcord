//! The host callbacks of the platform-fed backend must not allocate or free (CLAUDE.md real-time
//! rule), also not while the engine reopens its streams. A counting global allocator checks it;
//! counts are per thread, so the engine's control thread and parallel tests do not interfere.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::Duration;

use mumcord_audio::io::{
    AudioHandles, AudioIo, BackendKind, DeviceEvent, ExternalFormat, external,
};
use mumcord_audio::{FRAME_SAMPLES, Frame};

struct Counting;

thread_local! {
    static HEAP_CALLS: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    HEAP_CALLS.with(|count| count.set(count.get() + 1));
}

// SAFETY: forwards every call unchanged to the system allocator; it only counts the calls.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count();
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn heap_calls() -> usize {
    HEAP_CALLS.with(Cell::get)
}

fn wait_active(handles: &AudioHandles, count: usize) {
    for _ in 0..count {
        loop {
            let event = handles
                .events
                .recv_timeout(Duration::from_secs(5))
                .expect("stream start");
            if matches!(event, DeviceEvent::Active { .. }) {
                break;
            }
        }
    }
}

#[test]
fn host_callbacks_do_not_allocate_or_free() {
    let format = ExternalFormat {
        sample_rate: 48_000,
        channels: 2,
    };
    let (audio, mut capture, mut playback) = external(format, format).expect("external");
    let mut handles = AudioIo::start_with(BackendKind::External(audio), None, None).expect("start");
    wait_active(&handles, 2);

    let captured_f32 = [0.25f32; 2 * FRAME_SAMPLES];
    let captured_i16 = [1000i16; 2 * FRAME_SAMPLES];
    let mut played_f32 = [0.0f32; 2 * FRAME_SAMPLES];
    let mut played_i16 = [0i16; 2 * FRAME_SAMPLES];
    let mut frame: Frame = [0.0; FRAME_SAMPLES];
    let mut measured = 0;

    // Three rounds: the streams as first opened, then reopened twice (the old streams are
    // handed back to the engine, never freed in the callback).
    for round in 0..3 {
        if round > 0 {
            handles.io.set_low_latency(round == 1).expect("reopen");
            wait_active(&handles, 2);
        }
        for _ in 0..50 {
            handles.sink.write_frame(&[0.5; FRAME_SAMPLES]);
            let before = heap_calls();
            capture.push_f32(&captured_f32);
            capture.push_i16(&captured_i16);
            playback.pull_f32(&mut played_f32);
            playback.pull_i16(&mut played_i16);
            measured += heap_calls() - before;
            while handles.source.read_frame(&mut frame) {}
        }
    }
    assert_eq!(measured, 0, "a host callback used the heap");
    assert!(played_f32.iter().any(|&s| s != 0.0) || played_i16.iter().any(|&s| s != 0));
}
