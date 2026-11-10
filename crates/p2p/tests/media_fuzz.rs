//! Bounded, deterministic fuzz harness for every decoder that reads the media profile from the
//! network (FR-7.94, AC-7.83).
//!
//! The repository has no fuzz infrastructure and CI runs stable Rust, so this is a self-contained
//! mutator instead of `cargo-fuzz`: it takes the committed corpus in `tests/corpus/`, mutates it
//! with a fixed seeded PRNG (splitmix64), and feeds every case to a decoder. All panics and
//! hangs fail the test:
//!
//! - a panic fails the test and the failure message names the base seed, the case and the input,
//! - a case that runs longer than [`CASE_TIMEOUT`] aborts the harness with the same details,
//! - a media reader that stops making progress fails instead of looping forever.
//!
//! The run is reproducible: the same corpus, seed and case count produce the same cases. The
//! default budget is [`DEFAULT_CASES`] per decoder; the `media-fuzz` CI job runs the ignored
//! tests with `--ignored`. `MUMCORD_FUZZ_SEED` and `MUMCORD_FUZZ_CASES` override the seed and
//! the count (the failure message prints the values needed to repeat one case).
//!
//! The two allocation tests at the end are part of the normal test run: they check the promise
//! of FR-7.94 that oversized input is refused before anything proportional to it is allocated.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::env;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use mumcord_p2p::broadcast::{HEADER_LEN, Header};
use mumcord_p2p::media::{
    CodecError, ControlKind, Description, MAX_DESCRIPTION, MAX_VIDEO_MESSAGE, MediaError,
    MediaReader,
};

/// Cases per decoder when the environment does not say otherwise.
const DEFAULT_CASES: usize = 4_096;
/// The seeded corpus lives next to this file.
const CORPUS: &str = "tests/corpus";
/// Longest a single case may run before the harness calls it a hang. A decoder that spends this
/// long on at most 256 KiB of input would stall a relay connection.
const CASE_TIMEOUT: Duration = Duration::from_secs(30);
/// Largest mutated input; above the profile's 4 MiB video cap the decoder must reject on the
/// declared length alone, so larger cases add nothing but time.
const MAX_INPUT: usize = 256 * 1024;

// ---------------------------------------------------------------------------------------------
// Allocation accounting, for the two limit tests.
// ---------------------------------------------------------------------------------------------

thread_local! {
    static ALLOCATED_BYTES: Cell<usize> = const { Cell::new(0) };
    static ALLOCATION_COUNT: Cell<usize> = const { Cell::new(0) };
}

/// Counts allocations and bytes per thread. Every case and the watchdog run on other threads
/// or outside the measured call, so the numbers belong to the measured call alone.
struct CountingAlloc;

// SAFETY: every method delegates to the system allocator and only updates const-initialised
// thread-local `Cell`s, which do not allocate.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATED_BYTES.with(|bytes| bytes.set(bytes.get() + layout.size()));
        ALLOCATION_COUNT.with(|count| count.set(count.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATED_BYTES.with(|bytes| bytes.set(bytes.get() + new_size));
        ALLOCATION_COUNT.with(|count| count.set(count.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

/// What one measured call allocated on this thread.
fn measure<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let bytes = ALLOCATED_BYTES.with(Cell::get);
    let value = f();
    (value, ALLOCATED_BYTES.with(Cell::get) - bytes)
}

// ---------------------------------------------------------------------------------------------
// Watchdog: a hanging decoder fails the process with the input that hung it.
// ---------------------------------------------------------------------------------------------

/// The case currently being decoded.
struct Running {
    started: Instant,
    index: usize,
    seed: u64,
    input: Arc<Vec<u8>>,
}

/// Aborts the harness when one case runs longer than [`CASE_TIMEOUT`].
struct Watchdog {
    stop: Arc<AtomicBool>,
    current: Arc<Mutex<Option<Running>>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Watchdog {
    fn start(label: &'static str) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let current: Arc<Mutex<Option<Running>>> = Arc::new(Mutex::new(None));
        let handle = {
            let stop = Arc::clone(&stop);
            let current = Arc::clone(&current);
            thread::Builder::new()
                .name("fuzz-watchdog".to_owned())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        thread::sleep(Duration::from_millis(25));
                        let hung = current.lock().ok().and_then(|current| {
                            current.as_ref().and_then(|running| {
                                (running.started.elapsed() > CASE_TIMEOUT).then(|| {
                                    (running.index, running.seed, Arc::clone(&running.input))
                                })
                            })
                        });
                        if let Some((index, seed, input)) = hung {
                            eprintln!(
                                "fuzz hang in {label}: one case ran longer than \
                                 {CASE_TIMEOUT:?}, base seed={seed:#018x} case={index} input={}",
                                hex(&input)
                            );
                            // Exit code 101 is how a failed test ends; the harness itself is
                            // spinning in the hung decoder and cannot report.
                            std::process::exit(101);
                        }
                    }
                })
                .expect("spawn the fuzz watchdog")
        };
        Self {
            stop,
            current,
            handle: Some(handle),
        }
    }

    /// Marks the case now running.
    fn enter(&self, index: usize, seed: u64, input: &Arc<Vec<u8>>) {
        if let Ok(mut current) = self.current.lock() {
            *current = Some(Running {
                started: Instant::now(),
                index,
                seed,
                input: Arc::clone(input),
            });
        }
    }

    /// Clears the running case.
    fn leave(&self) {
        if let Ok(mut current) = self.current.lock() {
            *current = None;
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Deterministic mutation.
// ---------------------------------------------------------------------------------------------

/// splitmix64: small, deterministic and good enough to spread mutations over a corpus.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        assert!(bound > 0, "an empty range has no value");
        (self.next_u64() % bound as u64) as usize
    }

    fn byte(&mut self) -> u8 {
        self.next_u64() as u8
    }
}

/// The committed corpus of one decoder, sorted by file name so every run sees the same order.
fn corpus(name: &str) -> Vec<Arc<Vec<u8>>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(CORPUS)
        .join(name);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("read the fuzz corpus {}: {error}", dir.display()))
        .map(|entry| entry.expect("read a corpus directory entry").path())
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "the fuzz corpus {} has no seed files",
        dir.display()
    );
    files
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(&path).expect("read a fuzz corpus file");
            Arc::new(bytes)
        })
        .collect()
}

/// The base seed: fixed unless the environment asks for another run.
fn base_seed() -> u64 {
    match env::var("MUMCORD_FUZZ_SEED") {
        Ok(text) => match text.strip_prefix("0x") {
            Some(hex) => u64::from_str_radix(hex, 16).expect("MUMCORD_FUZZ_SEED is a hex u64"),
            None => text.parse().expect("MUMCORD_FUZZ_SEED is a decimal u64"),
        },
        Err(_) => 0x4D75_6D63_6F72_6400,
    }
}

/// The case count: the fixed CI budget unless the environment asks for another run.
fn case_count() -> usize {
    match env::var("MUMCORD_FUZZ_CASES") {
        Ok(text) => text.parse().expect("MUMCORD_FUZZ_CASES is a number"),
        Err(_) => DEFAULT_CASES,
    }
}

/// One deterministic case for `index`: mostly mutations of the corpus, plus raw, spliced and
/// boundary-sized inputs.
fn generate(rng: &mut Rng, seeds: &[Arc<Vec<u8>>]) -> Vec<u8> {
    match rng.below(10) {
        // Most cases must stay recognisable for a decoder to get past its first checks.
        0..=4 => {
            let seed = rng.below(seeds.len());
            mutate(rng, &seeds[seed])
        }
        // Small random inputs reach the early error paths.
        5 => {
            let len = rng.below(64);
            random(rng, len)
        }
        // Medium random inputs reach header and length parsers.
        6 => {
            let len = rng.below(4_096);
            random(rng, len)
        }
        // Exactly around the 64 KiB chunk boundary, where lengths and chunking change shape.
        7 => {
            let len = 65_500 + rng.below(80);
            random(rng, len)
        }
        // Splices combine a valid prefix with a valid suffix.
        8 => {
            let first = rng.below(seeds.len());
            let second = rng.below(seeds.len());
            let first = &seeds[first];
            let second = &seeds[second];
            let at_first = rng.below(first.len() + 1);
            let at_second = rng.below(second.len() + 1);
            let mut bytes = first[..at_first].to_vec();
            bytes.extend_from_slice(&second[at_second..]);
            truncate(&mut bytes);
            bytes
        }
        // An input over the description cap, to exercise the size check itself.
        _ => {
            let len = MAX_DESCRIPTION + 1 + rng.below(4_096);
            random(rng, len)
        }
    }
}

/// Random bytes of at most [`MAX_INPUT`].
fn random(rng: &mut Rng, len: usize) -> Vec<u8> {
    (0..len.min(MAX_INPUT)).map(|_| rng.byte()).collect()
}

/// One to eight mutations of `seed`.
fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut bytes = seed.to_vec();
    for _ in 0..1 + rng.below(8) {
        match rng.below(9) {
            // Flip one bit, which keeps a document mostly valid.
            0 if !bytes.is_empty() => {
                let at = rng.below(bytes.len());
                bytes[at] ^= 1 << rng.below(8);
            }
            // Overwrite one byte.
            1 if !bytes.is_empty() => {
                let at = rng.below(bytes.len());
                bytes[at] = rng.byte();
            }
            // Cut the input short, as a dropped connection does.
            2 if !bytes.is_empty() => {
                let at = rng.below(bytes.len() + 1);
                bytes.truncate(at);
            }
            // Append random bytes, as a peer that starts the next document does.
            3 => {
                for _ in 0..1 + rng.below(32) {
                    bytes.push(rng.byte());
                }
            }
            // Insert a run of bytes at one position.
            4 => {
                let at = if bytes.is_empty() {
                    0
                } else {
                    rng.below(bytes.len() + 1)
                };
                let run: Vec<u8> = (0..1 + rng.below(16)).map(|_| rng.byte()).collect();
                bytes.splice(at..at, run);
            }
            // Delete a run of bytes.
            5 if !bytes.is_empty() => {
                let at = rng.below(bytes.len());
                let end = at + 1 + rng.below(bytes.len() - at);
                bytes.drain(at..end);
            }
            // Repeat a run.
            6 if !bytes.is_empty() => {
                let at = rng.below(bytes.len());
                let end = at + 1 + rng.below(bytes.len() - at);
                let run = bytes[at..end].to_vec();
                bytes.splice(end..end, run);
            }
            // A byte value that changes a length or a varint.
            7 if !bytes.is_empty() => {
                let at = rng.below(bytes.len());
                bytes[at] = [0x00, 0x7F, 0x80, 0xFF][rng.below(4)];
            }
            // Replace a run with zeros.
            _ if !bytes.is_empty() => {
                let at = rng.below(bytes.len());
                let end = at + 1 + rng.below(bytes.len() - at);
                bytes[at..end].fill(0);
            }
            _ => {}
        }
    }
    truncate(&mut bytes);
    bytes
}

/// Keeps a case within [`MAX_INPUT`].
fn truncate(bytes: &mut Vec<u8>) {
    if bytes.len() > MAX_INPUT {
        bytes.truncate(MAX_INPUT);
    }
}

/// The hex of at most `limit` bytes, so a failure names the input without flooding the log.
fn hex_prefix(input: &[u8], limit: usize) -> String {
    let shown = input.len().min(limit);
    let hex: String = input[..shown]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if shown == input.len() {
        format!("len={} {hex}", input.len())
    } else {
        format!("len={} {hex}…{} more", input.len(), input.len() - shown)
    }
}

/// The hex of a small input, or the prefix of a large one.
fn hex(input: &[u8]) -> String {
    hex_prefix(input, 4_096)
}

/// Runs `decode` over the cases of one decoder. The closure must let both `Ok` and `Err`
/// through: the harness fails only on panics, hangs and lost progress.
fn fuzz(label: &'static str, corpus_name: &str, mut decode: impl FnMut(&[u8])) {
    let seeds = corpus(corpus_name);
    let base = base_seed();
    let seed = base ^ label_seed(label);
    let cases = case_count();
    let watchdog = Watchdog::start(label);
    let mut rng = Rng::new(seed);
    let started = Instant::now();
    for index in 0..cases {
        let input = Arc::new(generate(&mut rng, &seeds));
        watchdog.enter(index, base, &input);
        let outcome = catch_unwind(AssertUnwindSafe(|| decode(&input)));
        watchdog.leave();
        if let Err(payload) = outcome {
            eprintln!(
                "fuzz failure in {label}: base seed={base:#018x} case={index} input={}",
                hex(&input)
            );
            panic!(
                "the {label} decoder panicked: {} \
                 (repeat with MUMCORD_FUZZ_SEED={base:#x} MUMCORD_FUZZ_CASES={})",
                panic_text(payload),
                index + 1
            );
        }
    }
    eprintln!("fuzz {label}: {cases} cases in {:.2?}", started.elapsed());
}

/// A stable per-decoder seed offset, so each decoder sees its own case sequence from one base
/// seed.
fn label_seed(label: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in label.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

/// The message of a panic payload.
fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

// ---------------------------------------------------------------------------------------------
// The decoders.
// ---------------------------------------------------------------------------------------------

/// Drives [`MediaReader`] over `input` as a link would: pieces of any size, every message until
/// the input is used up, then the end-of-link check. A reader that returns `None` with input
/// left, or needs more steps than the input has bytes, is broken and fails the case.
fn read_media(input: &[u8]) {
    let mut reader = MediaReader::new();
    let piece = if input.len() > 16_384 {
        input.len()
    } else {
        1 + usize::from(input.first().copied().unwrap_or(0)) % 61
    };
    let mut steps = 0usize;
    for part in input.chunks(piece) {
        let mut rest = part;
        loop {
            steps += 1;
            assert!(
                steps <= input.len().saturating_mul(2) + 16,
                "the media reader makes no progress and would loop forever"
            );
            match reader.read(&mut rest) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    assert!(
                        rest.is_empty(),
                        "the media reader returned None without taking every byte"
                    );
                    break;
                }
                // The stream continues after a message outside the profile.
                Err(CodecError::Dropped(_)) => {
                    if rest.is_empty() {
                        break;
                    }
                }
                // A malformed or truncated stream latches; closing it must not panic either.
                Err(_) => {
                    let _ = reader.finish();
                    return;
                }
            }
        }
    }
    let _ = reader.finish();
}

#[test]
#[ignore = "fuzz harness: run by the media-fuzz CI job with --ignored"]
fn fuzz_media_reader_survives_hostile_documents() {
    fuzz("media-reader", "media", read_media);
}

#[test]
#[ignore = "fuzz harness: run by the media-fuzz CI job with --ignored"]
fn fuzz_description_parser_survives_hostile_documents() {
    fuzz("description", "description", |input| {
        let parsed = Description::parse(input);
        if input.len() > MAX_DESCRIPTION {
            assert_eq!(
                parsed,
                Err(MediaError::TooLarge),
                "oversized input is refused"
            );
        }
    });
}

#[test]
#[ignore = "fuzz harness: run by the media-fuzz CI job with --ignored"]
fn fuzz_legacy_header_survives_hostile_bytes() {
    fuzz("legacy-header", "header", |input| {
        if let Ok(header) = Header::decode(input) {
            // A header the decoder accepts must be one the encoder accepts, byte for byte:
            // the two sides agree on every field.
            let encoded = header.encode().expect("a decoded header can be encoded");
            assert_eq!(&encoded[..], &input[..HEADER_LEN]);
        }
        // Extra bytes after the 14-byte header are ignored by both sides.
    });
}

/// `ControlKind::from_byte` is the whole channel-0 kind decoder of the profile today. Its byte
/// space is small enough to check exhaustively, in the same dedicated job.
#[test]
#[ignore = "fuzz harness: run by the media-fuzz CI job with --ignored"]
fn fuzz_control_kinds_cover_every_byte() {
    for byte in 0..=u8::MAX {
        match ControlKind::from_byte(byte) {
            Some(kind) => {
                assert_eq!(kind.to_byte(), byte, "kind {kind:?} round-trips");
                assert!(byte <= 6, "only kinds 0 to 6 exist");
            }
            None => assert!(byte > 6, "kind {byte} is part of the profile"),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Limit checks that are part of the normal test run (FR-7.94).
// ---------------------------------------------------------------------------------------------

/// A document whose first message on `channel` declares `declared` bytes, followed by no payload.
fn message_declaring(channel: u8, declared: u64) -> Vec<u8> {
    let mut doc = Vec::new();
    doc.extend_from_slice(b"#!bovnar 1.2\n.media=\0");
    // One chunk: tag 0x01, the chunk body length in u16 little endian, then the channel and
    // length varints. The body is the varints alone; the payload never arrives.
    let mut body = Vec::new();
    body.push(channel);
    let mut value = declared;
    loop {
        let byte = value as u8 & 0x7F;
        value >>= 7;
        if value == 0 {
            body.push(byte);
            break;
        }
        body.push(byte | 0x80);
    }
    doc.push(0x01);
    doc.extend_from_slice(&(body.len() as u16).to_le_bytes());
    doc.extend_from_slice(&body);
    doc
}

#[test]
fn description_oversized_input_is_refused_before_parsing() {
    // A MiB of plausible document text: the parser must stop at the 8 KiB cap and never hand
    // the input to Bovnar.
    let oversized = vec![b'.'; MAX_DESCRIPTION + 1];
    let (result, bytes) = measure(|| Description::parse(&oversized));
    assert_eq!(result, Err(MediaError::TooLarge));
    assert!(
        bytes < 1_024,
        "the size check allocated {bytes} bytes for a {} byte input",
        oversized.len()
    );
}

#[test]
fn media_reader_refuses_an_oversized_declared_length_without_allocating() {
    // The mux reader's own cap: a declared length over the video cap is malformed and never
    // buffered.
    let doc = message_declaring(1, MAX_VIDEO_MESSAGE as u64 + 1);
    let mut reader = MediaReader::new();
    let mut input = &doc[..];
    let (result, bytes) = measure(|| reader.read(&mut input));
    assert_eq!(
        result,
        Err(CodecError::Malformed(Some(
            bovnar::ErrorCode::ValueOutOfRange
        )))
    );
    assert!(
        bytes < 1_024,
        "a hostile declared length allocated {bytes} bytes before it was refused"
    );
}

#[test]
fn media_reader_drops_an_over_cap_audio_message_before_buffering_it() {
    // Under the mux cap but over the audio channel's cap: the profile check runs on the peek,
    // so not one payload byte is gathered (the payload never arrives here).
    let doc = message_declaring(5, 4_161);
    let mut reader = MediaReader::new();
    let mut input = &doc[..];
    let (result, bytes) = measure(|| reader.read(&mut input));
    assert_eq!(result, Err(CodecError::Dropped(MediaError::TooLarge)));
    assert!(
        bytes < 1_024,
        "an over-cap audio message allocated {bytes} bytes before it was dropped"
    );
}
