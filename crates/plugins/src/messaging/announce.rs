//! The plugin announce `mumcord.plugins` (FR-6.35).
//!
//! Compact JSON `{"v":1,"p":["echolings@1","foo@1~a1b2c3d4","bar@1~dev"]}` sent to peers whose
//! hello lists feature `plugins`, at most once per 10 s. Plugins with visibility Nobody are not
//! announced. Two clients exchange a plugin's messages only if both announce the same id from the
//! same source.

use std::fmt;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::manifest::{Visibility, is_valid_id};

/// Plugin data id of the announce.
pub const ANNOUNCE_DATA_ID: &str = "mumcord.plugins";
/// Announce format version.
pub const ANNOUNCE_VERSION: u32 = 1;
/// Most entries in one announce.
pub const MAX_ENTRIES: usize = 32;
/// Minimum time between two announces.
pub const MIN_INTERVAL: Duration = Duration::from_secs(10);
/// Larger frames are ignored without parsing; 32 maximal entries fit easily.
pub const MAX_ANNOUNCE_BYTES: usize = 4096;

/// Where an installed plugin came from, as far as peers need to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceTag {
    Official,
    /// Third-party repository: the first 4 bytes (8 hex) of its key fingerprint.
    Repo([u8; 4]),
    /// Loaded in Developer mode.
    Dev,
}

/// One announced plugin: `<id>@<major>[~<8hex>|~dev]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnnounceEntry {
    pub id: String,
    pub api_major: u32,
    pub source: SourceTag,
}

impl AnnounceEntry {
    /// True if messages between the two plugins may flow: same id and same source.
    pub fn matches(&self, id: &str, source: SourceTag) -> bool {
        self.id == id && self.source == source
    }

    fn parse(text: &str) -> Option<Self> {
        let (id, rest) = text.split_once('@')?;
        let (major, source) = match rest.split_once('~') {
            None => (rest, SourceTag::Official),
            Some((major, "dev")) => (major, SourceTag::Dev),
            Some((major, hex)) => (major, SourceTag::Repo(parse_fingerprint(hex)?)),
        };
        if !is_valid_id(id) || major.is_empty() || !major.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some(Self {
            id: id.to_owned(),
            api_major: major.parse().ok()?,
            source,
        })
    }
}

impl fmt::Display for AnnounceEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.id, self.api_major)?;
        match self.source {
            SourceTag::Official => Ok(()),
            SourceTag::Dev => f.write_str("~dev"),
            SourceTag::Repo(fp) => {
                f.write_str("~")?;
                fp.iter().try_for_each(|b| write!(f, "{b:02x}"))
            }
        }
    }
}

fn parse_fingerprint(hex: &str) -> Option<[u8; 4]> {
    if hex.len() != 8 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    let mut out = [0u8; 4];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

#[derive(Serialize, Deserialize)]
struct Wire {
    v: u32,
    p: Vec<String>,
}

/// Builds the announce from the installed, running plugins and their visibility. Plugins with
/// visibility Nobody are left out; at most [`MAX_ENTRIES`] entries are kept.
pub fn build<'a>(plugins: impl IntoIterator<Item = (&'a AnnounceEntry, Visibility)>) -> Vec<u8> {
    let wire = Wire {
        v: ANNOUNCE_VERSION,
        p: plugins
            .into_iter()
            .filter(|(_, visibility)| *visibility != Visibility::Nobody)
            .map(|(entry, _)| entry.to_string())
            .take(MAX_ENTRIES)
            .collect(),
    };
    // Serialising a struct of a number and strings cannot fail.
    serde_json::to_vec(&wire).unwrap_or_default()
}

/// Parses a peer's announce. Untrusted input: oversized frames, unknown versions and malformed
/// JSON yield an empty list, malformed entries are skipped, and only the first
/// [`MAX_ENTRIES`] entries count.
pub fn parse(bytes: &[u8]) -> Vec<AnnounceEntry> {
    if bytes.len() > MAX_ANNOUNCE_BYTES {
        return Vec::new();
    }
    match serde_json::from_slice::<Wire>(bytes) {
        Ok(wire) if wire.v == ANNOUNCE_VERSION => wire
            .p
            .iter()
            .take(MAX_ENTRIES)
            .filter_map(|e| AnnounceEntry::parse(e))
            .collect(),
        _ => Vec::new(),
    }
}

/// Keeps announces to at most one per [`MIN_INTERVAL`]. The caller marks a change (plugin set or
/// visibility changed, a new peer with feature `plugins` appeared) and sends when [`Self::poll`]
/// says so; a change inside the interval is sent once the interval has passed.
#[derive(Debug, Default)]
pub struct AnnounceThrottle {
    last_sent: Option<Instant>,
    pending: bool,
}

impl AnnounceThrottle {
    pub fn mark_changed(&mut self) {
        self.pending = true;
    }

    /// When the next announce is due, if one is pending.
    pub fn due_at(&self, now: Instant) -> Option<Instant> {
        if !self.pending {
            return None;
        }
        Some(self.last_sent.map_or(now, |t| (t + MIN_INTERVAL).max(now)))
    }

    /// Returns true (and records the send) if a pending announce may go out now.
    pub fn poll(&mut self, now: Instant) -> bool {
        let allowed = self
            .last_sent
            .is_none_or(|t| now.duration_since(t) >= MIN_INTERVAL);
        if self.pending && allowed {
            self.pending = false;
            self.last_sent = Some(now);
            true
        } else {
            false
        }
    }

    /// Forgets the last send, e.g. after a reconnect, so the first announce goes out at once.
    pub fn reset(&mut self) {
        self.last_sent = None;
        self.pending = true;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn entry(id: &str, source: SourceTag) -> AnnounceEntry {
        AnnounceEntry {
            id: id.into(),
            api_major: 1,
            source,
        }
    }

    #[test]
    fn builds_the_spec_example() {
        let a = entry("echolings", SourceTag::Official);
        let b = entry("foo", SourceTag::Repo([0xa1, 0xb2, 0xc3, 0xd4]));
        let c = entry("bar", SourceTag::Dev);
        let bytes = build([
            (&a, Visibility::Everyone),
            (&b, Visibility::Friends),
            (&c, Visibility::Everyone),
        ]);
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            r#"{"v":1,"p":["echolings@1","foo@1~a1b2c3d4","bar@1~dev"]}"#
        );
        assert_eq!(parse(&bytes), vec![a, b, c]);
    }

    #[test]
    fn nobody_is_not_announced() {
        let a = entry("echolings", SourceTag::Official);
        assert_eq!(build([(&a, Visibility::Nobody)]), br#"{"v":1,"p":[]}"#);
    }

    #[test]
    fn caps_entries() {
        let entries: Vec<_> = (0..40)
            .map(|i| entry(&format!("plugin-{i}"), SourceTag::Official))
            .collect();
        let bytes = build(entries.iter().map(|e| (e, Visibility::Everyone)));
        assert_eq!(parse(&bytes).len(), MAX_ENTRIES);

        let names: Vec<String> = (0..40).map(|i| format!("\"plugin-{i}@1\"")).collect();
        let raw = format!(r#"{{"v":1,"p":[{}]}}"#, names.join(","));
        assert_eq!(parse(raw.as_bytes()).len(), MAX_ENTRIES);
    }

    #[test]
    fn parse_is_lenient_with_untrusted_input() {
        assert!(parse(b"not json").is_empty());
        assert!(parse(br#"{"v":2,"p":["echolings@1"]}"#).is_empty());
        assert!(parse(&vec![b' '; MAX_ANNOUNCE_BYTES + 1]).is_empty());
        let got = parse(
            br#"{"v":1,"p":["echolings@1","Bad@1","x@1","dice@","dice@1~ABCDEF12","dice@1~abc","dice@-1","dice@1~deadbeef","poll@2~dev"],"extra":true}"#,
        );
        assert_eq!(
            got,
            vec![
                entry("echolings", SourceTag::Official),
                entry("dice", SourceTag::Repo([0xde, 0xad, 0xbe, 0xef])),
                AnnounceEntry {
                    id: "poll".into(),
                    api_major: 2,
                    source: SourceTag::Dev
                },
            ]
        );
    }

    #[test]
    fn matches_needs_same_id_and_source() {
        let e = entry("dice", SourceTag::Repo([1, 2, 3, 4]));
        assert!(e.matches("dice", SourceTag::Repo([1, 2, 3, 4])));
        assert!(!e.matches("dice", SourceTag::Repo([1, 2, 3, 5])));
        assert!(!e.matches("dice", SourceTag::Official));
        assert!(!e.matches("poll", SourceTag::Repo([1, 2, 3, 4])));
    }

    #[test]
    fn throttle_sends_at_most_once_per_ten_seconds() {
        let t0 = Instant::now();
        let mut throttle = AnnounceThrottle::default();
        assert!(!throttle.poll(t0), "nothing pending");
        throttle.mark_changed();
        assert!(throttle.poll(t0));
        throttle.mark_changed();
        assert_eq!(throttle.due_at(t0), Some(t0 + MIN_INTERVAL));
        assert!(!throttle.poll(t0 + Duration::from_secs(9)));
        assert!(throttle.poll(t0 + MIN_INTERVAL));
        assert!(!throttle.poll(t0 + Duration::from_secs(30)), "sent already");
        assert_eq!(throttle.due_at(t0), None);
        throttle.reset();
        assert!(throttle.poll(t0 + Duration::from_secs(31)));
    }
}
