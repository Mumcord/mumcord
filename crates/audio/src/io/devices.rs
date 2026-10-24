//! Device list filtering (FR-1.4): show a short, meaningful list.
//!
//! cpal's ALSA host reports every PCM alias (raw `hw:`, `plughw:`, `surround*`, resampler
//! plugins, ...). Only the sound-server entries and one friendly entry per sound card are kept.
//! IDs are never rewritten, so stored settings keep working.

use std::collections::HashSet;

use super::DeviceInfo;

/// Sound-server PCMs that are kept on ALSA.
const ALSA_KEEP: [&str; 3] = ["default", "pipewire", "pulse"];

/// Whether an ALSA PCM name (the part after `alsa:`) is worth showing.
fn keep_alsa(pcm: &str) -> bool {
    ALSA_KEEP.contains(&pcm) || pcm.starts_with("sysdefault:CARD=")
}

/// Drops duplicate and raw ALSA entries and puts the OS default first, otherwise keeping order.
pub(crate) fn filter(devices: Vec<DeviceInfo>) -> Vec<DeviceInfo> {
    let mut seen = HashSet::new();
    let mut kept: Vec<DeviceInfo> = devices
        .into_iter()
        .filter(|d| d.is_default || d.id.strip_prefix("alsa:").is_none_or(keep_alsa))
        .filter(|d| seen.insert(d.id.clone()))
        .collect();
    // Stable sort: defaults first, everything else in original order.
    kept.sort_by_key(|d| !d.is_default);
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, name: &str, is_default: bool) -> DeviceInfo {
        DeviceInfo {
            id: id.to_owned(),
            name: name.to_owned(),
            is_default,
        }
    }

    #[test]
    fn alsa_list_is_reduced_to_servers_and_cards() {
        let input = vec![
            dev("alsa:null", "Discard", false),
            dev("alsa:sysdefault", "Default Audio Device", false),
            dev("alsa:lavrate", "Rate Converter", false),
            dev("alsa:pipewire", "PipeWire Sound Server", false),
            dev("alsa:pulse", "PulseAudio Sound Server", false),
            dev("alsa:default", "Default ALSA Output", true),
            dev(
                "alsa:sysdefault:CARD=II",
                "RODECaster Pro II, USB Audio",
                false,
            ),
            dev(
                "alsa:front:CARD=II,DEV=0",
                "RODECaster Pro II, USB Audio",
                false,
            ),
            dev(
                "alsa:surround51:CARD=II,DEV=0",
                "RODECaster Pro II, USB Audio",
                false,
            ),
            dev("alsa:hdmi:CARD=HDMI,DEV=0", "HDA ATI HDMI", false),
            dev("alsa:usbstream:CARD=II", "RODECaster Pro II", false),
            dev(
                "alsa:hw:CARD=0,DEV=0",
                "RODECaster Pro II, USB Audio",
                false,
            ),
            dev(
                "alsa:plughw:CARD=0,DEV=0",
                "RODECaster Pro II, USB Audio",
                false,
            ),
        ];
        let ids: Vec<String> = filter(input).into_iter().map(|d| d.id).collect();
        assert_eq!(
            ids,
            [
                "alsa:default",
                "alsa:pipewire",
                "alsa:pulse",
                "alsa:sysdefault:CARD=II"
            ]
        );
    }

    #[test]
    fn non_alsa_devices_pass_through_without_duplicates() {
        let input = vec![
            dev("wasapi:a", "Speakers", false),
            dev("wasapi:b", "Headset", true),
            dev("wasapi:a", "Speakers", false),
        ];
        let out = filter(input);
        let ids: Vec<&str> = out.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["wasapi:b", "wasapi:a"]);
    }

    #[test]
    fn default_not_filtered_even_if_unusual() {
        let out = filter(vec![dev("alsa:hw:CARD=0,DEV=0", "Raw", true)]);
        assert_eq!(out.len(), 1);
    }
}
