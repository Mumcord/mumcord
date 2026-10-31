//! Finds and reads the data of an installed official Mumble client, 1.4 and 1.5, on Linux,
//! Windows and macOS (FR-3.93, issue #66): server favourites from `mumble.sqlite`, the
//! client certificate and a few settings. Everything is read-only: Mumble's files are never
//! changed, and nothing here writes Mumcord data (see `session::mumble_import`).

mod favourites;
mod keys;
mod qt;
mod settings;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use favourites::Favourite;
pub use keys::{KeyIssue, ptt_key};
pub use settings::{MumbleSettings, Transmit};

/// The operating system whose Mumble format is read (key codes, file locations).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Windows,
    Macos,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}

/// Where to look. Built from the process environment in the app; tests point every
/// directory at a fixture, so they never see the user's real Mumble data.
#[derive(Debug, Clone, Default)]
pub struct MumbleEnv {
    pub platform: Option<Platform>,
    pub home: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME` or `~/.config` (Linux).
    pub config_home: Option<PathBuf>,
    /// `$XDG_DATA_HOME` or `~/.local/share` (Linux).
    pub data_home: Option<PathBuf>,
    /// `%APPDATA%` (Windows).
    pub appdata: Option<PathBuf>,
    /// `%LOCALAPPDATA%` (Windows).
    pub local_appdata: Option<PathBuf>,
    /// Read 1.4 settings from the Windows registry.
    pub registry: bool,
}

impl MumbleEnv {
    pub fn from_process() -> Self {
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let home = var("HOME").or_else(|| var("USERPROFILE"));
        Self {
            platform: Some(Platform::current()),
            config_home: var("XDG_CONFIG_HOME")
                .or_else(|| home.as_ref().map(|h| h.join(".config"))),
            data_home: var("XDG_DATA_HOME")
                .or_else(|| home.as_ref().map(|h| h.join(".local").join("share"))),
            appdata: var("APPDATA"),
            local_appdata: var("LOCALAPPDATA"),
            home,
            registry: cfg!(target_os = "windows"),
        }
    }

    fn platform(&self) -> Platform {
        self.platform.unwrap_or_else(Platform::current)
    }

    /// Directories Mumble 1.5 keeps `mumble_settings.json` in (`Settings::findSettingsLocation`).
    fn json_dirs(&self) -> Vec<PathBuf> {
        let mm = |base: &Option<PathBuf>, parts: &[&str]| {
            base.as_ref()
                .map(|b| parts.iter().fold(b.clone(), |p, part| p.join(part)))
        };
        match self.platform() {
            Platform::Linux => vec![
                mm(&self.config_home, &["Mumble", "Mumble"]),
                mm(&self.data_home, &["Mumble", "Mumble"]),
                mm(&self.config_home, &["Mumble"]),
                self.config_home.clone(),
            ],
            Platform::Windows => vec![
                mm(&self.local_appdata, &["Mumble", "Mumble"]),
                mm(&self.appdata, &["Mumble", "Mumble"]),
                mm(&self.appdata, &["Mumble"]),
            ],
            Platform::Macos => vec![
                mm(&self.home, &["Library", "Preferences", "Mumble", "Mumble"]),
                mm(
                    &self.home,
                    &["Library", "Application Support", "Mumble", "Mumble"],
                ),
                mm(&self.home, &["Library", "Preferences", "Mumble"]),
                mm(&self.home, &["Library", "Preferences"]),
            ],
        }
        .into_iter()
        .flatten()
        .collect()
    }

    /// The 1.4 settings files (`QSettings` defaults); on Windows they live in the registry.
    fn legacy_files(&self) -> Vec<PathBuf> {
        match self.platform() {
            Platform::Linux => self
                .config_home
                .iter()
                .flat_map(|c| {
                    ["Mumble.conf", "mumble.conf", "Mumble.ini", "mumble.ini"]
                        .map(|name| c.join("Mumble").join(name))
                })
                .collect(),
            Platform::Macos => self
                .home
                .iter()
                .map(|h| {
                    h.join("Library")
                        .join("Preferences")
                        .join("net.sourceforge.mumble.Mumble.plist")
                })
                .collect(),
            Platform::Windows => Vec::new(),
        }
    }

    /// Directories `Database::findOrCreateDatabase` searches for `mumble.sqlite`.
    fn database_dirs(&self) -> Vec<PathBuf> {
        let mm = |base: &Option<PathBuf>, parts: &[&str]| {
            base.as_ref()
                .map(|b| parts.iter().fold(b.clone(), |p, part| p.join(part)))
        };
        match self.platform() {
            Platform::Linux => vec![
                mm(&self.data_home, &["Mumble", "Mumble"]),
                mm(&self.data_home, &["data", "Mumble", "Mumble"]),
                mm(&self.config_home, &["Mumble"]),
                self.home.clone(),
            ],
            Platform::Windows => vec![
                mm(&self.appdata, &["Mumble", "Mumble"]),
                mm(&self.appdata, &["Mumble"]),
                mm(&self.local_appdata, &["Mumble", "Mumble"]),
            ],
            Platform::Macos => vec![
                mm(
                    &self.home,
                    &["Library", "Application Support", "Mumble", "Mumble"],
                ),
                mm(&self.home, &["Library", "Application Support", "Mumble"]),
                mm(&self.home, &["Library", "Preferences", "Mumble"]),
                self.home.clone(),
            ],
        }
        .into_iter()
        .flatten()
        .collect()
    }
}

/// Which settings format was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MumbleVersion {
    /// `mumble_settings.json` (1.5 and later).
    Json,
    /// `QSettings` (1.4 and earlier).
    Legacy,
    /// Only the database was found.
    DatabaseOnly,
}

/// Everything read from an installed Mumble client.
#[derive(Debug, Clone)]
pub struct MumbleData {
    pub version: MumbleVersion,
    pub settings: MumbleSettings,
    pub favourites: Vec<Favourite>,
    /// The favourites could not be read (damaged or unknown database); the rest still is.
    pub favourites_failed: bool,
}

/// Looks for an installed Mumble client and reads its data. `None` when nothing was found.
pub fn read(env: &MumbleEnv) -> Option<MumbleData> {
    let (version, settings) = read_settings(env)?;
    let database = settings
        .database_location
        .clone()
        .filter(|p| p.is_file())
        .or_else(|| find_database(env));
    let (favourites, favourites_failed) = match &database {
        Some(path) => match favourites::read(path) {
            Ok(list) => (list, false),
            Err(e) => {
                tracing::warn!(error = %e, "reading the Mumble favourites failed");
                (Vec::new(), true)
            }
        },
        None => (Vec::new(), false),
    };
    if version == MumbleVersion::DatabaseOnly && database.is_none() {
        return None;
    }
    Some(MumbleData {
        version,
        settings,
        favourites,
        favourites_failed,
    })
}

fn read_settings(env: &MumbleEnv) -> Option<(MumbleVersion, MumbleSettings)> {
    for dir in env.json_dirs() {
        let path = dir.join("mumble_settings.json");
        if let Some(text) = read_text(&path) {
            match settings::from_json(&text) {
                Some(s) => return Some((MumbleVersion::Json, s)),
                None => tracing::warn!("the Mumble settings file is not valid JSON"),
            }
        }
    }
    for path in env.legacy_files() {
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let values = if path.extension().is_some_and(|e| e == "plist") {
            settings::parse_plist(&data)
        } else {
            Some(settings::parse_ini(&String::from_utf8_lossy(&data)))
        };
        match values {
            Some(values) => return Some((MumbleVersion::Legacy, settings::from_legacy(&values))),
            None => tracing::warn!("the Mumble settings file could not be read"),
        }
    }
    if env.registry
        && let Some(values) = registry::read()
    {
        return Some((MumbleVersion::Legacy, settings::from_legacy(&values)));
    }
    Some((MumbleVersion::DatabaseOnly, MumbleSettings::default()))
}

fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn find_database(env: &MumbleEnv) -> Option<PathBuf> {
    env.database_dirs().into_iter().find_map(|dir| {
        [".mumble.sqlite", "mumble.sqlite"]
            .into_iter()
            .map(|name| dir.join(name))
            .find(|p| p.is_file())
    })
}

#[cfg(target_os = "windows")]
mod registry {
    //! Mumble 1.4 on Windows keeps its settings in `HKCU\Software\Mumble\Mumble`, written by
    //! `QSettings`: groups are sub-keys, numbers are DWORDs, lists are multi-strings and
    //! strings with NUL characters (`@ByteArray`, `@Variant`) are UTF-16 binary values.

    use std::collections::HashMap;

    use windows_registry::{CURRENT_USER, Key, Type};

    use super::settings::LegacyValue;

    pub fn read() -> Option<HashMap<String, LegacyValue>> {
        let root = CURRENT_USER.open(r"Software\Mumble\Mumble").ok()?;
        let mut values = HashMap::new();
        collect(&root, "", 0, &mut values);
        Some(values)
    }

    fn collect(key: &Key, prefix: &str, depth: u8, out: &mut HashMap<String, LegacyValue>) {
        if depth > 4 {
            return;
        }
        if let Ok(values) = key.values() {
            for (name, value) in values {
                let utf16 = |bytes: &[u8]| -> String {
                    let units: Vec<u16> = bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_le_bytes(*c))
                        .collect();
                    String::from_utf16_lossy(&units)
                        .trim_end_matches('\0')
                        .to_owned()
                };
                let converted = match value.ty() {
                    Type::U32 | Type::U64 => {
                        let mut bytes = [0u8; 8];
                        let n = value.len().min(8);
                        bytes[..n].copy_from_slice(&value[..n]);
                        LegacyValue::Int(i64::from_le_bytes(bytes))
                    }
                    Type::String | Type::ExpandString | Type::Bytes => {
                        LegacyValue::Text(vec![utf16(&value)])
                    }
                    Type::MultiString => LegacyValue::Text(
                        utf16(&value)
                            .split('\0')
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .collect(),
                    ),
                    Type::Other(_) => continue,
                };
                out.insert(format!("{prefix}{name}"), converted);
            }
        }
        if let Ok(subkeys) = key.keys() {
            for name in subkeys {
                if let Ok(sub) = key.open(&name) {
                    collect(&sub, &format!("{prefix}{name}/"), depth + 1, out);
                }
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod registry {
    use std::collections::HashMap;

    use super::settings::LegacyValue;

    /// Only Windows has a registry.
    pub fn read() -> Option<HashMap<String, LegacyValue>> {
        None
    }
}

/// Matches a device Mumble stored against Mumcord's device list: by id (Mumcord ids may
/// carry a backend prefix in front of the same system name) or by display name.
pub fn match_device<'a, D>(
    stored: &[String],
    devices: &'a [D],
    id: fn(&D) -> &str,
    name: fn(&D) -> &str,
) -> Option<&'a D> {
    stored.iter().find_map(|wanted| {
        devices.iter().find(|d| {
            let id = id(d);
            id == wanted
                || id
                    .strip_suffix(wanted.as_str())
                    .is_some_and(|p| p.ends_with(':'))
                || name(d).eq_ignore_ascii_case(wanted)
        })
    })
}

/// Server key used to find duplicates: host (case-insensitive) and port.
pub fn server_key(host: &str, port: u16) -> (String, u16) {
    (host.trim().to_ascii_lowercase(), port)
}

/// Removes favourites that point to the same server as an earlier one.
pub fn unique_favourites(favourites: Vec<Favourite>) -> Vec<Favourite> {
    let mut seen: HashMap<(String, u16), ()> = HashMap::new();
    favourites
        .into_iter()
        .filter(|f| seen.insert(server_key(&f.host, f.port), ()).is_none())
        .collect()
}

/// Test helper for other modules: writes a Mumble `servers` table.
#[cfg(test)]
pub(crate) fn favourites_fixture(path: &Path, rows: &[(&str, &str, i64, &str, Option<&str>)]) {
    favourites::tests::write_db(path, rows);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn linux_env(root: &Path) -> MumbleEnv {
        MumbleEnv {
            platform: Some(Platform::Linux),
            home: Some(root.join("home")),
            config_home: Some(root.join("config")),
            data_home: Some(root.join("data")),
            appdata: None,
            local_appdata: None,
            registry: false,
        }
    }

    #[test]
    fn nothing_found_in_an_empty_profile() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(read(&linux_env(dir.path())).is_none());
    }

    #[test]
    fn mumble_1_5_on_linux() {
        let dir = tempfile::tempdir().expect("tempdir");
        let env = linux_env(dir.path());
        let config = dir.path().join("config/Mumble/Mumble");
        let data = dir.path().join("data/Mumble/Mumble");
        std::fs::create_dir_all(&config).expect("mkdir");
        std::fs::create_dir_all(&data).expect("mkdir");
        std::fs::write(
            config.join("mumble_settings.json"),
            r#"{"audio": {"transmit_mode": "Continuous"}}"#,
        )
        .expect("write");
        favourites::tests::write_db(
            &data.join("mumble.sqlite"),
            &[("A", "a.example", 64738, "zoe", None)],
        );
        let found = read(&env).expect("found");
        assert_eq!(found.version, MumbleVersion::Json);
        assert_eq!(found.settings.transmit, Some(Transmit::Continuous));
        assert_eq!(found.favourites.len(), 1);
        assert!(!found.favourites_failed);
    }

    #[test]
    fn mumble_1_4_on_linux_with_a_configured_database() {
        let dir = tempfile::tempdir().expect("tempdir");
        let env = linux_env(dir.path());
        let config = dir.path().join("config/Mumble");
        std::fs::create_dir_all(&config).expect("mkdir");
        let db = dir.path().join("elsewhere.sqlite");
        favourites::tests::write_db(&db, &[("B", "b.example", 1, "", Some("pw"))]);
        std::fs::write(
            config.join("Mumble.conf"),
            format!("[General]\ndatabaselocation={}\n", db.display()),
        )
        .expect("write");
        let found = read(&env).expect("found");
        assert_eq!(found.version, MumbleVersion::Legacy);
        assert_eq!(found.favourites[0].password.as_deref(), Some("pw"));
    }

    #[test]
    fn a_damaged_database_still_shows_the_rest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let env = linux_env(dir.path());
        let data = dir.path().join("data/Mumble/Mumble");
        std::fs::create_dir_all(&data).expect("mkdir");
        std::fs::write(data.join("mumble.sqlite"), b"not a database").expect("write");
        let found = read(&env).expect("found");
        assert_eq!(found.version, MumbleVersion::DatabaseOnly);
        assert!(found.favourites_failed);
    }

    #[test]
    fn windows_and_macos_locations() {
        let root = Path::new("/r");
        let windows = MumbleEnv {
            platform: Some(Platform::Windows),
            appdata: Some(root.join("Roaming")),
            local_appdata: Some(root.join("Local")),
            ..MumbleEnv::default()
        };
        assert_eq!(windows.json_dirs()[0], root.join("Local/Mumble/Mumble"));
        assert_eq!(windows.database_dirs()[1], root.join("Roaming/Mumble"));
        assert!(windows.legacy_files().is_empty());
        let mac = MumbleEnv {
            platform: Some(Platform::Macos),
            home: Some(root.to_path_buf()),
            ..MumbleEnv::default()
        };
        assert_eq!(
            mac.legacy_files(),
            vec![root.join("Library/Preferences/net.sourceforge.mumble.Mumble.plist")]
        );
        assert_eq!(
            mac.database_dirs()[0],
            root.join("Library/Application Support/Mumble/Mumble")
        );
    }

    #[test]
    fn devices_match_by_id_suffix_or_name() {
        let devices = [("pw:alsa_input.usb", "USB Headset"), ("pw:mic2", "Webcam")];
        fn id<'a>(d: &(&'a str, &'a str)) -> &'a str {
            d.0
        }
        fn name<'a>(d: &(&'a str, &'a str)) -> &'a str {
            d.1
        }
        let found = match_device(&["alsa_input.usb".into()], &devices, id, name);
        assert_eq!(found, Some(&devices[0]));
        let found = match_device(&["gone".into(), "webcam".into()], &devices, id, name);
        assert_eq!(found, Some(&devices[1]));
        assert_eq!(
            match_device(&["input.usb".into()], &devices, id, name),
            None
        );
    }

    #[test]
    fn duplicate_favourites_keep_the_first() {
        let fav = |name: &str, host: &str| Favourite {
            name: name.into(),
            host: host.into(),
            port: 64738,
            username: String::new(),
            password: None,
        };
        let unique = unique_favourites(vec![fav("A", "x.example"), fav("B", "X.example ")]);
        assert_eq!(unique.len(), 1);
        assert_eq!(unique[0].name, "A");
    }
}
