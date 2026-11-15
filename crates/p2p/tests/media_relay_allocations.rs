//! AC-7.82's deterministic half: once a media stream is warm, the relay's peek/skip and
//! peek/forward paths allocate nothing per message, whether the whole stream lies in one input
//! slice or arrives in 1200-byte mux pieces. The benchmark `crates/p2p/benches/media.rs` reports
//! the nanoseconds; this test guards the allocation figure in CI without any timing.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use bovnar::codec::{Item, MuxReader};
use mumcord_p2p::media::{MAX_VIDEO_MESSAGE, MediaWriter};

/// Size of one network read in the split-input runs: the inner MTU of the TCP mux path.
const PIECE: usize = 1200;
/// Messages in the prepared document. One warm-up pass and one measured pass read them.
const MESSAGES: usize = 16;

/// Counts allocations per thread, so the check is not disturbed by test threads running in
/// parallel and the counter itself never allocates.
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

/// A deterministic payload whose bytes differ per `seed`.
fn payload(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|at| (at as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

/// The full document of `MESSAGES` messages, terminator included.
fn document(writer: &MediaWriter, channel: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    writer.begin(&mut out);
    for _ in 0..MESSAGES {
        writer
            .write_message(&mut out, channel, payload)
            .expect("the test payload is within the profile");
    }
    writer.finish(&mut out);
    out
}

fn mux_reader() -> MuxReader {
    MuxReader::with_options(MAX_VIDEO_MESSAGE as u64, Some("media"))
        .expect("the media profile's key and limits are valid")
}

/// Consumes only a document opener, leaving `reader` at the first message.
fn open(reader: &mut MuxReader, opening: &[u8]) {
    let mut input = opening;
    let item = reader.next_item(&mut input, 0).expect("valid opener");
    assert!(item.is_none(), "the opener holds no message");
}

/// One relay pass over `input` in slices of `piece` bytes, skipping or forwarding every message.
/// Returns the number of messages seen. Everything this function allocates is what the test
/// counts: reading the channel and length, then dropping or forwarding the message.
fn relay(reader: &mut MuxReader, input: &[u8], piece: usize, forward: bool) -> usize {
    let mut seen = 0;
    for part in input.chunks(piece) {
        let mut input = part;
        loop {
            match reader.next_item(&mut input, 0).expect("valid stream") {
                Some(Item::Peek(peek)) => {
                    black_box((peek.channel, peek.len));
                    if forward {
                        peek.forward();
                    } else {
                        peek.skip();
                        seen += 1;
                    }
                }
                Some(Item::Wire(message)) => {
                    black_box(message.bytes.len());
                    seen += 1;
                }
                Some(Item::Message(_)) => unreachable!("every message is skipped or forwarded"),
                None => break,
            }
        }
    }
    seen
}

/// One size, both relay paths, whole-stream and 1200-byte-piece input.
fn relay_allocates_nothing_per_message(channel: u16, len: usize) {
    let writer = MediaWriter::new();
    let frame = payload(len, 7);
    let doc = document(&writer, channel, &frame);
    let opening = writer.opening();
    let rest = &doc[opening.len()..];

    for (piece, face) in [(rest.len(), "whole stream"), (PIECE, "1200-byte pieces")] {
        for forward in [false, true] {
            let path = if forward { "peek/forward" } else { "peek/skip" };
            let mut reader = mux_reader();
            // Warm up: grows the reader's channel list and gather buffer once per stream.
            open(&mut reader, opening);
            assert_eq!(relay(&mut reader, rest, piece, forward), MESSAGES);

            // The measured pass opens a fresh stream on the same reader, then only the relay
            // loop is counted. Reopening a stream is once-per-stream work, not per packet.
            reader.reset();
            open(&mut reader, opening);
            let before = allocations();
            let seen = relay(&mut reader, rest, piece, forward);
            let spent = allocations() - before;
            assert_eq!(seen, MESSAGES);
            assert_eq!(
                spent, 0,
                "{len} B {path} ({face}) allocated {spent} times per {MESSAGES} messages"
            );
        }
    }
}

#[test]
fn a_150_byte_audio_message_is_relayed_without_allocating() {
    relay_allocates_nothing_per_message(5, 150);
}

#[test]
fn a_30_kb_video_delta_is_relayed_without_allocating() {
    relay_allocates_nothing_per_message(1, 30_000);
}

#[test]
fn a_400_kb_keyframe_is_relayed_without_allocating() {
    relay_allocates_nothing_per_message(1, 400_000);
}
