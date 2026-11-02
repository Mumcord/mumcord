//! Server-suggested plugins (FR-6.77 … FR-6.79): parses the `mumcord.srvplugins` message of
//! a registered, pinned sender (the engine has already checked the sender), resolves every
//! entry and keeps the per server and identity state. Nothing is ever sent back to the bot.

use std::collections::HashSet;

use mumcord_store::{IdentityId, PluginSuggestion, ServerId, Store, UnixMillis};
use semver::Version;
use serde::Deserialize;

use crate::registry::{Index, RevocationList, check_repository_url};

/// At most this many entries are read (FR-6.77).
pub const MAX_ENTRIES: usize = 8;
/// Longest `for` text in characters.
pub const MAX_REASON_CHARS: usize = 40;
/// How long "Not now" hides a suggestion if `seq` stays the same.
pub const NOT_NOW_MS: UnixMillis = 30 * 24 * 60 * 60 * 1000;

const STATE_NEW: &str = "new";
const STATE_NOT_NOW: &str = "not-now";
const STATE_DISMISSED: &str = "dismissed";
const STATE_INSTALLED: &str = "installed";
/// Plugin id of the row that records "Don't ask again for this server". It is not a valid
/// plugin id, so it can never clash with a real suggestion.
const SERVER_ROW: &str = "*";

/// A parsed `mumcord.srvplugins` message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    pub seq: i64,
    pub entries: Vec<Entry>,
}

/// One announced plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    /// Lowest plugin version the server wants.
    pub min: Version,
    pub source: EntrySource,
    /// Why the server suggests it (`for`), untrusted text: render as plain text.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntrySource {
    Official,
    Repository { url: String, key: Option<String> },
}

#[derive(Deserialize)]
struct RawMessage {
    v: u32,
    seq: i64,
    plugins: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawEntry {
    id: String,
    min: String,
    src: String,
    #[serde(default)]
    key: Option<String>,
    #[serde(rename = "for")]
    reason: String,
}

/// Parses the message leniently: an unknown `level` or other extra field is ignored and a
/// broken entry is dropped without hurting the others. `None` if the message as a whole is
/// not a version 1 announcement.
pub fn parse(bytes: &[u8], developer_mode: bool) -> Option<Announcement> {
    let raw: RawMessage = serde_json::from_slice(bytes).ok()?;
    if raw.v != 1 || raw.seq < 0 {
        return None;
    }
    let mut seen = HashSet::new();
    let entries = raw
        .plugins
        .into_iter()
        .take(MAX_ENTRIES)
        .filter_map(|value| entry(value, developer_mode))
        .filter(|e| seen.insert(e.id.clone()))
        .collect();
    Some(Announcement {
        seq: raw.seq,
        entries,
    })
}

fn entry(value: serde_json::Value, developer_mode: bool) -> Option<Entry> {
    let raw: RawEntry = serde_json::from_value(value).ok()?;
    if !valid_id(&raw.id) || raw.reason.is_empty() || raw.reason.chars().count() > MAX_REASON_CHARS
    {
        return None;
    }
    let min = Version::parse(&raw.min).ok()?;
    let source = if raw.src == "official" {
        EntrySource::Official
    } else {
        // The dialog shows the address, so only addresses it would accept get that far.
        check_repository_url(&raw.src, developer_mode).ok()?;
        EntrySource::Repository {
            url: raw.src,
            key: raw.key.filter(|k| !k.trim().is_empty()),
        }
    };
    Some(Entry {
        id: raw.id,
        min,
        source,
        reason: raw.reason,
    })
}

/// `^[a-z][a-z0-9-]{2,31}$`, the manifest's plugin id rule.
fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (3..=32).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// What the resolver needs to know about the client.
pub struct Environment<'a> {
    /// The cached official index; `None` if it was never fetched.
    pub index: Option<&'a Index>,
    pub revoked: &'a RevocationList,
    /// Whether a plugin with this id is installed.
    pub installed: &'a dyn Fn(&str) -> bool,
}

/// A suggestion the user should see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub plugin_id: String,
    pub reason: String,
    pub seq: i64,
    pub source: SuggestionSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuggestionSource {
    /// Install or open the details of the official plugin.
    Official { version: String },
    /// Open the Add repository dialog with `key` as Expected key.
    Repository { url: String, key: Option<String> },
}

/// Applies an announcement to the stored state and returns what to show now.
///
/// `sender_pin` is the certificate hash of the pinned announcer, stored beside the state.
/// Entries that are installed, revoked, unresolvable or snoozed are left out.
pub fn process(
    store: &Store,
    server: ServerId,
    identity: IdentityId,
    sender_pin: &str,
    announcement: &Announcement,
    env: &Environment<'_>,
    now: UnixMillis,
) -> mumcord_store::Result<Vec<Suggestion>> {
    let stored = store.plugin_suggestions(server, identity)?;
    if stored
        .iter()
        .any(|s| s.plugin_id == SERVER_ROW && s.state == STATE_DISMISSED)
    {
        return Ok(Vec::new());
    }
    let mut visible = Vec::new();
    for entry in &announcement.entries {
        let Some(source) = resolve(entry, env) else {
            continue;
        };
        let previous = stored.iter().find(|s| s.plugin_id == entry.id);
        let installed = (env.installed)(&entry.id);
        let (state, until) = next_state(previous, announcement.seq, installed, now);
        if previous
            .is_none_or(|p| (p.state.as_str(), p.seq, p.until) != (state, announcement.seq, until))
        {
            store.save_plugin_suggestion(&PluginSuggestion {
                server_id: server,
                identity_id: identity,
                plugin_id: entry.id.clone(),
                seq: announcement.seq,
                state: state.to_owned(),
                until,
                sender_pin: sender_pin.to_ascii_lowercase(),
            })?;
        }
        if state == STATE_NEW {
            visible.push(Suggestion {
                plugin_id: entry.id.clone(),
                reason: entry.reason.clone(),
                seq: announcement.seq,
                source,
            });
        }
    }
    Ok(visible)
}

/// The state machine of FR-6.79.
fn next_state(
    previous: Option<&PluginSuggestion>,
    seq: i64,
    installed: bool,
    now: UnixMillis,
) -> (&'static str, Option<UnixMillis>) {
    if installed {
        return (STATE_INSTALLED, None);
    }
    let Some(previous) = previous else {
        return (STATE_NEW, None);
    };
    let changed = previous.seq != seq;
    match previous.state.as_str() {
        STATE_DISMISSED => (STATE_DISMISSED, None),
        STATE_NOT_NOW if !changed && previous.until.is_some_and(|until| until > now) => {
            (STATE_NOT_NOW, previous.until)
        }
        // A removed plugin is not suggested again until the server changes its list.
        STATE_INSTALLED if !changed => (STATE_INSTALLED, None),
        _ => (STATE_NEW, None),
    }
}

/// Checks an entry against the index and the revocations; `None` means "do not suggest".
fn resolve(entry: &Entry, env: &Environment<'_>) -> Option<SuggestionSource> {
    match &entry.source {
        EntrySource::Official => {
            let plugin = env.index?.plugin(&entry.id)?;
            let latest = plugin.latest();
            if Version::parse(&latest.version).ok()? < entry.min {
                return None;
            }
            if env
                .revoked
                .find(&entry.id, &latest.version, &latest.sha256, None)
                .is_some()
            {
                return None;
            }
            Some(SuggestionSource::Official {
                version: latest.version.clone(),
            })
        }
        EntrySource::Repository { url, key } => {
            // Only whole-plugin revocations can be matched before the package is known.
            if env.revoked.find(&entry.id, "", "", None).is_some() {
                return None;
            }
            Some(SuggestionSource::Repository {
                url: url.clone(),
                key: key.clone(),
            })
        }
    }
}

/// "Not now": hidden until `seq` changes or 30 days have passed.
pub fn not_now(
    store: &Store,
    server: ServerId,
    identity: IdentityId,
    plugin_id: &str,
    now: UnixMillis,
) -> mumcord_store::Result<()> {
    set_state(
        store,
        server,
        identity,
        plugin_id,
        STATE_NOT_NOW,
        Some(now + NOT_NOW_MS),
    )
}

/// "Don't ask again" for one plugin.
pub fn dismiss(
    store: &Store,
    server: ServerId,
    identity: IdentityId,
    plugin_id: &str,
) -> mumcord_store::Result<()> {
    set_state(store, server, identity, plugin_id, STATE_DISMISSED, None)
}

/// "Don't ask again for this server": no suggestion of this server is shown any more.
pub fn dismiss_server(
    store: &Store,
    server: ServerId,
    identity: IdentityId,
) -> mumcord_store::Result<()> {
    set_state(store, server, identity, SERVER_ROW, STATE_DISMISSED, None)
}

/// Call after the user installed the plugin from a suggestion.
pub fn mark_installed(
    store: &Store,
    server: ServerId,
    identity: IdentityId,
    plugin_id: &str,
) -> mumcord_store::Result<()> {
    set_state(store, server, identity, plugin_id, STATE_INSTALLED, None)
}

fn set_state(
    store: &Store,
    server: ServerId,
    identity: IdentityId,
    plugin_id: &str,
    state: &str,
    until: Option<UnixMillis>,
) -> mumcord_store::Result<()> {
    let existing = store
        .plugin_suggestions(server, identity)?
        .into_iter()
        .find(|s| s.plugin_id == plugin_id);
    let (seq, sender_pin) = existing
        .map(|s| (s.seq, s.sender_pin))
        .unwrap_or_else(|| (0, String::new()));
    store.save_plugin_suggestion(&PluginSuggestion {
        server_id: server,
        identity_id: identity,
        plugin_id: plugin_id.to_owned(),
        seq,
        state: state.to_owned(),
        until,
        sender_pin,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{IndexKind, Revocation, Severity};
    use mumcord_store::{NewIdentity, NewServer};
    use serde_json::json;

    const NOW: UnixMillis = 1_000_000;

    fn store() -> (tempfile::TempDir, Store, ServerId, IdentityId) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(dir.path()).expect("store");
        let server = store
            .add_server(&NewServer::new("A", "localhost", "Tester"))
            .expect("server")
            .id;
        let identity = store
            .add_identity(&NewIdentity::new("One", "one", "h1", 1))
            .expect("identity")
            .id;
        (dir, store, server, identity)
    }

    fn index() -> Index {
        let version = json!({"version": "1.2.0", "api": 1,
            "url": "https://example.org/dice.mcplugin", "size": 12,
            "sha256": "cd".repeat(32), "permissions": []});
        let plugin = |id: &str| {
            json!({"id": id, "name": {"en": "N"}, "summary": {"en": "S"},
                "author_keys": ["RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3"],
                "reviewed": true, "updated": "2026-10-01", "versions": [version]})
        };
        let bytes = serde_json::to_vec(&json!({"format": 1, "sequence": 1,
            "plugins": [plugin("dice"), plugin("poll")]}))
        .expect("json");
        Index::parse(&bytes, &IndexKind::Official).expect("index")
    }

    fn message(plugins: serde_json::Value, seq: i64) -> Announcement {
        let bytes =
            serde_json::to_vec(&json!({"v": 1, "seq": seq, "plugins": plugins})).expect("json");
        parse(&bytes, false).expect("announcement")
    }

    fn official(id: &str) -> serde_json::Value {
        json!({"id": id, "min": "1.0.0", "src": "official", "for": "Game night"})
    }

    fn run(
        store: &Store,
        ids: (ServerId, IdentityId),
        ann: &Announcement,
        installed: &[&str],
        revoked: &RevocationList,
        now: UnixMillis,
    ) -> Vec<String> {
        let index = index();
        let installed: Vec<String> = installed.iter().map(|s| (*s).to_owned()).collect();
        let is_installed = |id: &str| installed.iter().any(|i| i == id);
        let env = Environment {
            index: Some(&index),
            revoked,
            installed: &is_installed,
        };
        process(store, ids.0, ids.1, "ABCD", ann, &env, now)
            .expect("process")
            .into_iter()
            .map(|s| s.plugin_id)
            .collect()
    }

    fn none() -> RevocationList {
        RevocationList {
            sequence: 0,
            entries: Vec::new(),
        }
    }

    #[test]
    fn parse_ignores_level_and_drops_bad_entries() {
        let ann = message(
            json!([
                {"id": "dice", "min": "1.0.0", "src": "official", "for": "x", "level": "required"},
                {"id": "Bad", "min": "1.0.0", "src": "official", "for": "x"},
                {"id": "poll", "min": "nope", "src": "official", "for": "x"},
                {"id": "poll", "min": "1.0.0", "src": "official", "for": ""},
                {"id": "poll", "min": "1.0.0", "src": "official", "for": "y".repeat(41)},
                {"id": "echo", "min": "1.0.0", "src": "http://example.org/index.json", "for": "x"},
                {"id": "third", "min": "0.1.0", "src": "https://example.org/index.json",
                 "key": "RWQabc", "for": "Our tools"}
            ]),
            3,
        );
        assert_eq!(ann.entries.len(), 2);
        assert_eq!(ann.entries[0].id, "dice");
        assert_eq!(
            ann.entries[1].source,
            EntrySource::Repository {
                url: "https://example.org/index.json".into(),
                key: Some("RWQabc".into())
            }
        );
    }

    #[test]
    fn parse_takes_eight_entries_and_rejects_other_versions() {
        let many: Vec<_> = (0..12).map(|i| official(&format!("plug{i:02}"))).collect();
        assert_eq!(message(json!(many), 1).entries.len(), MAX_ENTRIES);
        assert!(parse(br#"{"v":2,"seq":1,"plugins":[]}"#, false).is_none());
        assert!(parse(b"nonsense", false).is_none());
    }

    #[test]
    fn official_entries_resolve_and_skip_installed() {
        let (_d, store, server, identity) = store();
        let ann = message(
            json!([official("dice"), official("poll"), official("ghost")]),
            1,
        );
        let shown = run(&store, (server, identity), &ann, &["poll"], &none(), NOW);
        assert_eq!(shown, ["dice"]);
        let rows = store.plugin_suggestions(server, identity).expect("rows");
        let states: Vec<_> = rows.iter().map(|r| (&*r.plugin_id, &*r.state)).collect();
        assert_eq!(states, [("dice", "new"), ("poll", "installed")]);
        assert_eq!(rows[0].sender_pin, "abcd");
    }

    #[test]
    fn min_above_the_latest_version_is_not_suggested() {
        let (_d, store, server, identity) = store();
        let mut dice = official("dice");
        dice["min"] = json!("2.0.0");
        let ann = message(json!([dice]), 1);
        assert!(run(&store, (server, identity), &ann, &[], &none(), NOW).is_empty());
    }

    #[test]
    fn revoked_plugins_are_never_suggested() {
        let (_d, store, server, identity) = store();
        let revoked = RevocationList {
            sequence: 1,
            entries: vec![Revocation {
                id: "dice".into(),
                versions: Vec::new(),
                sha256: Vec::new(),
                author_key: None,
                severity: Severity::Disable,
                reason: [("en".to_owned(), "bad".to_owned())].into(),
                since: "2026-10-01".into(),
            }],
        };
        let third = json!({"id": "dice", "min": "0.1.0", "src": "https://example.org/index.json",
            "for": "x"});
        let ann = message(json!([official("dice"), official("poll"), third]), 1);
        assert_eq!(
            run(&store, (server, identity), &ann, &[], &revoked, NOW),
            ["poll"]
        );
    }

    #[test]
    fn not_now_waits_for_a_new_seq_or_thirty_days() {
        let (_d, store, server, identity) = store();
        let ids = (server, identity);
        let ann = message(json!([official("dice")]), 1);
        assert_eq!(run(&store, ids, &ann, &[], &none(), NOW), ["dice"]);
        not_now(&store, server, identity, "dice", NOW).expect("not now");
        assert!(run(&store, ids, &ann, &[], &none(), NOW + 1000).is_empty());
        assert!(run(&store, ids, &ann, &[], &none(), NOW + NOT_NOW_MS - 1).is_empty());
        assert_eq!(
            run(&store, ids, &ann, &[], &none(), NOW + NOT_NOW_MS),
            ["dice"]
        );
        not_now(&store, server, identity, "dice", NOW).expect("not now");
        let newer = message(json!([official("dice")]), 2);
        assert_eq!(run(&store, ids, &newer, &[], &none(), NOW + 1000), ["dice"]);
    }

    #[test]
    fn dismissed_stays_hidden_even_with_a_new_seq() {
        let (_d, store, server, identity) = store();
        let ids = (server, identity);
        let ann = message(json!([official("dice"), official("poll")]), 1);
        run(&store, ids, &ann, &[], &none(), NOW);
        dismiss(&store, server, identity, "dice").expect("dismiss");
        let newer = message(json!([official("dice"), official("poll")]), 5);
        assert_eq!(run(&store, ids, &newer, &[], &none(), NOW), ["poll"]);
    }

    #[test]
    fn dismissing_the_server_silences_all_suggestions() {
        let (_d, store, server, identity) = store();
        let ids = (server, identity);
        let ann = message(json!([official("dice")]), 1);
        dismiss_server(&store, server, identity).expect("dismiss");
        let newer = message(json!([official("dice"), official("poll")]), 9);
        assert!(run(&store, ids, &newer, &[], &none(), NOW).is_empty());
        assert!(run(&store, ids, &ann, &[], &none(), NOW).is_empty());
    }

    #[test]
    fn state_is_kept_per_identity() {
        let (_d, store, server, one) = store();
        let two = store
            .add_identity(&NewIdentity::new("Two", "two", "h2", 2))
            .expect("identity")
            .id;
        let ann = message(json!([official("dice")]), 1);
        run(&store, (server, one), &ann, &[], &none(), NOW);
        dismiss(&store, server, one, "dice").expect("dismiss");
        assert_eq!(
            run(&store, (server, two), &ann, &[], &none(), NOW),
            ["dice"]
        );
    }

    #[test]
    fn removed_plugin_returns_only_after_a_seq_change() {
        let (_d, store, server, identity) = store();
        let ids = (server, identity);
        let ann = message(json!([official("dice")]), 1);
        run(&store, ids, &ann, &["dice"], &none(), NOW);
        assert!(run(&store, ids, &ann, &[], &none(), NOW).is_empty());
        let newer = message(json!([official("dice")]), 2);
        assert_eq!(run(&store, ids, &newer, &[], &none(), NOW), ["dice"]);
    }

    #[test]
    fn third_party_entry_carries_the_expected_key() {
        let (_d, store, server, identity) = store();
        let ann = message(
            json!([{"id": "third", "min": "0.1.0", "src": "https://example.org/index.json",
                    "key": "RWQabc", "for": "Our tools"}]),
            1,
        );
        let env = Environment {
            index: None,
            revoked: &none(),
            installed: &|_| false,
        };
        let shown = process(&store, server, identity, "AB", &ann, &env, NOW).expect("process");
        assert_eq!(
            shown[0].source,
            SuggestionSource::Repository {
                url: "https://example.org/index.json".into(),
                key: Some("RWQabc".into())
            }
        );
    }
}
