//! The stream description of the Bovnar media profile (FR-7.95): one embedded Bovnar document on
//! channel 0, kind [`super::KIND_DESCRIPTION`], sent at stream start, on every track change and at
//! every rekey.
//!
//! ```bovnar
//! .v = <uint:8> 1;
//! .timebase = <uint:32> 1;
//! .kind = "screen";                       # "screen" | "camera"
//! .id = "…";                              # camera session id, absent for screen
//! .tracks = [
//!     { .ch = <uint:8> 1; .kind = "video"; .codec = "av1"; .layer = <uint:8> 0;
//!       .width = <uint:16> 1920; .height = <uint:16> 1080; .rate = <float:32> 30.0; },
//!     { .ch = <uint:8> 5; .kind = "audio"; .codec = "opus";
//!       .rate = <uint:32> 48000; .bitrate = <uint:32> 96; }
//! ];
//! ```
//!
//! The DOM of Bovnar 2.1.0 requires every struct in one array to have the same key set, so the
//! writer gives every track all eight keys: a video track carries `.bitrate = 0`, an audio track
//! `.layer`, `.width` and `.height` as 0. A reader ignores the fields that do not apply to the
//! track's kind, as it ignores any field it does not know.
//!
//! Unknown fields are ignored; a missing required field, a field of the wrong type and a value
//! outside the profile are separate errors. The size is checked against [`super::MAX_DESCRIPTION`]
//! before the document is parsed, so oversized input never reaches the parser. Errors name a
//! static field only and never carry received content.

use bovnar::{Base, DomDoc, DomNode, DomType, EmitSpec, Event, Writer};

use super::{
    CHANNEL_AUDIO, CHANNEL_VIDEO_BASE, CHANNEL_VIDEO_LAST, DESCRIPTION_VERSION, MAX_DESCRIPTION,
    MAX_LAYER, MAX_TRACKS, MediaError,
};

/// A camera session id is 16 lowercase hex characters, the id of the camera's `on` message
/// (spec 08, section 6.5).
const CAMERA_ID_LEN: usize = 16;

/// What a description describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    /// A screen share: video tracks and optional audio.
    Screen,
    /// A direct camera stream: video tracks only, tied to a camera session id.
    Camera,
}

impl StreamKind {
    /// The `.kind` string.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Screen => "screen",
            Self::Camera => "camera",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "screen" => Some(Self::Screen),
            "camera" => Some(Self::Camera),
            _ => None,
        }
    }
}

/// A video codec a track can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    Av1,
    Vp9,
}

impl VideoCodec {
    /// The `.codec` string.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Av1 => "av1",
            Self::Vp9 => "vp9",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "av1" => Some(Self::Av1),
            "vp9" => Some(Self::Vp9),
            _ => None,
        }
    }
}

/// An audio codec a track can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec {
    Opus,
}

impl AudioCodec {
    /// The `.codec` string.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Opus => "opus",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "opus" => Some(Self::Opus),
            _ => None,
        }
    }
}

/// One video track: frames of temporal layer `layer` arrive on `channel = 1 + layer`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoTrack {
    pub channel: u16,
    pub codec: VideoCodec,
    pub layer: u8,
    pub width: u16,
    pub height: u16,
    /// Frame rate in Hz.
    pub rate: f32,
}

/// The Opus audio track on channel 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioTrack {
    pub channel: u16,
    pub codec: AudioCodec,
    /// Sample rate in Hz.
    pub rate: u32,
    /// Bitrate in kbit/s.
    pub bitrate: u32,
}

/// One track of a description.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Track {
    Video(VideoTrack),
    Audio(AudioTrack),
}

impl Track {
    /// The channel this track's messages arrive on.
    pub const fn channel(&self) -> u16 {
        match self {
            Self::Video(video) => video.channel,
            Self::Audio(audio) => audio.channel,
        }
    }
}

/// A parsed stream description (FR-7.95).
#[derive(Debug, Clone, PartialEq)]
pub struct Description {
    /// The stream kind: a screen share or a direct camera.
    pub kind: StreamKind,
    /// Ticks per microsecond of the 8-byte track timestamps; 1 at the current microsecond
    /// timebase.
    pub timebase: u32,
    /// Camera session id; present exactly for [`StreamKind::Camera`].
    pub id: Option<String>,
    /// The tracks, one per used channel.
    pub tracks: Vec<Track>,
}

impl Description {
    /// Parses and validates a received description. The size is checked first, so oversized
    /// input never reaches the parser.
    pub fn parse(data: &[u8]) -> Result<Self, MediaError> {
        if data.len() > MAX_DESCRIPTION {
            return Err(MediaError::TooLarge);
        }
        let doc = DomDoc::parse(data).map_err(|_| MediaError::Malformed)?;

        let version = uint(required(doc.get("v"), "v")?, "v", u8::MAX.into())?;
        if version < DESCRIPTION_VERSION.into() {
            return Err(MediaError::OutOfRange("v"));
        }
        let timebase = uint(
            required(doc.get("timebase"), "timebase")?,
            "timebase",
            u32::MAX.into(),
        )?;
        if timebase == 0 {
            return Err(MediaError::OutOfRange("timebase"));
        }
        let kind = StreamKind::from_name(text(required(doc.get("kind"), "kind")?, "kind")?)
            .ok_or(MediaError::Invalid("kind"))?;
        let id = match doc.get("id") {
            Some(node) => {
                let id = text(node, "id")?;
                if !is_camera_id(id) {
                    return Err(MediaError::Invalid("id"));
                }
                Some(id.to_owned())
            }
            None => None,
        };
        match (kind, &id) {
            (StreamKind::Screen, Some(_)) => return Err(MediaError::Invalid("id")),
            (StreamKind::Camera, None) => return Err(MediaError::Missing("id")),
            _ => {}
        }

        let tracks_node = required(doc.get("tracks"), "tracks")?;
        if tracks_node.dom_type() != Some(DomType::Array) {
            return Err(MediaError::WrongType("tracks"));
        }
        if tracks_node.array_dims() > 1 {
            return Err(MediaError::Invalid("tracks"));
        }
        let count = tracks_node.len();
        if count == 0 || count > MAX_TRACKS {
            return Err(MediaError::OutOfRange("tracks"));
        }
        let mut tracks = Vec::with_capacity(count);
        let mut used_channels = 0u8;
        for index in 0..count {
            let track = tracks_node
                .at(index)
                .ok_or(MediaError::Malformed)
                .and_then(|node| parse_track(node, kind))?;
            let bit = 1u8 << track.channel();
            if used_channels & bit != 0 {
                return Err(MediaError::Invalid("tracks"));
            }
            used_channels |= bit;
            tracks.push(track);
        }

        Ok(Self {
            kind,
            timebase: timebase as u32,
            id,
            tracks,
        })
    }

    /// Writes and validates the description, refusing one over [`MAX_DESCRIPTION`]. Returns
    /// [`MediaError`] for a model outside the profile instead of writing it.
    pub fn encode(&self) -> Result<Vec<u8>, MediaError> {
        self.validate()?;
        let mut writer =
            Writer::to_mem(MAX_DESCRIPTION, false).map_err(|_| MediaError::TooLarge)?;
        write_document(&mut writer, self).map_err(|_| MediaError::TooLarge)?;
        writer.finish().map_err(|_| MediaError::TooLarge)?;
        Ok(writer.get_output().to_vec())
    }

    /// Checks the model against the profile, so an invalid description is never written.
    fn validate(&self) -> Result<(), MediaError> {
        match (self.kind, &self.id) {
            (StreamKind::Screen, None) => {}
            (StreamKind::Screen, Some(_)) => return Err(MediaError::Invalid("id")),
            (StreamKind::Camera, Some(id)) if is_camera_id(id) => {}
            (StreamKind::Camera, Some(_)) => return Err(MediaError::Invalid("id")),
            (StreamKind::Camera, None) => return Err(MediaError::Missing("id")),
        }
        if self.timebase == 0 {
            return Err(MediaError::OutOfRange("timebase"));
        }
        if self.tracks.is_empty() || self.tracks.len() > MAX_TRACKS {
            return Err(MediaError::OutOfRange("tracks"));
        }
        let mut used_channels = 0u8;
        for track in &self.tracks {
            match track {
                Track::Video(video) => {
                    if !(CHANNEL_VIDEO_BASE..=CHANNEL_VIDEO_LAST).contains(&video.channel) {
                        return Err(MediaError::OutOfRange("ch"));
                    }
                    if video.layer > MAX_LAYER {
                        return Err(MediaError::OutOfRange("layer"));
                    }
                    if video.channel != CHANNEL_VIDEO_BASE + u16::from(video.layer) {
                        return Err(MediaError::Invalid("ch"));
                    }
                    if video.width == 0 {
                        return Err(MediaError::OutOfRange("width"));
                    }
                    if video.height == 0 {
                        return Err(MediaError::OutOfRange("height"));
                    }
                    if !video.rate.is_finite() || video.rate <= 0.0 {
                        return Err(MediaError::OutOfRange("rate"));
                    }
                }
                Track::Audio(audio) => {
                    if self.kind == StreamKind::Camera {
                        return Err(MediaError::Invalid("tracks"));
                    }
                    if audio.channel != CHANNEL_AUDIO {
                        return Err(MediaError::OutOfRange("ch"));
                    }
                    if audio.rate == 0 {
                        return Err(MediaError::OutOfRange("rate"));
                    }
                    if audio.bitrate == 0 {
                        return Err(MediaError::OutOfRange("bitrate"));
                    }
                }
            }
            let bit = 1u8 << track.channel();
            if used_channels & bit != 0 {
                return Err(MediaError::Invalid("tracks"));
            }
            used_channels |= bit;
        }
        Ok(())
    }
}

fn is_camera_id(id: &str) -> bool {
    id.len() == CAMERA_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}

fn required<'a>(node: Option<DomNode<'a>>, field: &'static str) -> Result<DomNode<'a>, MediaError> {
    node.ok_or(MediaError::Missing(field))
}

fn text<'a>(node: DomNode<'a>, field: &'static str) -> Result<&'a str, MediaError> {
    node.as_str().ok_or(MediaError::WrongType(field))
}

/// An unsigned integer field up to `max`; a value outside it is out of range, not a parse error.
fn uint(node: DomNode<'_>, field: &'static str, max: u64) -> Result<u64, MediaError> {
    let value = node.as_u64().ok_or(MediaError::WrongType(field))?;
    if value > max {
        return Err(MediaError::OutOfRange(field));
    }
    Ok(value)
}

fn parse_track(node: DomNode<'_>, stream: StreamKind) -> Result<Track, MediaError> {
    if node.dom_type() != Some(DomType::Struct) {
        return Err(MediaError::WrongType("tracks"));
    }
    let kind = text(required(node.get("kind"), "kind")?, "kind")?;
    let channel = uint(required(node.get("ch"), "ch")?, "ch", u8::MAX.into())?;
    match kind {
        "video" => {
            let layer = uint(
                required(node.get("layer"), "layer")?,
                "layer",
                u8::MAX.into(),
            )?;
            if !(u64::from(CHANNEL_VIDEO_BASE)..=u64::from(CHANNEL_VIDEO_LAST)).contains(&channel) {
                return Err(MediaError::OutOfRange("ch"));
            }
            if layer > u64::from(MAX_LAYER) {
                return Err(MediaError::OutOfRange("layer"));
            }
            if channel != u64::from(CHANNEL_VIDEO_BASE) + layer {
                return Err(MediaError::Invalid("ch"));
            }
            let codec =
                VideoCodec::from_name(text(required(node.get("codec"), "codec")?, "codec")?)
                    .ok_or(MediaError::Invalid("codec"))?;
            let width = uint(
                required(node.get("width"), "width")?,
                "width",
                u16::MAX.into(),
            )?;
            let height = uint(
                required(node.get("height"), "height")?,
                "height",
                u16::MAX.into(),
            )?;
            let rate_node = required(node.get("rate"), "rate")?;
            if rate_node.dom_type() != Some(DomType::Float) {
                return Err(MediaError::WrongType("rate"));
            }
            let rate = rate_node.as_float().ok_or(MediaError::WrongType("rate"))?;
            if width == 0 {
                return Err(MediaError::OutOfRange("width"));
            }
            if height == 0 {
                return Err(MediaError::OutOfRange("height"));
            }
            let rate = rate as f32;
            if !rate.is_finite() || rate <= 0.0 {
                return Err(MediaError::OutOfRange("rate"));
            }
            Ok(Track::Video(VideoTrack {
                channel: channel as u16,
                codec,
                layer: layer as u8,
                width: width as u16,
                height: height as u16,
                rate,
            }))
        }
        "audio" => {
            if stream == StreamKind::Camera {
                return Err(MediaError::Invalid("tracks"));
            }
            if channel != u64::from(CHANNEL_AUDIO) {
                return Err(MediaError::OutOfRange("ch"));
            }
            let codec =
                AudioCodec::from_name(text(required(node.get("codec"), "codec")?, "codec")?)
                    .ok_or(MediaError::Invalid("codec"))?;
            let rate = uint(required(node.get("rate"), "rate")?, "rate", u32::MAX.into())?;
            let bitrate = uint(
                required(node.get("bitrate"), "bitrate")?,
                "bitrate",
                u32::MAX.into(),
            )?;
            if rate == 0 {
                return Err(MediaError::OutOfRange("rate"));
            }
            if bitrate == 0 {
                return Err(MediaError::OutOfRange("bitrate"));
            }
            Ok(Track::Audio(AudioTrack {
                channel: channel as u16,
                codec,
                rate: rate as u32,
                bitrate: bitrate as u32,
            }))
        }
        _ => Err(MediaError::Invalid("kind")),
    }
}

fn write_document(writer: &mut Writer, description: &Description) -> bovnar::Result<()> {
    writer.write_uint("v", DESCRIPTION_VERSION.into(), 8, Base::DECIMAL)?;
    writer.write_uint("timebase", description.timebase.into(), 32, Base::DECIMAL)?;
    writer.write_string("kind", description.kind.name())?;
    if let Some(id) = &description.id {
        writer.write_string("id", id)?;
    }
    writer.emit(Event::AssignmentStart, EmitSpec::key("tracks"))?;
    writer.begin_array_row()?;
    for track in &description.tracks {
        writer.emit(Event::StructStart, EmitSpec::NONE)?;
        // Every track carries the same keys in the same order: the Bovnar DOM refuses an array
        // whose structs differ in shape. Fields that do not apply to the track's kind are zero
        // and are ignored by every reader.
        let (channel, kind, codec, layer, width, height, rate, rate_is_float, bitrate) = match track
        {
            Track::Video(video) => (
                video.channel,
                "video",
                video.codec.name(),
                video.layer,
                video.width,
                video.height,
                video.rate.into(),
                true,
                0,
            ),
            Track::Audio(audio) => (
                audio.channel,
                "audio",
                audio.codec.name(),
                0,
                0,
                0,
                audio.rate.into(),
                false,
                audio.bitrate,
            ),
        };
        writer.write_uint("ch", channel.into(), 8, Base::DECIMAL)?;
        writer.write_string("kind", kind)?;
        writer.write_string("codec", codec)?;
        writer.write_uint("layer", layer.into(), 8, Base::DECIMAL)?;
        writer.write_uint("width", width.into(), 16, Base::DECIMAL)?;
        writer.write_uint("height", height.into(), 16, Base::DECIMAL)?;
        if rate_is_float {
            writer.write_float("rate", rate, 32)?;
        } else {
            writer.write_uint("rate", rate as u64, 32, Base::DECIMAL)?;
        }
        writer.write_uint("bitrate", bitrate.into(), 32, Base::DECIMAL)?;
        writer.emit(Event::StructEnd, EmitSpec::NONE)?;
    }
    writer.end_array_row()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAMERA_ID: &str = "0123456789abcdef";

    fn video(channel: u16, layer: u8) -> Track {
        Track::Video(VideoTrack {
            channel,
            codec: VideoCodec::Av1,
            layer,
            width: 1920,
            height: 1080,
            rate: 30.0,
        })
    }

    fn audio() -> Track {
        Track::Audio(AudioTrack {
            channel: CHANNEL_AUDIO,
            codec: AudioCodec::Opus,
            rate: 48_000,
            bitrate: 96,
        })
    }

    fn screen() -> Description {
        Description {
            kind: StreamKind::Screen,
            timebase: 1,
            id: None,
            tracks: vec![video(1, 0), video(2, 1), audio()],
        }
    }

    fn camera() -> Description {
        Description {
            kind: StreamKind::Camera,
            timebase: 1,
            id: Some(CAMERA_ID.into()),
            tracks: vec![video(1, 0)],
        }
    }

    fn valid() -> Vec<u8> {
        screen().encode().expect("valid description")
    }

    fn text_document(inner: &str) -> Vec<u8> {
        format!(r#".v = 1; .timebase = 1; .kind = "screen"; .tracks = [ {{ {inner} }} ];"#)
            .into_bytes()
    }

    const VIDEO_FIELDS: [(&str, &str); 7] = [
        ("ch", ".ch = 1"),
        ("kind", r#".kind = "video""#),
        ("codec", r#".codec = "av1""#),
        ("layer", ".layer = 0"),
        ("width", ".width = 1920"),
        ("height", ".height = 1080"),
        ("rate", ".rate = 30.0"),
    ];

    const AUDIO_FIELDS: [(&str, &str); 5] = [
        ("ch", ".ch = 5"),
        ("kind", r#".kind = "audio""#),
        ("codec", r#".codec = "opus""#),
        ("rate", ".rate = 48000"),
        ("bitrate", ".bitrate = 96"),
    ];

    fn joined(fields: &[(&str, &str)]) -> String {
        fields.iter().map(|(_, text)| format!("{text}; ")).collect()
    }

    fn without(fields: &[(&str, &str)], name: &str) -> String {
        joined(
            &fields
                .iter()
                .copied()
                .filter(|(field, _)| *field != name)
                .collect::<Vec<_>>(),
        )
    }

    fn with(fields: &[(&str, &str)], name: &str, replacement: &str) -> String {
        let mut fields = fields.to_vec();
        for (field, text) in &mut fields {
            if *field == name {
                *text = replacement;
            }
        }
        joined(&fields)
    }

    #[test]
    fn a_screen_description_round_trips() {
        let description = screen();
        let bytes = description.encode().expect("encodes");
        assert!(bytes.len() <= MAX_DESCRIPTION);
        assert_eq!(bovnar::json::detect(&bytes), bovnar::json::Format::Bovnar);
        assert_eq!(Description::parse(&bytes), Ok(description));
    }

    #[test]
    fn a_camera_description_round_trips() {
        let description = camera();
        let bytes = description.encode().expect("encodes");
        assert_eq!(Description::parse(&bytes), Ok(description));
    }

    #[test]
    fn encoded_fields_have_the_spec_types() {
        let bytes = screen().encode().expect("encodes");
        let doc = DomDoc::parse(&bytes).expect("bovnar");
        assert_eq!(doc.get("v").and_then(DomNode::as_u8), Some(1));
        assert_eq!(doc.get("timebase").and_then(DomNode::as_u32), Some(1));
        assert_eq!(doc.get("kind").and_then(DomNode::as_str), Some("screen"));
        assert!(doc.get("id").is_none());
        let tracks = doc.get("tracks").expect("tracks");
        assert_eq!(tracks.dom_type(), Some(DomType::Array));
        assert_eq!(tracks.len(), 3);
        let first = tracks.at(0).expect("track");
        assert_eq!(first.get("ch").and_then(DomNode::as_u8), Some(1));
        assert_eq!(first.get("kind").and_then(DomNode::as_str), Some("video"));
        assert_eq!(first.get("codec").and_then(DomNode::as_str), Some("av1"));
        assert_eq!(first.get("layer").and_then(DomNode::as_u8), Some(0));
        assert_eq!(first.get("width").and_then(DomNode::as_u16), Some(1920));
        assert_eq!(first.get("height").and_then(DomNode::as_u16), Some(1080));
        assert_eq!(first.get("rate").and_then(DomNode::as_float), Some(30.0));
        let last = tracks.at(2).expect("track");
        assert_eq!(last.get("ch").and_then(DomNode::as_u8), Some(5));
        assert_eq!(last.get("rate").and_then(DomNode::as_u32), Some(48_000));
        assert_eq!(last.get("bitrate").and_then(DomNode::as_u32), Some(96));

        let camera = DomDoc::parse(&camera().encode().expect("encodes")).expect("bovnar");
        assert_eq!(camera.get("kind").and_then(DomNode::as_str), Some("camera"));
        assert_eq!(camera.get("id").and_then(DomNode::as_str), Some(CAMERA_ID));
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let bytes = text_document(
            r#".ch = 1; .kind = "video"; .codec = "vp9"; .layer = 0; .width = 640;
               .height = 480; .rate = 30.0; .hdr = [1, 2]; .x = "y";"#,
        );
        let parsed = Description::parse(&bytes).expect("unknown fields are ignored");
        assert_eq!(parsed.tracks.len(), 1);
        let Track::Video(track) = parsed.tracks[0] else {
            panic!("video track expected");
        };
        assert_eq!(track.codec, VideoCodec::Vp9);
        assert_eq!(track.width, 640);
    }

    #[test]
    fn a_newer_version_with_unknown_fields_is_read() {
        let bytes = br#".v = 4; .timebase = 1; .kind = "screen"; .meta = [1];
                .tracks = [ { .ch = 1; .kind = "video"; .codec = "av1"; .layer = 0;
                              .width = 16; .height = 16; .rate = 30.0; } ];"#
            .to_vec();
        assert!(Description::parse(&bytes).is_ok());
    }

    #[test]
    fn every_missing_required_field_is_reported() {
        let cases: [(&str, &[u8]); 4] = [
            ("v", br#".timebase = 1; .kind = "screen"; .tracks = [];"#),
            ("timebase", br#".v = 1; .kind = "screen"; .tracks = [];"#),
            ("kind", br#".v = 1; .timebase = 1; .tracks = [];"#),
            ("tracks", br#".v = 1; .timebase = 1; .kind = "screen";"#),
        ];
        for (field, bytes) in cases {
            assert_eq!(
                Description::parse(bytes),
                Err(MediaError::Missing(field)),
                "{field}"
            );
        }
        assert_eq!(
            Description::parse(br#".v = 1; .timebase = 1; .kind = "camera"; .tracks = [];"#),
            Err(MediaError::Missing("id"))
        );

        for (field, _) in VIDEO_FIELDS {
            let bytes = text_document(&without(&VIDEO_FIELDS, field));
            assert_eq!(
                Description::parse(&bytes),
                Err(MediaError::Missing(field)),
                "{field}"
            );
        }
        for (field, _) in AUDIO_FIELDS {
            let bytes = text_document(&without(&AUDIO_FIELDS, field));
            assert_eq!(
                Description::parse(&bytes),
                Err(MediaError::Missing(field)),
                "{field}"
            );
        }
    }

    #[test]
    fn wrong_types_are_reported() {
        let cases: [(&str, &[u8]); 12] = [
            (
                "v",
                br#".v = "1"; .timebase = 1; .kind = "screen"; .tracks = [];"#,
            ),
            (
                "timebase",
                br#".v = 1; .timebase = 1.5; .kind = "screen"; .tracks = [];"#,
            ),
            (
                "kind",
                br#".v = 1; .timebase = 1; .kind = 1; .tracks = [];"#,
            ),
            (
                "tracks",
                br#".v = 1; .timebase = 1; .kind = "screen"; .tracks = 1;"#,
            ),
            (
                "tracks",
                br#".v = 1; .timebase = 1; .kind = "screen"; .tracks = [1];"#,
            ),
            (
                "ch",
                br#".v = 1; .timebase = 1; .kind = "screen";
                    .tracks = [ { .ch = "1"; .kind = "video"; .codec = "av1"; .layer = 0;
                                  .width = 1; .height = 1; .rate = 1.0; } ];"#,
            ),
            (
                "kind",
                br#".v = 1; .timebase = 1; .kind = "screen";
                    .tracks = [ { .ch = 1; .kind = 7; .codec = "av1"; .layer = 0;
                                  .width = 1; .height = 1; .rate = 1.0; } ];"#,
            ),
            (
                "codec",
                br#".v = 1; .timebase = 1; .kind = "screen";
                    .tracks = [ { .ch = 1; .kind = "video"; .codec = 7; .layer = 0;
                                  .width = 1; .height = 1; .rate = 1.0; } ];"#,
            ),
            (
                "layer",
                br#".v = 1; .timebase = 1; .kind = "screen";
                    .tracks = [ { .ch = 1; .kind = "video"; .codec = "av1"; .layer = "0";
                                  .width = 1; .height = 1; .rate = 1.0; } ];"#,
            ),
            (
                "width",
                br#".v = 1; .timebase = 1; .kind = "screen";
                    .tracks = [ { .ch = 1; .kind = "video"; .codec = "av1"; .layer = 0;
                                  .width = "1"; .height = 1; .rate = 1.0; } ];"#,
            ),
            (
                "height",
                br#".v = 1; .timebase = 1; .kind = "screen";
                    .tracks = [ { .ch = 1; .kind = "video"; .codec = "av1"; .layer = 0;
                                  .width = 1; .height = "1"; .rate = 1.0; } ];"#,
            ),
            (
                "rate",
                br#".v = 1; .timebase = 1; .kind = "screen";
                    .tracks = [ { .ch = 1; .kind = "video"; .codec = "av1"; .layer = 0;
                                  .width = 1; .height = 1; .rate = 30; } ];"#,
            ),
        ];
        for (field, bytes) in cases {
            assert_eq!(
                Description::parse(bytes),
                Err(MediaError::WrongType(field)),
                "{field}"
            );
        }
        let id = br#".v = 1; .timebase = 1; .kind = "camera"; .id = 7; .tracks = [];"#;
        assert_eq!(Description::parse(id), Err(MediaError::WrongType("id")));
    }

    #[test]
    fn values_outside_the_profile_are_reported() {
        let cases: [(&str, Vec<u8>, MediaError); 17] = [
            (
                "version",
                br#".v = 0; .timebase = 1; .kind = "screen"; .tracks = [];"#.to_vec(),
                MediaError::OutOfRange("v"),
            ),
            (
                "timebase",
                br#".v = 1; .timebase = 0; .kind = "screen"; .tracks = [];"#.to_vec(),
                MediaError::OutOfRange("timebase"),
            ),
            (
                "kind",
                br#".v = 1; .timebase = 1; .kind = "tv"; .tracks = [];"#.to_vec(),
                MediaError::Invalid("kind"),
            ),
            (
                "empty tracks",
                br#".v = 1; .timebase = 1; .kind = "screen"; .tracks = [];"#.to_vec(),
                MediaError::OutOfRange("tracks"),
            ),
            (
                "screen id",
                br#".v = 1; .timebase = 1; .kind = "screen"; .id = "0123456789abcdef";
                    .tracks = [];"#
                    .to_vec(),
                MediaError::Invalid("id"),
            ),
            (
                "camera id",
                br#".v = 1; .timebase = 1; .kind = "camera"; .id = "xyz";
                    .tracks = [];"#
                    .to_vec(),
                MediaError::Invalid("id"),
            ),
            (
                "video channel",
                text_document(&with(&VIDEO_FIELDS, "ch", ".ch = 5")),
                MediaError::OutOfRange("ch"),
            ),
            (
                "video channel zero",
                text_document(&with(&VIDEO_FIELDS, "ch", ".ch = 0")),
                MediaError::OutOfRange("ch"),
            ),
            (
                "layer",
                text_document(&with(&VIDEO_FIELDS, "layer", ".layer = 4")),
                MediaError::OutOfRange("layer"),
            ),
            (
                "channel mapping",
                text_document(&with(&VIDEO_FIELDS, "layer", ".layer = 1")),
                MediaError::Invalid("ch"),
            ),
            (
                "width",
                text_document(&with(&VIDEO_FIELDS, "width", ".width = 0")),
                MediaError::OutOfRange("width"),
            ),
            (
                "height",
                text_document(&with(&VIDEO_FIELDS, "height", ".height = 0")),
                MediaError::OutOfRange("height"),
            ),
            (
                "rate",
                text_document(&with(&VIDEO_FIELDS, "rate", ".rate = 0.0")),
                MediaError::OutOfRange("rate"),
            ),
            (
                "video codec",
                text_document(&with(&VIDEO_FIELDS, "codec", r#".codec = "h264""#)),
                MediaError::Invalid("codec"),
            ),
            (
                "audio codec",
                text_document(&with(&AUDIO_FIELDS, "codec", r#".codec = "aac""#)),
                MediaError::Invalid("codec"),
            ),
            (
                "audio channel",
                text_document(&with(&AUDIO_FIELDS, "ch", ".ch = 6")),
                MediaError::OutOfRange("ch"),
            ),
            (
                "audio bitrate",
                text_document(&with(&AUDIO_FIELDS, "bitrate", ".bitrate = 0")),
                MediaError::OutOfRange("bitrate"),
            ),
        ];
        for (name, bytes, error) in cases {
            assert_eq!(Description::parse(&bytes), Err(error), "{name}");
        }
        let audio_rate = text_document(&with(&AUDIO_FIELDS, "rate", ".rate = 0"));
        assert_eq!(
            Description::parse(&audio_rate),
            Err(MediaError::OutOfRange("rate"))
        );
    }

    #[test]
    fn a_camera_description_has_video_tracks_only() {
        let bytes = br#".v = 1; .timebase = 1; .kind = "camera"; .id = "0123456789abcdef";
            .tracks = [ { .ch = 5; .kind = "audio"; .codec = "opus"; .rate = 48000;
                          .bitrate = 96; } ];"#
            .to_vec();
        assert_eq!(
            Description::parse(&bytes),
            Err(MediaError::Invalid("tracks"))
        );
    }

    #[test]
    fn duplicate_channels_and_too_many_tracks_are_refused() {
        let duplicate = br#".v = 1; .timebase = 1; .kind = "screen";
            .tracks = [
                { .ch = 1; .kind = "video"; .codec = "av1"; .layer = 0;
                  .width = 1; .height = 1; .rate = 1.0; },
                { .ch = 1; .kind = "video"; .codec = "av1"; .layer = 0;
                  .width = 1; .height = 1; .rate = 1.0; }
            ];"#
        .to_vec();
        assert_eq!(
            Description::parse(&duplicate),
            Err(MediaError::Invalid("tracks"))
        );
        let track = r#"{ .ch = {ch}; .kind = "video"; .codec = "av1"; .layer = {layer};
                        .width = 1; .height = 1; .rate = 1.0; }"#;
        let tracks: Vec<String> = [(1, 0), (2, 1), (3, 2), (4, 3), (2, 1), (3, 2)]
            .iter()
            .map(|(ch, layer)| {
                track
                    .replace("{ch}", &ch.to_string())
                    .replace("{layer}", &layer.to_string())
            })
            .collect();
        let too_many = format!(
            r#".v = 1; .timebase = 1; .kind = "screen"; .tracks = [ {} ];"#,
            tracks.join(", ")
        );
        assert_eq!(
            Description::parse(too_many.as_bytes()),
            Err(MediaError::OutOfRange("tracks"))
        );
    }

    #[test]
    fn the_description_cap_is_checked_before_parsing() {
        let base = valid();
        // An unknown field is ignored, so it can pad the document to the exact cap.
        let overhead = base.len() + b".pad = \"\";".len();
        let mut bytes = base.clone();
        bytes.extend_from_slice(
            format!(r#".pad = "{}";"#, "a".repeat(MAX_DESCRIPTION - overhead)).as_bytes(),
        );
        assert_eq!(bytes.len(), MAX_DESCRIPTION);
        assert!(
            Description::parse(&bytes).is_ok(),
            "exactly 8 KiB is allowed"
        );
        bytes.push(b';');
        assert_eq!(Description::parse(&bytes), Err(MediaError::TooLarge));
        let garbage = vec![b'x'; MAX_DESCRIPTION + 1];
        assert_eq!(Description::parse(&garbage), Err(MediaError::TooLarge));
    }

    #[test]
    fn malformed_documents_are_refused() {
        // An empty document is valid Bovnar but lacks every required field.
        assert_eq!(Description::parse(b""), Err(MediaError::Missing("v")));
        for bytes in [
            &b"not a document"[..],
            &br#"{"v":1,"timebase":1,"kind":"screen","tracks":[]}"#[..],
            &b"\xff\xfe\x00"[..],
        ] {
            assert_eq!(Description::parse(bytes), Err(MediaError::Malformed));
        }
        let bytes = valid();
        for cut in 0..bytes.len() {
            assert!(
                Description::parse(&bytes[..cut]).is_err(),
                "truncated at {cut}"
            );
        }
    }

    #[test]
    fn the_writer_refuses_a_model_outside_the_profile() {
        let mut screen_with_id = screen();
        screen_with_id.id = Some(CAMERA_ID.into());
        assert_eq!(screen_with_id.encode(), Err(MediaError::Invalid("id")));

        let mut camera_without_id = camera();
        camera_without_id.id = None;
        assert_eq!(camera_without_id.encode(), Err(MediaError::Missing("id")));

        let mut bad_layer = camera();
        bad_layer.tracks = vec![video(2, 0)];
        assert_eq!(bad_layer.encode(), Err(MediaError::Invalid("ch")));

        let mut bad_rate = camera();
        bad_rate.tracks = vec![Track::Video(VideoTrack {
            channel: 1,
            codec: VideoCodec::Vp9,
            layer: 0,
            width: 16,
            height: 16,
            rate: f32::NAN,
        })];
        assert_eq!(bad_rate.encode(), Err(MediaError::OutOfRange("rate")));

        let mut camera_with_audio = camera();
        camera_with_audio.tracks = vec![video(1, 0), audio()];
        assert_eq!(
            camera_with_audio.encode(),
            Err(MediaError::Invalid("tracks"))
        );

        let mut empty = screen();
        empty.tracks.clear();
        assert_eq!(empty.encode(), Err(MediaError::OutOfRange("tracks")));
    }

    #[test]
    fn error_messages_never_carry_received_content() {
        let codec = text_document(&with(&VIDEO_FIELDS, "codec", r#".codec = "h264""#));
        let error = Description::parse(&codec).expect_err("refused");
        assert!(!error.to_string().contains("h264"));
        assert_eq!(error.to_string(), "invalid value for field codec");

        let id = br#".v = 1; .timebase = 1; .kind = "camera"; .id = "secret-value";
            .tracks = [];"#;
        let error = Description::parse(id).expect_err("refused");
        assert!(!error.to_string().contains("secret-value"));
        assert_eq!(error.to_string(), "invalid value for field id");
    }

    #[test]
    fn every_document_written_reads_back_identically() {
        let mut tracks = vec![video(1, 0), video(2, 1), video(3, 2), video(4, 3), audio()];
        tracks[0] = Track::Video(VideoTrack {
            channel: 1,
            codec: VideoCodec::Vp9,
            layer: 0,
            width: 3840,
            height: 2160,
            rate: 59.94,
        });
        let description = Description {
            kind: StreamKind::Screen,
            timebase: 1000,
            id: None,
            tracks,
        };
        let bytes = description.encode().expect("encodes");
        assert_eq!(Description::parse(&bytes), Ok(description));
        assert_eq!(bytes.iter().filter(|b| **b == b'{').count(), 5);
    }
}
