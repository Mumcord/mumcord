//! Media frames of a screen share on a direct stream ([`crate::p2p::STREAM_SCREEN`]).
//!
//! A direct stream uses one of two wire formats:
//!
//! * [`Format::Media`] is the 0.9.0 Bovnar media document (FR-7.89). One endless `.media`
//!   document carries a description on channel 0 and frames on channels `1 + layer` (video) and
//!   [`mumcord_p2p::media::CHANNEL_AUDIO`] (audio). Every track message is a flags byte, an
//!   8-byte capture timestamp and the payload (FR-7.92). The width, height and codec of a video
//!   frame come from the description, not from the frame.
//! * [`Format::Legacy`] is the fixed 20/21-byte header of 0.6.x/0.8.x, kept for fallback
//!   (FR-7.98). Version 1 knows only VP9; version 2 adds a codec byte.
//!
//! Both formats use [`Frame`] / [`FrameHeader`] internally, so nothing above the wire changes.
//! Version 2 is written only to viewers whose `Watch` listed codecs, so an old viewer keeps
//! reading version 1 with VP9 (FR-7.58).
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

use mumcord_p2p::media::{
    self, CHANNEL_CONTROL, ChannelUse, CodecError, ControlKind, Description, MediaReader,
    MediaWriter, StreamKind, Track, VideoCodec,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::inspector::{MediaFormat, MediaTap, MediaTapSink, TapDirection};

pub use mumcord_screenshare::Codec;

/// The wire format of a direct screen stream (FR-7.89, FR-7.98).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// The fixed v1/v2 header of 0.8.x, used only as the capability fallback (FR-7.98).
    Legacy(u8),
    /// The 0.9.0 Bovnar media document (FR-7.89).
    Media,
}

/// What the description of a direct media stream must say before any of its frames is read
/// (FR-8.92): a camera stream names the camera session it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DescriptionCheck {
    /// Whatever the sharer announces; the screen-share viewer filters frames itself.
    Any,
    /// A camera stream: `.kind = "camera"` and exactly this camera session id.
    Camera(String),
}

impl DescriptionCheck {
    fn accepts(&self, description: &Description) -> bool {
        match self {
            Self::Any => true,
            Self::Camera(id) => {
                description.kind == StreamKind::Camera
                    && description.id.as_deref() == Some(id.as_str())
            }
        }
    }

    /// Whether a description this check does not accept ends the stream instead of being dropped.
    fn strict(&self) -> bool {
        !matches!(self, Self::Any)
    }
}

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

/// The wire size of one legacy frame: its header plus its payload.
fn wire_size(version: u8, payload_len: usize) -> u32 {
    let header = header_len(version).unwrap_or(0);
    u32::try_from(header + payload_len).unwrap_or(u32::MAX)
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
    #[error("the legacy frame header carries no stream description")]
    NoDescription,
    #[error("media stream: {0}")]
    Media(#[from] CodecError),
    #[error("media profile: {0}")]
    Profile(#[from] media::MediaError),
    #[error("the media stream is already finished")]
    Finished,
    #[error("the media stream description is not what this viewer expects")]
    Refused,
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
    pub(crate) fn validate(&self) -> Result<(), WireError> {
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
    read_frame_after_first(reader, bytes[0]).await
}

/// Reads a legacy frame whose first header byte was already read (the format detector needs
/// it to tell the two formats apart).
async fn read_frame_after_first<R: AsyncRead + Unpin>(
    reader: &mut R,
    first: u8,
) -> Result<Option<Frame>, WireError> {
    let mut bytes = [0u8; MAX_HEADER_LEN];
    bytes[0] = first;
    let len = header_len(first).ok_or(WireError::BadVersion(first))?;
    reader.read_exact(&mut bytes[1..len]).await?;
    let header = FrameHeader::decode(&bytes[..len])?;
    let len = usize::try_from(header.payload_len).map_err(|_| WireError::BadField)?;
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await?;
    Ok(Some(Frame { header, payload }))
}

/// Writes frames in one wire format (FR-7.89, FR-7.98).
///
/// A media writer opens the `.media` document with the first message and closes it in
/// [`finish`](Self::finish); a legacy writer has no document state. Both take the same [`Frame`]
/// type, so the sharer only chooses the format, not a second framing path.
pub struct FrameWriter<W> {
    inner: W,
    format: Format,
    media: MediaWriter,
    started: bool,
    finished: bool,
    /// One track message before the mux adds its channel and length.
    payload: Vec<u8>,
    /// One complete mux message.
    message: Vec<u8>,
    /// Where each framed message goes for the packet inspector (FR-7.101).
    tap: Option<MediaTapSink>,
}

impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    pub fn new(inner: W, format: Format) -> Self {
        Self {
            inner,
            format,
            media: MediaWriter::new(),
            started: false,
            finished: false,
            payload: Vec::new(),
            message: Vec::new(),
            tap: None,
        }
    }

    /// Records every message this writer frames for the packet inspector.
    pub fn with_tap(mut self, tap: Option<MediaTapSink>) -> Self {
        self.tap = tap;
        self
    }

    fn record(&self, tap: MediaTap<'_>) {
        if let Some(sink) = &self.tap {
            sink.record(TapDirection::Sent, tap);
        }
    }

    /// Writes the stream description (FR-7.95); only the media format has one.
    pub async fn write_description(&mut self, description: &Description) -> Result<(), WireError> {
        if !matches!(self.format, Format::Media) {
            return Err(WireError::NoDescription);
        }
        if self.finished {
            return Err(WireError::Finished);
        }
        let document = description.encode()?;
        self.payload.clear();
        self.payload.push(media::KIND_DESCRIPTION);
        self.payload.extend_from_slice(&document);
        if self.payload.len() > media::MAX_DESCRIPTION + 1 {
            return Err(WireError::TooLarge(media::MAX_DESCRIPTION as u32 + 1));
        }
        let size = u32::try_from(self.payload.len()).unwrap_or(u32::MAX);
        self.write_media_message(CHANNEL_CONTROL).await?;
        self.record(MediaTap::control(
            MediaFormat::Document,
            ControlKind::Description,
            size,
            Some(description),
        ));
        Ok(())
    }

    /// Writes one frame in the format this writer was opened with.
    pub async fn write_frame(&mut self, frame: &Frame) -> Result<(), WireError> {
        match self.format {
            Format::Legacy(version) => {
                write_frame(&mut self.inner, frame, version).await?;
                self.record(MediaTap::legacy(
                    version,
                    frame.header.keyframe,
                    wire_size(version, frame.payload.len()),
                ));
                Ok(())
            }
            Format::Media => self.write_media_frame(frame).await,
        }
    }

    async fn write_media_frame(&mut self, frame: &Frame) -> Result<(), WireError> {
        if self.finished {
            return Err(WireError::Finished);
        }
        if usize::try_from(frame.header.payload_len).ok() != Some(frame.payload.len()) {
            return Err(WireError::PayloadMismatch);
        }
        frame.header.validate()?;
        let channel = match frame.header.kind {
            FrameKind::Video(_) => {
                media::video_channel(frame.header.temporal_layer).ok_or(WireError::BadField)?
            }
            FrameKind::Audio => media::CHANNEL_AUDIO,
        };
        let len = 1 + 8 + frame.payload.len();
        if media::message_limit(channel).is_some_and(|limit| len > limit) {
            return Err(WireError::TooLarge(len as u32));
        }
        self.payload.clear();
        self.payload.push(if frame.header.keyframe {
            media::FLAG_KEYFRAME
        } else {
            0
        });
        self.payload
            .extend_from_slice(&frame.header.timestamp_us.to_be_bytes());
        self.payload.extend_from_slice(&frame.payload);
        let flags = self.payload.first().copied().unwrap_or(0);
        let size = u32::try_from(self.payload.len()).unwrap_or(u32::MAX);
        self.write_media_message(channel).await?;
        self.record(MediaTap::track(
            MediaFormat::Document,
            channel,
            flags,
            size,
        ));
        Ok(())
    }

    /// Sends the composed [`payload`](Self::payload) on `channel`, opening the document first.
    async fn write_media_message(&mut self, channel: u16) -> Result<(), WireError> {
        if !self.started {
            self.inner.write_all(self.media.opening()).await?;
            self.started = true;
        }
        self.message.clear();
        self.media
            .write_message(&mut self.message, channel, &self.payload)?;
        self.inner.write_all(&self.message).await?;
        Ok(())
    }

    /// Closes the media document; a legacy stream ends with the last frame. Calling it twice
    /// writes the terminator once.
    pub async fn finish(&mut self) -> Result<(), WireError> {
        if !matches!(self.format, Format::Media) || self.finished {
            return Ok(());
        }
        if self.started {
            self.inner.write_all(self.media.terminator()).await?;
        }
        self.finished = true;
        Ok(())
    }

    /// The stream underneath, after [`finish`](Self::finish).
    pub fn into_inner(self) -> W {
        self.inner
    }
}

/// Reads frames in one wire format (FR-7.89, FR-7.98).
///
/// A media reader accepts arbitrary byte splits, keeps the latest description and applies a
/// changed description at the next keyframe; a legacy reader accepts v1 and v2 frames. Frames
/// that do not match the description are dropped, so a bad peer never becomes a broken picture.
pub struct FrameReader<R> {
    inner: R,
    media: Option<MediaFraming>,
    /// The format is not known yet: the first byte tells the media document apart from a legacy
    /// frame (FR-7.97). A 0.8.x sharer and a 0.9.0 sharer both serve direct viewers.
    auto: bool,
    /// The first legacy header byte read while detecting the format.
    first: Option<u8>,
    /// The legacy header version once the format is known; `None` for a media document.
    legacy: Option<u8>,
    /// What the description must say; see [`Self::with_check`].
    check: DescriptionCheck,
    /// Where every framed message goes for the packet inspector (FR-7.101).
    tap: Option<MediaTapSink>,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    pub fn new(inner: R, format: Format) -> Self {
        let check = DescriptionCheck::Any;
        Self {
            inner,
            media: matches!(format, Format::Media).then(|| MediaFraming::new(check.clone())),
            auto: false,
            first: None,
            legacy: match format {
                Format::Legacy(version) => Some(version),
                Format::Media => None,
            },
            check,
            tap: None,
        }
    }

    /// A reader that accepts both formats and recognizes the first one from the stream.
    pub fn auto(inner: R) -> Self {
        Self {
            inner,
            media: None,
            auto: true,
            first: None,
            legacy: None,
            check: DescriptionCheck::Any,
            tap: None,
        }
    }

    /// Records every message this reader frames for the packet inspector.
    pub fn with_tap(mut self, tap: Option<MediaTapSink>) -> Self {
        self.tap = tap.clone();
        if let Some(media) = &mut self.media {
            media.tap = tap;
        }
        self
    }

    /// Accepts only a description `check` allows and ends the stream on any other (FR-8.92):
    /// frames that belong to another camera session are never decoded.
    pub fn with_check(mut self, check: DescriptionCheck) -> Self {
        if let Some(media) = &mut self.media {
            media.check = check.clone();
        }
        self.check = check;
        self
    }

    /// The next frame, or `Ok(None)` when the stream ended at a frame or message boundary.
    pub async fn read_frame(&mut self) -> Result<Option<Frame>, WireError> {
        if self.auto {
            let mut first = [0u8; 1];
            if self.inner.read(&mut first).await? == 0 {
                return Ok(None);
            }
            self.auto = false;
            match first[0] {
                b'#' => {
                    let mut media = MediaFraming::new(self.check.clone());
                    media.tap = self.tap.clone();
                    media.buffer.push(b'#');
                    self.media = Some(media);
                }
                V1 | V2 => {
                    self.first = Some(first[0]);
                    self.legacy = Some(first[0]);
                }
                other => return Err(WireError::BadVersion(other)),
            }
        }
        match &mut self.media {
            Some(media) => media.read_frame(&mut self.inner).await,
            None => {
                let frame = match self.first.take() {
                    Some(first) => read_frame_after_first(&mut self.inner, first).await?,
                    None => read_frame(&mut self.inner).await?,
                };
                if let (Some(frame), Some(version), Some(sink)) = (&frame, self.legacy, &self.tap) {
                    sink.record(
                        TapDirection::Received,
                        MediaTap::legacy(
                            version,
                            frame.header.keyframe,
                            wire_size(version, frame.payload.len()),
                        ),
                    );
                }
                Ok(frame)
            }
        }
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

/// The state a media document needs between reads: the mux reader, bytes not yet parsed, the
/// active description and one waiting for the next keyframe.
struct MediaFraming {
    reader: MediaReader,
    buffer: Vec<u8>,
    description: Option<Description>,
    pending: Option<Description>,
    eof: bool,
    check: DescriptionCheck,
    /// A description this check refused: the stream is over.
    refused: bool,
    /// Where every framed message goes for the packet inspector (FR-7.101).
    tap: Option<MediaTapSink>,
}

impl MediaFraming {
    fn new(check: DescriptionCheck) -> Self {
        Self {
            reader: MediaReader::new(),
            buffer: Vec::new(),
            description: None,
            pending: None,
            eof: false,
            check,
            refused: false,
            tap: None,
        }
    }

    /// Records one received track message; its flags byte is all the inspector keeps of it.
    fn record_track(&self, channel: u16, payload: &[u8]) {
        let Some(sink) = &self.tap else {
            return;
        };
        let flags = payload.first().copied().unwrap_or(0);
        let size = u32::try_from(payload.len()).unwrap_or(u32::MAX);
        sink.record(
            TapDirection::Received,
            MediaTap::track(MediaFormat::Document, channel, flags, size),
        );
    }

    /// Records one received channel-0 message; only a description has readable content.
    fn record_control(
        &self,
        kind: Option<ControlKind>,
        size: u32,
        description: Option<&Description>,
    ) {
        let Some(sink) = &self.tap else {
            return;
        };
        sink.record(
            TapDirection::Received,
            MediaTap {
                format: MediaFormat::Document,
                channel: Some(CHANNEL_CONTROL),
                flags: None,
                control: kind,
                size,
                keyframe: false,
                description,
            },
        );
    }

    async fn read_frame<R: AsyncRead + Unpin>(
        &mut self,
        inner: &mut R,
    ) -> Result<Option<Frame>, WireError> {
        loop {
            if let Some(frame) = self.next_frame()? {
                return Ok(Some(frame));
            }
            if self.eof {
                self.reader.finish()?;
                return Ok(None);
            }
            let mut chunk = [0u8; 8192];
            let read = inner.read(&mut chunk).await?;
            if read == 0 {
                self.eof = true;
            } else {
                self.buffer.extend_from_slice(&chunk[..read]);
            }
        }
    }

    /// One complete message, if the buffer holds one. A dropped message is logged and the scan
    /// goes on; only a broken document is fatal.
    fn next_frame(&mut self) -> Result<Option<Frame>, WireError> {
        loop {
            if self.refused {
                return Err(WireError::Refused);
            }
            let (consumed, step) = {
                let mut input = &self.buffer[..];
                let result = self.reader.read(&mut input);
                let consumed = self.buffer.len() - input.len();
                let step = match result {
                    Ok(Some(message)) => Step::Message(message.channel, message.payload.to_vec()),
                    Ok(None) => Step::None,
                    Err(CodecError::Dropped(reason)) => {
                        tracing::debug!(%reason, "dropping a media message outside the profile");
                        Step::Dropped
                    }
                    Err(error) => return Err(WireError::Media(error)),
                };
                (consumed, step)
            };
            if consumed > 0 {
                self.buffer.drain(..consumed);
            }
            match step {
                Step::Dropped if consumed == 0 => return Ok(None),
                Step::Dropped => continue,
                Step::Message(channel, payload) => {
                    if let Some(frame) = self.message_frame(channel, &payload) {
                        return Ok(Some(frame));
                    }
                }
                Step::None => return Ok(None),
            }
        }
    }

    fn message_frame(&mut self, channel: u16, payload: &[u8]) -> Option<Frame> {
        match media::channel_use(channel) {
            Some(ChannelUse::Control) => {
                self.description_message(payload);
                None
            }
            Some(ChannelUse::VideoLayer(layer)) => {
                self.record_track(channel, payload);
                self.video_frame(layer, payload)
            }
            Some(ChannelUse::Audio) => {
                self.record_track(channel, payload);
                self.audio_frame(payload)
            }
            Some(ChannelUse::PluginData | ChannelUse::Reserved) | None => {
                tracing::debug!(
                    channel,
                    "dropping a media message for a channel not used by a share"
                );
                None
            }
        }
    }

    fn description_message(&mut self, payload: &[u8]) {
        let size = u32::try_from(payload.len()).unwrap_or(u32::MAX);
        let Some((&kind, body)) = payload.split_first() else {
            tracing::debug!("dropping an empty media control message");
            return;
        };
        match ControlKind::from_byte(kind) {
            Some(ControlKind::Description) => match Description::parse(body) {
                Ok(description) => {
                    self.record_control(
                        Some(ControlKind::Description),
                        size,
                        Some(&description),
                    );
                    if !self.check.accepts(&description) {
                        tracing::debug!(
                            kind = description.kind.name(),
                            "refusing a media description this viewer did not ask for"
                        );
                        self.refused = true;
                        return;
                    }
                    tracing::trace!(
                        kind = description.kind.name(),
                        tracks = description.tracks.len(),
                        "media stream description"
                    );
                    if self.description.is_some() {
                        self.pending = Some(description);
                    } else {
                        self.description = Some(description);
                    }
                }
                Err(error) => {
                    self.record_control(Some(ControlKind::Description), size, None);
                    tracing::debug!(%error, "dropping an invalid media description");
                    // A viewer that waits for one particular stream closes on it; a screen
                    // share keeps reading and waits for a description it can use.
                    self.refused = self.check.strict();
                }
            },
            Some(kind) => {
                self.record_control(Some(kind), size, None);
                tracing::debug!(?kind, "ignoring a direct media control message");
            }
            None => {
                self.record_control(None, size, None);
                tracing::debug!(kind, "ignoring an unknown media control message");
            }
        }
    }

    fn video_frame(&mut self, layer: u8, payload: &[u8]) -> Option<Frame> {
        let (keyframe, timestamp_us, body) = split_track(payload)?;
        if keyframe && layer != 0 {
            tracing::debug!(layer, "dropping a keyframe outside the base layer");
            return None;
        }
        if keyframe && let Some(description) = self.pending.take() {
            self.description = Some(description);
        }
        let description = self.description.as_ref()?;
        let channel = media::CHANNEL_VIDEO_BASE + u16::from(layer);
        let Some(Track::Video(track)) = description
            .tracks
            .iter()
            .find(|track| track.channel() == channel)
        else {
            tracing::debug!(
                channel,
                "dropping a video frame the description does not list"
            );
            return None;
        };
        let header = FrameHeader {
            kind: FrameKind::Video(codec(track.codec)),
            keyframe,
            temporal_layer: layer,
            timestamp_us,
            width: track.width,
            height: track.height,
            payload_len: u32::try_from(body.len()).ok()?,
        };
        header.validate().ok()?;
        Some(Frame {
            header,
            payload: body.to_vec(),
        })
    }

    fn audio_frame(&mut self, payload: &[u8]) -> Option<Frame> {
        let (keyframe, timestamp_us, body) = split_track(payload)?;
        if keyframe {
            tracing::debug!("dropping an audio frame marked as a keyframe");
            return None;
        }
        let description = self.description.as_ref()?;
        if !description.tracks.iter().any(
            |track| matches!(track, Track::Audio(audio) if audio.channel == media::CHANNEL_AUDIO),
        ) {
            tracing::debug!("dropping audio the description does not list");
            return None;
        }
        let header = FrameHeader {
            kind: FrameKind::Audio,
            keyframe: false,
            temporal_layer: 0,
            timestamp_us,
            width: 0,
            height: 0,
            payload_len: u32::try_from(body.len()).ok()?,
        };
        header.validate().ok()?;
        Some(Frame {
            header,
            payload: body.to_vec(),
        })
    }
}

enum Step {
    Message(u16, Vec<u8>),
    Dropped,
    None,
}

fn split_track(payload: &[u8]) -> Option<(bool, u64, &[u8])> {
    let (&flags, rest) = payload.split_first()?;
    let keyframe = media::check_flags(flags).ok()?;
    let timestamp = rest.get(..8)?;
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(timestamp);
    let body = &rest[8..];
    if body.is_empty() {
        return None;
    }
    Some((keyframe, u64::from_be_bytes(bytes), body))
}

fn codec(codec: VideoCodec) -> Codec {
    match codec {
        VideoCodec::Vp9 => Codec::Vp9,
        VideoCodec::Av1 => Codec::Av1,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::Arc;

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

    fn screen_description(codec: Codec, width: u16, height: u16) -> Description {
        let codec = match codec {
            Codec::Vp9 => VideoCodec::Vp9,
            Codec::Av1 => VideoCodec::Av1,
        };
        let video = |layer: u8| {
            Track::Video(mumcord_p2p::media::VideoTrack {
                channel: media::CHANNEL_VIDEO_BASE + u16::from(layer),
                codec,
                layer,
                width,
                height,
                rate: 30.0,
            })
        };
        Description {
            kind: mumcord_p2p::media::StreamKind::Screen,
            timebase: 1,
            id: None,
            tracks: vec![
                video(0),
                video(1),
                video(2),
                Track::Audio(mumcord_p2p::media::AudioTrack {
                    channel: media::CHANNEL_AUDIO,
                    codec: mumcord_p2p::media::AudioCodec::Opus,
                    rate: 48_000,
                    bitrate: 96,
                }),
            ],
        }
    }

    fn frame(codec: Codec, keyframe: bool, layer: u8, timestamp_us: u64, payload: &[u8]) -> Frame {
        Frame {
            header: FrameHeader {
                kind: FrameKind::Video(codec),
                keyframe,
                temporal_layer: layer,
                timestamp_us,
                width: 1280,
                height: 720,
                payload_len: payload.len() as u32,
            },
            payload: payload.to_vec(),
        }
    }

    fn audio_frame(payload: &[u8]) -> Frame {
        Frame {
            header: FrameHeader {
                payload_len: payload.len() as u32,
                ..audio()
            },
            payload: payload.to_vec(),
        }
    }

    /// An async reader that hands out at most `chunk` bytes per poll, for split tests.
    struct Chunks<'a> {
        bytes: &'a [u8],
        at: usize,
        chunk: usize,
    }

    impl<'a> Chunks<'a> {
        fn new(bytes: &'a [u8], chunk: usize) -> Self {
            Self {
                bytes,
                at: 0,
                chunk,
            }
        }
    }

    impl AsyncRead for Chunks<'_> {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            let rest = &self.bytes[self.at..];
            let len = rest.len().min(self.chunk).min(buf.remaining());
            buf.put_slice(&rest[..len]);
            self.at += len;
            std::task::Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn media_document_round_trips_frames_and_fills_the_description() {
        let description = screen_description(Codec::Vp9, 1280, 720);
        let frames = vec![
            frame(Codec::Vp9, true, 0, 1_000, &[1, 2, 3, 4]),
            frame(Codec::Vp9, false, 2, 2_000, &[5, 6, 7, 8, 9, 10]),
            audio_frame(&[11, 12, 13]),
        ];
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf, Format::Media);
        writer.write_description(&description).await.unwrap();
        for frame in &frames {
            writer.write_frame(frame).await.unwrap();
        }
        writer.finish().await.unwrap();
        assert!(buf.starts_with(b"#!bovnar 1.2\n.media=\0"));
        assert!(buf.ends_with(b"\0;"));

        let mut reader = FrameReader::new(buf.as_slice(), Format::Media);
        let mut read = Vec::new();
        while let Some(frame) = reader.read_frame().await.unwrap() {
            read.push(frame);
        }
        assert_eq!(read, frames, "the description fills the frame fields");
    }

    #[tokio::test]
    async fn every_message_of_the_document_is_tapped_for_the_inspector() {
        let inspector = Arc::new(crate::inspector::Inspector::default());
        inspector.set_enabled(true);
        let tap = MediaTapSink::new(&inspector, 1);
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf, Format::Media).with_tap(Some(tap.clone()));
        writer
            .write_description(&screen_description(Codec::Vp9, 1280, 720))
            .await
            .unwrap();
        writer
            .write_frame(&frame(Codec::Vp9, true, 1, 0, &[1, 2, 3, 4]))
            .await
            .unwrap();
        writer.finish().await.unwrap();

        let sent = inspector.poll(None, Vec::new());
        assert_eq!(sent.packets.len(), 2);
        let description = sent.packets[0].media.as_ref().expect("a media row");
        assert_eq!(description.channel, Some(CHANNEL_CONTROL));
        assert_eq!(description.control, Some(media::KIND_DESCRIPTION));
        assert!(
            sent.packets[0]
                .decoded
                .as_deref()
                .is_some_and(|d: &str| d.contains("\"kind\": \"screen\"")),
            "the description is decoded"
        );
        let layer = sent.packets[1].media.as_ref().expect("a media row");
        assert_eq!(layer.channel, Some(2), "channel = 1 + layer");
        assert!(layer.keyframe);
        assert_eq!(sent.packets[1].size, 1 + 8 + 4, "flags, timestamp, payload");
        assert_eq!(sent.packets[1].raw_hex, "", "no media content, ever");

        let mut reader = FrameReader::new(buf.as_slice(), Format::Media).with_tap(Some(tap));
        while reader.read_frame().await.unwrap().is_some() {}
        let received = inspector.poll(Some(sent.packets[1].id), Vec::new());
        assert_eq!(received.packets.len(), 2);
        assert!(received.packets.iter().all(|p| p.direction == TapDirection::Received));
        let layer = received.packets[1].media.as_ref().expect("a media row");
        assert_eq!(layer.channel, Some(2));
        assert!(layer.keyframe);
    }

    #[tokio::test]
    async fn legacy_frames_are_tapped_with_their_version() {
        let inspector = Arc::new(crate::inspector::Inspector::default());
        inspector.set_enabled(true);
        let tap = MediaTapSink::new(&inspector, 1);
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf, Format::Legacy(V2)).with_tap(Some(tap.clone()));
        writer
            .write_frame(&frame(Codec::Vp9, true, 0, 0, &[1, 2, 3]))
            .await
            .unwrap();

        let sent = inspector.poll(None, Vec::new());
        let row = &sent.packets[0];
        assert_eq!(row.kind, ".media legacy v2");
        assert_eq!(row.size, (MAX_HEADER_LEN + 3) as u32);
        let view = row.media.as_ref().expect("a media row");
        assert_eq!((view.channel, view.keyframe), (None, true));

        let mut reader = FrameReader::auto(buf.as_slice()).with_tap(Some(tap));
        assert!(reader.read_frame().await.unwrap().is_some());
        let received = inspector.poll(Some(row.id), Vec::new());
        assert_eq!(received.packets[0].kind, ".media legacy v2");
        assert_eq!(received.packets[0].direction, TapDirection::Received);
    }

    #[tokio::test]
    async fn a_changed_description_applies_at_the_next_keyframe() {
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf, Format::Media);
        writer
            .write_description(&screen_description(Codec::Vp9, 640, 360))
            .await
            .unwrap();
        writer
            .write_frame(&frame(Codec::Vp9, true, 0, 0, &[1]))
            .await
            .unwrap();
        writer
            .write_description(&screen_description(Codec::Vp9, 1280, 720))
            .await
            .unwrap();
        writer
            .write_frame(&frame(Codec::Vp9, false, 0, 1, &[2]))
            .await
            .unwrap();
        writer
            .write_frame(&frame(Codec::Vp9, true, 0, 2, &[3]))
            .await
            .unwrap();
        writer.finish().await.unwrap();

        let mut reader = FrameReader::new(buf.as_slice(), Format::Media);
        let first = reader.read_frame().await.unwrap().unwrap();
        let delta = reader.read_frame().await.unwrap().unwrap();
        let key = reader.read_frame().await.unwrap().unwrap();
        assert_eq!((first.header.width, first.header.height), (640, 360));
        assert_eq!((delta.header.width, delta.header.height), (640, 360));
        assert_eq!((key.header.width, key.header.height), (1280, 720));
        assert!(reader.read_frame().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn media_reader_drops_frames_outside_the_description() {
        let description = screen_description(Codec::Vp9, 1280, 720);
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf, Format::Media);
        writer.write_description(&description).await.unwrap();
        writer
            .write_frame(&frame(Codec::Vp9, true, 0, 0, &[1]))
            .await
            .unwrap();
        // The writer does not know the description's track set; the reader must drop this.
        writer
            .write_frame(&frame(Codec::Vp9, false, 3, 1, &[2]))
            .await
            .unwrap();
        writer
            .write_frame(&frame(Codec::Vp9, false, 0, 2, &[3]))
            .await
            .unwrap();
        writer.finish().await.unwrap();

        let mut reader = FrameReader::new(buf.as_slice(), Format::Media);
        let mut read = Vec::new();
        while let Some(frame) = reader.read_frame().await.unwrap() {
            read.push(frame);
        }
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].payload, [1]);
        assert_eq!(read[1].payload, [3]);
    }

    #[tokio::test]
    async fn media_reader_drops_a_keyframe_outside_the_base_layer() {
        let description = screen_description(Codec::Vp9, 1280, 720);
        let mut document = Vec::new();
        let writer = MediaWriter::new();
        writer.begin(&mut document);
        let mut message = vec![media::KIND_DESCRIPTION];
        message.extend_from_slice(&description.encode().unwrap());
        writer
            .write_message(&mut document, CHANNEL_CONTROL, &message)
            .unwrap();
        writer
            .write_message(
                &mut document,
                media::CHANNEL_VIDEO_BASE + 2,
                &[media::FLAG_KEYFRAME, 0, 0, 0, 0, 0, 0, 0, 0, 9],
            )
            .unwrap();
        writer.finish(&mut document);
        let mut reader = FrameReader::new(document.as_slice(), Format::Media);
        assert!(reader.read_frame().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn media_reader_accepts_any_byte_split_and_an_unterminated_end() {
        let description = screen_description(Codec::Vp9, 1280, 720);
        let frames = vec![
            frame(Codec::Vp9, true, 0, 0, &[1, 2, 3, 4]),
            frame(Codec::Vp9, false, 1, 1, &[5, 6, 7]),
        ];
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf, Format::Media);
        writer.write_description(&description).await.unwrap();
        for frame in &frames {
            writer.write_frame(frame).await.unwrap();
        }
        // No finish: an end at a message boundary is a clean stop.

        let mut reader = FrameReader::new(Chunks::new(&buf, 1), Format::Media);
        let mut read = Vec::new();
        while let Some(frame) = reader.read_frame().await.unwrap() {
            read.push(frame);
        }
        assert_eq!(read, frames);
    }

    #[tokio::test]
    async fn media_reader_reports_a_stream_cut_inside_a_message() {
        let description = screen_description(Codec::Vp9, 1280, 720);
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf, Format::Media);
        writer.write_description(&description).await.unwrap();
        writer
            .write_frame(&frame(Codec::Vp9, true, 0, 0, &[1, 2, 3, 4]))
            .await
            .unwrap();
        writer
            .write_frame(&frame(Codec::Vp9, false, 0, 1, &[5, 6, 7, 8]))
            .await
            .unwrap();
        writer.finish().await.unwrap();
        drop(writer);
        buf.truncate(buf.len() - 3);

        let mut reader = FrameReader::new(buf.as_slice(), Format::Media);
        assert!(reader.read_frame().await.unwrap().is_some());
        assert!(matches!(
            reader.read_frame().await,
            Err(WireError::Media(CodecError::Truncated))
        ));
    }

    #[tokio::test]
    async fn auto_reader_accepts_a_media_and_a_legacy_sharer() {
        let description = screen_description(Codec::Vp9, 1280, 720);
        let media_frames = vec![
            frame(Codec::Vp9, true, 0, 0, &[1, 2, 3]),
            frame(Codec::Vp9, false, 1, 1, &[4, 5]),
        ];
        let legacy_frames = vec![
            frame(Codec::Vp9, true, 0, 0, &[6, 7]),
            audio_frame(&[8, 9, 10]),
        ];
        let mut media = Vec::new();
        let mut writer = FrameWriter::new(&mut media, Format::Media);
        writer.write_description(&description).await.unwrap();
        for frame in &media_frames {
            writer.write_frame(frame).await.unwrap();
        }
        writer.finish().await.unwrap();

        let mut legacy = Vec::new();
        let mut writer = FrameWriter::new(&mut legacy, Format::Legacy(V2));
        for frame in &legacy_frames {
            writer.write_frame(frame).await.unwrap();
        }

        let mut reader = FrameReader::auto(media.as_slice());
        let mut read = Vec::new();
        while let Some(frame) = reader.read_frame().await.unwrap() {
            read.push(frame);
        }
        assert_eq!(read, media_frames);

        let mut reader = FrameReader::auto(legacy.as_slice());
        let mut read = Vec::new();
        while let Some(frame) = reader.read_frame().await.unwrap() {
            read.push(frame);
        }
        assert_eq!(read, legacy_frames);
    }

    #[tokio::test]
    async fn a_late_frame_before_the_first_description_is_dropped() {
        let description = screen_description(Codec::Vp9, 1280, 720);
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf, Format::Media);
        writer
            .write_frame(&frame(Codec::Vp9, true, 0, 0, &[1]))
            .await
            .unwrap();
        writer.write_description(&description).await.unwrap();
        writer
            .write_frame(&frame(Codec::Vp9, true, 0, 1, &[2]))
            .await
            .unwrap();
        writer.finish().await.unwrap();

        let mut reader = FrameReader::new(buf.as_slice(), Format::Media);
        let frame = reader.read_frame().await.unwrap().unwrap();
        assert_eq!(frame.payload, [2], "only the frame after the description");
        assert!(reader.read_frame().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn auto_reader_refuses_an_unknown_first_byte() {
        let bytes = [9u8];
        let mut reader = FrameReader::auto(bytes.as_slice());
        assert!(matches!(
            reader.read_frame().await,
            Err(WireError::BadVersion(9))
        ));
    }

    /// FR-8.92: a camera viewer reads only the camera session it asked for; a screen share
    /// still reads whatever its sharer announces.
    #[tokio::test]
    async fn a_camera_reader_refuses_a_description_of_another_session() {
        const ID: &str = "0123456789abcdef";
        let camera = |id: &str| {
            let mut description = screen_description(Codec::Vp9, 640, 360);
            description.kind = StreamKind::Camera;
            description.id = Some(id.to_owned());
            // A camera carries video only (FR-8.92).
            description
                .tracks
                .retain(|track| matches!(track, Track::Video(_)));
            description
        };
        async fn read(description: &Description, check: DescriptionCheck) -> Result<(), WireError> {
            let mut buf = Vec::new();
            let mut writer = FrameWriter::new(&mut buf, Format::Media);
            writer.write_description(description).await.unwrap();
            writer
                .write_frame(&frame(Codec::Vp9, true, 0, 0, &[1, 2, 3]))
                .await
                .unwrap();
            let mut reader = FrameReader::new(buf.as_slice(), Format::Media).with_check(check);
            let first = reader.read_frame().await?;
            assert!(
                first.is_some(),
                "a matching description still carries frames"
            );
            Ok(())
        }
        let check = DescriptionCheck::Camera(ID.to_owned());
        read(&camera(ID), check.clone()).await.unwrap();
        for refused in [
            camera("ffffffffffffffff"),
            // A screen share is never a camera stream, whatever id it claims.
            screen_description(Codec::Vp9, 640, 360),
        ] {
            let refused = read(&refused, check.clone()).await;
            assert!(matches!(refused, Err(WireError::Refused)));
        }
        // A screen-share viewer checks nothing.
        read(
            &screen_description(Codec::Vp9, 640, 360),
            DescriptionCheck::Any,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn the_format_switch_writes_both_formats() {
        let frames = vec![
            frame(Codec::Vp9, true, 0, 0, &[1, 2]),
            Frame {
                header: FrameHeader {
                    payload_len: 2,
                    ..audio()
                },
                payload: vec![3, 4],
            },
        ];
        for (format, prefix) in [
            (Format::Legacy(V1), 1u8),
            (Format::Legacy(V2), 2u8),
            (Format::Media, b'#'),
        ] {
            let mut buf = Vec::new();
            let mut writer = FrameWriter::new(&mut buf, format);
            if format == Format::Media {
                writer
                    .write_description(&screen_description(Codec::Vp9, 1280, 720))
                    .await
                    .unwrap();
            }
            for frame in &frames {
                writer.write_frame(frame).await.unwrap();
            }
            writer.finish().await.unwrap();
            assert_eq!(buf[0], prefix);
            let mut reader = FrameReader::new(buf.as_slice(), format);
            if format == Format::Media {
                while reader.read_frame().await.unwrap().is_some() {}
            } else {
                for expected in &frames {
                    assert_eq!(reader.read_frame().await.unwrap().as_ref(), Some(expected));
                }
            }
        }
    }

    #[tokio::test]
    async fn the_media_format_refuses_a_description_that_is_missing_or_late() {
        let mut buf = Vec::new();
        let mut writer = FrameWriter::new(&mut buf, Format::Legacy(V2));
        assert!(matches!(
            writer
                .write_description(&screen_description(Codec::Vp9, 640, 360))
                .await,
            Err(WireError::NoDescription)
        ));
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
