//! TCP control-channel framing: every message is a 2-byte big-endian type, a 4-byte big-endian
//! payload length and the payload (a protobuf message, or a raw voice packet for `UdpTunnel`).

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::error::ProtocolError;

/// Length of the frame header (type + length).
pub const HEADER_LEN: usize = 6;

/// Largest payload accepted from the server. Mumble's own client uses the same 8 MiB limit;
/// anything larger is treated as a broken stream rather than buffered.
pub const MAX_PAYLOAD_LEN: usize = 8 * 1024 * 1024 - 1;

/// Control-channel message types, numbered as in the Mumble protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MessageType {
    Version = 0,
    UdpTunnel = 1,
    Authenticate = 2,
    Ping = 3,
    Reject = 4,
    ServerSync = 5,
    ChannelRemove = 6,
    ChannelState = 7,
    UserRemove = 8,
    UserState = 9,
    BanList = 10,
    TextMessage = 11,
    PermissionDenied = 12,
    Acl = 13,
    QueryUsers = 14,
    CryptSetup = 15,
    ContextActionModify = 16,
    ContextAction = 17,
    UserList = 18,
    VoiceTarget = 19,
    PermissionQuery = 20,
    CodecVersion = 21,
    UserStats = 22,
    RequestBlob = 23,
    ServerConfig = 24,
    SuggestConfig = 25,
    PluginDataTransmission = 26,
}

impl MessageType {
    /// Maps a wire type number; `None` for types this client does not know.
    pub fn from_u16(value: u16) -> Option<Self> {
        use MessageType::*;
        const ALL: [MessageType; 27] = [
            Version,
            UdpTunnel,
            Authenticate,
            Ping,
            Reject,
            ServerSync,
            ChannelRemove,
            ChannelState,
            UserRemove,
            UserState,
            BanList,
            TextMessage,
            PermissionDenied,
            Acl,
            QueryUsers,
            CryptSetup,
            ContextActionModify,
            ContextAction,
            UserList,
            VoiceTarget,
            PermissionQuery,
            CodecVersion,
            UserStats,
            RequestBlob,
            ServerConfig,
            SuggestConfig,
            PluginDataTransmission,
        ];
        ALL.get(usize::from(value)).copied()
    }
}

/// One undecoded control-channel message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Wire type number (kept raw so unknown types can be skipped).
    pub kind: u16,
    /// Message payload.
    pub payload: Bytes,
}

impl Frame {
    /// Encodes a protobuf message into a frame.
    pub fn from_message<M: prost::Message>(kind: MessageType, message: &M) -> Self {
        Self {
            kind: kind as u16,
            payload: Bytes::from(message.encode_to_vec()),
        }
    }

    /// The known message type, if any.
    pub fn message_type(&self) -> Option<MessageType> {
        MessageType::from_u16(self.kind)
    }

    /// Serializes header and payload for the wire.
    pub fn encode(&self, out: &mut BytesMut) {
        out.reserve(HEADER_LEN + self.payload.len());
        out.put_u16(self.kind);
        // Outgoing payloads are built by this crate and far below 4 GiB.
        out.put_u32(self.payload.len() as u32);
        out.put_slice(&self.payload);
    }
}

/// Takes one complete frame off the front of `buf`, or returns `None` if more bytes are needed.
pub fn decode(buf: &mut BytesMut) -> Result<Option<Frame>, ProtocolError> {
    if buf.len() < HEADER_LEN {
        return Ok(None);
    }
    let kind = u16::from_be_bytes([buf[0], buf[1]]);
    let len = u32::from_be_bytes([buf[2], buf[3], buf[4], buf[5]]) as usize;
    if len > MAX_PAYLOAD_LEN {
        return Err(ProtocolError::FrameTooLarge(len));
    }
    if buf.len() < HEADER_LEN + len {
        buf.reserve(HEADER_LEN + len - buf.len());
        return Ok(None);
    }
    buf.advance(HEADER_LEN);
    let payload = buf.split_to(len).freeze();
    Ok(Some(Frame { kind, payload }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto;

    fn encoded(frame: &Frame) -> BytesMut {
        let mut out = BytesMut::new();
        frame.encode(&mut out);
        out
    }

    #[test]
    fn round_trip_protobuf_message() {
        let ping = proto::Ping {
            timestamp: Some(42),
            ..Default::default()
        };
        let frame = Frame::from_message(MessageType::Ping, &ping);
        let mut buf = encoded(&frame);
        assert_eq!(
            &buf[..HEADER_LEN],
            &[0, 3, 0, 0, 0, frame.payload.len() as u8]
        );
        let decoded = decode(&mut buf).expect("valid").expect("complete");
        assert_eq!(decoded, frame);
        assert_eq!(decoded.message_type(), Some(MessageType::Ping));
        let back: proto::Ping = prost::Message::decode(decoded.payload).expect("protobuf");
        assert_eq!(back.timestamp, Some(42));
        assert!(buf.is_empty());
    }

    #[test]
    fn partial_frames_wait_for_more_bytes() {
        let frame = Frame {
            kind: 1,
            payload: Bytes::from_static(b"voice"),
        };
        let full = encoded(&frame);
        let mut buf = BytesMut::new();
        for (i, byte) in full.iter().enumerate() {
            buf.put_u8(*byte);
            let result = decode(&mut buf).expect("valid");
            if i + 1 < full.len() {
                assert!(result.is_none());
            } else {
                assert_eq!(result, Some(frame.clone()));
            }
        }
    }

    #[test]
    fn several_frames_in_one_buffer() {
        let a = Frame {
            kind: 3,
            payload: Bytes::new(),
        };
        let b = Frame {
            kind: 11,
            payload: Bytes::from_static(b"hi"),
        };
        let mut buf = encoded(&a);
        buf.extend_from_slice(&encoded(&b));
        assert_eq!(decode(&mut buf).expect("valid"), Some(a));
        assert_eq!(decode(&mut buf).expect("valid"), Some(b));
        assert_eq!(decode(&mut buf).expect("valid"), None);
    }

    #[test]
    fn oversized_frame_is_rejected() {
        let mut buf = BytesMut::new();
        buf.put_u16(7);
        buf.put_u32(MAX_PAYLOAD_LEN as u32 + 1);
        assert!(matches!(
            decode(&mut buf),
            Err(ProtocolError::FrameTooLarge(_))
        ));
    }

    #[test]
    fn unknown_type_is_kept_raw() {
        let mut buf = encoded(&Frame {
            kind: 999,
            payload: Bytes::new(),
        });
        let frame = decode(&mut buf).expect("valid").expect("complete");
        assert_eq!(frame.kind, 999);
        assert_eq!(frame.message_type(), None);
        assert_eq!(
            MessageType::from_u16(26),
            Some(MessageType::PluginDataTransmission)
        );
    }
}
