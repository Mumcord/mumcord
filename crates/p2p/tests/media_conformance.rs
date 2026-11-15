//! Conformance vectors for the media profile's codec against the C reference (FR-7.89, AC-7.83).
//!
//! The media profile uses the pure-Rust `bovnar::codec` (`MuxWriter`/`MuxReader`). The C library
//! that `bovnar` 2.1.0 links is the reference for the wire format: `bovnar::Writer::mux_begin`/
//! `mux_send`/`mux_end` call `bvnr_mux_begin`/`bvnr_mux_send`/`bvnr_mux_end`, and
//! `bovnar::stream::mux_load` reads through `bvnr_demux_on_event`. Both are reached through the
//! crate's safe public API; the raw FFI module is private.
//!
//! Every vector is checked in both directions and byte for byte:
//!
//! - the Rust document must equal the C document for the same messages,
//! - the C demultiplexer must read the Rust document,
//! - the Rust reader must read the C document.
//!
//! The vectors cover the sizes and shapes the profile and its codec meet: an empty message, 1 B,
//! 150 B, 30 KB, exactly 64 KiB, messages over 64 KiB (multi-chunk), the chunk boundaries, large
//! channel ids and a message cut across chunks.

use bovnar::Writer;
use bovnar::codec::{Message, MuxReader, MuxWriter};
use bovnar::stream::mux_load;

use mumcord_p2p::media::{MAX_AUDIO_MESSAGE, MAX_PLUGIN_MESSAGE, MediaReader, MediaWriter};

/// The octet-stream key of the media document (FR-7.89).
const KEY: &str = "media";
/// The media document's preamble: the Bovnar version directive (FR-7.89).
const PREAMBLE: &[u8] = b"#!bovnar 1.2\n";

/// One conformance vector.
struct Vector {
    /// Names the vector in a failure message.
    name: &'static str,
    /// `(channel, payload)` messages of the document, in order.
    messages: Vec<(u64, Vec<u8>)>,
}

/// A deterministic payload: `len` bytes, each derived from its offset and `seed`.
fn payload(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|at| {
            (at as u8)
                .wrapping_mul(37)
                .wrapping_add(seed)
                .wrapping_add((at >> 8) as u8)
        })
        .collect()
}

/// The vectors the media profile uses: channels 0–6 and every size and cut of FR-7.94.
fn profile_vectors() -> Vec<Vector> {
    vec![
        Vector {
            name: "empty message on the control channel",
            messages: vec![(0, Vec::new())],
        },
        Vector {
            name: "empty messages on every profile channel",
            messages: vec![
                (0, Vec::new()),
                (1, Vec::new()),
                (5, Vec::new()),
                (6, Vec::new()),
            ],
        },
        Vector {
            name: "1 byte on audio",
            messages: vec![(5, payload(1, 1))],
        },
        Vector {
            name: "150 bytes of audio",
            messages: vec![(5, payload(150, 2))],
        },
        Vector {
            name: "30 KB video delta",
            messages: vec![(1, payload(30_000, 3))],
        },
        Vector {
            name: "a control message with every kind byte",
            messages: vec![(0, (0..=6).collect())],
        },
        Vector {
            name: "exactly 64 KiB on plugin data",
            messages: vec![(6, payload(65_536, 4))],
        },
        Vector {
            name: "one byte over 64 KiB on video",
            messages: vec![(2, payload(65_537, 5))],
        },
        Vector {
            name: "two chunks on video",
            messages: vec![(1, payload(70_000, 6))],
        },
        Vector {
            name: "the chunk boundary exactly filled",
            // Channel 1 has a one-byte varint and a three-byte length here: 65,532 payload
            // bytes fill the first chunk; one more needs a second chunk.
            messages: vec![(1, payload(65_532, 7)), (1, payload(65_533, 8))],
        },
        Vector {
            name: "three chunks on a video layer",
            messages: vec![(3, payload(131_072, 9))],
        },
        Vector {
            name: "a message split at the second chunk boundary",
            messages: vec![(2, payload(65_532 + 65_535, 10))],
        },
        Vector {
            name: "all profile sizes in one document",
            messages: vec![
                (0, payload(16, 11)),
                (1, payload(150, 12)),
                (4, payload(30_000, 13)),
                (5, payload(150, 14)),
                (6, payload(65_536, 15)),
            ],
        },
        Vector {
            name: "a full-size audio message at its cap",
            messages: vec![(5, payload(MAX_AUDIO_MESSAGE, 16))],
        },
        Vector {
            name: "a full-size plugin message at its cap",
            messages: vec![(6, payload(MAX_PLUGIN_MESSAGE, 17))],
        },
        Vector {
            name: "a plugin message at the whole-message cap",
            messages: vec![(6, payload(MAX_PLUGIN_MESSAGE, 18))],
        },
    ]
}

/// The vectors of the codec itself: channel ids the media profile reserves, up to the largest a
/// varint can carry.
fn codec_vectors() -> Vec<Vector> {
    vec![
        Vector {
            name: "the last reserved channel id",
            messages: vec![(4_095, payload(64, 21))],
        },
        Vector {
            name: "a channel id beyond u16",
            messages: vec![(65_535, payload(64, 22)), (1 << 20, payload(64, 23))],
        },
        Vector {
            name: "the largest channel id varint",
            messages: vec![(u64::MAX, payload(64, 24))],
        },
        Vector {
            name: "multi-chunk on a channel beyond u16",
            messages: vec![(70_000, payload(131_072, 25))],
        },
        Vector {
            name: "large channel ids with an empty message",
            messages: vec![(4_095, Vec::new()), (u64::MAX, Vec::new())],
        },
    ]
}

/// The document `messages` produce through the media profile's own writer.
fn media_document(messages: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let writer = MediaWriter::new();
    let mut out = Vec::new();
    writer.begin(&mut out);
    for (channel, bytes) in messages {
        let channel = u16::try_from(*channel).expect("a media channel fits u16");
        writer
            .write_message(&mut out, channel, bytes)
            .expect("the vector is inside the media profile");
    }
    writer.finish(&mut out);
    out
}

/// The codec document `messages` produce: the same writer the media profile builds on, without
/// the profile's channel and size checks.
fn codec_document(messages: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let opening = MediaWriter::new().opening().to_vec();
    let assignment = b".media=\0";
    assert!(
        opening.ends_with(assignment),
        "the media opening ends with the assignment and the stream's opening NUL"
    );
    let preamble = &opening[..opening.len() - assignment.len()];
    assert_eq!(preamble, PREAMBLE, "the media document's preamble");
    let writer = MuxWriter::with_preamble(preamble, KEY).expect("valid media writer");
    let mut out = Vec::new();
    writer.begin(&mut out);
    for (channel, bytes) in messages {
        writer.write_message(&mut out, *channel, bytes);
    }
    writer.finish(&mut out);
    out
}

/// The document the C writer (`bvnr_mux_begin`, `bvnr_mux_send`, `bvnr_mux_end`) produces.
fn c_document(messages: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let payload_bytes: usize = messages.iter().map(|(_, bytes)| bytes.len()).sum();
    // The text around the messages plus one chunk header per 64 KiB of payload, generously.
    let cap = payload_bytes + messages.len() * 64 + payload_bytes / 65_536 * 4 + 4_096;
    let mut writer = Writer::to_mem_with(cap, false, 0, true).expect("C writer");
    writer.mux_begin(KEY).expect("bvnr_mux_begin");
    for (channel, bytes) in messages {
        writer.mux_send(*channel, bytes).expect("bvnr_mux_send");
    }
    writer.mux_end().expect("bvnr_mux_end");
    writer.finish().expect("C writer finish");
    writer.get_output().to_vec()
}

/// The messages the C demultiplexer reads from `document` (`bvnr_demux_on_event`).
fn c_demux(document: &[u8]) -> Vec<(u64, Vec<u8>)> {
    // A cap of 0 is the C demultiplexer's default, 64 MiB.
    mux_load(document, 0, Some(KEY)).expect("the C demux reads the document")
}

/// The messages the media profile's reader reads from `document`.
fn media_reader(document: &[u8]) -> Vec<(u64, Vec<u8>)> {
    let mut reader = MediaReader::new();
    let mut input = document;
    let mut messages = Vec::new();
    while let Some(message) = reader
        .read(&mut input)
        .expect("the media reader reads the document")
    {
        messages.push((u64::from(message.channel), message.payload.to_vec()));
    }
    assert!(
        input.is_empty(),
        "the media reader takes the whole document"
    );
    reader.finish().expect("a terminated document ends cleanly");
    messages
}

/// The messages the pure-Rust codec reader reads from `document`.
fn mux_reader(document: &[u8]) -> Vec<(u64, Vec<u8>)> {
    let mut reader = MuxReader::new();
    let mut input = document;
    let mut messages = Vec::new();
    while let Some(Message { channel, payload }) = reader
        .read(&mut input)
        .expect("the codec reader reads the document")
    {
        messages.push((channel, payload.to_vec()));
    }
    assert!(
        input.is_empty(),
        "the codec reader takes the whole document"
    );
    reader.finish().expect("a terminated document ends cleanly");
    messages
}

/// Checks one vector in both directions.
fn check(vector: &Vector, profile: bool) {
    let rust = if profile {
        media_document(&vector.messages)
    } else {
        codec_document(&vector.messages)
    };
    let c = c_document(&vector.messages);

    assert_eq!(
        rust, c,
        "{}: Rust output differs from bvnr_mux_send",
        vector.name
    );
    assert_eq!(
        c_demux(&rust),
        vector.messages,
        "{}: the C demux on Rust output",
        vector.name
    );
    assert_eq!(
        mux_reader(&c),
        vector.messages,
        "{}: the Rust reader on C output",
        vector.name
    );
    if profile {
        assert_eq!(
            media_reader(&c),
            vector.messages,
            "{}: the media reader on C output",
            vector.name
        );
    }
}

#[test]
fn the_profile_vectors_match_the_c_reference_both_ways() {
    for vector in profile_vectors() {
        check(&vector, true);
    }
}

#[test]
fn the_codec_vectors_match_the_c_reference_both_ways() {
    for vector in codec_vectors() {
        check(&vector, false);
    }
}

#[test]
fn rust_output_is_read_by_the_c_demux_in_pieces_too() {
    // The C demultiplexer is a feed; feeding a Rust document in pieces of one byte is the
    // hardest split a relay can produce and must give the same messages.
    use bovnar::live::{Demux, DemuxItem, DemuxOptions};

    let vector = Vector {
        name: "a multi-chunk message and a small one",
        messages: vec![(1, payload(70_000, 31)), (5, payload(150, 32))],
    };
    let document = media_document(&vector.messages);
    let options = DemuxOptions {
        deliver_events: false,
        ..DemuxOptions::default()
    };
    let mut demux = Demux::new(&options).expect("C demux");
    let mut messages = Vec::new();
    for byte in &document {
        let mut input = std::slice::from_ref(byte);
        while let Some(item) = demux.next(&mut input).expect("the C demux reads the byte") {
            if let DemuxItem::Message { channel, payload } = item {
                messages.push((channel, payload.to_vec()));
            }
        }
        assert!(input.is_empty(), "the C demux takes every byte");
    }
    demux.finish().expect("the C demux finishes");
    let mut empty: &[u8] = &[];
    while let Some(item) = demux.next(&mut empty).expect("closing events") {
        assert!(
            !matches!(item, DemuxItem::Message { .. }),
            "no message follows the terminator"
        );
    }
    assert_eq!(messages, vector.messages);
}
