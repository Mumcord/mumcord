//! The binary protocol (version [`VERSION`]).
//!
//! Every message is a frame: a little-endian `u16` payload length (at most [`MAX_PAYLOAD`])
//! followed by the payload, whose first byte is the message type. Strings are UTF-8 with a `u8`
//! byte length (longer ones are cut at a character boundary when encoding). Decoding checks every
//! length and rejects anything malformed or unknown, so either side can drop a bad peer.
//!
//! | Type | Message     | Direction        | Payload after the type byte |
//! | ---- | ----------- | ---------------- | --------------------------- |
//! | 1    | `Hello`     | game → Mumcord   | magic `MCOV`, `u16` version, `u8` api, `u32` pid, `u32` Steam app id (0 = none), str executable |
//! | 2    | `Config`    | Mumcord → game   | `u8` enabled, `u8` corner, `u8` size %, `u8` opacity %, `u8` flags (bit 0 whole channel, bit 1 channel name) |
//! | 3    | `Scene`     | Mumcord → game   | str channel name, `u8` count (≤ [`MAX_PEOPLE`]), per person: `u8` flags (bit 0 talking, bit 1 muted, bit 2 deafened, bit 3 self), str name |
//! | 4    | `Heartbeat` | Mumcord → game   | – |

/// Protocol version; bumped on any incompatible change. Mumcord answers a different version
/// with a disabled [`Config`].
pub const VERSION: u16 = 1;
/// Largest payload of one frame.
pub const MAX_PAYLOAD: usize = 8 * 1024;
/// Most people one scene lists.
pub const MAX_PEOPLE: usize = 16;

const MAGIC: &[u8; 4] = b"MCOV";
const TYPE_HELLO: u8 = 1;
const TYPE_CONFIG: u8 = 2;
const TYPE_SCENE: u8 = 3;
const TYPE_HEARTBEAT: u8 = 4;

/// The graphics API the game draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    Vulkan,
    OpenGl,
    D3d11,
    D3d12,
}

impl Api {
    fn code(self) -> u8 {
        match self {
            Api::Vulkan => 1,
            Api::OpenGl => 2,
            Api::D3d11 => 3,
            Api::D3d12 => 4,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            1 => Api::Vulkan,
            2 => Api::OpenGl,
            3 => Api::D3d11,
            4 => Api::D3d12,
            _ => return None,
        })
    }
}

/// A game that loaded the overlay introduces itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub version: u16,
    pub api: Api,
    pub pid: u32,
    /// From Steam's `SteamAppId` / `SteamGameId` environment, `None` outside Steam.
    pub steam_app_id: Option<u32>,
    /// Executable file name, e.g. `cs2` or `Game-Win64-Shipping.exe` (Proton).
    pub exe: String,
}

/// Screen corner the overlay sits in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Corner {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    fn code(self) -> u8 {
        match self {
            Corner::TopLeft => 0,
            Corner::TopRight => 1,
            Corner::BottomLeft => 2,
            Corner::BottomRight => 3,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => Corner::TopLeft,
            1 => Corner::TopRight,
            2 => Corner::BottomLeft,
            3 => Corner::BottomRight,
            _ => return None,
        })
    }
}

/// Mumcord's decision and options for one game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Draw at all. False: the game side draws nothing and Mumcord sends no scenes.
    pub enabled: bool,
    pub corner: Corner,
    /// Size in percent of the default (50–200).
    pub size_percent: u8,
    /// Opacity in percent (20–100).
    pub opacity_percent: u8,
    /// List everyone in the own channel, not only who is talking.
    pub show_whole_channel: bool,
    pub show_channel_name: bool,
}

impl Config {
    /// "Draw nothing".
    pub const DISABLED: Config = Config {
        enabled: false,
        corner: Corner::TopLeft,
        size_percent: 100,
        opacity_percent: 90,
        show_whole_channel: false,
        show_channel_name: true,
    };
}

/// One person shown in the overlay. `name` is plain text from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub name: String,
    pub talking: bool,
    pub muted: bool,
    pub deafened: bool,
    pub is_self: bool,
}

/// What the overlay shows right now. Empty `people`: nothing to draw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scene {
    /// Own channel name, empty when it should not be shown.
    pub channel: String,
    pub people: Vec<Person>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Hello(Hello),
    Config(Config),
    Scene(Scene),
    Heartbeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The payload ended early or has bytes left over.
    Length,
    /// Unknown message type, bad magic, unknown enum value or invalid UTF-8.
    Invalid,
    /// A frame header announced more than [`MAX_PAYLOAD`] bytes.
    TooLarge,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DecodeError::Length => "wrong message length",
            DecodeError::Invalid => "invalid message",
            DecodeError::TooLarge => "message too large",
        })
    }
}

impl std::error::Error for DecodeError {}

impl Message {
    /// The complete frame (length header and payload).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![0, 0];
        match self {
            Message::Hello(h) => {
                out.push(TYPE_HELLO);
                out.extend_from_slice(MAGIC);
                out.extend_from_slice(&h.version.to_le_bytes());
                out.push(h.api.code());
                out.extend_from_slice(&h.pid.to_le_bytes());
                out.extend_from_slice(&h.steam_app_id.unwrap_or(0).to_le_bytes());
                put_str(&mut out, &h.exe);
            }
            Message::Config(c) => {
                out.push(TYPE_CONFIG);
                out.push(u8::from(c.enabled));
                out.push(c.corner.code());
                out.push(c.size_percent);
                out.push(c.opacity_percent);
                out.push(u8::from(c.show_whole_channel) | (u8::from(c.show_channel_name) << 1));
            }
            Message::Scene(s) => {
                out.push(TYPE_SCENE);
                put_str(&mut out, &s.channel);
                let people = &s.people[..s.people.len().min(MAX_PEOPLE)];
                // At most MAX_PEOPLE (16), fits a u8.
                out.push(u8::try_from(people.len()).unwrap_or(0));
                for p in people {
                    out.push(
                        u8::from(p.talking)
                            | (u8::from(p.muted) << 1)
                            | (u8::from(p.deafened) << 2)
                            | (u8::from(p.is_self) << 3),
                    );
                    put_str(&mut out, &p.name);
                }
            }
            Message::Heartbeat => out.push(TYPE_HEARTBEAT),
        }
        // 1 + 17 × 256 bytes at most: always below MAX_PAYLOAD, so the length fits a u16.
        let len = u16::try_from(out.len() - 2).unwrap_or(u16::MAX);
        out[..2].copy_from_slice(&len.to_le_bytes());
        out
    }

    /// Decodes one payload (the bytes after the frame header).
    pub fn decode(payload: &[u8]) -> Result<Message, DecodeError> {
        let mut r = Reader(payload);
        let message = match r.u8()? {
            TYPE_HELLO => {
                if r.take(4)? != MAGIC {
                    return Err(DecodeError::Invalid);
                }
                let version = r.u16()?;
                let api = Api::from_code(r.u8()?).ok_or(DecodeError::Invalid)?;
                let pid = r.u32()?;
                let steam = r.u32()?;
                let exe = r.str()?;
                Message::Hello(Hello {
                    version,
                    api,
                    pid,
                    steam_app_id: (steam != 0).then_some(steam),
                    exe,
                })
            }
            TYPE_CONFIG => {
                let enabled = r.u8()? != 0;
                let corner = Corner::from_code(r.u8()?).ok_or(DecodeError::Invalid)?;
                let size_percent = r.u8()?;
                let opacity_percent = r.u8()?;
                let flags = r.u8()?;
                Message::Config(Config {
                    enabled,
                    corner,
                    size_percent,
                    opacity_percent,
                    show_whole_channel: flags & 1 != 0,
                    show_channel_name: flags & 2 != 0,
                })
            }
            TYPE_SCENE => {
                let channel = r.str()?;
                let count = usize::from(r.u8()?);
                if count > MAX_PEOPLE {
                    return Err(DecodeError::Invalid);
                }
                let mut people = Vec::with_capacity(count);
                for _ in 0..count {
                    let flags = r.u8()?;
                    people.push(Person {
                        talking: flags & 1 != 0,
                        muted: flags & 2 != 0,
                        deafened: flags & 4 != 0,
                        is_self: flags & 8 != 0,
                        name: r.str()?,
                    });
                }
                Message::Scene(Scene { channel, people })
            }
            TYPE_HEARTBEAT => Message::Heartbeat,
            _ => return Err(DecodeError::Invalid),
        };
        if r.0.is_empty() {
            Ok(message)
        } else {
            Err(DecodeError::Length)
        }
    }
}

/// Payload length from a frame header.
pub fn frame_len(header: [u8; 2]) -> Result<usize, DecodeError> {
    let len = usize::from(u16::from_le_bytes(header));
    if len == 0 {
        Err(DecodeError::Length)
    } else if len > MAX_PAYLOAD {
        Err(DecodeError::TooLarge)
    } else {
        Ok(len)
    }
}

/// Reads one frame from a blocking reader.
pub fn read_message(reader: &mut impl std::io::Read) -> std::io::Result<Message> {
    let mut header = [0u8; 2];
    reader.read_exact(&mut header)?;
    let len = frame_len(header).map_err(std::io::Error::other)?;
    let mut payload = [0u8; MAX_PAYLOAD];
    let payload = &mut payload[..len];
    reader.read_exact(payload)?;
    Message::decode(payload).map_err(std::io::Error::other)
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    let mut end = s.len().min(usize::from(u8::MAX));
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    // `end` ≤ 255.
    out.push(u8::try_from(end).unwrap_or(0));
    out.extend_from_slice(&s.as_bytes()[..end]);
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.0.len() < n {
            return Err(DecodeError::Length);
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, DecodeError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn str(&mut self) -> Result<String, DecodeError> {
        let len = usize::from(self.u8()?);
        let bytes = self.take(len)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| DecodeError::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(message: &Message) -> Message {
        let frame = message.encode();
        let len = frame_len([frame[0], frame[1]]).expect("length");
        assert_eq!(len, frame.len() - 2);
        Message::decode(&frame[2..]).expect("decode")
    }

    fn scene() -> Scene {
        Scene {
            channel: "Lobby".into(),
            people: vec![
                Person {
                    name: "Ana".into(),
                    talking: true,
                    muted: false,
                    deafened: false,
                    is_self: true,
                },
                Person {
                    name: "Jörg 🎧".into(),
                    talking: false,
                    muted: true,
                    deafened: true,
                    is_self: false,
                },
            ],
        }
    }

    #[test]
    fn every_message_round_trips() {
        let messages = [
            Message::Hello(Hello {
                version: VERSION,
                api: Api::Vulkan,
                pid: 4242,
                steam_app_id: Some(730),
                exe: "cs2".into(),
            }),
            Message::Hello(Hello {
                version: VERSION,
                api: Api::D3d12,
                pid: 1,
                steam_app_id: None,
                exe: "Game.exe".into(),
            }),
            Message::Config(Config {
                enabled: true,
                corner: Corner::BottomRight,
                size_percent: 150,
                opacity_percent: 60,
                show_whole_channel: true,
                show_channel_name: false,
            }),
            Message::Config(Config::DISABLED),
            Message::Scene(scene()),
            Message::Scene(Scene::default()),
            Message::Heartbeat,
        ];
        for m in &messages {
            assert_eq!(&round_trip(m), m);
        }
    }

    #[test]
    fn long_strings_are_cut_at_a_character_boundary() {
        let name = "ä".repeat(200); // 400 bytes
        let mut s = scene();
        s.people[0].name = name;
        let Message::Scene(back) = round_trip(&Message::Scene(s)) else {
            panic!("not a scene");
        };
        assert_eq!(back.people[0].name, "ä".repeat(127));
    }

    #[test]
    fn scenes_are_capped() {
        let mut s = scene();
        s.people = (0..40).map(|_| s.people[0].clone()).collect();
        let Message::Scene(back) = round_trip(&Message::Scene(s)) else {
            panic!("not a scene");
        };
        assert_eq!(back.people.len(), MAX_PEOPLE);
        // The largest possible scene stays within one frame.
        let mut big = Scene {
            channel: "c".repeat(255),
            people: Vec::new(),
        };
        big.people = (0..MAX_PEOPLE)
            .map(|_| Person {
                name: "n".repeat(255),
                ..scene().people[0].clone()
            })
            .collect();
        assert!(Message::Scene(big).encode().len() - 2 <= MAX_PAYLOAD);
    }

    #[test]
    fn malformed_payloads_are_rejected() {
        assert_eq!(Message::decode(&[]), Err(DecodeError::Length));
        assert_eq!(Message::decode(&[99]), Err(DecodeError::Invalid));
        assert_eq!(
            Message::decode(&[TYPE_HELLO, b'X', b'X', b'X', b'X']),
            Err(DecodeError::Invalid)
        );
        // Truncated config.
        assert_eq!(
            Message::decode(&[TYPE_CONFIG, 1, 0]),
            Err(DecodeError::Length)
        );
        // Unknown corner.
        assert_eq!(
            Message::decode(&[TYPE_CONFIG, 1, 9, 100, 90, 0]),
            Err(DecodeError::Invalid)
        );
        // Trailing bytes.
        assert_eq!(
            Message::decode(&[TYPE_HEARTBEAT, 0]),
            Err(DecodeError::Length)
        );
        // Too many people.
        assert_eq!(
            Message::decode(&[TYPE_SCENE, 0, 17]),
            Err(DecodeError::Invalid)
        );
        // Invalid UTF-8 in a name.
        assert_eq!(
            Message::decode(&[TYPE_SCENE, 0, 1, 0, 1, 0xff]),
            Err(DecodeError::Invalid)
        );
        assert_eq!(frame_len([0, 0]), Err(DecodeError::Length));
        assert_eq!(frame_len([0xff, 0xff]), Err(DecodeError::TooLarge));
    }

    #[test]
    fn frames_are_read_from_a_stream() {
        let mut bytes = Message::Heartbeat.encode();
        bytes.extend(Message::Scene(scene()).encode());
        let mut cursor = std::io::Cursor::new(bytes);
        assert_eq!(
            read_message(&mut cursor).expect("first"),
            Message::Heartbeat
        );
        assert_eq!(
            read_message(&mut cursor).expect("second"),
            Message::Scene(scene())
        );
        assert!(read_message(&mut cursor).is_err());
    }
}
