//! Mumble version numbers and the features that depend on the server version.

use std::fmt;

/// A Mumble protocol version (major.minor.patch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MumbleVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl MumbleVersion {
    /// The protocol version this client announces.
    pub const CLIENT: Self = Self::new(1, 5, 0);
    /// First version with channel listeners ("following" channels).
    pub const LISTENING: Self = Self::new(1, 4, 0);
    /// First version that stores listener volume adjustments on the server.
    pub const LISTENER_VOLUME: Self = Self::new(1, 5, 0);

    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Decodes the legacy 32-bit format (`major << 16 | minor << 8 | patch`).
    pub fn from_v1(value: u32) -> Self {
        Self::new(
            (value >> 16) as u16,
            ((value >> 8) & 0xff) as u16,
            (value & 0xff) as u16,
        )
    }

    /// Decodes the 64-bit format introduced with 1.5 (`major << 48 | minor << 32 | patch << 16`).
    pub fn from_v2(value: u64) -> Self {
        Self::new(
            (value >> 48) as u16,
            (value >> 32) as u16,
            (value >> 16) as u16,
        )
    }

    /// Legacy encoding; minor and patch are clamped to 8 bits as the format requires.
    pub fn to_v1(self) -> u32 {
        (u32::from(self.major) << 16)
            | (u32::from(self.minor.min(0xff)) << 8)
            | u32::from(self.patch.min(0xff))
    }

    pub fn to_v2(self) -> u64 {
        (u64::from(self.major) << 48)
            | (u64::from(self.minor) << 32)
            | (u64::from(self.patch) << 16)
    }

    /// Picks the precise v2 value when present, otherwise the legacy one.
    pub fn from_wire(v1: Option<u32>, v2: Option<u64>) -> Option<Self> {
        match (v2, v1) {
            (Some(v2), _) if v2 != 0 => Some(Self::from_v2(v2)),
            (_, Some(v1)) => Some(Self::from_v1(v1)),
            _ => None,
        }
    }

    /// Whether the server supports channel listeners (needed to follow channels).
    pub fn supports_listening(self) -> bool {
        self >= Self::LISTENING
    }

    /// Whether the server accepts and echoes listener volume adjustments.
    pub fn supports_listener_volume(self) -> bool {
        self >= Self::LISTENER_VOLUME
    }
}

impl fmt::Display for MumbleVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings_round_trip() {
        let v = MumbleVersion::new(1, 5, 735);
        assert_eq!(MumbleVersion::from_v2(v.to_v2()), v);
        assert_eq!(MumbleVersion::CLIENT.to_v1(), 0x0001_0500);
        assert_eq!(MumbleVersion::CLIENT.to_v2(), 0x0001_0005_0000_0000);
        assert_eq!(
            MumbleVersion::from_v1(0x0001_0402),
            MumbleVersion::new(1, 4, 2)
        );
        assert_eq!(v.to_v1(), 0x0001_05ff);
    }

    #[test]
    fn wire_prefers_v2() {
        let v2 = MumbleVersion::new(1, 5, 735).to_v2();
        assert_eq!(
            MumbleVersion::from_wire(Some(0x0001_05ff), Some(v2)),
            Some(MumbleVersion::new(1, 5, 735))
        );
        assert_eq!(
            MumbleVersion::from_wire(Some(0x0001_0300), None),
            Some(MumbleVersion::new(1, 3, 0))
        );
        assert_eq!(MumbleVersion::from_wire(None, None), None);
    }

    #[test]
    fn capabilities() {
        assert!(!MumbleVersion::new(1, 3, 4).supports_listening());
        assert!(MumbleVersion::new(1, 4, 230).supports_listening());
        assert!(!MumbleVersion::new(1, 4, 230).supports_listener_volume());
        assert!(MumbleVersion::new(1, 5, 735).supports_listener_volume());
    }
}
