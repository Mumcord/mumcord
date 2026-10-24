//! Plaintext Mumble UDP packets in both wire formats. The same bytes travel encrypted over UDP
//! or unencrypted inside a TCP `UDPTunnel` message.
//!
//! - Legacy (servers up to 1.4): header byte `type << 5 | target`, then Mumble's own varints
//!   (`PacketDataStream`): [session (server → client only)], sequence, Opus size with the
//!   terminator flag `0x2000`, Opus data, optional 3 positional floats (native byte order, i.e.
//!   little endian on every platform Mumble runs on).
//! - Protobuf (servers ≥ 1.5): header byte `0` (Audio) or `1` (Ping) followed by the
//!   `MumbleUDP.proto` message.

use bytes::Bytes;
use prost::Message;

use crate::udp_proto;
use crate::version::MumbleVersion;

/// Largest plaintext packet Mumble accepts (`Mumble::Protocol::MAX_UDP_PACKET_SIZE`).
pub(crate) const MAX_PACKET: usize = 1024;

const LEGACY_PING: u8 = 1;
const LEGACY_OPUS: u8 = 4;
const PROTO_AUDIO: u8 = 0;
const PROTO_PING: u8 = 1;
/// Largest Opus payload the legacy size field can express.
const LEGACY_MAX_OPUS: usize = 0x1FFF;
const LEGACY_TERMINATOR: u64 = 0x2000;

/// First server version that speaks the protobuf UDP format (`PROTOBUF_INTRODUCTION_VERSION`).
const PROTOBUF_VERSION: MumbleVersion = MumbleVersion::new(1, 5, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Legacy,
    Protobuf,
}

impl Format {
    /// The server encodes for each receiver by the receiver's announced version (we announce
    /// 1.5), so the lower of both decides; an unknown server version means legacy.
    pub(crate) fn for_server(version: Option<MumbleVersion>) -> Self {
        match version {
            Some(v) if v >= PROTOBUF_VERSION => Self::Protobuf,
            _ => Self::Legacy,
        }
    }
}

/// An audio packet as it goes to the server.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AudioOut<'a> {
    pub target: u8,
    pub seq: u64,
    pub payload: &'a [u8],
    pub is_last: bool,
    pub positional: Option<[f32; 3]>,
}

/// An audio packet as it comes from the server.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AudioIn {
    pub session: u32,
    pub seq: u64,
    pub payload: Bytes,
    pub is_last: bool,
    /// Legacy target bits / protobuf context (see `voice` module docs).
    pub context: u8,
    pub positional: Option<[f32; 3]>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Incoming {
    Audio(AudioIn),
    Ping(u64),
}

pub(crate) fn encode_audio(format: Format, audio: &AudioOut<'_>) -> Option<Vec<u8>> {
    match format {
        Format::Legacy => {
            if audio.payload.len() > LEGACY_MAX_OPUS || audio.target >= 32 {
                return None;
            }
            let mut out = Vec::with_capacity(audio.payload.len() + 32);
            out.push(LEGACY_OPUS << 5 | audio.target);
            put_varint(&mut out, audio.seq);
            let mut size = audio.payload.len() as u64;
            if audio.is_last {
                size |= LEGACY_TERMINATOR;
            }
            put_varint(&mut out, size);
            out.extend_from_slice(audio.payload);
            if let Some(pos) = audio.positional {
                for f in pos {
                    out.extend_from_slice(&f.to_le_bytes());
                }
            }
            Some(out)
        }
        Format::Protobuf => {
            let msg = udp_proto::Audio {
                header: Some(udp_proto::audio::Header::Target(u32::from(audio.target))),
                sender_session: 0,
                frame_number: audio.seq,
                opus_data: audio.payload.to_vec(),
                positional_data: audio.positional.map(Vec::from).unwrap_or_default(),
                volume_adjustment: 0.0,
                is_terminator: audio.is_last,
            };
            Some(with_header(PROTO_AUDIO, &msg))
        }
    }
}

pub(crate) fn encode_ping(format: Format, timestamp: u64) -> Vec<u8> {
    match format {
        Format::Legacy => {
            let mut out = vec![LEGACY_PING << 5];
            put_varint(&mut out, timestamp);
            out
        }
        Format::Protobuf => with_header(
            PROTO_PING,
            &udp_proto::Ping {
                timestamp,
                ..Default::default()
            },
        ),
    }
}

/// Decodes a packet from the server; `None` for malformed packets and codecs other than Opus.
pub(crate) fn decode(format: Format, data: &[u8]) -> Option<Incoming> {
    let (&header, body) = data.split_first()?;
    match format {
        Format::Legacy => decode_legacy(header, body),
        Format::Protobuf => match header {
            PROTO_AUDIO => {
                let m = udp_proto::Audio::decode(body).ok()?;
                let context = match m.header {
                    Some(udp_proto::audio::Header::Context(c))
                    | Some(udp_proto::audio::Header::Target(c)) => u8::try_from(c).ok()?,
                    None => 0,
                };
                let positional = (m.positional_data.len() >= 3).then(|| {
                    [
                        m.positional_data[0],
                        m.positional_data[1],
                        m.positional_data[2],
                    ]
                });
                Some(Incoming::Audio(AudioIn {
                    session: m.sender_session,
                    seq: m.frame_number,
                    payload: Bytes::from(m.opus_data),
                    is_last: m.is_terminator,
                    context,
                    positional,
                }))
            }
            PROTO_PING => Some(Incoming::Ping(
                udp_proto::Ping::decode(body).ok()?.timestamp,
            )),
            _ => None,
        },
    }
}

fn decode_legacy(header: u8, body: &[u8]) -> Option<Incoming> {
    let mut r = Reader(body);
    match header >> 5 {
        LEGACY_PING => Some(Incoming::Ping(r.varint()?)),
        LEGACY_OPUS => {
            let session = u32::try_from(r.varint()?).ok()?;
            let seq = r.varint()?;
            let size = r.varint()?;
            let payload = Bytes::copy_from_slice(r.take((size & 0x1FFF) as usize)?);
            let positional = match r.0.len() {
                0 => None,
                12 => {
                    let f =
                        |i: usize| f32::from_le_bytes([r.0[i], r.0[i + 1], r.0[i + 2], r.0[i + 3]]);
                    Some([f(0), f(4), f(8)])
                }
                _ => return None,
            };
            Some(Incoming::Audio(AudioIn {
                session,
                seq,
                payload,
                is_last: size & LEGACY_TERMINATOR != 0,
                context: header & 0x1F,
                positional,
            }))
        }
        _ => None,
    }
}

fn with_header(header: u8, msg: &impl Message) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + msg.encoded_len());
    out.push(header);
    msg.encode(&mut out)
        .expect("Vec grows as needed, encoding cannot run out of space");
    out
}

/// Mumble's `PacketDataStream` varint for non-negative values.
fn put_varint(out: &mut Vec<u8>, v: u64) {
    if v < 0x80 {
        out.push(v as u8);
    } else if v < 0x4000 {
        out.extend_from_slice(&[0x80 | (v >> 8) as u8, v as u8]);
    } else if v < 0x20_0000 {
        out.extend_from_slice(&[0xC0 | (v >> 16) as u8, (v >> 8) as u8, v as u8]);
    } else if v < 0x1000_0000 {
        out.extend_from_slice(&[
            0xE0 | (v >> 24) as u8,
            (v >> 16) as u8,
            (v >> 8) as u8,
            v as u8,
        ]);
    } else if v < 0x1_0000_0000 {
        out.push(0xF0);
        out.extend_from_slice(&(v as u32).to_be_bytes());
    } else {
        out.push(0xF4);
        out.extend_from_slice(&v.to_be_bytes());
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }

    fn byte(&mut self) -> Option<u64> {
        self.take(1).map(|b| u64::from(b[0]))
    }

    fn be(&mut self, n: usize) -> Option<u64> {
        Some(
            self.take(n)?
                .iter()
                .fold(0u64, |acc, &b| acc << 8 | u64::from(b)),
        )
    }

    /// Decodes a `PacketDataStream` varint (negative forms included for completeness).
    fn varint(&mut self) -> Option<u64> {
        let v = self.byte()?;
        Some(if v & 0x80 == 0 {
            v & 0x7F
        } else if v & 0xC0 == 0x80 {
            (v & 0x3F) << 8 | self.byte()?
        } else if v & 0xF0 == 0xF0 {
            match v & 0xFC {
                0xF0 => self.be(4)?,
                0xF4 => self.be(8)?,
                0xF8 => !self.varint()?,
                _ => !(v & 0x03),
            }
        } else if v & 0xF0 == 0xE0 {
            (v & 0x0F) << 24 | self.be(3)?
        } else {
            (v & 0x1F) << 16 | self.be(2)?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip_and_known_bytes() {
        for v in [
            0u64,
            0x7F,
            0x80,
            0x3FFF,
            0x4000,
            0x1F_FFFF,
            0x20_0000,
            0xFFF_FFFF,
            0x1000_0000,
            0xFFFF_FFFF,
            0x1_0000_0000,
            u64::MAX >> 1,
        ] {
            let mut out = Vec::new();
            put_varint(&mut out, v);
            let mut r = Reader(&out);
            assert_eq!(r.varint(), Some(v), "{v:#x}");
            assert!(r.0.is_empty());
        }
        let mut out = Vec::new();
        put_varint(&mut out, 300);
        assert_eq!(out, [0x81, 0x2C]);
        assert_eq!(Reader(&[0xFC]).varint(), Some(!0), "-1 short form");
        assert_eq!(Reader(&[0x81]).varint(), None, "truncated");
    }

    #[test]
    fn version_picks_format() {
        assert_eq!(Format::for_server(None), Format::Legacy);
        assert_eq!(
            Format::for_server(Some(MumbleVersion::new(1, 4, 287))),
            Format::Legacy
        );
        assert_eq!(
            Format::for_server(Some(MumbleVersion::new(1, 5, 735))),
            Format::Protobuf
        );
    }

    #[test]
    fn legacy_client_audio_layout() {
        let audio = AudioOut {
            target: 0,
            seq: 300,
            payload: &[1, 2, 3],
            is_last: true,
            positional: None,
        };
        let out = encode_audio(Format::Legacy, &audio).expect("encode");
        // Opus type 4 in the top bits, seq 300, size 3 | terminator 0x2000.
        assert_eq!(out, [0x80, 0x81, 0x2C, 0xA0, 0x03, 1, 2, 3]);
    }

    /// Server → client legacy packet: like the client's, plus the session after the header.
    fn legacy_from_server(target: u8, session: u64, out: &AudioOut<'_>) -> Vec<u8> {
        let client = encode_audio(Format::Legacy, out).expect("encode");
        let mut v = vec![client[0] | target];
        put_varint(&mut v, session);
        v.extend_from_slice(&client[1..]);
        v
    }

    #[test]
    fn legacy_server_audio_decodes() {
        let out = AudioOut {
            target: 0,
            seq: 42,
            payload: &[9; 200],
            is_last: false,
            positional: Some([1.0, -2.5, 3.25]),
        };
        let data = legacy_from_server(3, 77, &out);
        let Some(Incoming::Audio(a)) = decode(Format::Legacy, &data) else {
            panic!("not audio");
        };
        assert_eq!((a.session, a.seq, a.is_last, a.context), (77, 42, false, 3));
        assert_eq!(&a.payload[..], &[9; 200][..]);
        assert_eq!(a.positional, Some([1.0, -2.5, 3.25]));

        let mut bad = data.clone();
        bad.push(0);
        assert_eq!(decode(Format::Legacy, &bad), None, "trailing garbage");
        assert_eq!(decode(Format::Legacy, &data[..10]), None, "truncated");
        assert_eq!(decode(Format::Legacy, &[2 << 5, 0, 0]), None, "speex");
    }

    #[test]
    fn protobuf_audio_roundtrip() {
        let out = AudioOut {
            target: 0,
            seq: 123_456,
            payload: &[5; 60],
            is_last: true,
            positional: None,
        };
        let data = encode_audio(Format::Protobuf, &out).expect("encode");
        assert_eq!(data[0], PROTO_AUDIO);
        let mut m = udp_proto::Audio::decode(&data[1..]).expect("proto");
        assert_eq!(m.header, Some(udp_proto::audio::Header::Target(0)));
        // What the server sends back: sender session and a context instead of the target.
        m.sender_session = 9;
        m.header = Some(udp_proto::audio::Header::Context(3));
        let Some(Incoming::Audio(a)) = decode(Format::Protobuf, &with_header(PROTO_AUDIO, &m))
        else {
            panic!("not audio");
        };
        assert_eq!(
            (a.session, a.seq, a.is_last, a.context, a.positional),
            (9, 123_456, true, 3, None)
        );
        assert_eq!(&a.payload[..], &[5; 60][..]);
    }

    #[test]
    fn pings_roundtrip_both_formats() {
        for f in [Format::Legacy, Format::Protobuf] {
            let p = encode_ping(f, 987_654_321);
            assert_eq!(decode(f, &p), Some(Incoming::Ping(987_654_321)));
        }
        assert_eq!(encode_ping(Format::Legacy, 5), [0x20, 5]);
    }

    #[test]
    fn legacy_rejects_oversized_payload() {
        let big = vec![0; LEGACY_MAX_OPUS + 1];
        let out = AudioOut {
            target: 0,
            seq: 0,
            payload: &big,
            is_last: false,
            positional: None,
        };
        assert_eq!(encode_audio(Format::Legacy, &out), None);
    }
}
