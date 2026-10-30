//! Device list filtering (FR-1.4): show a short, meaningful list.
//!
//! cpal's ALSA host reports every PCM alias (raw `hw:`, `plughw:`, `surround*`, resampler
//! plugins, ...). Only the sound-server entries and one friendly entry per sound card are kept.
//! IDs are never rewritten, so stored settings keep working.
//!
//! On a PipeWire desktop the list is the sound server's sinks and sources instead (issue #183),
//! identified by their node names; [`migrate`] maps a choice saved from the ALSA list onto them.

use std::collections::HashSet;

use super::DeviceInfo;
#[cfg(target_os = "linux")]
use super::DeviceMigration;

#[cfg(target_os = "linux")]
/// A sink or source as the sound server reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerNode {
    /// Stable across restarts and replugging, e.g. `alsa_output.usb-…HiFi__Line1__sink`.
    pub node_name: String,
    /// What the desktop shows, e.g. "RØDECaster Pro II Chat".
    pub description: Option<String>,
    /// The ALSA card id (`CARD=…`) of a node backed by a sound card.
    pub alsa_card: Option<String>,
}

#[cfg(target_os = "linux")]
/// The device list for the sound server's nodes: ids are `prefix` + node name, names the
/// friendly descriptions, the default first.
pub(crate) fn server_devices(
    prefix: &str,
    nodes: &[ServerNode],
    default: Option<&str>,
) -> Vec<DeviceInfo> {
    let infos = nodes
        .iter()
        .map(|node| DeviceInfo {
            id: format!("{prefix}{}", node.node_name),
            name: node
                .description
                .clone()
                .filter(|d| !d.trim().is_empty())
                .unwrap_or_else(|| node.node_name.clone()),
            is_default: default == Some(node.node_name.as_str()),
        })
        .collect();
    filter(infos)
}

#[cfg(target_os = "linux")]
/// ALSA PCMs that stood for the sound server's default device.
const ALSA_SERVER_DEFAULTS: [&str; 4] = ["default", "sysdefault", "pipewire", "pulse"];

#[cfg(target_os = "linux")]
/// Maps a device id saved from the ALSA list onto the sound server's nodes: a sound card with
/// exactly one node in this direction becomes that node; the server's own default PCMs become
/// the system default; anything else (a card with several nodes, an unplugged card, a raw PCM)
/// falls back to the system default with a notice, as there is no way to tell which node was
/// meant. Ids from the node list are kept.
pub(crate) fn migrate(saved: &str, prefix: &str, nodes: &[ServerNode]) -> DeviceMigration {
    let Some(pcm) = saved.strip_prefix("alsa:") else {
        return DeviceMigration::Keep;
    };
    if ALSA_SERVER_DEFAULTS.contains(&pcm) {
        return DeviceMigration::Default { notice: false };
    }
    let card = pcm
        .split_once("CARD=")
        .map(|(_, rest)| rest.split(',').next().unwrap_or(rest));
    let mut matching = nodes
        .iter()
        .filter(|node| card.is_some() && node.alsa_card.as_deref() == card);
    match (matching.next(), matching.next()) {
        (Some(node), None) => DeviceMigration::Replace(format!("{prefix}{}", node.node_name)),
        _ => DeviceMigration::Default { notice: true },
    }
}

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

    #[cfg(target_os = "linux")]
    mod server {
        use super::*;

        fn node(name: &str, description: &str, card: Option<&str>) -> ServerNode {
            ServerNode {
                node_name: name.to_owned(),
                description: Some(description.to_owned()),
                alsa_card: card.map(str::to_owned),
            }
        }

        const RODE_SYSTEM: &str = "alsa_output.usb-R__DE_RODECaster_Pro_II-00.HiFi__Speaker__sink";
        const RODE_CHAT: &str = "alsa_output.usb-R__DE_RODECaster_Pro_II-00.HiFi__Line1__sink";
        const HDMI: &str = "alsa_output.pci-0000_08_00.1.hdmi-stereo";

        fn rodecaster_outputs() -> Vec<ServerNode> {
            vec![
                node(HDMI, "Navi 48 HDMI/DP Audio", Some("HDMI")),
                node(RODE_SYSTEM, "RØDECaster Pro II System", Some("II")),
                node(RODE_CHAT, "RØDECaster Pro II Chat", Some("II")),
                node("easyeffects_sink", "Easy Effects Sink", None),
            ]
        }

        #[test]
        fn every_node_is_its_own_device_with_its_friendly_name() {
            let list = server_devices("pipewire:", &rodecaster_outputs(), Some(RODE_SYSTEM));
            let shown: Vec<(&str, &str, bool)> = list
                .iter()
                .map(|d| (d.id.as_str(), d.name.as_str(), d.is_default))
                .collect();
            assert_eq!(
                shown,
                [
                    (
                        "pipewire:alsa_output.usb-R__DE_RODECaster_Pro_II-00.HiFi__Speaker__sink",
                        "RØDECaster Pro II System",
                        true
                    ),
                    (
                        "pipewire:alsa_output.pci-0000_08_00.1.hdmi-stereo",
                        "Navi 48 HDMI/DP Audio",
                        false
                    ),
                    (
                        "pipewire:alsa_output.usb-R__DE_RODECaster_Pro_II-00.HiFi__Line1__sink",
                        "RØDECaster Pro II Chat",
                        false
                    ),
                    ("pipewire:easyeffects_sink", "Easy Effects Sink", false),
                ]
            );
        }

        #[test]
        fn ids_do_not_depend_on_order_or_default() {
            let mut nodes = rodecaster_outputs();
            let before = server_devices("pipewire:", &nodes, Some(RODE_SYSTEM));
            nodes.reverse();
            let after = server_devices("pipewire:", &nodes, Some(RODE_CHAT));
            let ids = |list: &[DeviceInfo]| {
                let mut ids: Vec<String> = list.iter().map(|d| d.id.clone()).collect();
                ids.sort();
                ids
            };
            assert_eq!(ids(&before), ids(&after));
            assert!(after[0].is_default && after[0].name.ends_with("Chat"));
        }

        #[test]
        fn a_node_without_description_is_named_by_its_node_name() {
            let nodes = [ServerNode {
                node_name: "null-sink".into(),
                description: Some(" ".into()),
                alsa_card: None,
            }];
            let list = server_devices("pipewire:", &nodes, None);
            assert_eq!(list[0].name, "null-sink");
            assert!(!list[0].is_default, "no default known");
        }

        #[test]
        fn saved_server_defaults_become_the_system_default_silently() {
            for saved in [
                "alsa:default",
                "alsa:pipewire",
                "alsa:pulse",
                "alsa:sysdefault",
            ] {
                assert_eq!(
                    migrate(saved, "pipewire:", &rodecaster_outputs()),
                    DeviceMigration::Default { notice: false },
                    "{saved}"
                );
            }
        }

        #[test]
        fn a_card_with_one_node_migrates_to_that_node() {
            assert_eq!(
                migrate(
                    "alsa:sysdefault:CARD=HDMI",
                    "pipewire:",
                    &rodecaster_outputs()
                ),
                DeviceMigration::Replace(format!("pipewire:{HDMI}"))
            );
            assert_eq!(
                migrate(
                    "alsa:hw:CARD=HDMI,DEV=3",
                    "pipewire:",
                    &rodecaster_outputs()
                ),
                DeviceMigration::Replace(format!("pipewire:{HDMI}"))
            );
        }

        #[test]
        fn an_ambiguous_or_missing_card_falls_back_with_a_notice() {
            for saved in [
                "alsa:sysdefault:CARD=II",
                "alsa:sysdefault:CARD=Unplugged",
                "alsa:hw:0,0",
            ] {
                assert_eq!(
                    migrate(saved, "pipewire:", &rodecaster_outputs()),
                    DeviceMigration::Default { notice: true },
                    "{saved}"
                );
            }
        }

        #[test]
        fn ids_that_are_not_from_alsa_are_kept() {
            for saved in [format!("pipewire:{RODE_CHAT}"), "wasapi:{0.0.0}".to_owned()] {
                assert_eq!(
                    migrate(&saved, "pipewire:", &rodecaster_outputs()),
                    DeviceMigration::Keep
                );
            }
        }
    }
}
