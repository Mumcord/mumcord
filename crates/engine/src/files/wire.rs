//! Plugin data `mumcord.file` (section 6.4): the small messages between sender and recipient
//! while a download is requested, queued, started or cancelled.

use serde::{Deserialize, Serialize};

use super::is_share_id;

/// Plugin data id of these messages.
pub const DATA_ID: &str = "mumcord.file";

/// Largest payload, in bytes.
pub const MAX_PAYLOAD_BYTES: usize = 1000;

const VERSION: u8 = 1;

/// One `mumcord.file` message. The sender is the session the server stamped on the data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileMessage {
    /// Recipient to sender: please send the file.
    Request { id: String },
    /// Sender to recipient: waiting in line; 1 = next.
    Queued { id: String, position: u16 },
    /// Sender to recipient: start now.
    Go { id: String },
    /// Sender to recipient: the queue is full.
    Busy { id: String },
    /// Sender to recipient: share ended, unknown or refused.
    Gone { id: String },
    /// Recipient to sender: not wanted any more.
    Cancel { id: String },
}

impl FileMessage {
    pub fn id(&self) -> &str {
        match self {
            Self::Request { id }
            | Self::Queued { id, .. }
            | Self::Go { id }
            | Self::Busy { id }
            | Self::Gone { id }
            | Self::Cancel { id } => id,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Wire {
    v: u8,
    t: String,
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pos: Option<u16>,
}

/// Encodes a message as compact JSON; `None` if the share id is not valid.
pub fn encode(message: &FileMessage) -> Option<Vec<u8>> {
    if !is_share_id(message.id()) {
        return None;
    }
    let (t, pos) = match message {
        FileMessage::Request { .. } => ("req", None),
        FileMessage::Queued { position, .. } => ("queued", Some(*position)),
        FileMessage::Go { .. } => ("go", None),
        FileMessage::Busy { .. } => ("busy", None),
        FileMessage::Gone { .. } => ("gone", None),
        FileMessage::Cancel { .. } => ("cancel", None),
    };
    let wire = Wire {
        v: VERSION,
        t: t.to_owned(),
        id: message.id().to_owned(),
        pos,
    };
    serde_json::to_vec(&wire).ok()
}

/// Decodes received plugin data. Oversized, malformed, unknown or invalid messages give `None`.
pub fn decode(data: &[u8]) -> Option<FileMessage> {
    if data.len() > MAX_PAYLOAD_BYTES {
        return None;
    }
    let wire: Wire = serde_json::from_slice(data).ok()?;
    if wire.v < VERSION || !is_share_id(&wire.id) {
        return None;
    }
    let id = wire.id;
    match wire.t.as_str() {
        "req" => Some(FileMessage::Request { id }),
        "queued" => Some(FileMessage::Queued {
            id,
            position: wire.pos?,
        }),
        "go" => Some(FileMessage::Go { id }),
        "busy" => Some(FileMessage::Busy { id }),
        "gone" => Some(FileMessage::Gone { id }),
        "cancel" => Some(FileMessage::Cancel { id }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef";

    fn all() -> Vec<FileMessage> {
        let id = || ID.to_owned();
        vec![
            FileMessage::Request { id: id() },
            FileMessage::Queued {
                id: id(),
                position: 3,
            },
            FileMessage::Go { id: id() },
            FileMessage::Busy { id: id() },
            FileMessage::Gone { id: id() },
            FileMessage::Cancel { id: id() },
        ]
    }

    #[test]
    fn every_message_round_trips_within_the_limit() {
        for m in all() {
            let bytes = encode(&m).expect("valid");
            assert!(bytes.len() <= MAX_PAYLOAD_BYTES);
            assert_eq!(decode(&bytes), Some(m));
        }
    }

    #[test]
    fn queued_needs_a_position() {
        let json = format!(r#"{{"v":1,"t":"queued","id":"{ID}"}}"#);
        assert_eq!(decode(json.as_bytes()), None);
        let json = format!(r#"{{"v":1,"t":"queued","id":"{ID}","pos":70000}}"#);
        assert_eq!(decode(json.as_bytes()), None);
    }

    #[test]
    fn invalid_data_is_dropped() {
        let bad = [
            format!(r#"{{"v":0,"t":"go","id":"{ID}"}}"#),
            format!(r#"{{"v":1,"t":"nope","id":"{ID}"}}"#),
            r#"{"v":1,"t":"go","id":"abc"}"#.to_owned(),
            format!(r#"{{"v":1,"t":"go","id":"{}"}}"#, ID.to_uppercase()),
            r#"{"v":1,"t":"go"}"#.to_owned(),
            "[]".to_owned(),
            String::new(),
        ];
        for json in bad {
            assert_eq!(decode(json.as_bytes()), None, "{json}");
        }
        assert_eq!(decode(&[0xff, 0xfe]), None);
    }

    #[test]
    fn higher_versions_and_unknown_fields_are_accepted() {
        let json = format!(r#"{{"v":4,"t":"go","id":"{ID}","extra":[1]}}"#);
        assert_eq!(
            decode(json.as_bytes()),
            Some(FileMessage::Go { id: ID.into() })
        );
    }

    #[test]
    fn oversized_payloads_are_dropped() {
        let pad = " ".repeat(MAX_PAYLOAD_BYTES);
        let json = format!(r#"{{"v":1,"t":"go","id":"{ID}"}}{pad}"#);
        assert_eq!(decode(json.as_bytes()), None);
    }

    #[test]
    fn encoding_refuses_a_bad_id() {
        assert_eq!(encode(&FileMessage::Go { id: "x".into() }), None);
    }
}
