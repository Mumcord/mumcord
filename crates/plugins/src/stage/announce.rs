//! Stage host frames (FR-9.47): how hosts tell each other about plugin activities.
//!
//! They travel on the plugin's own data id `mumcord.p.<id>` as host frames
//! ([`envelope::encode_host`] with [`envelope::HOST_KIND_STAGE`]), so they need no
//! `peers.messages` and never reach a plugin. The body is a flat Bovnar document (FR-9.93):
//! `v` version, `t` type, `a` activity id, and per type `n` title, `m` most participants, `e` web
//! entry, `s` starter session, `p` participant sessions (octets, u32 little-endian each) and
//! `pn` plugin name. Everything received is untrusted and checked here before use.

use bovnar::{Base, DomDoc, Writer};

use crate::messaging::envelope::{self, HOST_KIND_STAGE, MAX_PAYLOAD};

const VERSION: u8 = 1;
/// Activity ids are 16 lowercase hex digits, chosen by the starter's host.
pub const ACTIVITY_ID_LEN: usize = 16;
/// The longest title (WIT `stage.start`).
pub const MAX_TITLE_CHARS: usize = 64;
/// The longest web entry carried for joiners (path and query below `web/`).
pub const MAX_ENTRY_BYTES: usize = 256;
/// The longest plugin name in a Get-plugin notice.
pub const MAX_NAME_CHARS: usize = 64;
/// No activity has more participants (FR-9.46).
pub const MAX_PARTICIPANTS: u32 = 16;

/// What a starter tells the others about an activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announced {
    pub title: String,
    pub max: u32,
    /// The web entry for an activity with web content; `None` for a tree.
    pub web: Option<String>,
}

/// One stage frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// The sender started an activity and is its only participant.
    Start { id: String, info: Announced },
    /// The full state, for someone who arrived after the start.
    State {
        id: String,
        info: Announced,
        starter: u32,
        participants: Vec<u32>,
    },
    /// The sender joined.
    Join { id: String },
    /// The sender left.
    Leave { id: String },
    /// The starter ended the activity for everyone.
    End { id: String },
    /// "<starter> is playing <name> · Get plugin" for someone without the official plugin.
    Notice {
        id: String,
        title: String,
        name: String,
    },
    /// The notice is over (the starter left or the activity ended).
    NoticeEnd { id: String },
}

impl Frame {
    pub fn id(&self) -> &str {
        match self {
            Self::Start { id, .. }
            | Self::State { id, .. }
            | Self::Join { id }
            | Self::Leave { id }
            | Self::End { id }
            | Self::Notice { id, .. }
            | Self::NoticeEnd { id } => id,
        }
    }
}

/// Whether `id` looks like an activity id.
pub fn is_activity_id(id: &str) -> bool {
    id.len() == ACTIVITY_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Whether `title` is a valid activity title: 1 to 64 characters, no control characters (Bovnar
/// strings cannot carry them, and the UI shows the title on one line).
pub fn is_title(title: &str) -> bool {
    let chars = title.chars().count();
    (1..=MAX_TITLE_CHARS).contains(&chars) && !title.chars().any(char::is_control)
}

fn is_entry(entry: &str) -> bool {
    !entry.is_empty() && entry.len() <= MAX_ENTRY_BYTES && !entry.chars().any(char::is_control)
}

/// The whole host frame for `frame` (envelope included), or `None` if a field is out of range.
pub fn encode(frame: &Frame) -> Option<Vec<u8>> {
    if !is_activity_id(frame.id()) {
        return None;
    }
    let body = write(MAX_PAYLOAD, |w| {
        w.write_uint("v", VERSION.into(), 8, Base::DECIMAL)?;
        let t = match frame {
            Frame::Start { .. } => "start",
            Frame::State { .. } => "state",
            Frame::Join { .. } => "join",
            Frame::Leave { .. } => "leave",
            Frame::End { .. } => "end",
            Frame::Notice { .. } => "notice",
            Frame::NoticeEnd { .. } => "notice-end",
        };
        w.write_string("t", t)?;
        w.write_string("a", frame.id())?;
        match frame {
            Frame::Start { info, .. } => write_info(w, info),
            Frame::State {
                info,
                starter,
                participants,
                ..
            } => {
                write_info(w, info)?;
                w.write_uint("s", (*starter).into(), 32, Base::DECIMAL)?;
                let sessions: Vec<u8> = participants.iter().flat_map(|s| s.to_le_bytes()).collect();
                w.write_octets("p", &sessions)
            }
            Frame::Notice { title, name, .. } => {
                w.write_string("n", title)?;
                let name: String = name
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(MAX_NAME_CHARS)
                    .collect();
                w.write_string("pn", &name)
            }
            Frame::Join { .. }
            | Frame::Leave { .. }
            | Frame::End { .. }
            | Frame::NoticeEnd { .. } => Ok(()),
        }
    })?;
    Some(envelope::encode_host(HOST_KIND_STAGE, &body))
}

fn write_info(w: &mut Writer, info: &Announced) -> bovnar::Result<()> {
    w.write_string("n", &info.title)?;
    w.write_uint("m", info.max.into(), 8, Base::DECIMAL)?;
    match &info.web {
        Some(entry) => w.write_string("e", entry),
        None => Ok(()),
    }
}

fn write(cap: usize, fields: impl FnOnce(&mut Writer) -> bovnar::Result<()>) -> Option<Vec<u8>> {
    let mut w = Writer::to_mem(cap, false).ok()?;
    fields(&mut w).and_then(|()| w.finish()).ok()?;
    Some(w.get_output().to_vec())
}

/// Decodes the body of a stage host frame. Oversized, malformed or invalid bodies give `None`.
pub fn decode(body: &[u8]) -> Option<Frame> {
    if body.len() > MAX_PAYLOAD {
        return None;
    }
    let doc = DomDoc::parse(body).ok()?;
    let text = |key: &str| doc.get(key).and_then(|n| n.as_str());
    if doc.get("v")?.as_u8()? < VERSION {
        return None;
    }
    let id = text("a").filter(|id| is_activity_id(id))?.to_owned();
    let info = || -> Option<Announced> {
        let title = text("n").filter(|t| is_title(t))?.to_owned();
        let max = doc.get("m")?.as_u32()?;
        let web = match doc.get("e") {
            None => None,
            Some(node) => Some(node.as_str().filter(|e| is_entry(e))?.to_owned()),
        };
        Some(Announced {
            title,
            max: max.clamp(1, MAX_PARTICIPANTS),
            web,
        })
    };
    Some(match text("t")? {
        "start" => Frame::Start { id, info: info()? },
        "state" => {
            let sessions = doc.get("p")?.as_bytes()?;
            if sessions.len() % 4 != 0 || sessions.len() / 4 > MAX_PARTICIPANTS as usize {
                return None;
            }
            Frame::State {
                id,
                info: info()?,
                starter: doc.get("s")?.as_u32()?,
                participants: sessions
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| u32::from_le_bytes(*c))
                    .collect(),
            }
        }
        "join" => Frame::Join { id },
        "leave" => Frame::Leave { id },
        "end" => Frame::End { id },
        "notice" => Frame::Notice {
            id,
            title: text("n").filter(|t| is_title(t))?.to_owned(),
            name: text("pn")
                .filter(|n| !n.is_empty() && n.chars().count() <= MAX_NAME_CHARS)
                .filter(|n| !n.chars().any(char::is_control))?
                .to_owned(),
        },
        "notice-end" => Frame::NoticeEnd { id },
        _ => return None,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef";

    fn info(web: Option<&str>) -> Announced {
        Announced {
            title: "Board Brawl".into(),
            max: 4,
            web: web.map(str::to_owned),
        }
    }

    fn body(frame: &Frame) -> Vec<u8> {
        let bytes = encode(frame).unwrap();
        let (kind, body) = envelope::decode_host(&bytes).unwrap();
        assert_eq!(kind, HOST_KIND_STAGE);
        body.to_vec()
    }

    #[test]
    fn every_frame_round_trips_as_a_bovnar_document() {
        let id = || ID.to_owned();
        for frame in [
            Frame::Start {
                id: id(),
                info: info(None),
            },
            Frame::Start {
                id: id(),
                info: info(Some("board/index.html?x=1")),
            },
            Frame::State {
                id: id(),
                info: info(None),
                starter: 7,
                participants: vec![7, 9, 70_000],
            },
            Frame::Join { id: id() },
            Frame::Leave { id: id() },
            Frame::End { id: id() },
            Frame::Notice {
                id: id(),
                title: "Board Brawl".into(),
                name: "Board Brawl".into(),
            },
            Frame::NoticeEnd { id: id() },
        ] {
            let body = body(&frame);
            assert_eq!(
                bovnar::json::detect(&body),
                bovnar::json::Format::Bovnar,
                "{frame:?}"
            );
            assert_eq!(decode(&body), Some(frame));
        }
    }

    #[test]
    fn bad_ids_and_titles_are_not_encoded() {
        let start = |id: &str, title: &str| Frame::Start {
            id: id.into(),
            info: Announced {
                title: title.into(),
                max: 2,
                web: None,
            },
        };
        assert!(encode(&start("short", "T")).is_none());
        assert!(encode(&start("0123456789ABCDEF", "T")).is_none());
        assert!(!is_title(""));
        assert!(!is_title("a\nb"));
        assert!(!is_title(&"x".repeat(65)));
        assert!(is_title(&"ü".repeat(64)));
    }

    #[test]
    fn untrusted_bodies_are_checked() {
        let doc = |text: &str| text.as_bytes().to_vec();
        for bad in [
            doc("not bovnar"),
            doc(r#".v = 1; .t = "join"; .a = "nothex";"#),
            doc(r#".v = 1; .t = "dance"; .a = "0123456789abcdef";"#),
            doc(r#".v = 1; .t = "start"; .a = "0123456789abcdef";"#),
            doc(r#".v = 0; .t = "join"; .a = "0123456789abcdef";"#),
            vec![b' '; MAX_PAYLOAD + 1],
        ] {
            assert_eq!(decode(&bad), None, "{}", String::from_utf8_lossy(&bad));
        }
    }

    #[test]
    fn the_most_participants_are_clamped_to_16() {
        let mut big = info(None);
        big.max = 200;
        let body = body(&Frame::Start {
            id: ID.into(),
            info: big,
        });
        let Some(Frame::Start { info, .. }) = decode(&body) else {
            panic!("start")
        };
        assert_eq!(info.max, MAX_PARTICIPANTS);
    }
}
