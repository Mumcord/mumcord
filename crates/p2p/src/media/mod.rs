//! The Bovnar media stream profile of spec 07 (FR-7.89 … FR-7.95) and the direct camera stream of
//! spec 08 (FR-8.92, FR-8.93): the fixed channel map, the channel-0 control kinds, the stream
//! description and the limits of the new media format.
//!
//! A new-format media link carries one endless Bovnar document,
//! `.media = \x00 …messages… \x00;`, where a message is `varint(channel) varint(length) bytes`.
//! [`MediaWriter`] and [`MediaReader`] write and read that document over `bovnar::codec`; this
//! module names the profile both sides must agree on.
//!
//! | Channel | Use |
//! |---|---|
//! | 0 | Control and stream description, prefixed with a [`ControlKind`] byte |
//! | 1–4 | Video temporal layers 0–3, `channel = 1 + layer` |
//! | 5 | Audio (Opus) |
//! | 6 | Plugin relay data |
//! | 7–[`MAX_CHANNEL`] | Reserved |
//!
//! Every track message starts with one clear flags byte ([`FLAG_KEYFRAME`] is the only known bit).
//! See [`Description`] for the channel-0 description document.
//!
//! Everything described here comes from the network and is untrusted. The description parser
//! checks the size before parsing, validates each field against the profile, and reports errors
//! with a static field name only, never with received content.

mod codec;
mod description;

pub use codec::{CodecError, MediaMessage, MediaReader, MediaWriter};
pub use description::{
    AudioCodec, AudioTrack, Description, StreamKind, Track, VideoCodec, VideoTrack,
};

/// Channel of control messages and the stream description.
pub const CHANNEL_CONTROL: u16 = 0;
/// First video channel; a frame for temporal layer `n` travels on [`CHANNEL_VIDEO_BASE`] + `n`.
pub const CHANNEL_VIDEO_BASE: u16 = 1;
/// Last video channel: the profile has room for four temporal layers.
pub const CHANNEL_VIDEO_LAST: u16 = 4;
/// Channel of Opus audio packets.
pub const CHANNEL_AUDIO: u16 = 5;
/// Channel of plugin relay data.
pub const CHANNEL_PLUGIN_DATA: u16 = 6;
/// Highest channel the profile allows; 7 to [`MAX_CHANNEL`] are reserved.
pub const MAX_CHANNEL: u16 = 4095;
/// Number of video temporal layers the channel map has room for.
pub const MAX_VIDEO_LAYERS: u8 = 4;
/// Highest temporal layer index (0-based).
pub const MAX_LAYER: u8 = MAX_VIDEO_LAYERS - 1;
/// Most tracks one description may list: one per used channel (1–5).
pub const MAX_TRACKS: usize = 5;

/// Control kind of the stream description (FR-7.91).
pub const KIND_DESCRIPTION: u8 = 0;
/// Control kind of a subscriber's key request.
pub const KIND_KEY_REQUEST: u8 = 1;
/// Control kind of the publisher's key message.
pub const KIND_KEY: u8 = 2;
/// Control kind of a subscriber's keyframe request.
pub const KIND_KEYFRAME: u8 = 3;
/// Control kind of a subscriber's highest temporal layer.
pub const KIND_LAYERS: u8 = 4;
/// Control kind of a subscriber's wanted tile height.
pub const KIND_WANT: u8 = 5;
/// Control kind of the relay's notice that a subscriber is gone.
pub const KIND_GONE: u8 = 6;

/// Largest video track message, flags and encryption header included (FR-7.94).
pub const MAX_VIDEO_MESSAGE: usize = 4 * 1024 * 1024 + 64;
/// Largest audio track message, flags and encryption header included (FR-7.94).
pub const MAX_AUDIO_MESSAGE: usize = 4 * 1024 + 64;
/// Largest plugin relay data message, flags and encryption header included (FR-7.94).
pub const MAX_PLUGIN_MESSAGE: usize = 64 * 1024 + 64;
/// Largest description document, checked before parsing (FR-7.94).
pub const MAX_DESCRIPTION: usize = 8 * 1024;

/// The `.v` of the description this client writes.
pub const DESCRIPTION_VERSION: u8 = 1;
/// The Bovnar format version of the `.media` document (FR-7.89).
pub const BOVNAR_VERSION: (u16, u16) = (1, 2);

/// Flag bit of a keyframe; all other bits must be zero (FR-7.92, FR-7.94).
pub const FLAG_KEYFRAME: u8 = 0x01;
/// The flags bits this profile defines.
pub const KNOWN_FLAGS: u8 = FLAG_KEYFRAME;

/// Why received data is outside the media profile. Logs carry the reason only: the field names
/// are static schema names, never received content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MediaError {
    #[error("media data is too large")]
    TooLarge,
    #[error("malformed media document")]
    Malformed,
    #[error("missing field {0}")]
    Missing(&'static str),
    #[error("wrong type for field {0}")]
    WrongType(&'static str),
    #[error("value out of range for field {0}")]
    OutOfRange(&'static str),
    #[error("invalid value for field {0}")]
    Invalid(&'static str),
}

/// What a channel of the media profile carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelUse {
    /// Channel 0: control messages and the stream description.
    Control,
    /// A video temporal layer, 0 to [`MAX_LAYER`].
    VideoLayer(u8),
    /// Channel 5: Opus audio.
    Audio,
    /// Channel 6: plugin relay data.
    PluginData,
    /// Channels 7 to [`MAX_CHANNEL`]: reserved, never used by this profile.
    Reserved,
}

/// The channel-0 control kinds (FR-7.91).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ControlKind {
    Description = KIND_DESCRIPTION,
    KeyRequest = KIND_KEY_REQUEST,
    Key = KIND_KEY,
    Keyframe = KIND_KEYFRAME,
    Layers = KIND_LAYERS,
    Want = KIND_WANT,
    Gone = KIND_GONE,
}

impl ControlKind {
    /// The one-byte kind a channel-0 message starts with.
    pub const fn to_byte(self) -> u8 {
        self as u8
    }

    /// The kind for a received kind byte, or `None` for one outside the profile.
    pub const fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            KIND_DESCRIPTION => Self::Description,
            KIND_KEY_REQUEST => Self::KeyRequest,
            KIND_KEY => Self::Key,
            KIND_KEYFRAME => Self::Keyframe,
            KIND_LAYERS => Self::Layers,
            KIND_WANT => Self::Want,
            KIND_GONE => Self::Gone,
            _ => return None,
        })
    }
}

/// Classifies a channel; `None` outside 0 to [`MAX_CHANNEL`] (FR-7.90).
pub const fn channel_use(channel: u16) -> Option<ChannelUse> {
    match channel {
        CHANNEL_CONTROL => Some(ChannelUse::Control),
        CHANNEL_VIDEO_BASE..=CHANNEL_VIDEO_LAST => {
            Some(ChannelUse::VideoLayer((channel - CHANNEL_VIDEO_BASE) as u8))
        }
        CHANNEL_AUDIO => Some(ChannelUse::Audio),
        CHANNEL_PLUGIN_DATA => Some(ChannelUse::PluginData),
        7..=MAX_CHANNEL => Some(ChannelUse::Reserved),
        _ => None,
    }
}

/// The channel a video frame of `layer` travels on, or `None` for a layer the map has no room
/// for.
pub const fn video_channel(layer: u8) -> Option<u16> {
    if layer < MAX_VIDEO_LAYERS {
        Some(CHANNEL_VIDEO_BASE + layer as u16)
    } else {
        None
    }
}

/// The size cap of a whole track message on `channel`, flags and encryption header included.
/// Channel 0 has no single cap: its description is capped at [`MAX_DESCRIPTION`] and the other
/// control kinds have their own layouts.
pub const fn message_limit(channel: u16) -> Option<usize> {
    match channel {
        CHANNEL_VIDEO_BASE..=CHANNEL_VIDEO_LAST => Some(MAX_VIDEO_MESSAGE),
        CHANNEL_AUDIO => Some(MAX_AUDIO_MESSAGE),
        CHANNEL_PLUGIN_DATA => Some(MAX_PLUGIN_MESSAGE),
        _ => None,
    }
}

/// Checks a received message's channel and total size against the profile (FR-7.94). A message
/// on a reserved channel or outside 0 to [`MAX_CHANNEL`] is refused; so is one over its
/// channel's cap.
pub const fn check_message(channel: u16, len: usize) -> Result<ChannelUse, MediaError> {
    let Some(use_) = channel_use(channel) else {
        return Err(MediaError::OutOfRange("channel"));
    };
    if matches!(use_, ChannelUse::Reserved) {
        return Err(MediaError::Invalid("channel"));
    }
    if let Some(limit) = message_limit(channel)
        && len > limit
    {
        return Err(MediaError::TooLarge);
    }
    Ok(use_)
}

/// Reads the flags byte of a track message: `Ok(true)` for a keyframe, `Ok(false)` for a delta
/// frame, `Err` when bits outside [`KNOWN_FLAGS`] are set (FR-7.92, FR-7.94).
pub const fn check_flags(flags: u8) -> Result<bool, MediaError> {
    if flags & !KNOWN_FLAGS != 0 {
        return Err(MediaError::Invalid("flags"));
    }
    Ok(flags & FLAG_KEYFRAME != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_channel_is_classified() {
        assert_eq!(channel_use(CHANNEL_CONTROL), Some(ChannelUse::Control));
        for layer in 0..MAX_VIDEO_LAYERS {
            assert_eq!(
                channel_use(1 + u16::from(layer)),
                Some(ChannelUse::VideoLayer(layer))
            );
        }
        assert_eq!(channel_use(CHANNEL_AUDIO), Some(ChannelUse::Audio));
        assert_eq!(
            channel_use(CHANNEL_PLUGIN_DATA),
            Some(ChannelUse::PluginData)
        );
        for channel in CHANNEL_PLUGIN_DATA + 1..=MAX_CHANNEL {
            assert_eq!(channel_use(channel), Some(ChannelUse::Reserved));
        }
        assert_eq!(channel_use(MAX_CHANNEL), Some(ChannelUse::Reserved));
        assert_eq!(channel_use(MAX_CHANNEL + 1), None);
        assert_eq!(channel_use(u16::MAX), None);
    }

    #[test]
    fn video_channels_are_one_plus_the_layer() {
        for layer in 0..MAX_VIDEO_LAYERS {
            assert_eq!(video_channel(layer), Some(1 + u16::from(layer)));
        }
        assert_eq!(video_channel(MAX_VIDEO_LAYERS), None);
        assert_eq!(video_channel(u8::MAX), None);
    }

    #[test]
    fn every_control_kind_round_trips_through_its_byte() {
        let all = [
            (KIND_DESCRIPTION, ControlKind::Description),
            (KIND_KEY_REQUEST, ControlKind::KeyRequest),
            (KIND_KEY, ControlKind::Key),
            (KIND_KEYFRAME, ControlKind::Keyframe),
            (KIND_LAYERS, ControlKind::Layers),
            (KIND_WANT, ControlKind::Want),
            (KIND_GONE, ControlKind::Gone),
        ];
        for (byte, kind) in all {
            assert_eq!(kind.to_byte(), byte);
            assert_eq!(ControlKind::from_byte(byte), Some(kind));
        }
        for byte in KIND_GONE + 1..=u8::MAX {
            assert_eq!(ControlKind::from_byte(byte), None, "{byte}");
        }
    }

    #[test]
    fn the_kind_numbers_are_the_spec_table() {
        assert_eq!(
            [
                KIND_DESCRIPTION,
                KIND_KEY_REQUEST,
                KIND_KEY,
                KIND_KEYFRAME,
                KIND_LAYERS,
                KIND_WANT,
                KIND_GONE,
            ],
            [0, 1, 2, 3, 4, 5, 6]
        );
    }

    #[test]
    fn every_limit_boundary_is_exact() {
        for channel in CHANNEL_VIDEO_BASE..=CHANNEL_VIDEO_LAST {
            let limit = message_limit(channel).expect("video limit");
            assert_eq!(limit, 4 * 1024 * 1024 + 64);
            assert_eq!(
                check_message(channel, limit),
                Ok(ChannelUse::VideoLayer((channel - 1) as u8))
            );
            assert_eq!(check_message(channel, limit + 1), Err(MediaError::TooLarge));
        }
        let audio = message_limit(CHANNEL_AUDIO).expect("audio limit");
        assert_eq!(audio, 4 * 1024 + 64);
        assert_eq!(check_message(CHANNEL_AUDIO, audio), Ok(ChannelUse::Audio));
        assert_eq!(
            check_message(CHANNEL_AUDIO, audio + 1),
            Err(MediaError::TooLarge)
        );
        let plugin = message_limit(CHANNEL_PLUGIN_DATA).expect("plugin limit");
        assert_eq!(plugin, 64 * 1024 + 64);
        assert_eq!(
            check_message(CHANNEL_PLUGIN_DATA, plugin),
            Ok(ChannelUse::PluginData)
        );
        assert_eq!(
            check_message(CHANNEL_PLUGIN_DATA, plugin + 1),
            Err(MediaError::TooLarge)
        );
    }

    #[test]
    fn only_track_channels_have_a_message_limit() {
        assert_eq!(message_limit(CHANNEL_CONTROL), None);
        for channel in CHANNEL_PLUGIN_DATA + 1..=MAX_CHANNEL {
            assert_eq!(message_limit(channel), None);
        }
        assert_eq!(
            check_message(CHANNEL_CONTROL, usize::MAX),
            Ok(ChannelUse::Control)
        );
        assert_eq!(
            check_message(CHANNEL_PLUGIN_DATA + 1, 1),
            Err(MediaError::Invalid("channel"))
        );
        assert_eq!(
            check_message(MAX_CHANNEL + 1, 1),
            Err(MediaError::OutOfRange("channel"))
        );
        assert_eq!(
            check_message(u16::MAX, 0),
            Err(MediaError::OutOfRange("channel"))
        );
    }

    #[test]
    fn the_description_limit_is_eight_kibibytes() {
        assert_eq!(MAX_DESCRIPTION, 8 * 1024);
        assert_eq!(MAX_CHANNEL, 4095);
    }

    #[test]
    fn flags_carry_only_the_keyframe_bit() {
        assert_eq!(check_flags(0), Ok(false));
        assert_eq!(check_flags(FLAG_KEYFRAME), Ok(true));
        for flags in [0x02u8, 0x03, 0x80, 0xff] {
            assert_eq!(check_flags(flags), Err(MediaError::Invalid("flags")));
        }
        assert_eq!(KNOWN_FLAGS, 0x01);
    }

    #[test]
    fn version_constants_are_the_profile_versions() {
        assert_eq!(DESCRIPTION_VERSION, 1);
        assert_eq!(BOVNAR_VERSION, (1, 2));
    }
}
