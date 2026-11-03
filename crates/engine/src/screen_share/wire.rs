//! Media frames of a screen share on a direct stream ([`crate::p2p::STREAM_SCREEN`]).
//!
//! After the stream kind byte the sharer writes frames back to back, each a fixed header and
//! its payload (one video frame or one Opus packet). Version 1 (0.6.x) knows only VP9; version 2
//! adds a codec byte. The sharer writes version 2 only to viewers whose `Watch` listed codecs, so
//! an old viewer keeps reading version 1 with VP9 (FR-7.58).
//!
//! | v1 bytes | v2 bytes | field                                                  |
//! |----------|----------|--------------------------------------------------------|
//! | 1        | 1        | version ([`V1`] or [`V2`])                             |
//! | 1        | 1        | kind: 1 video, 2 audio (Opus)                          |
//! | –        | 1        | codec: 1 VP9, 2 AV1 for video; 0 for audio             |
//! | 1        | 1        | flags: bit 0 keyframe, other bits must be zero         |
//! | 1        | 1        | temporal layer, at most [`MAX_TEMPORAL_LAYER`]         |
//! | 8        | 8        | capture timestamp in microseconds                      |
//! | 2        | 2        | width in pixels (0 for audio)                          |
//! | 2        | 2        | height in pixels (0 for audio)                         |
//! | 4        | 4        | payload size in bytes                                  |
//!
//! The viewer's direction of the stream carries single-byte requests to the sharer; the only one
//! is [`KEYFRAME_REQUEST`] (after the viewer lost frames or switched decoders). Unknown bytes are
//! ignored.
//!
//! Integers are big-endian. Frames come from another client and are untrusted: the header is
//! checked before any payload is read, so a bad size never makes the reader allocate.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub use mumcord_screenshare::Codec;

/// The frame version of 0.6.x: VP9 only, no codec byte.
pub const V1: u8 = 1;
/// The frame version with a codec byte (FR-7.58).
pub const V2: u8 = 2;
/// The longest header of any version this client reads.
pub const MAX_HEADER_LEN: usize = 21;
/// Viewer to sharer: send a keyframe soon.
pub const KEYFRAME_REQUEST: u8 = 1;
/// Four temporal layers (0–3) at 60 fps (FR-7.37).
pub const MAX_TEMPORAL_LAYER: u8 = 3;
/// Far above a 1080p keyframe; anything larger is a broken or hostile sender.
pub const MAX_VIDEO_PAYLOAD: u32 = 4 * 1024 * 1024;
/// An Opus packet is at most 1275 bytes per frame; this leaves room for multi-frame packets.
pub const MAX_AUDIO_PAYLOAD: u32 = 4 * 1024;
pub const MAX_WIDTH: u16 = 7680;
pub const MAX_HEIGHT: u16 = 4320;

const FLAG_KEYFRAME: u8 = 0b1;

/// The header size of `version`; `None` for a version this client cannot read.
pub fn header_len(version: u8) -> Option<usize> {
    match version {
        V1 => Some(20),
        V2 => Some(MAX_HEADER_LEN),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Video(Codec),
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub kind: FrameKind,
    pub keyframe: bool,
    pub temporal_layer: u8,
    pub timestamp_us: u64,
    pub width: u16,
    pub height: u16,
    pub payload_len: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub header: FrameHeader,
    pub payload: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("screen share frame version {0} is not supported")]
    BadVersion(u8),
    #[error("unknown screen share frame kind {0}")]
    UnknownKind(u8),
    #[error("unknown screen share codec {0}")]
    UnknownCodec(u8),
    #[error("screen share frame version 1 carries only VP9")]
    CodecNotInVersion,
    #[error("screen share frame of {0} bytes is too large")]
    TooLarge(u32),
    #[error("screen share frame header is invalid")]
    BadField,
    #[error("screen share frame payload does not match its header")]
    PayloadMismatch,
    #[error("screen share stream failed: {0}")]
    Io(#[from] std::io::Error),
}

/// An encoded header; only the first `len` bytes count.
pub struct HeaderBytes {
    bytes: [u8; MAX_HEADER_LEN],
    len: usize,
}

impl HeaderBytes {
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl FrameHeader {
    /// Checks the header as the reader does, so a writer never sends what the other side drops.
    fn validate(&self) -> Result<(), WireError> {
        if self.temporal_layer > MAX_TEMPORAL_LAYER {
            return Err(WireError::BadField);
        }
        match self.kind {
            FrameKind::Video(_) => {
                if self.payload_len > MAX_VIDEO_PAYLOAD {
                    return Err(WireError::TooLarge(self.payload_len));
                }
                if self.width == 0
                    || self.height == 0
                    || self.width > MAX_WIDTH
                    || self.height > MAX_HEIGHT
                {
                    return Err(WireError::BadField);
                }
            }
            FrameKind::Audio => {
                if self.payload_len > MAX_AUDIO_PAYLOAD {
                    return Err(WireError::TooLarge(self.payload_len));
                }
                if self.width != 0 || self.height != 0 || self.keyframe || self.temporal_layer != 0
                {
                    return Err(WireError::BadField);
                }
            }
        }
        if self.payload_len == 0 {
            return Err(WireError::BadField);
        }
        Ok(())
    }

    /// The header as `version` writes it; version 1 refuses anything but VP9 video and audio.
    pub fn encode(&self, version: u8) -> Result<HeaderBytes, WireError> {
        self.validate()?;
        let len = header_len(version).ok_or(WireError::BadVersion(version))?;
        let (kind, codec) = match self.kind {
            FrameKind::Video(Codec::Vp9) => (1, 1),
            FrameKind::Video(Codec::Av1) => (1, 2),
            FrameKind::Audio => (2, 0),
        };
        if version == V1 && codec == 2 {
            return Err(WireError::CodecNotInVersion);
        }
        let mut bytes = [0u8; MAX_HEADER_LEN];
        bytes[0] = version;
        bytes[1] = kind;
        let rest = if version == V1 {
            &mut bytes[2..len]
        } else {
            bytes[2] = codec;
            &mut bytes[3..len]
        };
        rest[0] = if self.keyframe { FLAG_KEYFRAME } else { 0 };
        rest[1] = self.temporal_layer;
        rest[2..10].copy_from_slice(&self.timestamp_us.to_be_bytes());
        rest[10..12].copy_from_slice(&self.width.to_be_bytes());
        rest[12..14].copy_from_slice(&self.height.to_be_bytes());
        rest[14..18].copy_from_slice(&self.payload_len.to_be_bytes());
        Ok(HeaderBytes { bytes, len })
    }

    /// Reads a whole header of the length [`header_len`] gives for its first byte.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        let version = *bytes.first().ok_or(WireError::BadField)?;
        let len = header_len(version).ok_or(WireError::BadVersion(version))?;
        if bytes.len() != len {
            return Err(WireError::BadField);
        }
        let (codec, rest) = if version == V1 {
            (None, &bytes[2..])
        } else {
            (Some(bytes[2]), &bytes[3..])
        };
        let kind = match (bytes[1], codec) {
            (1, None | Some(1)) => FrameKind::Video(Codec::Vp9),
            (1, Some(2)) => FrameKind::Video(Codec::Av1),
            (1, Some(other)) => return Err(WireError::UnknownCodec(other)),
            (2, None | Some(0)) => FrameKind::Audio,
            (2, Some(_)) => return Err(WireError::BadField),
            (other, _) => return Err(WireError::UnknownKind(other)),
        };
        if rest[0] & !FLAG_KEYFRAME != 0 {
            return Err(WireError::BadField);
        }
        let mut timestamp = [0u8; 8];
        timestamp.copy_from_slice(&rest[2..10]);
        let header = Self {
            kind,
            keyframe: rest[0] & FLAG_KEYFRAME != 0,
            temporal_layer: rest[1],
            timestamp_us: u64::from_be_bytes(timestamp),
            width: u16::from_be_bytes([rest[10], rest[11]]),
            height: u16::from_be_bytes([rest[12], rest[13]]),
            payload_len: u32::from_be_bytes([rest[14], rest[15], rest[16], rest[17]]),
        };
        header.validate()?;
        Ok(header)
    }
}

/// Writes one frame in `version`; `frame.header.payload_len` must match the payload.
pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    frame: &Frame,
    version: u8,
) -> Result<(), WireError> {
    if usize::try_from(frame.header.payload_len).ok() != Some(frame.payload.len()) {
        return Err(WireError::PayloadMismatch);
    }
    writer
        .write_all(frame.header.encode(version)?.as_slice())
        .await?;
    writer.write_all(&frame.payload).await?;
    Ok(())
}

/// Reads the next frame of either version; `Ok(None)` when the stream ended cleanly between
/// frames.
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Option<Frame>, WireError> {
    let mut bytes = [0u8; MAX_HEADER_LEN];
    if reader.read(&mut bytes[..1]).await? == 0 {
        return Ok(None);
    }
    let len = header_len(bytes[0]).ok_or(WireError::BadVersion(bytes[0]))?;
    reader.read_exact(&mut bytes[1..len]).await?;
    let header = FrameHeader::decode(&bytes[..len])?;
    let len = usize::try_from(header.payload_len).map_err(|_| WireError::BadField)?;
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await?;
    Ok(Some(Frame { header, payload }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn video(codec: Codec, payload_len: u32) -> FrameHeader {
        FrameHeader {
            kind: FrameKind::Video(codec),
            keyframe: true,
            temporal_layer: 2,
            timestamp_us: 0x0102_0304_0506_0708,
            width: 1280,
            height: 720,
            payload_len,
        }
    }

    fn vp9(payload_len: u32) -> FrameHeader {
        video(Codec::Vp9, payload_len)
    }

    fn audio() -> FrameHeader {
        FrameHeader {
            kind: FrameKind::Audio,
            keyframe: false,
            temporal_layer: 0,
            timestamp_us: 20_000,
            width: 0,
            height: 0,
            payload_len: 3,
        }
    }

    fn encode(header: FrameHeader, version: u8) -> Vec<u8> {
        header.encode(version).unwrap().as_slice().to_vec()
    }

    #[test]
    fn version_1_bytes_stay_what_0_6_reads() {
        assert_eq!(
            encode(vp9(0x0a0b), V1),
            [
                1, 1, 1, 2, 1, 2, 3, 4, 5, 6, 7, 8, 0x05, 0x00, 0x02, 0xd0, 0, 0, 0x0a, 0x0b
            ]
        );
        assert_eq!(encode(audio(), V1)[..4], [1, 2, 0, 0]);
    }

    #[test]
    fn version_2_adds_the_codec_byte() {
        let bytes = encode(video(Codec::Av1, 0x0a0b), V2);
        assert_eq!(
            bytes,
            [
                2, 1, 2, 1, 2, 1, 2, 3, 4, 5, 6, 7, 8, 0x05, 0x00, 0x02, 0xd0, 0, 0, 0x0a, 0x0b
            ]
        );
        assert_eq!(encode(vp9(1), V2)[2], 1);
        assert_eq!(encode(audio(), V2)[2], 0);
    }

    #[test]
    fn headers_round_trip_in_both_versions() {
        for version in [V1, V2] {
            for header in [vp9(1234), audio()] {
                let bytes = encode(header, version);
                assert_eq!(bytes.len(), header_len(version).unwrap());
                assert_eq!(FrameHeader::decode(&bytes).unwrap(), header);
            }
        }
        let av1 = video(Codec::Av1, 99);
        assert_eq!(FrameHeader::decode(&encode(av1, V2)).unwrap(), av1);
    }

    #[test]
    fn version_1_never_carries_av1() {
        assert!(matches!(
            video(Codec::Av1, 10).encode(V1),
            Err(WireError::CodecNotInVersion)
        ));
    }

    #[test]
    fn rejects_unknown_version_kind_and_codec() {
        let mut bytes = encode(vp9(10), V1);
        bytes[0] = 3;
        assert!(matches!(
            FrameHeader::decode(&bytes),
            Err(WireError::BadVersion(3))
        ));
        assert!(vp9(10).encode(3).is_err());
        let mut bytes = encode(vp9(10), V1);
        bytes[1] = 9;
        assert!(matches!(
            FrameHeader::decode(&bytes),
            Err(WireError::UnknownKind(9))
        ));
        let mut bytes = encode(vp9(10), V2);
        bytes[2] = 7;
        assert!(matches!(
            FrameHeader::decode(&bytes),
            Err(WireError::UnknownCodec(7))
        ));
        let mut bytes = encode(audio(), V2);
        bytes[2] = 1;
        assert!(FrameHeader::decode(&bytes).is_err(), "audio with a codec");
        // A header cut to the other version's length is refused.
        assert!(FrameHeader::decode(&encode(vp9(10), V2)[..20]).is_err());
        assert!(FrameHeader::decode(&[]).is_err());
    }

    #[test]
    fn rejects_oversized_payloads() {
        let mut bytes = encode(vp9(10), V1);
        bytes[16..20].copy_from_slice(&(MAX_VIDEO_PAYLOAD + 1).to_be_bytes());
        assert!(matches!(
            FrameHeader::decode(&bytes),
            Err(WireError::TooLarge(_))
        ));
        let mut bytes = encode(audio(), V2);
        bytes[17..21].copy_from_slice(&(MAX_AUDIO_PAYLOAD + 1).to_be_bytes());
        assert!(matches!(
            FrameHeader::decode(&bytes),
            Err(WireError::TooLarge(_))
        ));
        bytes[17..21].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(FrameHeader::decode(&bytes).is_err());
    }

    #[test]
    fn rejects_bad_fields() {
        let cases: [(usize, u8); 5] = [
            (2, 0b10),  // unknown flag
            (3, 4),     // temporal layer too high
            (12, 0xff), // width too large
            (14, 0xff), // height too large
            (19, 0),    // empty payload (with byte 18 already zero)
        ];
        for (at, value) in cases {
            let mut bytes = encode(vp9(10), V1);
            bytes[at] = value;
            assert!(FrameHeader::decode(&bytes).is_err(), "v1 byte {at}");
            let mut bytes = encode(vp9(10), V2);
            bytes[at + 1] = value;
            assert!(FrameHeader::decode(&bytes).is_err(), "v2 byte {}", at + 1);
        }
        let mut bytes = encode(audio(), V1);
        bytes[13] = 1;
        assert!(FrameHeader::decode(&bytes).is_err(), "audio with a width");
    }

    #[test]
    fn writer_refuses_what_the_reader_rejects() {
        let mut header = vp9(10);
        header.temporal_layer = 7;
        assert!(header.encode(V2).is_err());
    }

    #[test]
    fn never_panics_on_arbitrary_headers() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        for n in 0..20_000 {
            let mut bytes = [0u8; MAX_HEADER_LEN];
            for b in &mut bytes {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *b = state as u8;
            }
            let version = if n % 2 == 0 { V1 } else { V2 };
            bytes[0] = version;
            let _ = FrameHeader::decode(&bytes[..header_len(version).unwrap()]);
        }
    }

    #[tokio::test]
    async fn frames_round_trip_over_a_stream() {
        let frames = vec![
            Frame {
                header: vp9(4),
                payload: vec![1, 2, 3, 4],
            },
            Frame {
                header: audio(),
                payload: vec![5, 6, 7],
            },
            Frame {
                header: video(Codec::Av1, 2),
                payload: vec![8, 9],
            },
        ];
        let mut buf = Vec::new();
        // An old sharer's v1 frames and a new sharer's v2 frames read the same way.
        write_frame(&mut buf, &frames[0], V1).await.unwrap();
        write_frame(&mut buf, &frames[1], V2).await.unwrap();
        write_frame(&mut buf, &frames[2], V2).await.unwrap();
        let mut reader = buf.as_slice();
        let mut read = Vec::new();
        while let Some(frame) = read_frame(&mut reader).await.unwrap() {
            read.push(frame);
        }
        assert_eq!(read, frames);
    }

    #[tokio::test]
    async fn truncated_streams_fail() {
        let frame = Frame {
            header: vp9(4),
            payload: vec![1, 2, 3, 4],
        };
        for version in [V1, V2] {
            let mut buf = Vec::new();
            write_frame(&mut buf, &frame, version).await.unwrap();
            let len = header_len(version).unwrap();
            for cut in [5, len, len + 2] {
                let mut reader = &buf[..cut];
                assert!(read_frame(&mut reader).await.is_err(), "cut at {cut}");
            }
        }
    }

    #[tokio::test]
    async fn payload_must_match_header() {
        let frame = Frame {
            header: vp9(5),
            payload: vec![1, 2, 3, 4],
        };
        let mut buf = Vec::new();
        assert!(matches!(
            write_frame(&mut buf, &frame, V2).await,
            Err(WireError::PayloadMismatch)
        ));
    }
}
