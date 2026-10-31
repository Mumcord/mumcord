//! Reads the official client's settings: `mumble_settings.json` (1.5) or the older
//! `QSettings` store (1.4: INI file on Linux, property list on macOS, registry on Windows).

use std::collections::HashMap;
use std::path::PathBuf;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value as Json;

use super::qt::{self, TextValue, Variant};

/// Mumble's global shortcut index of push-to-talk (`GlobalShortcutType::PushToTalk`).
const PTT_INDEX: i64 = 1;

/// How Mumble decides when to send voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transmit {
    Continuous,
    VoiceActivation,
    PushToTalk,
}

/// What the importer uses from Mumble's settings. The certificate stays raw PKCS#12 and is
/// only decoded when it is imported.
#[derive(Default, Clone, PartialEq)]
pub struct MumbleSettings {
    pub certificate: Option<Vec<u8>>,
    /// The last user name typed in the connect dialog; a fallback for favourites without one.
    pub username: Option<String>,
    pub transmit: Option<Transmit>,
    /// Output volume as a factor (1.0 = unchanged).
    pub volume: Option<f64>,
    /// Device ids or names Mumble stored for its audio system, best match first.
    pub input_devices: Vec<String>,
    pub output_devices: Vec<String>,
    /// Buttons of the push-to-talk shortcut, `None` when none is set.
    pub ptt_buttons: Option<Vec<Variant>>,
    pub database_location: Option<PathBuf>,
}

impl std::fmt::Debug for MumbleSettings {
    /// Leaves out the certificate: it is the user's identity and must never reach a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MumbleSettings")
            .field("has_certificate", &self.certificate.is_some())
            .field("transmit", &self.transmit)
            .field("volume", &self.volume)
            .field("ptt_buttons", &self.ptt_buttons.as_ref().map(Vec::len))
            .finish_non_exhaustive()
    }
}

/// Audio backends by the name Mumble stores as the audio system, with their settings key.
const BACKENDS: [(&str, &str); 7] = [
    ("PipeWire", "pipewire"),
    ("PulseAudio", "pulseaudio"),
    ("ALSA", "alsa"),
    ("WASAPI", "wasapi"),
    ("CoreAudio", "coreaudio"),
    ("OSS", "oss"),
    ("PortAudio", "portaudio"),
];

/// Device names of the chosen audio system first, then those of the other systems.
fn ordered_devices(system: Option<&str>, lookup: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let mut keys: Vec<&str> = BACKENDS.iter().map(|(_, key)| *key).collect();
    if let Some(chosen) = system.and_then(|s| BACKENDS.iter().find(|(name, _)| *name == s)) {
        keys.retain(|k| *k != chosen.1);
        keys.insert(0, chosen.1);
    }
    let mut found: Vec<String> = Vec::new();
    for key in keys {
        if let Some(device) = lookup(key).filter(|d| !d.is_empty())
            && !found.contains(&device)
        {
            found.push(device);
        }
    }
    found
}

fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

/// Parses Mumble 1.5's `mumble_settings.json`. Only values that differ from Mumble's defaults
/// are in the file, so a missing value means the default.
pub fn from_json(text: &str) -> Option<MumbleSettings> {
    let json: Json = serde_json::from_str(text).ok()?;
    let str_at = |category: &str, key: &str| -> Option<String> {
        json.get(category)?.get(key)?.as_str().and_then(non_empty)
    };
    let certificate = json
        .get("certificate")
        .and_then(Json::as_str)
        .and_then(|b| BASE64.decode(b.trim()).ok())
        .filter(|c| !c.is_empty());
    let transmit = str_at("audio", "transmit_mode").and_then(|t| match t.as_str() {
        "Continuous" => Some(Transmit::Continuous),
        "VAD" => Some(Transmit::VoiceActivation),
        "PTT" => Some(Transmit::PushToTalk),
        _ => None,
    });
    let input_system = str_at("audio", "input_system");
    let output_system = str_at("audio", "output_system");
    let ptt_buttons = json
        .get("shortcuts")
        .and_then(|s| s.get("defined"))
        .and_then(Json::as_array)
        .and_then(|defined| {
            defined
                .iter()
                .find(|s| s.get("index").and_then(Json::as_i64) == Some(PTT_INDEX))
        })
        .and_then(|s| s.get("buttons")?.as_array())
        .map(|buttons| {
            buttons
                .iter()
                .map(|b| {
                    b.as_str()
                        .and_then(|b| BASE64.decode(b).ok())
                        .and_then(|raw| qt::decode_variant(&raw))
                        .unwrap_or(Variant::Invalid)
                })
                .collect::<Vec<_>>()
        })
        .filter(|b| !b.is_empty());
    Some(MumbleSettings {
        certificate,
        username: str_at("last_connection", "username"),
        transmit,
        volume: json
            .get("audio")
            .and_then(|a| a.get("volume"))
            .and_then(Json::as_f64),
        input_devices: ordered_devices(input_system.as_deref(), |b| {
            str_at("audio_backend", &format!("{b}_input"))
        }),
        output_devices: ordered_devices(output_system.as_deref(), |b| {
            str_at("audio_backend", &format!("{b}_output"))
        }),
        ptt_buttons,
        database_location: str_at("misc", "database_location").map(PathBuf::from),
    })
}

/// One value of the older `QSettings` stores, keyed like `"audio/transmit"`.
#[derive(Debug, Clone, PartialEq)]
pub enum LegacyValue {
    /// Text values, already unescaped; more than one for a list.
    Text(Vec<String>),
    Bytes(Vec<u8>),
    Int(i64),
    Float(f64),
    Bool(bool),
    List(Vec<LegacyValue>),
}

impl LegacyValue {
    fn text(&self) -> Option<TextValue> {
        match self {
            Self::Text(items) => items.first().map(|s| qt::text_value(s)),
            _ => None,
        }
    }

    fn string(&self) -> Option<String> {
        match self.text()? {
            TextValue::String(s) => non_empty(&s),
            _ => None,
        }
    }

    fn bytes(&self) -> Option<Vec<u8>> {
        match self {
            Self::Bytes(b) => Some(b.clone()),
            _ => match self.text()? {
                TextValue::Bytes(b) | TextValue::Variant(Variant::ByteArray(b)) => Some(b),
                _ => None,
            },
        }
    }

    fn number(&self) -> Option<f64> {
        match self {
            #[expect(clippy::cast_precision_loss, reason = "small settings values")]
            Self::Int(i) => Some(*i as f64),
            Self::Float(f) => Some(*f),
            _ => self.string()?.parse().ok(),
        }
    }

    fn int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => self.string()?.parse().ok(),
        }
    }

    /// The items of a list; a single value is a list of one.
    fn variants(&self) -> Vec<Variant> {
        match self {
            Self::Text(items) => items
                .iter()
                .map(|s| match qt::text_value(s) {
                    TextValue::Variant(v) => v,
                    TextValue::String(s) => Variant::String(s),
                    TextValue::Bytes(b) => Variant::ByteArray(b),
                })
                .flat_map(|v| match v {
                    // A whole list stored as one `@Variant`.
                    Variant::List(items)
                        if items.iter().all(|i| !matches!(i, Variant::Uuid(_))) =>
                    {
                        items
                    }
                    other => vec![other],
                })
                .collect(),
            Self::Int(i) => vec![Variant::Int(*i)],
            Self::List(items) => items.iter().flat_map(Self::variants).collect(),
            Self::Bytes(b) => vec![Variant::ByteArray(b.clone())],
            Self::Float(_) | Self::Bool(_) => vec![Variant::Invalid],
        }
    }
}

/// Interprets the 1.4 keys (`Settings::legacyLoad`).
pub fn from_legacy(values: &HashMap<String, LegacyValue>) -> MumbleSettings {
    let get = |key: &str| values.get(key);
    let string = |key: &str| get(key).and_then(LegacyValue::string);
    let transmit = get("audio/transmit")
        .and_then(LegacyValue::int)
        .and_then(|n| match n {
            0 => Some(Transmit::Continuous),
            1 => Some(Transmit::VoiceActivation),
            2 => Some(Transmit::PushToTalk),
            _ => None,
        });
    let shortcut_count = get("shortcuts/size")
        .and_then(LegacyValue::int)
        .unwrap_or(0)
        .min(200);
    let ptt_buttons = (1..=shortcut_count)
        .find(|i| {
            get(&format!("shortcuts/{i}/index")).and_then(LegacyValue::int) == Some(PTT_INDEX)
        })
        .and_then(|i| get(&format!("shortcuts/{i}/keys")))
        .map(LegacyValue::variants)
        .filter(|b| !b.is_empty());
    let input_system = string("audio/input");
    let output_system = string("audio/output");
    MumbleSettings {
        certificate: get("net/certificate")
            .and_then(LegacyValue::bytes)
            .filter(|c| !c.is_empty()),
        username: string("ui/username"),
        transmit,
        volume: get("audio/volume").and_then(LegacyValue::number),
        input_devices: ordered_devices(input_system.as_deref(), |b| string(&format!("{b}/input"))),
        output_devices: ordered_devices(output_system.as_deref(), |b| {
            string(&format!("{b}/output"))
        }),
        ptt_buttons,
        database_location: string("databaselocation").map(PathBuf::from),
    }
}

/// Reads a `QSettings` INI file (Linux, 1.4): `[group]` sections, `key=value` lines, `\` in
/// keys separates sub-groups. Keys of `[General]` have no group.
pub fn parse_ini(text: &str) -> HashMap<String, LegacyValue> {
    let mut values = HashMap::new();
    let mut group = String::new();
    for line in text.lines() {
        let line = line.trim_start();
        if line.starts_with(';') || line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|l| l.trim_end().strip_suffix(']'))
        {
            group = if name.eq_ignore_ascii_case("General") {
                String::new()
            } else {
                format!("{}/", name.replace('\\', "/"))
            };
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = format!("{group}{}", key.trim().replace('\\', "/"));
            values.insert(key, LegacyValue::Text(qt::ini_unescape(value)));
        }
    }
    values
}

/// Reads the property list Qt writes on macOS (1.4): keys use `.` between groups.
pub fn parse_plist(data: &[u8]) -> Option<HashMap<String, LegacyValue>> {
    fn convert(value: &plist::Value) -> Option<LegacyValue> {
        Some(match value {
            plist::Value::String(s) => LegacyValue::Text(vec![s.clone()]),
            plist::Value::Data(d) => LegacyValue::Bytes(d.clone()),
            plist::Value::Boolean(b) => LegacyValue::Bool(*b),
            plist::Value::Integer(i) => LegacyValue::Int(i.as_signed()?),
            plist::Value::Real(r) => LegacyValue::Float(*r),
            plist::Value::Array(items) => {
                LegacyValue::List(items.iter().filter_map(convert).collect())
            }
            _ => return None,
        })
    }
    let root = plist::Value::from_reader(std::io::Cursor::new(data)).ok()?;
    let dict = root.as_dictionary()?;
    Some(
        dict.iter()
            .filter_map(|(key, value)| {
                // Qt swaps `/` and `.` (and keeps real dots as a middle dot).
                let key: String = key
                    .chars()
                    .map(|c| match c {
                        '.' => '/',
                        '\u{b7}' => '.',
                        c => c,
                    })
                    .collect();
                Some((key, convert(value)?))
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mumble_import::qt::tests::{encode, ini_variant};

    fn b64(v: &Variant) -> String {
        BASE64.encode(encode(v))
    }

    #[test]
    fn json_settings_of_mumble_1_5() {
        let text = serde_json::json!({
            "settings_version": 1,
            "certificate": BASE64.encode(b"p12"),
            "audio": {"transmit_mode": "PTT", "volume": 1.5, "input_system": "PulseAudio"},
            "audio_backend": {
                "alsa_input": "hw:1",
                "pulseaudio_input": "alsa_input.usb-headset",
                "pulseaudio_output": "alsa_output.speakers",
            },
            "last_connection": {"username": "Zoe"},
            "misc": {"database_location": "/data/mumble.sqlite"},
            "shortcuts": {"defined": [
                {"index": 4, "buttons": [b64(&Variant::UInt(70))], "suppress": false, "data": ""},
                {"index": 1, "buttons": [b64(&Variant::UInt(37)), b64(&Variant::UInt(65))],
                 "suppress": false, "data": ""},
            ]},
        })
        .to_string();
        let settings = from_json(&text).expect("parsed");
        assert_eq!(settings.certificate.as_deref(), Some(&b"p12"[..]));
        assert_eq!(settings.username.as_deref(), Some("Zoe"));
        assert_eq!(settings.transmit, Some(Transmit::PushToTalk));
        assert_eq!(settings.volume, Some(1.5));
        assert_eq!(
            settings.input_devices,
            vec!["alsa_input.usb-headset".to_owned(), "hw:1".to_owned()]
        );
        assert_eq!(
            settings.output_devices,
            vec!["alsa_output.speakers".to_owned()]
        );
        assert_eq!(
            settings.ptt_buttons,
            Some(vec![Variant::UInt(37), Variant::UInt(65)])
        );
        assert_eq!(
            settings.database_location,
            Some(PathBuf::from("/data/mumble.sqlite"))
        );
    }

    #[test]
    fn json_defaults_and_damage() {
        let settings = from_json(r#"{"certificate": ""}"#).expect("parsed");
        assert_eq!(settings.certificate, None);
        assert_eq!(settings.transmit, None);
        assert_eq!(settings.ptt_buttons, None);
        assert!(from_json("not json").is_none());
    }

    #[test]
    fn ini_settings_of_mumble_1_4() {
        let cert: String = [0x30u8, 0x82, 0x00, 0xff]
            .iter()
            .map(|b| format!("\\x{b:x}"))
            .collect();
        let text = format!(
            "[General]\ndatabaselocation=/home/z/.local/share/Mumble/Mumble/mumble.sqlite\n\n\
             [audio]\ntransmit=2\nvolume=0.8\ninput=PulseAudio\n\n\
             [pulseaudio]\ninput=alsa_input.usb\noutput=alsa_output.pci\n\n\
             [net]\ncertificate=@ByteArray({cert})\n\n\
             [shortcuts]\n1\\index=3\n1\\keys=70\n2\\index=1\n2\\keys=37, 65\nsize=2\n\n\
             [ui]\nusername=Zoe\n"
        );
        let settings = from_legacy(&parse_ini(&text));
        assert_eq!(
            settings.certificate.as_deref(),
            Some(&[0x30u8, 0x82, 0x00, 0xff][..])
        );
        assert_eq!(settings.transmit, Some(Transmit::PushToTalk));
        assert_eq!(settings.volume, Some(0.8));
        assert_eq!(settings.input_devices, vec!["alsa_input.usb".to_owned()]);
        assert_eq!(settings.output_devices, vec!["alsa_output.pci".to_owned()]);
        assert_eq!(settings.username.as_deref(), Some("Zoe"));
        assert_eq!(
            settings.ptt_buttons,
            Some(vec![
                Variant::String("37".into()),
                Variant::String("65".into())
            ])
        );
        assert_eq!(
            settings.database_location,
            Some(PathBuf::from(
                "/home/z/.local/share/Mumble/Mumble/mumble.sqlite"
            ))
        );
    }

    #[test]
    fn ini_shortcut_keys_as_variants() {
        // Windows 1.4 stores each key as a DirectInput pair inside `@Variant(...)`.
        let pair = Variant::List(vec![Variant::UInt(0x1E00), Variant::Uuid([1; 16])]);
        let text = format!(
            "[shortcuts]\n1\\index=1\n1\\keys={}\nsize=1\n",
            ini_variant(&pair)
        );
        let settings = from_legacy(&parse_ini(&text));
        assert_eq!(settings.ptt_buttons, Some(vec![pair]));
    }

    #[test]
    fn plist_settings_of_mumble_1_4_on_macos() {
        let mut dict = plist::Dictionary::new();
        dict.insert("net.certificate".into(), plist::Value::Data(vec![1, 2, 3]));
        dict.insert("audio.transmit".into(), plist::Value::Integer(1.into()));
        dict.insert("coreaudio.input".into(), "BuiltInMic".into());
        dict.insert("shortcuts.size".into(), plist::Value::Integer(1.into()));
        dict.insert("shortcuts.1.index".into(), plist::Value::Integer(1.into()));
        dict.insert(
            "shortcuts.1.keys".into(),
            plist::Value::Array(vec![
                plist::Value::Integer(0x10002.into()),
                plist::Value::Integer(0x31.into()),
            ]),
        );
        let mut data = Vec::new();
        plist::Value::Dictionary(dict)
            .to_writer_binary(&mut data)
            .expect("plist");
        let settings = from_legacy(&parse_plist(&data).expect("parsed"));
        assert_eq!(settings.certificate, Some(vec![1, 2, 3]));
        assert_eq!(settings.transmit, Some(Transmit::VoiceActivation));
        assert_eq!(settings.input_devices, vec!["BuiltInMic".to_owned()]);
        assert_eq!(
            settings.ptt_buttons,
            Some(vec![Variant::Int(0x10002), Variant::Int(0x31)])
        );
    }
}
