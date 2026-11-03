//! The start dialog's "Viewers get" line (FR-7.36): what a share started with the dialog's picks
//! would send, and how it would reach its viewers.

use mumcord_screenshare::Codec;

use super::encoder::ActiveEncoder;
use super::encoder::Av1Skipped;
use super::encoder::select::Selection;
use super::quality::Steps;
use super::signal::MAX_VIEWERS;

/// "{quality} · {fps} fps · {codec} · about {kbps} · {path}" before the share starts.
#[derive(Debug, Clone, PartialEq)]
pub struct SharePreview {
    /// The share's top quality step, e.g. "1080p30".
    pub quality: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Target bitrate of that step for the encoder's codec (FR-7.37).
    pub kbps: u32,
    /// The encoder the share would start on.
    pub encoder: ActiveEncoder,
    /// Manual AV1 would run in software without a passed self-test (FR-7.54).
    pub software_warning: bool,
    /// Why Automatic would not pick AV1.
    pub av1_skipped: Option<Av1Skipped>,
    pub path: SharePath,
}

/// How the share would reach its viewers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharePath {
    /// "sent once via relay, up to {max_viewers} viewers" (fan-out, FR-7.40).
    Broadcast { max_viewers: usize },
    /// "one copy per viewer, up to {max_viewers}".
    Copies { max_viewers: usize },
}

/// The preview of `selection`; `av1_blocked` when a current viewer cannot play AV1, so the
/// share would run VP9 (FR-7.58); `broadcast_max` when fan-out is available.
pub(crate) fn preview(
    selection: &Selection,
    av1_blocked: bool,
    broadcast_max: Option<u8>,
) -> SharePreview {
    // The candidates always end with software VP9, so a blocked AV1 still finds an encoder.
    let encoder = selection
        .candidates
        .iter()
        .find(|c| !av1_blocked || c.codec == Codec::Vp9)
        .map_or(
            ActiveEncoder {
                codec: Codec::Vp9,
                gpu: None,
            },
            |c| ActiveEncoder {
                codec: c.codec,
                gpu: c.gpu(),
            },
        );
    let quality = Steps::new(selection.prefs, encoder.codec).quality();
    let av1 = encoder.codec == Codec::Av1;
    SharePreview {
        quality: quality.label(),
        width: quality.width,
        height: quality.height,
        fps: quality.fps,
        kbps: quality.bitrate_kbps,
        encoder,
        software_warning: av1 && selection.software_warning,
        av1_skipped: if av1 { None } else { selection.av1_skipped },
        path: broadcast_max.map_or(
            SharePath::Copies {
                max_viewers: MAX_VIEWERS,
            },
            |n| SharePath::Broadcast {
                max_viewers: usize::from(n),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use mumcord_store::{ScreenFps, ScreenResolution};

    use super::super::encoder::GpuVendor;
    use super::super::encoder::select::{Candidate, Kind};
    use super::super::quality::QualityPrefs;
    use super::*;

    fn selection(candidates: Vec<Candidate>) -> Selection {
        Selection {
            candidates,
            prefs: QualityPrefs {
                resolution: ScreenResolution::P1440,
                fps: ScreenFps::Fps60,
                ..QualityPrefs::default()
            },
            av1_skipped: None,
            software_warning: true,
        }
    }

    const AV1_GPU: Candidate = Candidate {
        codec: Codec::Av1,
        kind: Kind::FakeHardware,
    };
    const VP9: Candidate = Candidate {
        codec: Codec::Vp9,
        kind: Kind::Software,
    };

    #[test]
    fn names_the_first_encoder_its_rate_and_one_copy_per_viewer() {
        let got = preview(&selection(vec![AV1_GPU, VP9]), false, None);
        assert_eq!(
            got.encoder,
            ActiveEncoder {
                codec: Codec::Av1,
                gpu: Some(GpuVendor::Unknown)
            }
        );
        assert_eq!(
            (got.quality.as_str(), got.fps, got.kbps),
            ("1440p60", 60, 9_000)
        );
        assert!(got.software_warning);
        assert_eq!(got.path, SharePath::Copies { max_viewers: 8 });
    }

    #[test]
    fn a_viewer_without_av1_makes_it_vp9_at_the_vp9_rate() {
        let got = preview(&selection(vec![AV1_GPU, VP9]), true, None);
        assert_eq!(
            got.encoder,
            ActiveEncoder {
                codec: Codec::Vp9,
                gpu: None
            }
        );
        assert_eq!(got.kbps, 12_000, "VP9 needs more for the same step");
        assert!(
            !got.software_warning,
            "the AV1 warning does not apply to VP9"
        );
    }

    #[test]
    fn fan_out_sends_once_via_relay() {
        let got = preview(&selection(vec![VP9]), false, Some(25));
        assert_eq!(got.path, SharePath::Broadcast { max_viewers: 25 });
    }
}
