//! Hardware encoder probe (FR-7.55): hwcodec tests each encoder by opening it and encoding a few
//! real frames, so a driver that claims support but fails is never used. This module keeps what
//! is Mumcord's: the encoders tried per platform, the time budget, the cache ids and the vendor.

use std::time::{Duration, Instant};

use hwcodec::{
    Codec, EncoderKind, EncoderStatus, HardwareEncoder, UnavailableReason, default_order,
    detect_encoder,
};
use mumcord_store::EncoderEntry;

use super::GpuVendor;
use super::select::{Candidate, Kind};
use crate::test_hooks::{ProbeOverride, ProbeResult};

/// The whole probe stays within this (spec 07 §9); encoders not reached in time count as
/// absent until the next check.
pub const BUDGET: Duration = Duration::from_secs(3);

/// The working hardware encoders, best first per codec.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hardware {
    pub av1: Vec<Candidate>,
    pub vp9: Vec<Candidate>,
}

/// Which vendor encoders this platform tries, in order (FR-7.53, FR-7.55). macOS has none.
fn targets() -> Vec<(Codec, HardwareEncoder)> {
    [Codec::Av1, Codec::Vp9]
        .into_iter()
        .flat_map(|codec| {
            default_order(codec)
                .into_iter()
                .filter_map(move |kind| match kind {
                    EncoderKind::Hardware(encoder) => Some((codec, encoder)),
                    _ => None,
                })
        })
        .collect()
}

/// The id an encoder is cached under, e.g. `av1_nvenc`.
pub fn cache_id(codec: Codec, encoder: HardwareEncoder) -> String {
    let codec = match codec {
        Codec::Av1 => "av1",
        Codec::Vp9 => "vp9",
        // hwcodec's enums are open for additions; Mumcord itself only creates AV1 and VP9.
        // Anything else is treated like VP9 and as an unknown encoder.
        _ => "vp9",
    };
    let encoder = match encoder {
        HardwareEncoder::Nvenc => "nvenc",
        HardwareEncoder::Vaapi => "vaapi",
        HardwareEncoder::Amf => "amf",
        HardwareEncoder::Qsv => "qsv",
        _ => "other",
    };
    format!("{codec}_{encoder}")
}

/// Probes every encoder of this platform; returns one cache entry each.
pub fn probe() -> Vec<EncoderEntry> {
    let started = Instant::now();
    targets()
        .into_iter()
        .map(|(codec, encoder)| {
            let (available, smoke_ok) = if started.elapsed() < BUDGET {
                probe_one(codec, encoder)
            } else {
                tracing::warn!(%encoder, ?codec, "encoder probe out of time");
                (false, false)
            };
            EncoderEntry {
                encoder: cache_id(codec, encoder),
                available,
                smoke_ok,
            }
        })
        .collect()
}

/// Whether the encoder opened and whether it then passed the test encode, as the cache stores it.
fn probe_one(codec: Codec, encoder: HardwareEncoder) -> (bool, bool) {
    let report = detect_encoder(codec, encoder);
    // Spec 07 §9: every probe result and its reason at info. Device and driver version name the
    // graphics hardware only, never the user.
    let device = report.device.as_deref();
    let driver_version = report.driver_version.as_deref();
    match report.status {
        EncoderStatus::Works => {
            tracing::info!(%encoder, ?codec, device, driver_version, "hardware encoder works");
            (true, true)
        }
        EncoderStatus::Unavailable { reason, detail, .. } => {
            tracing::info!(
                %encoder, ?codec, %reason, %detail, device, driver_version,
                "hardware encoder unavailable"
            );
            (reason == UnavailableReason::TestEncodeFailed, false)
        }
        // hwcodec's status is open for additions; a state Mumcord does not know is not usable.
        status => {
            tracing::info!(
                %encoder, ?codec, %status, device, driver_version,
                "hardware encoder in an unknown state"
            );
            (false, false)
        }
    }
}

/// The working encoders among cached entries.
pub fn from_entries(entries: &[EncoderEntry]) -> Hardware {
    let mut hardware = Hardware::default();
    for (codec, encoder) in targets() {
        let id = cache_id(codec, encoder);
        if !entries
            .iter()
            .any(|e| e.encoder == id && e.available && e.smoke_ok)
        {
            continue;
        }
        let candidate = Candidate {
            codec,
            kind: Kind::Hardware(encoder, vendor(encoder)),
        };
        match codec {
            Codec::Av1 => hardware.av1.push(candidate),
            _ => hardware.vp9.push(candidate),
        }
    }
    hardware
}

/// The probe result a debug test hook forces (`av1_hw`, `vp9_hw`, AC-7.36/37); anything not
/// listed is absent.
pub fn from_overrides(overrides: &[ProbeOverride]) -> Hardware {
    let mut hardware = Hardware::default();
    for o in overrides {
        let codec = match o.encoder.as_str() {
            "av1_hw" => Codec::Av1,
            "vp9_hw" => Codec::Vp9,
            other => {
                tracing::warn!(encoder = other, "unknown encoder in the probe test hook");
                continue;
            }
        };
        if o.result != ProbeResult::Ok {
            continue;
        }
        let candidate = Candidate {
            codec,
            kind: Kind::FakeHardware,
        };
        match codec {
            Codec::Av1 => hardware.av1.push(candidate),
            _ => hardware.vp9.push(candidate),
        }
    }
    hardware
}

fn vendor(encoder: HardwareEncoder) -> GpuVendor {
    match encoder {
        HardwareEncoder::Nvenc => GpuVendor::Nvidia,
        HardwareEncoder::Amf => GpuVendor::Amd,
        HardwareEncoder::Qsv => GpuVendor::Intel,
        HardwareEncoder::Vaapi => render_node_vendor(),
        _ => GpuVendor::Unknown,
    }
}

/// VA-API serves Intel and AMD alike; the first DRM render node's PCI vendor tells which.
fn render_node_vendor() -> GpuVendor {
    (128..136)
        .find_map(|n| {
            std::fs::read_to_string(format!("/sys/class/drm/renderD{n}/device/vendor")).ok()
        })
        .map_or(GpuVendor::Unknown, |id| match id.trim() {
            "0x8086" => GpuVendor::Intel,
            "0x1002" => GpuVendor::Amd,
            "0x10de" => GpuVendor::Nvidia,
            _ => GpuVendor::Unknown,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_covers_every_platform_encoder_within_the_budget() {
        let started = Instant::now();
        let entries = probe();
        assert!(started.elapsed() < BUDGET + Duration::from_secs(1));
        assert_eq!(entries.len(), targets().len());
        // Whatever this machine has, nothing failing its smoke test is offered.
        let found = from_entries(&entries);
        assert!(found.av1.len() + found.vp9.len() <= entries.iter().filter(|e| e.smoke_ok).count());
    }

    #[test]
    fn only_entries_that_passed_the_smoke_test_count() {
        let Some(&(codec, encoder)) = targets().first() else {
            return;
        };
        let entry = |available, smoke_ok| EncoderEntry {
            encoder: cache_id(codec, encoder),
            available,
            smoke_ok,
        };
        assert_eq!(from_entries(&[entry(true, false)]), Hardware::default());
        assert_eq!(from_entries(&[entry(false, false)]), Hardware::default());
        let found = from_entries(&[entry(true, true)]);
        assert_eq!(found.av1.len() + found.vp9.len(), 1);
    }

    #[test]
    fn the_hook_forces_hardware_on_or_off() {
        let overrides = [
            ProbeOverride {
                encoder: "av1_hw".into(),
                result: ProbeResult::SmokeFail,
            },
            ProbeOverride {
                encoder: "vp9_hw".into(),
                result: ProbeResult::Ok,
            },
        ];
        let forced = from_overrides(&overrides);
        assert!(forced.av1.is_empty());
        assert_eq!(
            forced.vp9,
            [Candidate {
                codec: Codec::Vp9,
                kind: Kind::FakeHardware
            }]
        );
    }

    #[test]
    fn cache_ids_are_codec_and_vendor() {
        assert_eq!(cache_id(Codec::Av1, HardwareEncoder::Nvenc), "av1_nvenc");
        assert_eq!(cache_id(Codec::Vp9, HardwareEncoder::Qsv), "vp9_qsv");
    }
}
