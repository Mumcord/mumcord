//! Direct audio links in chat (issue #260): a short probe before the click says whether a link
//! is a playable audio file and how large it is; the file itself is only fetched after the user
//! clicks play. The format is sniffed from the bytes, the server's content type is not trusted,
//! and files above [`AUDIO_LIMIT`] are refused.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use reqwest::Url;
use serde::{Deserialize, Serialize};

use super::fetch::{FetchError, Fetcher};

/// Larger audio files are refused; a few minutes of lossless audio still fit.
pub const AUDIO_LIMIT: usize = 32 * 1024 * 1024;
/// Enough of the file to recognise its format.
const PROBE_LIMIT: usize = 16 * 1024;
/// A file near the limit needs far longer than a page on a slow line.
const AUDIO_TIMEOUT: Duration = Duration::from_secs(120);
const AUDIO_ACCEPT: &str = "audio/*,video/webm;q=0.9,application/ogg;q=0.9,*/*;q=0.1";
/// Path endings of direct audio links.
const EXTENSIONS: &[&str] = &[
    "mp3", "ogg", "oga", "opus", "m4a", "aac", "wav", "flac", "weba", "webm",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioFormat {
    Mp3,
    /// Ogg container: Vorbis, Opus or FLAC.
    Ogg,
    /// Raw AAC in ADTS frames.
    Aac,
    /// MPEG-4 container (m4a).
    Mp4,
    Wav,
    Flac,
    Webm,
}

impl AudioFormat {
    /// The format of a file from its first bytes; `None` when it is none of the supported ones.
    pub fn sniff(data: &[u8]) -> Option<Self> {
        match data {
            [b'I', b'D', b'3', ..] => Some(Self::Mp3),
            [b'O', b'g', b'g', b'S', ..] => Some(Self::Ogg),
            [b'f', b'L', b'a', b'C', ..] => Some(Self::Flac),
            [
                b'R',
                b'I',
                b'F',
                b'F',
                _,
                _,
                _,
                _,
                b'W',
                b'A',
                b'V',
                b'E',
                ..,
            ] => Some(Self::Wav),
            [_, _, _, _, b'f', b't', b'y', b'p', ..] => Some(Self::Mp4),
            [0x1a, 0x45, 0xdf, 0xa3, ..] => Some(Self::Webm),
            // MPEG sync word; layer bits 00 mark AAC in ADTS, the others MPEG audio layers.
            [0xff, b, ..] if b & 0xf6 == 0xf0 => Some(Self::Aac),
            [0xff, b, ..] if b & 0xe0 == 0xe0 && b & 0x06 != 0 => Some(Self::Mp3),
            _ => None,
        }
    }

    pub fn media_type(self) -> &'static str {
        match self {
            Self::Mp3 => "audio/mpeg",
            Self::Ogg => "audio/ogg",
            Self::Aac => "audio/aac",
            Self::Mp4 => "audio/mp4",
            Self::Wav => "audio/wav",
            Self::Flac => "audio/flac",
            Self::Webm => "audio/webm",
        }
    }
}

/// Whether the link's path ends in an audio file extension.
pub fn has_audio_extension(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| EXTENSIONS.contains(&ext))
}

/// Whether a response's media type (lower case, without parameters) announces audio.
pub fn is_audio_type(media_type: &str) -> bool {
    media_type.starts_with("audio/") || media_type == "application/ogg"
}

/// What the card shows before the click.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioInfo {
    /// The link as posted (without fragment).
    pub url: String,
    pub format: AudioFormat,
    /// File size in bytes, when the server said.
    pub size: Option<u64>,
}

/// A fetched audio file, ready for the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFile {
    pub format: AudioFormat,
    pub bytes: Vec<u8>,
}

impl AudioFile {
    /// The file as a `data:` URL of its sniffed media type.
    pub fn data_url(&self) -> String {
        format!(
            "data:{};base64,{}",
            self.format.media_type(),
            STANDARD.encode(&self.bytes)
        )
    }
}

/// Reads the start of the file: format and size, without downloading it.
pub async fn probe(fetcher: &Fetcher, url: &Url) -> Result<AudioInfo, FetchError> {
    let head = fetcher
        .get(url.as_str(), AUDIO_ACCEPT, PROBE_LIMIT, true)
        .await?;
    if head.size.is_some_and(|n| n > AUDIO_LIMIT as u64) {
        return Err(FetchError::TooLarge);
    }
    let format = AudioFormat::sniff(&head.body).ok_or(FetchError::UnsupportedType)?;
    Ok(AudioInfo {
        url: url.to_string(),
        format,
        size: head.size,
    })
}

/// Fetches the whole file (at most [`AUDIO_LIMIT`] bytes) after the user clicked play.
pub async fn fetch(fetcher: &Fetcher, url: &Url) -> Result<AudioFile, FetchError> {
    let file = fetcher
        .get_large(url.as_str(), AUDIO_ACCEPT, AUDIO_LIMIT, AUDIO_TIMEOUT)
        .await?;
    let format = AudioFormat::sniff(&file.body).ok_or(FetchError::UnsupportedType)?;
    Ok(AudioFile {
        format,
        bytes: file.body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_audio_formats_from_the_bytes() {
        let cases: &[(&[u8], AudioFormat)] = &[
            (b"ID3\x04\x00\x00\x00\x00\x00\x00", AudioFormat::Mp3),
            (&[0xff, 0xfb, 0x90, 0x64], AudioFormat::Mp3),
            (&[0xff, 0xf1, 0x50, 0x80], AudioFormat::Aac),
            (b"OggS\x00\x02", AudioFormat::Ogg),
            (b"fLaC\x00\x00\x00\x22", AudioFormat::Flac),
            (b"RIFF\x24\x08\x00\x00WAVEfmt ", AudioFormat::Wav),
            (b"\x00\x00\x00\x20ftypM4A ", AudioFormat::Mp4),
            (&[0x1a, 0x45, 0xdf, 0xa3, 0x9f], AudioFormat::Webm),
        ];
        for (bytes, format) in cases {
            assert_eq!(AudioFormat::sniff(bytes), Some(*format), "{bytes:?}");
        }
        for bytes in [
            &b"<!doctype html>"[..],
            b"\x89PNG\r\n\x1a\n",
            b"RIFF\x24\x08\x00\x00AVI ",
            &[0xff, 0xd8, 0xff, 0xe0],
            b"",
        ] {
            assert_eq!(AudioFormat::sniff(bytes), None, "{bytes:?}");
        }
    }

    #[test]
    fn recognises_audio_extensions() {
        let ext = |u: &str| has_audio_extension(&Url::parse(u).expect("url"));
        assert!(ext("https://files.example.org/demos/night-drive.mp3"));
        assert!(ext("https://files.example.org/a/B.FLAC?download=1"));
        assert!(ext("https://files.example.org/voice.opus"));
        assert!(!ext("https://files.example.org/night-drive.mp3/"));
        assert!(!ext("https://files.example.org/page?file=x.mp3"));
        assert!(!ext("https://files.example.org/mp3"));
        assert!(!ext("https://files.example.org/cover.png"));
    }
}
