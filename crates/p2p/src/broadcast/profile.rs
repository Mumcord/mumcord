//! The Bovnar media profile on a relay broadcast link (FR-7.89 … FR-7.94).
//!
//! A new-format broadcast link carries the same endless `.media` document as a direct stream,
//! but the relay routes it by channel: channel 0 holds control and the stream description,
//! channels 1–4 the video temporal layers, 5 the share sound and 6 plugin data. This module is
//! the p2p side's message codec: it checks the channel and size limits and turns one message
//! into the mux chunks of [`crate::media::MediaWriter`], or back.
//!
//! The legacy 14-byte header is not part of this format. It stays in [`super::legacy`] for old
//! links, and no new link may emit it (FR-7.98).
//!
//! Everything read comes from the network and is untrusted: a message outside the profile is
//! refused before its payload is exposed.

use crate::media::{
    self, ChannelUse, CodecError, ControlKind, MAX_DESCRIPTION, MediaError, MediaReader,
    MediaWriter,
};

/// One message of a new-format broadcast link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// A channel-0 control message or the stream description.
    Control {
        kind: ControlKind,
        /// Everything after the kind byte.
        payload: Vec<u8>,
    },
    /// A track message on a video layer, the audio or the plugin data channel.
    Track {
        channel: u16,
        /// The flags byte, timestamp and encrypted body, exactly as the publisher sealed them.
        payload: Vec<u8>,
    },
}

impl Message {
    /// A control message; the description keeps its 8 KiB cap, and every control message needs
    /// at least its kind byte on the wire.
    pub fn control(kind: ControlKind, payload: Vec<u8>) -> Result<Self, MediaError> {
        if kind == ControlKind::Description && payload.len() > MAX_DESCRIPTION {
            return Err(MediaError::TooLarge);
        }
        Ok(Self::Control { kind, payload })
    }

    /// A track message on a channel that carries media or plugin data.
    pub fn track(channel: u16, payload: Vec<u8>) -> Result<Self, MediaError> {
        match media::check_message(channel, payload.len())? {
            ChannelUse::VideoLayer(_) | ChannelUse::Audio | ChannelUse::PluginData => {
                Ok(Self::Track { channel, payload })
            }
            ChannelUse::Control | ChannelUse::Reserved => Err(MediaError::Invalid("channel")),
        }
    }

    /// The channel this message travels on.
    pub const fn channel(&self) -> u16 {
        match self {
            Self::Control { .. } => media::CHANNEL_CONTROL,
            Self::Track { channel, .. } => *channel,
        }
    }

    /// The control kind, for a channel-0 message.
    pub const fn control_kind(&self) -> Option<ControlKind> {
        match self {
            Self::Control { kind, .. } => Some(*kind),
            Self::Track { .. } => None,
        }
    }

    /// Turns one decoded mux message into a profile message; the payload moves without a copy.
    fn from_parts(channel: u16, payload: Vec<u8>) -> Result<Self, MediaError> {
        match media::channel_use(channel) {
            Some(ChannelUse::Control) => {
                let (&kind, body) = payload.split_first().ok_or(MediaError::Malformed)?;
                let kind = ControlKind::from_byte(kind).ok_or(MediaError::Invalid("kind"))?;
                Self::control(kind, body.to_vec())
            }
            Some(ChannelUse::VideoLayer(_) | ChannelUse::Audio | ChannelUse::PluginData) => {
                Self::track(channel, payload)
            }
            Some(ChannelUse::Reserved) | None => Err(MediaError::Invalid("channel")),
        }
    }
}

/// Encodes new-format messages into the mux chunks of one broadcast link.
pub struct MessageWriter {
    mux: MediaWriter,
    /// The kind byte of a control message before the mux framing.
    body: Vec<u8>,
    /// The current message's chunks.
    wire: Vec<u8>,
}

impl MessageWriter {
    pub fn new() -> Self {
        Self {
            mux: MediaWriter::new(),
            body: Vec::new(),
            wire: Vec::new(),
        }
    }

    /// The bytes that open the link's document; write them once before the first message.
    pub fn opening(&self) -> &[u8] {
        self.mux.opening()
    }

    /// The bytes that close the document; write them once when the link ends.
    pub fn terminator(&self) -> &'static [u8] {
        self.mux.terminator()
    }

    /// Encodes `message` and returns its chunk bytes, valid until the next call.
    pub fn encode(&mut self, message: &Message) -> Result<&[u8], MediaError> {
        self.wire.clear();
        match message {
            Message::Control { kind, payload } => {
                self.body.clear();
                self.body.push(kind.to_byte());
                self.body.extend_from_slice(payload);
                self.mux
                    .write_message(&mut self.wire, media::CHANNEL_CONTROL, &self.body)?;
            }
            Message::Track { channel, payload } => {
                self.mux.write_message(&mut self.wire, *channel, payload)?;
            }
        }
        Ok(&self.wire)
    }
}

impl Default for MessageWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads new-format messages from the bytes of one broadcast link.
///
/// The transport feeds bytes in whatever pieces it has; a message can be split anywhere. A
/// message outside the profile is skipped and reported as [`CodecError::Dropped`], and the link
/// goes on; a broken document is fatal.
pub struct MessageReader {
    mux: MediaReader,
    buffer: Vec<u8>,
    fatal: Option<CodecError>,
}

impl MessageReader {
    pub fn new() -> Self {
        Self {
            mux: MediaReader::new(),
            buffer: Vec::new(),
            fatal: None,
        }
    }

    /// Adds the bytes of one transport piece.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// The next complete message, or `None` once every pushed byte has been taken.
    pub fn next_message(&mut self) -> Result<Option<Message>, CodecError> {
        if let Some(error) = self.fatal {
            return Err(error);
        }
        loop {
            let (consumed, step) = {
                let mut input = &self.buffer[..];
                let result = self.mux.read(&mut input);
                let consumed = self.buffer.len() - input.len();
                let step = match result {
                    Ok(Some(message)) => Step::Message(message.channel, message.payload.to_vec()),
                    Ok(None) => Step::None,
                    Err(CodecError::Dropped(error)) => {
                        tracing::debug!(%error, "dropping a broadcast message outside the profile");
                        Step::Dropped
                    }
                    Err(error) => {
                        self.fatal = Some(error);
                        return Err(error);
                    }
                };
                (consumed, step)
            };
            if consumed > 0 {
                self.buffer.drain(..consumed);
            }
            match step {
                Step::Message(channel, payload) => match Message::from_parts(channel, payload) {
                    Ok(message) => return Ok(Some(message)),
                    Err(error) => {
                        tracing::debug!(%error, "dropping a broadcast message outside the profile");
                    }
                },
                Step::Dropped if consumed == 0 => return Ok(None),
                Step::Dropped => {}
                Step::None => return Ok(None),
            }
        }
    }

    /// Checks that the link ended at a message boundary; call it when the transport ends.
    pub fn finish(&mut self) -> Result<(), CodecError> {
        if let Some(error) = self.fatal {
            return Err(error);
        }
        let result = self.mux.finish();
        if let Err(error) = result {
            self.fatal = Some(error);
        }
        result
    }
}

impl Default for MessageReader {
    fn default() -> Self {
        Self::new()
    }
}

enum Step {
    Message(u16, Vec<u8>),
    Dropped,
    None,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::MAX_VIDEO_MESSAGE;

    fn messages() -> Vec<Message> {
        let description = b".v = 1; .timebase = 1; .kind = \"screen\"; .tracks = [];".to_vec();
        vec![
            Message::control(ControlKind::Description, description).expect("description"),
            Message::control(ControlKind::Keyframe, Vec::new()).expect("keyframe"),
            Message::track(media::CHANNEL_VIDEO_BASE + 1, vec![0x00, 1, 2, 3, 4]).expect("video"),
            Message::track(media::CHANNEL_AUDIO, vec![0x00, 5, 6, 7, 8]).expect("audio"),
            Message::track(media::CHANNEL_PLUGIN_DATA, vec![9, 10, 11]).expect("plugin"),
        ]
    }

    /// The whole link as one byte vector, with the document opening and terminator.
    fn document(messages: &[Message]) -> Vec<u8> {
        let mut writer = MessageWriter::new();
        let mut out = writer.opening().to_vec();
        for message in messages {
            out.extend_from_slice(writer.encode(message).expect("message"));
        }
        out.extend_from_slice(writer.terminator());
        out
    }

    #[test]
    fn a_new_link_carries_the_media_document_without_the_legacy_header() {
        let messages = messages();
        let document = document(&messages);
        assert!(document.starts_with(b"#!bovnar 1.2\n.media=\0"));
        assert!(document.ends_with(b"\0;"));
        // A legacy unit would start with header version 1 and its kind byte, never with '#'.
        assert_ne!(document[0], super::super::HEADER_VERSION);
        assert_eq!(document[0], b'#');
    }

    #[test]
    fn every_message_round_trips_whole_and_split_at_every_byte() {
        let messages = messages();
        let document = document(&messages);
        for piece in [document.len(), 1, 2, 7, 64, 1024] {
            let mut reader = MessageReader::new();
            let mut read = Vec::new();
            for part in document.chunks(piece) {
                reader.push(part);
                while let Some(message) = reader.next_message().expect("stream") {
                    read.push(message);
                }
            }
            reader.finish().expect("the document ends cleanly");
            assert_eq!(read, messages, "feeding {piece} bytes at a time");
        }
    }

    #[test]
    fn controls_keep_their_kind_and_payload() {
        for kind in [
            ControlKind::Description,
            ControlKind::KeyRequest,
            ControlKind::Key,
            ControlKind::Keyframe,
            ControlKind::Layers,
            ControlKind::Want,
            ControlKind::Gone,
        ] {
            let message = Message::control(kind, vec![1, 2, 3]).expect("control");
            assert_eq!(message.channel(), media::CHANNEL_CONTROL);
            assert_eq!(message.control_kind(), Some(kind));
            let mut writer = MessageWriter::new();
            let mut reader = MessageReader::new();
            reader.push(writer.opening());
            reader.push(writer.encode(&message).expect("encode"));
            assert_eq!(reader.next_message().expect("read"), Some(message));
        }
    }

    #[test]
    fn channels_and_limits_are_checked_before_writing() {
        assert!(Message::track(media::CHANNEL_CONTROL, vec![0]).is_err());
        assert!(Message::track(7, vec![0]).is_err());
        assert!(
            Message::track(media::CHANNEL_AUDIO, vec![0; media::MAX_AUDIO_MESSAGE + 1]).is_err()
        );
        assert!(
            Message::track(
                media::CHANNEL_PLUGIN_DATA,
                vec![0; media::MAX_PLUGIN_MESSAGE + 1]
            )
            .is_err()
        );
        assert!(Message::track(media::CHANNEL_VIDEO_BASE, vec![0; MAX_VIDEO_MESSAGE + 1]).is_err());
        assert!(Message::control(ControlKind::Description, vec![0; MAX_DESCRIPTION + 1]).is_err());
    }

    #[test]
    fn a_message_outside_the_profile_is_skipped_and_the_link_goes_on() {
        use bovnar::codec::MuxWriter;

        let mut writer = MessageWriter::new();
        let mut document = Vec::new();
        let mux = MuxWriter::with_preamble(b"#!bovnar 1.2\n", "media").expect("opening");
        mux.begin(&mut document);
        // Channel 7 is reserved: the reader drops the message without exposing it.
        mux.write_message(&mut document, 7, b"hidden");
        let good = Message::track(media::CHANNEL_VIDEO_BASE, vec![0, 1, 2]).expect("video");
        document.extend_from_slice(writer.encode(&good).expect("encode"));

        let mut reader = MessageReader::new();
        reader.push(&document);
        assert_eq!(reader.next_message().expect("stream"), Some(good));
        assert_eq!(reader.next_message().expect("stream"), None);
    }

    #[test]
    fn a_stream_cut_inside_a_message_is_reported() {
        let document = document(&messages());
        let mut reader = MessageReader::new();
        reader.push(&document[..document.len() - 3]);
        while reader.next_message().expect("stream").is_some() {}
        assert!(matches!(reader.finish(), Err(CodecError::Truncated)));
    }
}
