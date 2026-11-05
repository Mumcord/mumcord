//! Checks of the sound files a package bundles (FR-9.65): Ogg Vorbis or WAV, each at most 10 s
//! and 1 MiB. Packages come from strangers, so the files are decoded here once at install time
//! to read their real length instead of trusting a header.

use std::io::Cursor;

use lewton::inside_ogg::OggStreamReader;

/// Longest sound.
pub const MAX_SOUND_SECONDS: f64 = 10.0;
/// Largest sound file.
pub const MAX_SOUND_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SoundError {
    #[error("The sound \"{0}\" is not in the package")]
    Missing(String),
    #[error("The sound \"{0}\" is not an Ogg Vorbis or WAV file")]
    BadFormat(String),
    #[error("The sound \"{0}\" is larger than 1 MiB")]
    TooLarge(String),
    #[error("The sound \"{0}\" is longer than 10 seconds")]
    TooLong(String),
}

/// Checks one bundled sound file given its package path and bytes.
pub fn check(name: &str, data: &[u8]) -> Result<(), SoundError> {
    if data.len() > MAX_SOUND_BYTES {
        return Err(SoundError::TooLarge(name.to_owned()));
    }
    let seconds = if name.ends_with(".wav") {
        wav_seconds(data)
    } else if name.ends_with(".ogg") {
        ogg_seconds(data)
    } else {
        None
    }
    .ok_or_else(|| SoundError::BadFormat(name.to_owned()))?;
    if seconds > MAX_SOUND_SECONDS {
        return Err(SoundError::TooLong(name.to_owned()));
    }
    Ok(())
}

fn wav_seconds(data: &[u8]) -> Option<f64> {
    let reader = hound::WavReader::new(Cursor::new(data)).ok()?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        return None;
    }
    let frames = f64::from(reader.len()) / f64::from(spec.channels);
    Some(frames / f64::from(spec.sample_rate))
}

fn ogg_seconds(data: &[u8]) -> Option<f64> {
    let mut reader = OggStreamReader::new(Cursor::new(data)).ok()?;
    let rate = f64::from(reader.ident_hdr.audio_sample_rate);
    let channels = f64::from(reader.ident_hdr.audio_channels);
    if rate == 0.0 || channels == 0.0 {
        return None;
    }
    let mut samples = 0usize;
    // Stop early once past the limit so a hostile file cannot make us decode for long.
    let limit = (MAX_SOUND_SECONDS * rate * channels) as usize + 1;
    while let Some(packet) = reader.read_dec_packet_itl().ok()? {
        samples += packet.len();
        if samples > limit {
            break;
        }
    }
    Some(samples as f64 / channels / rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(seconds: u32, rate: u32) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 8,
            sample_format: hound::SampleFormat::Int,
        };
        let mut out = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut out, spec).expect("writer");
        for _ in 0..seconds * rate {
            writer.write_sample(0i8).expect("sample");
        }
        writer.finalize().expect("finalize");
        out.into_inner()
    }

    #[test]
    fn accepts_a_short_wav() {
        assert_eq!(check("sounds/a.wav", &wav(2, 8000)), Ok(()));
        assert_eq!(check("sounds/a.wav", &wav(10, 8000)), Ok(()));
    }

    #[test]
    fn refuses_long_big_and_broken_files() {
        assert_eq!(
            check("sounds/a.wav", &wav(11, 8000)),
            Err(SoundError::TooLong("sounds/a.wav".into()))
        );
        assert_eq!(
            check("sounds/a.wav", &wav(30, 44100)),
            Err(SoundError::TooLarge("sounds/a.wav".into()))
        );
        assert_eq!(
            check("sounds/a.wav", b"not a wav"),
            Err(SoundError::BadFormat("sounds/a.wav".into()))
        );
        assert_eq!(
            check("sounds/a.ogg", b"OggS but not really"),
            Err(SoundError::BadFormat("sounds/a.ogg".into()))
        );
        assert_eq!(
            check("sounds/a.mp3", &wav(1, 8000)),
            Err(SoundError::BadFormat("sounds/a.mp3".into()))
        );
    }
}
