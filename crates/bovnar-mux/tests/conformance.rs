//! The prototype against the C reference of the `bovnar` crate (`bvnr_mux_send` and
//! `bvnr_demux`): byte-identical output for shared vectors, and each side reads the other.

use bovnar::stream::{mux_dump, mux_load};
use bovnar_mux::{Event, MuxReader, MuxWriter, TERMINATOR};

const KEY: &str = "media";
const MAX: u64 = 4 << 20;

fn vectors() -> Vec<(u64, Vec<u8>)> {
    let pattern = |len: usize, seed: u8| -> Vec<u8> {
        (0..len)
            .map(|i| (i as u8).wrapping_mul(13) ^ seed)
            .collect()
    };
    vec![
        (0, b".kind = \"video\"; .codec = \"av1\";".to_vec()),
        (5, pattern(151, 1)),
        (2, pattern(30 * 1024 + 1, 2)),
        (2, pattern(400 * 1024 + 1, 3)),
        (7, Vec::new()),
        (300, pattern(65531, 4)),
        (300, pattern(65532, 5)),
        (1, pattern(65533, 6)),
        (u64::MAX, pattern(65536, 7)),
        (1 << 40, pattern(3 * 65536, 8)),
    ]
}

/// The document around an empty mux stream, split at the `0x00` that opens the octet stream.
fn envelope() -> (Vec<u8>, Vec<u8>) {
    let empty: [(u64, &[u8]); 0] = [];
    let doc = mux_dump(&empty, KEY).expect("reference writes an empty stream");
    let open = doc
        .iter()
        .position(|&b| b == 0)
        .expect("octet stream opener");
    (doc[..=open].to_vec(), doc[open + 1..].to_vec())
}

fn our_body(messages: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (channel, data) in messages {
        for chunk in MuxWriter::new(*channel).chunks(data) {
            body.extend_from_slice(chunk.header());
            body.extend_from_slice(chunk.body());
        }
    }
    body
}

#[test]
fn our_output_is_byte_identical_and_readable_by_the_reference() {
    let messages = vectors();
    let (prefix, suffix) = envelope();
    assert_eq!(suffix.first(), Some(&TERMINATOR));
    let ours = [prefix, our_body(&messages), suffix].concat();
    let reference = mux_dump(&messages, KEY).expect("reference writes the vectors");
    assert!(ours == reference, "framing differs from bvnr_mux_send");
    assert_eq!(
        mux_load(&ours, MAX, Some(KEY)).expect("reference reads ours"),
        messages
    );
}

#[test]
fn we_read_the_reference_output() {
    let messages = vectors();
    let (prefix, suffix) = envelope();
    let reference = mux_dump(&messages, KEY).expect("reference writes the vectors");
    let mut rest = &reference[prefix.len()..];
    let mut reader = MuxReader::new(MAX);
    let mut got = Vec::new();
    loop {
        let (n, event) = reader.read(rest).expect("valid reference stream");
        rest = &rest[n..];
        match event {
            Some(Event::Message { channel, data }) => got.push((channel, data.to_vec())),
            Some(Event::End) => break,
            None => panic!("reference stream ended without a terminator"),
        }
    }
    assert_eq!(got, messages);
    assert_eq!(rest, &suffix[1..]);
}
