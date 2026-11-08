//! Per-packet cost of the Bovnar mux prototype against today's fixed broadcast header (#680).
//!
//! Run on one core: `taskset -c 2 cargo bench -p bovnar-mux`. Each case runs 11 times for about
//! 50 ms after a warm-up pass; the table shows the median. Allocations are counted by a global
//! allocator over all timed runs and divided by the packets processed.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bovnar_mux::{Event, MAX_CHUNK_HEADER, MuxReader, MuxWriter, RelayEvent, RelayReader};
use mumcord_p2p::broadcast::{Content, HEADER_LEN, Header, VideoCodec};

struct Counting;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

// SAFETY: forwards every call unchanged to the system allocator; the counter has no effect on
// the memory handed out.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Direct P2P header of today (`screen_share::wire`, version 2); only its size is compared.
const DIRECT_HEADER_LEN: usize = 21;
const MAX_MESSAGE: u64 = 4 << 20;
const RUNS: usize = 11;
const RUN_TIME: Duration = Duration::from_millis(50);
const CHANNEL: u64 = 2;

struct Case {
    name: &'static str,
    payload: usize,
    keyframe: bool,
    /// Packets per pass over a prepared stream (keeps the stream a few MB at most).
    per_pass: usize,
}

// Each size runs over a stream that fits the per-core L2 cache (as bytes a relay has just
// received would) and, for the large ones, over a 2–3 MB stream that only fits L3. Large
// messages touch one chunk header per 64 KiB, so the L3 rows are bound by cache latency.
const CASES: [Case; 5] = [
    Case {
        name: "Opus 150 B",
        payload: 150,
        keyframe: false,
        per_pass: 1000,
    },
    Case {
        name: "Video delta 30 KB, in L2",
        payload: 30 * 1024,
        keyframe: false,
        per_pass: 8,
    },
    Case {
        name: "Video delta 30 KB, in L3",
        payload: 30 * 1024,
        keyframe: false,
        per_pass: 64,
    },
    Case {
        name: "Keyframe 400 KB, in L2",
        payload: 400 * 1024,
        keyframe: true,
        per_pass: 1,
    },
    Case {
        name: "Keyframe 400 KB, in L3",
        payload: 400 * 1024,
        keyframe: true,
        per_pass: 8,
    },
];

struct Measurement {
    ns: f64,
    allocations: u64,
    packets: u64,
}

/// Runs `pass` (which handles `per_pass` packets) repeatedly and returns the median ns/packet.
fn measure(per_pass: usize, mut pass: impl FnMut()) -> Measurement {
    let mut samples = Vec::with_capacity(RUNS);
    pass();
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut packets = 0u64;
    for _ in 0..RUNS {
        // Reading the clock costs about as much as a small packet, so it is read once per
        // batch of at least 1000 packets.
        let batch = 1000usize.div_ceil(per_pass) as u64;
        let start = Instant::now();
        let mut passes = 0u64;
        while start.elapsed() < RUN_TIME {
            for _ in 0..batch {
                pass();
            }
            passes += batch;
        }
        let elapsed = start.elapsed();
        let run_packets = passes * per_pass as u64;
        packets += run_packets;
        samples.push(elapsed.as_nanos() as f64 / run_packets as f64);
    }
    // `samples` is allocated before counting starts and never grows.
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) - before;
    samples.sort_by(f64::total_cmp);
    Measurement {
        ns: samples[RUNS / 2],
        allocations,
        packets,
    }
}

fn header(case: &Case, sequence: u32) -> Header {
    let content = if case.payload <= 4096 {
        Content::Audio
    } else {
        Content::Video {
            codec: VideoCodec::Av1,
            keyframe: case.keyframe,
            temporal_layer: 0,
        }
    };
    Header {
        content,
        key_id: 1,
        sequence,
        payload_len: case.payload as u32,
    }
}

/// The Bovnar message: a flags byte, then the payload.
fn message(case: &Case) -> Vec<u8> {
    let mut message = vec![0x5A; case.payload + 1];
    message[0] = u8::from(case.keyframe);
    message
}

fn header_stream(case: &Case) -> Vec<u8> {
    let mut stream = Vec::new();
    for i in 0..case.per_pass {
        let encoded = header(case, i as u32).encode().expect("valid header");
        stream.extend_from_slice(&encoded);
        stream.resize(stream.len() + case.payload, 0x5A);
    }
    stream
}

fn mux_stream(case: &Case) -> Vec<u8> {
    let writer = MuxWriter::new(CHANNEL);
    let message = message(case);
    let mut stream = Vec::new();
    for _ in 0..case.per_pass {
        for chunk in writer.chunks(&message) {
            stream.extend_from_slice(chunk.header());
            stream.extend_from_slice(chunk.body());
        }
    }
    stream
}

fn header_write(case: &Case) -> Measurement {
    let mut out = [0u8; HEADER_LEN];
    let mut sequence = 0u32;
    measure(case.per_pass, || {
        for _ in 0..case.per_pass {
            sequence = sequence.wrapping_add(1);
            let encoded = black_box(header(case, sequence))
                .encode()
                .expect("valid header");
            out.copy_from_slice(&encoded);
            black_box(&out);
        }
    })
}

fn mux_write(case: &Case) -> Measurement {
    let writer = MuxWriter::new(CHANNEL);
    let message = message(case);
    // Chunk headers go into a caller buffer; the payload slices would go out by vectored write.
    let mut headers = [[0u8; MAX_CHUNK_HEADER]; 8];
    measure(case.per_pass, || {
        for _ in 0..case.per_pass {
            let mut chunks = black_box(&writer).chunks(black_box(&message));
            for header in &mut headers {
                let Some((len, body)) = chunks.write_next(header) else {
                    break;
                };
                black_box((&header[..len], body));
            }
            black_box(&headers);
        }
    })
}

/// Today's reader on a contiguous buffer: decode the header, borrow the payload. The relay does
/// the same and only looks at the keyframe flag, so both reads cost the same.
fn header_read(case: &Case) -> Measurement {
    let stream = header_stream(case);
    measure(case.per_pass, || {
        let mut rest = black_box(&stream[..]);
        while !rest.is_empty() {
            let header = Header::decode(rest).expect("valid header");
            let end = HEADER_LEN + header.payload_len as usize;
            let keyframe = matches!(header.content, Content::Video { keyframe: true, .. });
            black_box((keyframe, &rest[HEADER_LEN..end]));
            rest = &rest[end..];
        }
    })
}

fn mux_read(case: &Case) -> Measurement {
    let stream = mux_stream(case);
    let mut reader = MuxReader::new(MAX_MESSAGE);
    measure(case.per_pass, || {
        let mut rest = black_box(&stream[..]);
        while !rest.is_empty() {
            let (n, event) = reader.read(rest).expect("valid stream");
            if let Some(Event::Message { channel, data }) = event {
                black_box((channel, data));
            }
            rest = &rest[n..];
        }
    })
}

fn mux_relay(case: &Case) -> Measurement {
    let stream = mux_stream(case);
    let mut reader = RelayReader::new(MAX_MESSAGE);
    measure(case.per_pass, || {
        let mut rest = black_box(&stream[..]);
        while !rest.is_empty() {
            let (n, event) = reader.read(rest).expect("valid stream");
            if let Some(RelayEvent::Message { channel, flags, .. }) = event {
                black_box((channel, flags));
            }
            rest = &rest[n..];
        }
    })
}

fn main() {
    // `cargo bench` passes `--bench`; `cargo test --benches` passes nothing and only wants a
    // quick smoke run.
    if !std::env::args().any(|arg| arg == "--bench") {
        let stream = mux_stream(&CASES[0]);
        assert!(!stream.is_empty());
        return;
    }
    println!(
        "| Packet | Framing B (today relay/direct → mux) | Write ns (today → mux) | Full read ns (today → mux) | Relay read ns (today → mux) | Allocations mux (write/read/relay) |"
    );
    println!("|---|---|---|---|---|---|");
    for case in &CASES {
        let framing = MuxWriter::new(CHANNEL).framing_len(case.payload + 1) + 1;
        let today_write = header_write(case);
        let mux_write = mux_write(case);
        let today_read = header_read(case);
        let mux_read = mux_read(case);
        let mux_relay = mux_relay(case);
        println!(
            "| {} | {HEADER_LEN} / {DIRECT_HEADER_LEN} → {framing} | {:.1} → {:.1} | {:.1} → {:.1} | {:.1} → {:.1} | {} / {} / {} in {} packets |",
            case.name,
            today_write.ns,
            mux_write.ns,
            today_read.ns,
            mux_read.ns,
            today_read.ns,
            mux_relay.ns,
            mux_write.allocations,
            mux_read.allocations,
            mux_relay.allocations,
            mux_write.packets + mux_read.packets + mux_relay.packets,
        );
        assert_eq!(today_write.allocations + today_read.allocations, 0);
    }
}
