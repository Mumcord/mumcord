//! Mumble ACL permission bits, as found in `ServerState::permissions` and `PermissionDenied`.

pub const WRITE: u32 = 0x1;
pub const TRAVERSE: u32 = 0x2;
pub const ENTER: u32 = 0x4;
pub const SPEAK: u32 = 0x8;
pub const MUTE_DEAFEN: u32 = 0x10;
pub const MOVE: u32 = 0x20;
pub const MAKE_CHANNEL: u32 = 0x40;
pub const LINK_CHANNEL: u32 = 0x80;
pub const WHISPER: u32 = 0x100;
pub const TEXT_MESSAGE: u32 = 0x200;
pub const MAKE_TEMP_CHANNEL: u32 = 0x400;
pub const LISTEN: u32 = 0x800;
pub const KICK: u32 = 0x1_0000;
pub const BAN: u32 = 0x2_0000;
pub const REGISTER: u32 = 0x4_0000;
pub const SELF_REGISTER: u32 = 0x8_0000;
pub const RESET_USER_CONTENT: u32 = 0x10_0000;
