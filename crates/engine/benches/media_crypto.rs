//! Benchmark of the media profile's encryption framing and today's direct header (spec 07,
//! AC-7.82). Companion to `crates/p2p/benches/media.rs`, which measures the mux and relay path.
//!
//! Run the release profile pinned to one core, on an otherwise idle machine:
//!
//! ```text
//! taskset -c 2 cargo bench -p mumcord-engine --bench media_crypto
//! ```
//!
//! The numbers are medians of several timed rounds, in nanoseconds per packet, with the wire
//! bytes of the resulting message and the allocations per packet in the timed loop. Every
//! operation is labelled with what it includes:
//!
//! - **TrackSealer::seal** — the new relay path's publisher side (FR-7.92): the flags byte, the
//!   compact SFrame header, the AEAD over the timestamp and the frame, and the tag, into a
//!   pre-reserved buffer. No mux framing and no payload copy beyond the AEAD's own.
//! - **TrackOpener::open** — the viewer side of the same path: header decode, key lookup, replay
//!   check, AEAD open in place. Each timed packet was sealed outside the timed region into a
//!   reusable buffer, so no per-packet copy from the network is included.
//! - **FrameSealer::seal / FrameOpener::open** — today's 0.8.0 relay path: the 14-byte clear
//!   header is the AAD and the ciphertext follows it. This is the regression baseline.
//! - **direct header (21 B)** — today's clear `STREAM_SCREEN` v2 header: `FrameHeader::encode`
//!   and `FrameHeader::decode`, no payload and no encryption.
//!
//! The payload sizes are the encoded frames: 150 B Opus, a 30 KB video delta and a 400 KB
//! keyframe. The bytes column is the whole message this layer produces, so it adds the flags,
//! header, timestamp and tag to the frame size.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::time::Instant;

use mumcord_engine::screen_share::broadcast::crypto::{
    FrameOpener, FrameSealer, TrackOpener, TrackSealer,
};
use mumcord_engine::screen_share::wire::{Codec, FrameHeader, FrameKind, V2};
use mumcord_p2p::broadcast::{Content, VideoCodec};

/// Number of timed rounds per operation; the reported figure is their median.
const ROUNDS: usize = 7;

/// Test payload sizes of W5: an Opus packet, a video delta frame and a keyframe.
struct Case {
    name: &'static str,
    channel: u16,
    len: usize,
    keyframe: bool,
    /// Timed seal calls per round.
    seal_iters: usize,
    /// Messages in one prepared batch for the open rows.
    batch: usize,
}

const CASES: [Case; 3] = [
    Case {
        name: "150 B audio",
        channel: 5,
        len: 150,
        keyframe: false,
        seal_iters: 20_000,
        batch: 2_000,
    },
    Case {
        name: "30 KB video delta",
        channel: 1,
        len: 30_000,
        keyframe: false,
        seal_iters: 4_000,
        batch: 200,
    },
    Case {
        name: "400 KB keyframe",
        channel: 1,
        len: 400_000,
        keyframe: true,
        seal_iters: 300,
        batch: 16,
    },
];

/// Counts allocations per thread, so the figures are not disturbed by other threads and the
/// counter itself never allocates.
struct CountingAlloc;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: delegates to the system allocator; the counter is a const-initialised thread-local
// `Cell`, which never allocates.
unsafe impl GlobalAlloc for CountingAlloc {
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
static GLOBAL: CountingAlloc = CountingAlloc;

fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).expect("elapsed is finite"));
    samples[samples.len() / 2]
}

/// Times `op` in [`ROUNDS`] rounds of `iters` calls and returns the median nanoseconds per call
/// and the allocations per call of the last round.
fn measure(iters: usize, mut op: impl FnMut()) -> (f64, usize) {
    for _ in 0..iters.clamp(1, 200) {
        op();
    }
    let mut samples = Vec::with_capacity(ROUNDS);
    let mut allocs = 0;
    for round in 0..ROUNDS {
        let before = allocations();
        let start = Instant::now();
        for _ in 0..iters {
            op();
        }
        let elapsed = start.elapsed();
        if round + 1 == ROUNDS {
            allocs = (allocations() - before) / iters;
        }
        samples.push(elapsed.as_nanos() as f64 / iters as f64);
    }
    (median(samples), allocs)
}

/// Times opening a whole prepared batch of messages: `prepare` re-seals the batch untimed, `open`
/// is timed and opens every message once. Returns the median nanoseconds per message and the
/// allocations per message.
fn measure_batch<S, O>(
    units: usize,
    sealer: &mut S,
    opener: &mut O,
    batch: &mut [Vec<u8>],
    mut prepare: impl FnMut(&mut S, &mut [Vec<u8>]),
    mut open: impl FnMut(&mut O, &mut [Vec<u8>]),
) -> (f64, usize) {
    // Warm up: grow the buffers and derive the opener's key once.
    prepare(sealer, batch);
    open(opener, batch);
    let mut samples = Vec::with_capacity(ROUNDS);
    let mut allocs = 0;
    for round in 0..ROUNDS {
        prepare(sealer, batch);
        let before = allocations();
        let start = Instant::now();
        open(opener, batch);
        let elapsed = start.elapsed();
        if round + 1 == ROUNDS {
            allocs = (allocations() - before) / units;
        }
        samples.push(elapsed.as_nanos() as f64 / units as f64);
    }
    (median(samples), allocs)
}

/// A deterministic payload whose bytes differ per `seed`.
fn payload(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|at| (at as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

fn print_header() {
    println!(
        "{:<18} {:<31} {:>9} {:>9} {:>8}",
        "size", "operation", "ns/packet", "wire B", "allocs"
    );
}

fn print_row(size: &str, operation: &str, ns: f64, wire: usize, allocs: usize) {
    println!("{size:<18} {operation:<31} {ns:>9.1} {wire:>9} {allocs:>8}");
}

fn main() {
    print_header();
    for case in &CASES {
        let frame = payload(case.len, 23);
        let content = Content::Video {
            codec: VideoCodec::Vp9,
            keyframe: case.keyframe,
            temporal_layer: if case.keyframe { 0 } else { 2 },
        };

        // New relay path, publisher: flags + SFrame header + AEAD(timestamp, frame) + tag.
        let mut sealer = TrackSealer::new().expect("a sealer with a fresh key");
        let mut sealed = Vec::new();
        let (ns, allocs) = measure(case.seal_iters, || {
            sealer
                .seal(
                    case.channel,
                    case.keyframe,
                    1_234_567_890,
                    black_box(&frame),
                    &mut sealed,
                )
                .expect("the benchmark frame is within the profile");
            black_box(sealed.len());
        });
        let wire = sealed.len();
        print_row(case.name, "new: TrackSealer::seal", ns, wire, allocs);

        // New relay path, viewer: open the pre-sealed batch.
        let keys = sealer.keys();
        let mut opener = TrackOpener::new();
        for key in &keys {
            opener.add_key(key);
        }
        let mut batch: Vec<Vec<u8>> = vec![Vec::new(); case.batch];
        let seal_one = |sealer: &mut TrackSealer, buffer: &mut Vec<u8>| {
            sealer
                .seal(case.channel, case.keyframe, 1_234_567_890, &frame, buffer)
                .expect("the benchmark frame is within the profile");
        };
        let (ns, allocs) = measure_batch(
            case.batch,
            &mut sealer,
            &mut opener,
            &mut batch,
            |sealer, batch| {
                for buffer in batch {
                    seal_one(sealer, buffer);
                }
            },
            |opener, batch| {
                for buffer in batch {
                    let opened = opener
                        .open(case.channel, buffer)
                        .expect("the benchmark message opens");
                    black_box(opened.frame.len());
                }
            },
        );
        print_row(case.name, "new: TrackOpener::open", ns, wire, allocs);

        // Today's relay path, publisher: the 14-byte clear header is the AAD.
        let mut legacy = FrameSealer::new().expect("a sealer with a fresh key");
        let mut unit = Vec::new();
        let (ns, allocs) = measure(case.seal_iters, || {
            legacy
                .seal(content, black_box(&frame), &mut unit)
                .expect("the benchmark frame is within the profile");
            black_box(unit.len());
        });
        let legacy_wire = unit.len();
        print_row(case.name, "old: FrameSealer::seal", ns, legacy_wire, allocs);

        // Today's relay path, viewer: open the pre-sealed batch.
        let legacy_keys = legacy.keys();
        let mut legacy_opener = FrameOpener::new();
        for key in &legacy_keys {
            legacy_opener.add_key(key);
        }
        let mut legacy_batch: Vec<Vec<u8>> = vec![Vec::new(); case.batch];
        let legacy_seal_one = |sealer: &mut FrameSealer, buffer: &mut Vec<u8>| {
            sealer
                .seal(content, &frame, buffer)
                .expect("the benchmark frame is within the profile");
        };
        let (ns, allocs) = measure_batch(
            case.batch,
            &mut legacy,
            &mut legacy_opener,
            &mut legacy_batch,
            |sealer, batch| {
                for buffer in batch {
                    legacy_seal_one(sealer, buffer);
                }
            },
            |opener, batch| {
                for buffer in batch {
                    let (_, opened) = opener.open(buffer).expect("the benchmark unit opens");
                    black_box(opened.len());
                }
            },
        );
        print_row(case.name, "old: FrameOpener::open", ns, legacy_wire, allocs);

        // Today's direct stream: the 21-byte v2 header, encode and decode.
        let header = FrameHeader {
            kind: if case.channel == 5 {
                FrameKind::Audio
            } else {
                FrameKind::Video(Codec::Vp9)
            },
            keyframe: case.keyframe,
            // Audio sits on layer 0 like a keyframe; only delta video frames use layer 2.
            temporal_layer: if case.channel == 5 || case.keyframe {
                0
            } else {
                2
            },
            timestamp_us: 1_234_567_890,
            width: if case.channel == 5 { 0 } else { 1280 },
            height: if case.channel == 5 { 0 } else { 720 },
            payload_len: case.len as u32,
        };
        let (ns, allocs) = measure(200_000, || {
            let bytes = black_box(header).encode(V2).expect("valid header");
            black_box(bytes.as_slice());
            black_box(FrameHeader::decode(bytes.as_slice()).expect("valid header"));
        });
        print_row(case.name, "base: direct header (21 B)", ns, 21, allocs);
    }
}
