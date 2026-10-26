//! Media frames of a screen share on a direct stream ([`crate::p2p::STREAM_SCREEN`]).
//!
//! After the stream kind byte the sharer writes frames back to back, each a fixed header and
//! its payload (one VP9 frame or one Opus packet):
//!
//! | bytes | field                                              |
//! |-------|----------------------------------------------------|
//! | 1     | version ([`VERSION`])                              |
//! | 1     | kind: 1 video (VP9), 2 audio (Opus)                |
//! | 1     | flags: bit 0 keyframe, other bits must be zero     |
//! | 1     | temporal layer, at most [`MAX_TEMPORAL_LAYER`]     |
//! | 8     | capture timestamp in microseconds                  |
//! | 2     | width in pixels (0 for audio)                      |
//! | 2     | height in pixels (0 for audio)                     |
//! | 4     | payload size in bytes                              |
//!
//! The viewer's direction of the stream carries single-byte requests to the sharer; the only one
//! is [`KEYFRAME_REQUEST`] (after the viewer lost frames or switched decoders). Unknown bytes are
//! ignored.
//!
//! Integers are big-endian. Frames come from another client and are untrusted: the header is
//! checked before any payload is read, so a bad size never makes the reader allocate.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The frame version this client writes and reads; another version ends the stream, since the
/// header layout may differ.
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 20;
/// Viewer to sharer: send a keyframe soon.
pub const KEYFRAME_REQUEST: u8 = 1;
/// VP9 with three temporal layers (0, 1, 2) uses layers up to 2; one spare for a later mode.
pub const MAX_TEMPORAL_LAYER: u8 = 3;
/// Far above a 720p keyframe; anything larger is a broken or hostile sender.
pub const MAX_VIDEO_PAYLOAD: u32 = 4 * 1024 * 1024;
/// An Opus packet is at most 1275 bytes per frame; this leaves room for multi-frame packets.
pub const MAX_AUDIO_PAYLOAD: u32 = 4 * 1024;
pub const MAX_WIDTH: u16 = 7680;
pub const MAX_HEIGHT: u16 = 4320;

const FLAG_KEYFRAME: u8 = 0b1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Video,
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
    #[error("screen share frame of {0} bytes is too large")]
    TooLarge(u32),
    #[error("screen share frame header is invalid")]
    BadField,
    #[error("screen share frame payload does not match its header")]
    PayloadMismatch,
    #[error("screen share stream failed: {0}")]
    Io(#[from] std::io::Error),
}

impl FrameHeader {
    /// Checks the header as the reader does, so a writer never sends what the other side drops.
    fn validate(&self) -> Result<(), WireError> {
        if self.temporal_layer > MAX_TEMPORAL_LAYER {
            return Err(WireError::BadField);
        }
        match self.kind {
            FrameKind::Video => {
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

    pub fn encode(&self) -> Result<[u8; HEADER_LEN], WireError> {
        self.validate()?;
        let mut out = [0u8; HEADER_LEN];
        out[0] = VERSION;
        out[1] = match self.kind {
            FrameKind::Video => 1,
            FrameKind::Audio => 2,
        };
        out[2] = if self.keyframe { FLAG_KEYFRAME } else { 0 };
        out[3] = self.temporal_layer;
        out[4..12].copy_from_slice(&self.timestamp_us.to_be_bytes());
        out[12..14].copy_from_slice(&self.width.to_be_bytes());
        out[14..16].copy_from_slice(&self.height.to_be_bytes());
        out[16..20].copy_from_slice(&self.payload_len.to_be_bytes());
        Ok(out)
    }

    pub fn decode(bytes: &[u8; HEADER_LEN]) -> Result<Self, WireError> {
        if bytes[0] != VERSION {
            return Err(WireError::BadVersion(bytes[0]));
        }
        let kind = match bytes[1] {
            1 => FrameKind::Video,
            2 => FrameKind::Audio,
            other => return Err(WireError::UnknownKind(other)),
        };
        if bytes[2] & !FLAG_KEYFRAME != 0 {
            return Err(WireError::BadField);
        }
        let header = Self {
            kind,
            keyframe: bytes[2] & FLAG_KEYFRAME != 0,
            temporal_layer: bytes[3],
            timestamp_us: u64::from_be_bytes([
                bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11],
            ]),
            width: u16::from_be_bytes([bytes[12], bytes[13]]),
            height: u16::from_be_bytes([bytes[14], bytes[15]]),
            payload_len: u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
        };
        header.validate()?;
        Ok(header)
    }
}

/// Writes one frame; `frame.header.payload_len` must match the payload.
pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    frame: &Frame,
) -> Result<(), WireError> {
    if usize::try_from(frame.header.payload_len).ok() != Some(frame.payload.len()) {
        return Err(WireError::PayloadMismatch);
    }
    writer.write_all(&frame.header.encode()?).await?;
    writer.write_all(&frame.payload).await?;
    Ok(())
}

/// Reads the next frame; `Ok(None)` when the stream ended cleanly between frames.
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Option<Frame>, WireError> {
    let mut bytes = [0u8; HEADER_LEN];
    let first = reader.read(&mut bytes).await?;
    if first == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut bytes[first..]).await?;
    let header = FrameHeader::decode(&bytes)?;
    let len = usize::try_from(header.payload_len).map_err(|_| WireError::BadField)?;
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await?;
    Ok(Some(Frame { header, payload }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn video(payload_len: u32) -> FrameHeader {
        FrameHeader {
            kind: FrameKind::Video,
            keyframe: true,
            temporal_layer: 2,
            timestamp_us: 0x0102_0304_0506_0708,
            width: 1280,
            height: 720,
            payload_len,
        }
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

    #[test]
    fn header_round_trips() {
        for header in [video(1234), audio()] {
            let bytes = header.encode().unwrap();
            assert_eq!(FrameHeader::decode(&bytes).unwrap(), header);
        }
    }

    #[test]
    fn rejects_unknown_version_and_kind() {
        let mut bytes = video(10).encode().unwrap();
        bytes[0] = 2;
        assert!(matches!(
            FrameHeader::decode(&bytes),
            Err(WireError::BadVersion(2))
        ));
        let mut bytes = video(10).encode().unwrap();
        bytes[1] = 9;
        assert!(matches!(
            FrameHeader::decode(&bytes),
            Err(WireError::UnknownKind(9))
        ));
    }

    #[test]
    fn rejects_oversized_payloads() {
        let mut bytes = video(10).encode().unwrap();
        bytes[16..20].copy_from_slice(&(MAX_VIDEO_PAYLOAD + 1).to_be_bytes());
        assert!(matches!(
            FrameHeader::decode(&bytes),
            Err(WireError::TooLarge(_))
        ));
        let mut bytes = audio().encode().unwrap();
        bytes[16..20].copy_from_slice(&(MAX_AUDIO_PAYLOAD + 1).to_be_bytes());
        assert!(matches!(
            FrameHeader::decode(&bytes),
            Err(WireError::TooLarge(_))
        ));
        bytes[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
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
            let mut bytes = video(10).encode().unwrap();
            bytes[at] = value;
            assert!(FrameHeader::decode(&bytes).is_err(), "byte {at}");
        }
        let mut bytes = audio().encode().unwrap();
        bytes[13] = 1;
        assert!(FrameHeader::decode(&bytes).is_err(), "audio with a width");
    }

    #[test]
    fn writer_refuses_what_the_reader_rejects() {
        let mut header = video(10);
        header.temporal_layer = 7;
        assert!(header.encode().is_err());
    }

    #[test]
    fn never_panics_on_arbitrary_headers() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..10_000 {
            let mut bytes = [0u8; HEADER_LEN];
            for b in &mut bytes {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *b = state as u8;
            }
            bytes[0] = VERSION;
            let _ = FrameHeader::decode(&bytes);
        }
    }

    #[tokio::test]
    async fn frames_round_trip_over_a_stream() {
        let frames = vec![
            Frame {
                header: video(4),
                payload: vec![1, 2, 3, 4],
            },
            Frame {
                header: audio(),
                payload: vec![5, 6, 7],
            },
        ];
        let mut buf = Vec::new();
        for frame in &frames {
            write_frame(&mut buf, frame).await.unwrap();
        }
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
            header: video(4),
            payload: vec![1, 2, 3, 4],
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &frame).await.unwrap();
        for cut in [5, HEADER_LEN, HEADER_LEN + 2] {
            let mut reader = &buf[..cut];
            assert!(read_frame(&mut reader).await.is_err(), "cut at {cut}");
        }
    }

    #[tokio::test]
    async fn payload_must_match_header() {
        let frame = Frame {
            header: video(5),
            payload: vec![1, 2, 3, 4],
        };
        let mut buf = Vec::new();
        assert!(matches!(
            write_frame(&mut buf, &frame).await,
            Err(WireError::PayloadMismatch)
        ));
    }
}
