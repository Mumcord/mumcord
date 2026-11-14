//! Bounded, deterministic fuzz harness for every screen-share decoder that reads from the
//! network (FR-7.94, AC-7.83): the SFrame header, the SFrame track opener, the direct-track
//! decoder, the legacy broadcast opener, the key-exchange chunk reassembler and the key-update
//! decoder.
//!
//! It mirrors `crates/p2p/tests/media_fuzz.rs`: a self-contained splitmix64 mutator over the
//! committed corpus in `tests/corpus/`, a fixed seed and budget, a per-case watchdog, and a
//! failure message that names the seed, the case and the input bytes. The valid messages each
//! harness seals or encodes while it sets up are added as seeds too, so the success path is
//! exercised and the corpus is not the only source of well-formed input.
//!
//! The `media-fuzz` CI job runs these ignored tests with `--ignored`. `MUMCORD_FUZZ_SEED` and
//! `MUMCORD_FUZZ_CASES` override the seed and the count for a local run.

use std::env;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use mumcord_engine::screen_share::broadcast::crypto::{
    FrameOpener, FrameSealer, SFrameHeader, ShareKey, TrackOpener, TrackSealer, decode_direct,
    encode_direct,
};
use mumcord_engine::screen_share::broadcast::keyx::{self, CertKey, Reassembler};
use mumcord_p2p::BroadcastId;
use mumcord_p2p::broadcast::{Content, VideoCodec};

/// Cases per decoder when the environment does not say otherwise.
const DEFAULT_CASES: usize = 4_096;
/// The seeded corpus lives next to this file.
const CORPUS: &str = "tests/corpus";
/// Longest a single case may run before the harness calls it a hang.
const CASE_TIMEOUT: Duration = Duration::from_secs(30);

/// The broadcast the key-exchange harness runs on.
const BROADCAST: BroadcastId = BroadcastId(0x0123_4567_89ab_cdef);

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

/// One deterministic case: mostly mutations of the corpus, plus raw and spliced inputs.
fn generate(rng: &mut Rng, seeds: &[Arc<Vec<u8>>]) -> Vec<u8> {
    match rng.below(10) {
        // Most cases must stay recognisable for a decoder to get past its first checks.
        0..=6 => {
            let seed = rng.below(seeds.len());
            mutate(rng, &seeds[seed])
        }
        // Small random inputs reach the early error paths.
        7 => {
            let len = rng.below(256);
            random(rng, len)
        }
        // Medium random inputs reach header and length parsers.
        8 => {
            let len = rng.below(4_096);
            random(rng, len)
        }
        // Splices combine a valid prefix with a valid suffix.
        _ => {
            let first = rng.below(seeds.len());
            let second = rng.below(seeds.len());
            let first = &seeds[first];
            let second = &seeds[second];
            let at_first = rng.below(first.len() + 1);
            let at_second = rng.below(second.len() + 1);
            let mut bytes = first[..at_first].to_vec();
            bytes.extend_from_slice(&second[at_second..]);
            bytes
        }
    }
}

/// Random bytes.
fn random(rng: &mut Rng, len: usize) -> Vec<u8> {
    (0..len).map(|_| rng.byte()).collect()
}

/// One to eight mutations of `seed`.
fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut bytes = seed.to_vec();
    for _ in 0..1 + rng.below(8) {
        match rng.below(9) {
            // Flip one bit, which keeps a message mostly valid.
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
            // Append random bytes, as a peer that starts the next message does.
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
    bytes
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
/// through: the harness fails only on panics and hangs.
fn fuzz(
    label: &'static str,
    corpus_name: &str,
    extra: Vec<Vec<u8>>,
    mut decode: impl FnMut(&[u8]),
) {
    let mut seeds = corpus(corpus_name);
    seeds.extend(extra.into_iter().map(Arc::new));
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

#[test]
#[ignore = "fuzz harness: run by the media-fuzz CI job with --ignored"]
fn fuzz_sframe_header_survives_hostile_bytes() {
    // A few headers of every length class as extra seeds.
    let mut extra = Vec::new();
    for (key_id, counter) in [(0, 0), (0, 5), (4, 300), (u64::MAX, u64::MAX)] {
        let header = SFrameHeader { key_id, counter };
        let mut out = [0u8; SFrameHeader::MAX_LEN];
        let len = header.encode(&mut out);
        extra.push(out[..len].to_vec());
    }
    fuzz("sframe-header", "sframe", extra, |input| {
        if let Ok((header, used)) = SFrameHeader::decode(input) {
            assert!(used <= SFrameHeader::MAX_LEN, "the header stays in its cap");
            let mut out = [0u8; SFrameHeader::MAX_LEN];
            let len = header.encode(&mut out);
            assert_eq!(
                &out[..len],
                &input[..used],
                "a decoded header re-encodes byte for byte"
            );
        }
    });
}

/// A sealer with one valid message per content kind, plus the keys the matching opener needs.
fn sealed_tracks() -> (Vec<Vec<u8>>, Vec<ShareKey>) {
    let mut sealer = TrackSealer::new().expect("track sealer");
    let mut messages = Vec::new();
    let mut out = Vec::new();
    sealer
        .seal(1, true, 1_000, b"a keyframe frame", &mut out)
        .expect("seal a video keyframe");
    messages.push(out.clone());
    let mut keys = sealer.keys();
    sealer.rekey().expect("rekey");
    sealer
        .seal(1, true, 2_000, b"another keyframe frame", &mut out)
        .expect("seal with the new key");
    messages.push(out.clone());
    keys.extend(sealer.keys());
    sealer
        .seal(5, false, 3_000, b"an opus packet", &mut out)
        .expect("seal audio");
    messages.push(out);
    (messages, keys)
}

#[test]
#[ignore = "fuzz harness: run by the media-fuzz CI job with --ignored"]
fn fuzz_sframe_track_opener_survives_hostile_messages() {
    let (messages, keys) = sealed_tracks();
    let mut opener = TrackOpener::new();
    for key in &keys {
        opener.add_key(key);
    }
    fuzz("sframe-track-opener", "track", messages, |input| {
        // The channel is part of the AAD; the corpus carries no channel, so try each video
        // layer and the audio channel. Opening mutates the buffer in place.
        for channel in [1u16, 4, 5, 6] {
            let mut message = input.to_vec();
            let _ = opener.open(channel, &mut message);
        }
    });
}

#[test]
#[ignore = "fuzz harness: run by the media-fuzz CI job with --ignored"]
fn fuzz_direct_track_decoder_survives_hostile_messages() {
    let mut extra = Vec::new();
    for (channel, keyframe) in [(1u16, true), (4, false), (5, false), (6, false)] {
        let mut message = Vec::new();
        encode_direct(channel, keyframe, 1_234, b"a direct frame", &mut message)
            .expect("direct message");
        extra.push(message);
    }
    fuzz("direct-track", "direct", extra, |input| {
        for channel in [0u16, 1, 4, 5, 6, 7, 4_096] {
            let _ = decode_direct(channel, input);
        }
    });
}

/// A legacy sealer with one valid unit per content kind, plus the key the matching opener needs.
fn sealed_units() -> (Vec<Vec<u8>>, Vec<ShareKey>) {
    let mut sealer = FrameSealer::new().expect("frame sealer");
    let mut units = Vec::new();
    let mut out = Vec::new();
    for content in [
        Content::Video {
            codec: VideoCodec::Av1,
            keyframe: true,
            temporal_layer: 0,
        },
        Content::Audio,
        Content::Data,
    ] {
        sealer
            .seal(content, b"a legacy frame", &mut out)
            .expect("seal a legacy unit");
        units.push(out.clone());
    }
    (units, sealer.keys())
}

#[test]
#[ignore = "fuzz harness: run by the media-fuzz CI job with --ignored"]
fn fuzz_legacy_frame_opener_survives_hostile_units() {
    let (units, keys) = sealed_units();
    let mut opener = FrameOpener::new();
    for key in &keys {
        opener.add_key(key);
    }
    fuzz("legacy-frame-opener", "legacy", units, |input| {
        let mut unit = input.to_vec();
        let _ = opener.open(&mut unit);
    });
}

#[test]
#[ignore = "fuzz harness: run by the media-fuzz CI job with --ignored"]
fn fuzz_keyx_chunk_reassembler_survives_hostile_chunks() {
    let message = vec![b'k'; 2_000];
    let chunks = keyx::split(&message, 7).expect("split a key exchange message");
    fuzz("keyx-chunks", "keyx-chunk", chunks, |input| {
        let mut reassembler = Reassembler::<BroadcastId>::new();
        let _ = reassembler.push(BROADCAST, input, Instant::now());
    });
}

#[test]
#[ignore = "fuzz harness: run by the media-fuzz CI job with --ignored"]
fn fuzz_keyx_update_survives_hostile_updates() {
    let viewer = identity(&rcgen::PKCS_ED25519);
    let sharer = identity(&rcgen::PKCS_ED25519);
    let (pending, request) = keyx::request(BROADCAST, &cert_key(&viewer)).expect("request");
    let keys = vec![ShareKey::generate(4).expect("share key")];
    let (mut viewer_channel, answer) =
        keyx::answer_request(BROADCAST, &cert_key(&sharer), &request, |_| true, &keys)
            .expect("answer");
    let (mut sharer_channel, got) = pending
        .accept(&answer, &mumcord_store::cert_hash(&sharer.0))
        .expect("accept");
    assert_eq!(got, keys);
    let update = viewer_channel.update(&keys).expect("update");
    fuzz("keyx-update", "keyx-update", vec![update], move |input| {
        let _ = sharer_channel.open_update(input);
    });
}

// ---------------------------------------------------------------------------------------------
// Key-exchange setup.
// ---------------------------------------------------------------------------------------------

/// A self-signed certificate and the PKCS#8 key that goes with it.
fn identity(algorithm: &'static rcgen::SignatureAlgorithm) -> (Vec<u8>, Vec<u8>) {
    let key = rcgen::KeyPair::generate_for(algorithm).expect("key pair");
    let cert = rcgen::CertificateParams::new(vec!["user".to_owned()])
        .expect("certificate params")
        .self_signed(&key)
        .expect("self-signed certificate");
    (cert.der().to_vec(), key.serialize_der())
}

/// The key-exchange identity of a certificate and key pair.
fn cert_key((cert, key): &(Vec<u8>, Vec<u8>)) -> CertKey {
    CertKey::new(cert.clone(), key).expect("certificate key")
}
