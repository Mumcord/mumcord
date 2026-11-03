//! Which encoder a share uses (FR-7.53, FR-7.54, FR-7.37's software AV1 cap): a pure decision
//! from the settings, the probe and self-test results and the power state, so every rule is
//! testable without a GPU.

use mumcord_screenshare::{Codec, HardwareEncoder};
use mumcord_store::{ScreenCodec, ScreenFps, ScreenResolution};

use super::GpuVendor;
use crate::screen_share::quality::{AUTO_HEIGHT, QualityPrefs};

/// Highest size and rate software AV1 is used for (FR-7.37).
pub const SOFTWARE_AV1_CAP: (u32, u32) = (1080, 30);

/// One encoder the share can try.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    pub codec: Codec,
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Hardware(HardwareEncoder, GpuVendor),
    /// A hardware encoder a debug test hook claims exists; the software encoder of the same
    /// codec stands in for it so end-to-end tests see the hardware path in the UI.
    FakeHardware,
    Software,
}

impl Candidate {
    pub fn gpu(&self) -> Option<GpuVendor> {
        match self.kind {
            Kind::Hardware(_, vendor) => Some(vendor),
            Kind::FakeHardware => Some(GpuVendor::Unknown),
            Kind::Software => None,
        }
    }
}

/// Why Automatic did not pick AV1, for "VP9 (AV1 too heavy for this PC)" (FR-7.60).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Av1Skipped {
    /// The self-test failed or has not finished for these settings.
    TooHeavy,
    OnBattery,
    /// The picked quality is above what software AV1 is used for (FR-7.37).
    AboveSoftwareLimit,
}

/// Everything the selection looks at.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub codec: ScreenCodec,
    pub use_gpu: bool,
    /// Working hardware encoders (probe and smoke test passed), best first.
    pub av1_hardware: &'a [Candidate],
    pub vp9_hardware: &'a [Candidate],
    /// Whether the software AV1 self-test passed for the share's size, rate and Optimize for.
    pub selftest_passed: bool,
    pub on_battery: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    /// Encoders to try in order; the last one is always VP9 in software.
    pub candidates: Vec<Candidate>,
    /// The share's prefs, capped to 1080p30 when only software AV1 is left (manual AV1).
    pub prefs: QualityPrefs,
    pub av1_skipped: Option<Av1Skipped>,
    /// Manual AV1 runs in software without a passed self-test: the dialog warns "AV1 in
    /// software may be too heavy for this PC" (FR-7.54).
    pub software_warning: bool,
}

const VP9_SOFTWARE: Candidate = Candidate {
    codec: Codec::Vp9,
    kind: Kind::Software,
};
const AV1_SOFTWARE: Candidate = Candidate {
    codec: Codec::Av1,
    kind: Kind::Software,
};

pub fn select(inputs: &Inputs<'_>, prefs: QualityPrefs) -> Selection {
    let av1_hw: &[Candidate] = if inputs.use_gpu {
        inputs.av1_hardware
    } else {
        &[]
    };
    let vp9_hw: &[Candidate] = if inputs.use_gpu {
        inputs.vp9_hardware
    } else {
        &[]
    };
    let mut candidates = Vec::new();
    let mut av1_skipped = None;
    let mut software_warning = false;
    let mut prefs = prefs;
    match inputs.codec {
        ScreenCodec::Auto => {
            candidates.extend_from_slice(av1_hw);
            if av1_hw.is_empty() {
                av1_skipped = if above_software_cap(prefs) {
                    Some(Av1Skipped::AboveSoftwareLimit)
                } else if inputs.on_battery {
                    Some(Av1Skipped::OnBattery)
                } else if !inputs.selftest_passed {
                    Some(Av1Skipped::TooHeavy)
                } else {
                    None
                };
                if av1_skipped.is_none() {
                    candidates.push(AV1_SOFTWARE);
                }
            }
            candidates.extend_from_slice(vp9_hw);
        }
        ScreenCodec::Av1 => {
            candidates.extend_from_slice(av1_hw);
            if av1_hw.is_empty() {
                prefs = cap_for_software_av1(prefs);
                software_warning = !inputs.selftest_passed;
            }
            candidates.push(AV1_SOFTWARE);
        }
        ScreenCodec::Vp9 => candidates.extend_from_slice(vp9_hw),
    }
    candidates.push(VP9_SOFTWARE);
    Selection {
        candidates,
        prefs,
        av1_skipped,
        software_warning,
    }
}

/// The size and rate the prefs ask for at most, e.g. (1080, 30).
pub fn ceiling(prefs: QualityPrefs) -> (u32, u32) {
    let height = match prefs.resolution {
        ScreenResolution::Auto => AUTO_HEIGHT,
        ScreenResolution::P720 => 720,
        ScreenResolution::P1080 => 1080,
        ScreenResolution::P1440 => 1440,
    };
    let fps = match prefs.fps {
        ScreenFps::Fps15 => 15,
        ScreenFps::Fps30 => 30,
        ScreenFps::Fps60 => 60,
    };
    (height, fps)
}

pub fn above_software_cap(prefs: QualityPrefs) -> bool {
    let (height, fps) = ceiling(prefs);
    height > SOFTWARE_AV1_CAP.0 || fps > SOFTWARE_AV1_CAP.1
}

/// Lowers the prefs to at most 1080p30 for software AV1.
pub fn cap_for_software_av1(prefs: QualityPrefs) -> QualityPrefs {
    let (height, fps) = ceiling(prefs);
    QualityPrefs {
        resolution: if height > SOFTWARE_AV1_CAP.0 {
            ScreenResolution::P1080
        } else {
            prefs.resolution
        },
        fps: if fps > SOFTWARE_AV1_CAP.1 {
            ScreenFps::Fps30
        } else {
            prefs.fps
        },
        ..prefs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NVENC: Candidate = Candidate {
        codec: Codec::Av1,
        kind: Kind::Hardware(HardwareEncoder::Nvenc, GpuVendor::Nvidia),
    };
    const VP9_QSV: Candidate = Candidate {
        codec: Codec::Vp9,
        kind: Kind::Hardware(HardwareEncoder::Qsv, GpuVendor::Intel),
    };

    fn inputs<'a>(av1: &'a [Candidate], vp9: &'a [Candidate]) -> Inputs<'a> {
        Inputs {
            codec: ScreenCodec::Auto,
            use_gpu: true,
            av1_hardware: av1,
            vp9_hardware: vp9,
            selftest_passed: true,
            on_battery: false,
        }
    }

    fn prefs(resolution: ScreenResolution, fps: ScreenFps) -> QualityPrefs {
        QualityPrefs {
            resolution,
            fps,
            ..QualityPrefs::default()
        }
    }

    fn order(selection: &Selection) -> Vec<Candidate> {
        selection.candidates.clone()
    }

    #[test]
    fn automatic_follows_the_order_of_fr_7_53() {
        let all = select(&inputs(&[NVENC], &[VP9_QSV]), QualityPrefs::default());
        assert_eq!(order(&all), [NVENC, VP9_QSV, VP9_SOFTWARE]);
        let no_av1_gpu = select(&inputs(&[], &[VP9_QSV]), QualityPrefs::default());
        assert_eq!(order(&no_av1_gpu), [AV1_SOFTWARE, VP9_QSV, VP9_SOFTWARE]);
        assert_eq!(no_av1_gpu.av1_skipped, None);
        let nothing = select(&inputs(&[], &[]), QualityPrefs::default());
        assert_eq!(order(&nothing), [AV1_SOFTWARE, VP9_SOFTWARE]);
    }

    #[test]
    fn automatic_skips_software_av1_when_too_heavy_on_battery_or_above_1080p30() {
        let mut heavy = inputs(&[], &[VP9_QSV]);
        heavy.selftest_passed = false;
        let picked = select(&heavy, QualityPrefs::default());
        assert_eq!(order(&picked), [VP9_QSV, VP9_SOFTWARE]);
        assert_eq!(picked.av1_skipped, Some(Av1Skipped::TooHeavy));

        // AC-7.40: battery, no hardware AV1 → VP9.
        let mut battery = inputs(&[], &[]);
        battery.on_battery = true;
        let picked = select(&battery, QualityPrefs::default());
        assert_eq!(order(&picked), [VP9_SOFTWARE]);
        assert_eq!(picked.av1_skipped, Some(Av1Skipped::OnBattery));

        for high in [
            prefs(ScreenResolution::P1440, ScreenFps::Fps30),
            prefs(ScreenResolution::P1080, ScreenFps::Fps60),
        ] {
            let picked = select(&inputs(&[], &[]), high);
            assert_eq!(order(&picked), [VP9_SOFTWARE]);
            assert_eq!(picked.av1_skipped, Some(Av1Skipped::AboveSoftwareLimit));
            assert_eq!(picked.prefs, high);
        }
    }

    #[test]
    fn battery_and_the_cap_do_not_matter_with_a_hardware_av1_encoder() {
        let mut battery = inputs(&[NVENC], &[]);
        battery.on_battery = true;
        let high = prefs(ScreenResolution::P1440, ScreenFps::Fps60);
        let picked = select(&battery, high);
        assert_eq!(order(&picked), [NVENC, VP9_SOFTWARE]);
        assert_eq!(picked.prefs, high);
        assert_eq!(picked.av1_skipped, None);
    }

    #[test]
    fn graphics_card_off_uses_no_hardware_encoder() {
        let mut off = inputs(&[NVENC], &[VP9_QSV]);
        off.use_gpu = false;
        assert_eq!(
            order(&select(&off, QualityPrefs::default())),
            [AV1_SOFTWARE, VP9_SOFTWARE]
        );
        off.codec = ScreenCodec::Vp9;
        assert_eq!(
            order(&select(&off, QualityPrefs::default())),
            [VP9_SOFTWARE]
        );
    }

    #[test]
    fn manual_av1_uses_software_without_a_passed_selftest_and_caps_it() {
        let mut manual = inputs(&[], &[VP9_QSV]);
        manual.codec = ScreenCodec::Av1;
        manual.selftest_passed = false;
        manual.on_battery = true;
        let picked = select(&manual, prefs(ScreenResolution::P1440, ScreenFps::Fps60));
        assert_eq!(order(&picked), [AV1_SOFTWARE, VP9_SOFTWARE]);
        assert!(picked.software_warning);
        assert_eq!(
            picked.prefs,
            prefs(ScreenResolution::P1080, ScreenFps::Fps30)
        );

        manual.av1_hardware = &[NVENC];
        let picked = select(&manual, prefs(ScreenResolution::P1440, ScreenFps::Fps60));
        assert_eq!(order(&picked), [NVENC, AV1_SOFTWARE, VP9_SOFTWARE]);
        assert!(!picked.software_warning);
        assert_eq!(
            picked.prefs,
            prefs(ScreenResolution::P1440, ScreenFps::Fps60)
        );
    }

    #[test]
    fn manual_vp9_ignores_av1() {
        let mut manual = inputs(&[NVENC], &[VP9_QSV]);
        manual.codec = ScreenCodec::Vp9;
        let picked = select(&manual, QualityPrefs::default());
        assert_eq!(order(&picked), [VP9_QSV, VP9_SOFTWARE]);
        assert_eq!(picked.av1_skipped, None);
    }

    #[test]
    fn the_cap_keeps_lower_picks() {
        let low = prefs(ScreenResolution::P720, ScreenFps::Fps15);
        assert_eq!(cap_for_software_av1(low), low);
        assert!(!above_software_cap(QualityPrefs::default()));
        assert_eq!(ceiling(QualityPrefs::default()), (1080, 30));
    }
}
