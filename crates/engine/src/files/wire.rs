//! Plugin data `mumcord.file` (section 6.4): the small messages between sender and recipient
//! while a download is requested, queued, started or cancelled. Each message is a Bovnar document
//! (FR-7.75); this module and `offer` are the only places that know the encoding.

use bovnar::{DomDoc, DomNode, Writer};

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

/// Why a received Bovnar document was refused. Logs carry this reason and the size only, never
/// payload content (FR-7.75).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Rejected {
    #[error("too-large")]
    TooLarge,
    #[error("malformed")]
    Malformed,
    #[error("wrong-type")]
    WrongType,
    #[error("invalid")]
    Invalid,
}

/// Parses a Bovnar document after checking its size, so oversized input never reaches the parser.
pub(crate) fn parse_document(data: &[u8], max: usize) -> Result<DomDoc, Rejected> {
    if data.len() > max {
        return Err(Rejected::TooLarge);
    }
    DomDoc::parse(data).map_err(|_| Rejected::Malformed)
}

/// A required field; a missing one makes the document malformed.
pub(crate) fn field<'a>(doc: &'a DomDoc, key: &str) -> Result<DomNode<'a>, Rejected> {
    doc.get(key).ok_or(Rejected::Malformed)
}

pub(crate) fn str_field<'a>(doc: &'a DomDoc, key: &str) -> Result<&'a str, Rejected> {
    field(doc, key)?.as_str().ok_or(Rejected::WrongType)
}

pub(crate) fn u64_field(doc: &DomDoc, key: &str) -> Result<u64, Rejected> {
    field(doc, key)?.as_u64().ok_or(Rejected::WrongType)
}

/// The format version; any version from `min` on is read, unknown fields are ignored.
pub(crate) fn version_field(doc: &DomDoc, min: u8) -> Result<(), Rejected> {
    let v = field(doc, "v")?.as_u8().ok_or(Rejected::WrongType)?;
    if v < min {
        return Err(Rejected::Invalid);
    }
    Ok(())
}

/// Writes a flat Bovnar document of at most `cap` bytes with the fields `write` adds.
pub(crate) fn write_document(
    cap: usize,
    write: impl FnOnce(&mut Writer) -> bovnar::Result<()>,
) -> Option<Vec<u8>> {
    let mut w = Writer::to_mem(cap, false).ok()?;
    write(&mut w).and_then(|()| w.finish()).ok()?;
    Some(w.get_output().to_vec())
}

/// Encodes a message as a Bovnar document; `None` if the share id is not valid.
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
    write_document(MAX_PAYLOAD_BYTES, |w| {
        w.write_uint("v", VERSION.into(), 8, bovnar::Base::DECIMAL)?;
        w.write_string("t", t)?;
        w.write_string("id", message.id())?;
        if let Some(pos) = pos {
            w.write_uint("pos", pos.into(), 16, bovnar::Base::DECIMAL)?;
        }
        Ok(())
    })
}

/// Decodes received plugin data. Oversized, malformed, unknown or invalid messages give `None`.
pub fn decode(data: &[u8]) -> Option<FileMessage> {
    decode_checked(data).ok()
}

fn decode_checked(data: &[u8]) -> Result<FileMessage, Rejected> {
    let doc = parse_document(data, MAX_PAYLOAD_BYTES)?;
    version_field(&doc, VERSION)?;
    let id = str_field(&doc, "id")?;
    if !is_share_id(id) {
        return Err(Rejected::Invalid);
    }
    let id = id.to_owned();
    Ok(match str_field(&doc, "t")? {
        "req" => FileMessage::Request { id },
        "queued" => FileMessage::Queued {
            id,
            position: field(&doc, "pos")?.as_u16().ok_or(Rejected::WrongType)?,
        },
        "go" => FileMessage::Go { id },
        "busy" => FileMessage::Busy { id },
        "gone" => FileMessage::Gone { id },
        "cancel" => FileMessage::Cancel { id },
        _ => return Err(Rejected::Invalid),
    })
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

    fn doc(fields: &str) -> Vec<u8> {
        fields.replace("ID", ID).into_bytes()
    }

    #[test]
    fn every_message_round_trips_within_the_limit() {
        for m in all() {
            let bytes = encode(&m).expect("valid");
            assert!(bytes.len() <= MAX_PAYLOAD_BYTES);
            assert_eq!(bovnar::json::detect(&bytes), bovnar::json::Format::Bovnar);
            assert_eq!(decode(&bytes), Some(m));
        }
    }

    #[test]
    fn encoded_fields_are_typed() {
        let bytes = encode(&FileMessage::Queued {
            id: ID.into(),
            position: 2,
        })
        .expect("valid");
        let doc = DomDoc::parse(&bytes).expect("bovnar");
        assert_eq!(doc.get("v").and_then(DomNode::as_u8), Some(1));
        assert_eq!(doc.get("t").and_then(DomNode::as_str), Some("queued"));
        assert_eq!(doc.get("id").and_then(DomNode::as_str), Some(ID));
        assert_eq!(doc.get("pos").and_then(DomNode::as_u16), Some(2));
    }

    #[test]
    fn queued_needs_a_position() {
        let missing = doc(r#".v = 1; .t = "queued"; .id = "ID";"#);
        assert_eq!(decode_checked(&missing), Err(Rejected::Malformed));
        let too_big = doc(r#".v = 1; .t = "queued"; .id = "ID"; .pos = 70000;"#);
        assert_eq!(decode_checked(&too_big), Err(Rejected::WrongType));
    }

    #[test]
    fn invalid_data_is_dropped() {
        let bad = [
            (
                r#".v = 0; .t = "go"; .id = "ID";"#.to_owned(),
                Rejected::Invalid,
            ),
            (
                r#".v = 1; .t = "nope"; .id = "ID";"#.to_owned(),
                Rejected::Invalid,
            ),
            (
                r#".v = 1; .t = "go"; .id = "abc";"#.to_owned(),
                Rejected::Invalid,
            ),
            (
                format!(r#".v = 1; .t = "go"; .id = "{}";"#, ID.to_uppercase()),
                Rejected::Invalid,
            ),
            (r#".v = 1; .t = "go";"#.to_owned(), Rejected::Malformed),
            (
                r#".v = "1"; .t = "go"; .id = "ID";"#.to_owned(),
                Rejected::WrongType,
            ),
            (
                r#".v = 1; .t = 5; .id = "ID";"#.to_owned(),
                Rejected::WrongType,
            ),
            (
                r#".v = 1; .t = "go"; .id = "ID""#.to_owned(),
                Rejected::Malformed,
            ),
            (
                r#"{"v":1,"t":"go","id":"ID"}"#.to_owned(),
                Rejected::Malformed,
            ),
            (String::new(), Rejected::Malformed),
        ];
        for (text, reason) in bad {
            assert_eq!(decode_checked(&doc(&text)), Err(reason), "{text}");
        }
        assert_eq!(decode(&[0xff, 0xfe, 0x00]), None);
    }

    #[test]
    fn truncated_documents_are_dropped() {
        let bytes = encode(&FileMessage::Go { id: ID.into() }).expect("valid");
        for len in 0..bytes.len() {
            assert_eq!(decode(&bytes[..len]), None, "{len}");
        }
    }

    #[test]
    fn higher_versions_and_unknown_fields_are_accepted() {
        let text = doc(r#".v = 4; .t = "go"; .id = "ID"; .extra = [1, 2];"#);
        assert_eq!(decode(&text), Some(FileMessage::Go { id: ID.into() }));
    }

    #[test]
    fn oversized_payloads_are_dropped() {
        let pad = " ".repeat(MAX_PAYLOAD_BYTES);
        let text = doc(&format!(r#".v = 1; .t = "go"; .id = "ID";{pad}"#));
        assert_eq!(decode_checked(&text), Err(Rejected::TooLarge));
    }

    #[test]
    fn encoding_refuses_a_bad_id() {
        assert_eq!(encode(&FileMessage::Go { id: "x".into() }), None);
    }
}
