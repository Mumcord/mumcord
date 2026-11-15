//! Benchmark of the media profile's framing and the relay's route cost (spec 07, AC-7.82).
//!
//! Run the release profile pinned to one core, on an otherwise idle machine:
//!
//! ```text
//! taskset -c 2 cargo bench -p mumcord-p2p --bench media
//! ```
//!
//! The numbers are medians of several timed rounds, in nanoseconds per packet, with the wire
//! bytes each operation produces or consumes and the allocations per packet in the timed loop.
//! Every operation is labelled with what it includes:
//!
//! - **mux write** — [`MediaWriter::write_message`] into a pre-reserved `Vec`: the profile check,
//!   the chunk headers and the payload copy. The one-off document opener and terminator are not
//!   included.
//! - **relay peek/skip** — the relay's drop path: `MuxReader::next_item` returns a `Peek`, the
//!   channel and length are read, and the message is skipped without gathering or copying its
//!   payload. One `next_item` call per packet in the loop.
//! - **relay peek/forward** — the relay's forward path: the peeked message is forwarded and the
//!   next call returns `Item::Wire`; with the whole message in the input slice its chunks are
//!   borrowed, not copied. One peek and one settle call per packet.
//! - **full read** — [`MediaReader::read`] until the stream ends: peek, profile check and
//!   delivery. A message that fits one chunk is borrowed from the input; a multi-chunk message
//!   (above 64 KiB) is gathered into the reader's reusable buffer, one copy.
//! - **relay header (14 B)** — today's clear broadcast header: `Header::encode` and
//!   `Header::decode`, the 0.8.0 relay baseline. No payload, no encryption.
//!
//! The document opener and the first-message registration are paid once per stream, so the
//! reading rows open the stream outside the timed region; a link carries thousands of packets.
//! `warm` means the rest of the prepared document lies in one input slice, as a QUIC stream per
//! message usually looks. `1200 B pieces` feeds it in 1200-byte slices, the inner MTU of the TCP
//! mux path, where a message can straddle many reads.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::time::Instant;

use bovnar::codec::{Item, MuxReader};
use mumcord_p2p::broadcast::{Content, Header, VideoCodec};
use mumcord_p2p::media::{MAX_VIDEO_MESSAGE, MediaReader, MediaWriter};

/// Number of timed rounds per operation; the reported figure is their median.
const ROUNDS: usize = 7;
/// Size of one network read in the split-input runs: the inner MTU of the TCP mux path.
const PIECE: usize = 1200;

/// Test payload sizes of W5: an Opus packet, a video delta frame and a keyframe.
struct Case {
    name: &'static str,
    channel: u16,
    len: usize,
    /// Timed writer calls per round.
    write_iters: usize,
    /// Timed read passes per round.
    read_passes: usize,
    /// Messages in the prepared document for the reading paths.
    messages: usize,
}

const CASES: [Case; 3] = [
    Case {
        name: "150 B audio",
        channel: 5,
        len: 150,
        write_iters: 20_000,
        read_passes: 500,
        messages: 256,
    },
    Case {
        name: "30 KB video delta",
        channel: 1,
        len: 30_000,
        write_iters: 4_000,
        read_passes: 200,
        messages: 64,
    },
    Case {
        name: "400 KB keyframe",
        channel: 1,
        len: 400_000,
        write_iters: 500,
        read_passes: 20,
        messages: 16,
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
    // Warm every buffer and code path before the first timed round.
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

/// Times only the steady state of one stream: `open` runs untimed and leaves `reader` at the
/// first message, `feed` then consumes that stream's messages and is timed. Returns the median
/// nanoseconds and allocations per message. The opener and the first per-channel registration
/// are once-per-stream work, not per-packet work.
fn measure_stream<R>(
    reader: &mut R,
    messages: usize,
    passes: usize,
    mut open: impl FnMut(&mut R),
    mut feed: impl FnMut(&mut R),
) -> (f64, usize) {
    let mut round = |reader: &mut R| {
        let mut nanos = 0u128;
        let mut allocs = 0;
        for _ in 0..passes {
            open(reader);
            let before = allocations();
            let start = Instant::now();
            feed(reader);
            nanos += start.elapsed().as_nanos();
            allocs += allocations() - before;
        }
        let packets = (passes * messages) as f64;
        (nanos as f64 / packets, allocs / (passes * messages))
    };
    // Warm up: the first measured pass grows the reader's reusable buffers.
    let _ = round(reader);
    let mut samples = Vec::with_capacity(ROUNDS);
    let mut allocs = 0;
    for round_no in 0..ROUNDS {
        let (ns, spent) = round(reader);
        if round_no + 1 == ROUNDS {
            allocs = spent;
        }
        samples.push(ns);
    }
    (median(samples), allocs)
}

/// A deterministic payload whose bytes differ per `seed`.
fn payload(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|at| (at as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

/// The full document of `count` messages, terminator included.
fn document(writer: &MediaWriter, channel: u16, payload: &[u8], count: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(writer.opening().len() + count * (payload.len() + 8));
    writer.begin(&mut out);
    for _ in 0..count {
        writer
            .write_message(&mut out, channel, payload)
            .expect("the benchmark payload is within the profile");
    }
    writer.finish(&mut out);
    out
}

fn mux_reader() -> MuxReader {
    MuxReader::with_options(MAX_VIDEO_MESSAGE as u64, Some("media"))
        .expect("the media profile's key and limits are valid")
}

/// The bytes that open a stream, read before the timed region.
fn opening(writer: &MediaWriter) -> &[u8] {
    writer.opening()
}

/// Consumes only a document opener, leaving `reader` at the first message.
fn skip_opening(reader: &mut MuxReader, opening: &[u8]) {
    let mut input = opening;
    let item = reader.next_item(&mut input, 0).expect("valid opener");
    assert!(item.is_none(), "the opener holds no message");
    assert!(input.is_empty());
}

/// Reopens a media stream with the document opener, leaving `reader` at the first message.
fn open_stream(reader: &mut MediaReader, opening: &[u8]) {
    let mut input = opening;
    let message = reader.read(&mut input).expect("valid opener");
    assert!(message.is_none(), "the opener holds no message");
    assert!(input.is_empty());
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
    let writer = MediaWriter::new();
    for case in &CASES {
        let frame = payload(case.len, 17);
        let wire = {
            let mut out = Vec::new();
            writer.begin(&mut out);
            let start = out.len();
            writer
                .write_message(&mut out, case.channel, &frame)
                .expect("the benchmark payload is within the profile");
            out.len() - start
        };

        // Write: one message per timed call into a pre-reserved output buffer.
        let mut out = Vec::with_capacity(wire);
        let (ns, allocs) = measure(case.write_iters, || {
            out.clear();
            writer
                .write_message(&mut out, case.channel, &frame)
                .expect("the benchmark payload is within the profile");
            black_box(out.len());
        });
        print_row(case.name, "new: mux write", ns, wire, allocs);

        let doc = document(&writer, case.channel, &frame, case.messages);
        let opener = opening(&writer);
        let rest = &doc[opener.len()..];

        // Relay, whole document in one input slice: the common QUIC path.
        let mut reader = mux_reader();
        let (ns, allocs) = measure_stream(
            &mut reader,
            case.messages,
            case.read_passes,
            |reader| {
                reader.reset();
                skip_opening(reader, opener);
            },
            |reader| {
                let mut input = rest;
                let mut seen = 0;
                while let Some(item) = reader.next_item(&mut input, 0).expect("valid stream") {
                    match item {
                        Item::Peek(peek) => {
                            black_box((peek.channel, peek.len));
                            peek.skip();
                            seen += 1;
                        }
                        Item::Message(_) => unreachable!("every message is skipped"),
                        Item::Wire(_) => unreachable!("every message is skipped"),
                    }
                }
                assert_eq!(seen, case.messages);
            },
        );
        print_row(case.name, "new: relay peek/skip (warm)", ns, wire, allocs);

        let mut reader = mux_reader();
        let (ns, allocs) = measure_stream(
            &mut reader,
            case.messages,
            case.read_passes,
            |reader| {
                reader.reset();
                skip_opening(reader, opener);
            },
            |reader| {
                let mut input = rest;
                let mut seen = 0;
                while let Some(item) = reader.next_item(&mut input, 0).expect("valid stream") {
                    match item {
                        Item::Peek(peek) => {
                            black_box((peek.channel, peek.len));
                            peek.forward();
                        }
                        Item::Wire(message) => {
                            black_box(message.bytes.len());
                            seen += 1;
                        }
                        Item::Message(_) => unreachable!("every message is forwarded"),
                    }
                }
                assert_eq!(seen, case.messages);
            },
        );
        print_row(
            case.name,
            "new: relay peek/forward (warm)",
            ns,
            wire,
            allocs,
        );

        // The same two relay paths fed in 1200-byte reads, as the TCP mux path sees them.
        let mut reader = mux_reader();
        let (ns, allocs) = measure_stream(
            &mut reader,
            case.messages,
            case.read_passes,
            |reader| {
                reader.reset();
                skip_opening(reader, opener);
            },
            |reader| {
                let mut seen = 0;
                for piece in rest.chunks(PIECE) {
                    let mut input = piece;
                    loop {
                        match reader.next_item(&mut input, 0).expect("valid stream") {
                            Some(Item::Peek(peek)) => {
                                black_box((peek.channel, peek.len));
                                peek.skip();
                                seen += 1;
                            }
                            Some(Item::Message(_)) | Some(Item::Wire(_)) => {
                                unreachable!("every message is skipped")
                            }
                            None => break,
                        }
                    }
                }
                assert_eq!(seen, case.messages);
            },
        );
        print_row(case.name, "new: relay peek/skip (1200 B)", ns, wire, allocs);

        let mut reader = mux_reader();
        let (ns, allocs) = measure_stream(
            &mut reader,
            case.messages,
            case.read_passes,
            |reader| {
                reader.reset();
                skip_opening(reader, opener);
            },
            |reader| {
                let mut seen = 0;
                for piece in rest.chunks(PIECE) {
                    let mut input = piece;
                    loop {
                        match reader.next_item(&mut input, 0).expect("valid stream") {
                            Some(Item::Peek(peek)) => {
                                black_box((peek.channel, peek.len));
                                peek.forward();
                            }
                            Some(Item::Wire(message)) => {
                                black_box(message.bytes.len());
                                seen += 1;
                            }
                            Some(Item::Message(_)) => unreachable!("every message is forwarded"),
                            None => break,
                        }
                    }
                }
                assert_eq!(seen, case.messages);
            },
        );
        print_row(
            case.name,
            "new: relay peek/forward (1200 B)",
            ns,
            wire,
            allocs,
        );

        // Full read: the stream end and its terminator are part of the timed pass.
        let mut reader = MediaReader::new();
        let (ns, allocs) = measure_stream(
            &mut reader,
            case.messages,
            case.read_passes,
            |reader| {
                reader.reset();
                open_stream(reader, opener);
            },
            |reader| {
                let mut input = rest;
                let mut seen = 0;
                while let Some(message) = reader.read(&mut input).expect("valid stream") {
                    black_box(message.payload.len());
                    seen += 1;
                }
                assert_eq!(seen, case.messages);
            },
        );
        print_row(case.name, "new: full read (warm)", ns, wire, allocs);

        let mut reader = MediaReader::new();
        let (ns, allocs) = measure_stream(
            &mut reader,
            case.messages,
            case.read_passes,
            |reader| {
                reader.reset();
                open_stream(reader, opener);
            },
            |reader| {
                let mut seen = 0;
                for piece in rest.chunks(PIECE) {
                    let mut input = piece;
                    while let Some(message) = reader.read(&mut input).expect("valid stream") {
                        black_box(message.payload.len());
                        seen += 1;
                    }
                }
                assert_eq!(seen, case.messages);
            },
        );
        print_row(case.name, "new: full read (1200 B)", ns, wire, allocs);

        // Today's 14-byte clear relay header, the 0.8.0 baseline: encode plus decode.
        let header = match case.channel {
            5 => Header {
                content: Content::Audio,
                key_id: 1,
                sequence: 7,
                payload_len: case.len as u32,
            },
            _ => Header {
                content: Content::Video {
                    codec: VideoCodec::Vp9,
                    keyframe: case.len > 100_000,
                    temporal_layer: 0,
                },
                key_id: 1,
                sequence: 7,
                payload_len: case.len as u32,
            },
        };
        let (ns, allocs) = measure(200_000, || {
            let bytes = black_box(header).encode().expect("valid header");
            black_box(&bytes);
            black_box(Header::decode(black_box(&bytes)).expect("valid header"));
        });
        print_row(case.name, "base: relay header (14 B)", ns, 14, allocs);
    }
}
