//! The pure part of the updater (issue #168): which release to offer, what the update manifest
//! may point to, and whether a download is the one that was signed and published.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};

use crate::commands::updates::ReleaseNote;

/// A parsed `major.minor.patch[-pre]` version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    core: [u64; 3],
    /// Pre-releases sort before their release; the flag is inverted so the derived order fits.
    is_final: bool,
}

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let text = text.split('+').next()?;
        let (core, pre) = match text.split_once('-') {
            Some((core, _)) => (core, true),
            None => (text, false),
        };
        let mut parts = core.split('.');
        let mut next = || parts.next()?.parse::<u64>().ok();
        let version = Self {
            core: [next()?, next()?, next()?],
            is_final: !pre,
        };
        parts.next().is_none().then_some(version)
    }

    pub fn is_prerelease(&self) -> bool {
        !self.is_final
    }
}

/// The newest release to offer and the notes of every release since `current`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub version: String,
    /// Newest first.
    pub notes: Vec<ReleaseNote>,
}

/// Picks the update from a GitHub "list releases" document. Only the stable channel exists in
/// 0.5.0: pre-releases are offered only to users who run one. Never offers a downgrade, nor
/// the version the user skipped.
pub fn pick_offer(body: &[u8], current: &str, skipped: Option<&str>) -> Option<Offer> {
    let current = Version::parse(current)?;
    let releases: Vec<serde_json::Value> = serde_json::from_slice(body).ok()?;
    let mut newer: Vec<(Version, ReleaseNote)> = releases
        .iter()
        .filter_map(|release| {
            let flag = |key| release.get(key).and_then(serde_json::Value::as_bool);
            if flag("draft") == Some(true) {
                return None;
            }
            let tag = release.get("tag_name")?.as_str()?;
            let version = Version::parse(tag)?;
            let pre = flag("prerelease") == Some(true) || version.is_prerelease();
            if (pre && !current.is_prerelease()) || version <= current {
                return None;
            }
            let date = release
                .get("published_at")
                .and_then(serde_json::Value::as_str)
                .and_then(|d| d.get(..10))
                .map(str::to_string);
            let body = release
                .get("body")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let note = ReleaseNote {
                version: tag.trim().trim_start_matches('v').to_string(),
                date,
                changes: whats_new(body),
            };
            Some((version, note))
        })
        .collect();
    newer.sort_by_key(|(version, _)| std::cmp::Reverse(*version));
    let newest = newer.first()?.1.version.clone();
    if skipped.is_some_and(|s| Version::parse(s) == Version::parse(&newest)) {
        return None;
    }
    Some(Offer {
        version: newest,
        notes: newer.into_iter().map(|(_, note)| note).collect(),
    })
}

/// The "What's new in X" section the release workflow copies from CHANGELOG.md into the release
/// text; the download table and signing notes around it are not shown in the app.
fn whats_new(body: &str) -> String {
    let mut lines = body
        .lines()
        .skip_while(|l| !l.starts_with("## What's new in"));
    if lines.next().is_none() {
        return String::new();
    }
    lines
        .take_while(|l| !l.starts_with("## "))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Why an update could not be prepared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    /// The manifest is not one, announces another version or has no bundle for this platform.
    Manifest,
    /// The bundle's address is not inside this release's download folder.
    Address,
    Signature,
    Checksum,
}

/// The bundle for `target` from the update manifest (`latest.json`, Tauri's static format).
/// The address must lie in `release_base`, the release's own download folder, so a tampered
/// manifest cannot send the app anywhere else.
pub fn manifest_entry(
    body: &[u8],
    version: &str,
    target: &str,
    release_base: &str,
) -> Result<(String, String, serde_json::Value), Rejected> {
    let manifest: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| Rejected::Manifest)?;
    let announced = manifest.get("version").and_then(serde_json::Value::as_str);
    if announced.and_then(Version::parse) != Version::parse(version) {
        return Err(Rejected::Manifest);
    }
    let entry = manifest
        .get("platforms")
        .and_then(|p| p.get(target))
        .ok_or(Rejected::Manifest)?;
    let field = |key| {
        entry
            .get(key)
            .and_then(serde_json::Value::as_str)
            .ok_or(Rejected::Manifest)
    };
    let url = field("url")?;
    let signature = field("signature")?;
    let file = url.strip_prefix(release_base).ok_or(Rejected::Address)?;
    if file.is_empty() || file.contains(['/', '\\', '?', '#']) || file.starts_with('.') {
        return Err(Rejected::Address);
    }
    Ok((url.to_string(), signature.to_string(), manifest))
}

/// The file name a bundle has in `SHA256SUMS`: the last part of its address.
pub fn file_name(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or(url)
}

/// Checks the minisign signature Tauri's updater uses (key and signature each base64 of the
/// minisign text) and that it was made for `version`, so an old signed bundle cannot be passed
/// off as a newer release.
pub fn verify_signature(
    data: &[u8],
    signature: &str,
    pubkey: &str,
    version: &str,
) -> Result<(), Rejected> {
    let decode = |text: &str| {
        STANDARD
            .decode(text.trim())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or(Rejected::Signature)
    };
    let key = PublicKey::decode(&decode(pubkey)?).map_err(|_| Rejected::Signature)?;
    let signature = Signature::decode(&decode(signature)?).map_err(|_| Rejected::Signature)?;
    key.verify(data, &signature, true)
        .map_err(|_| Rejected::Signature)?;
    // Readable only now: the global signature covers the trusted comment.
    let signed = signature
        .trusted_comment()
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"));
    match signed {
        Some(signed) if Version::parse(signed) == Version::parse(version) => Ok(()),
        _ => Err(Rejected::Signature),
    }
}

/// Whether `SHA256SUMS` (the `sha256sum` output the release publishes) lists `file` with the
/// checksum of `data`.
pub fn checksum_matches(sums: &[u8], file: &str, data: &[u8]) -> bool {
    let actual = Sha256::digest(data);
    let actual: String = actual.iter().map(|b| format!("{b:02x}")).collect();
    String::from_utf8_lossy(sums).lines().any(|line| {
        let Some((hash, name)) = line.split_once(char::is_whitespace) else {
            return false;
        };
        let name = name.trim_start().trim_start_matches('*');
        name == file && hash.eq_ignore_ascii_case(&actual)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn release(tag: &str, pre: bool, body: &str) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag,
            "prerelease": pre,
            "draft": false,
            "published_at": "2026-11-12T10:00:00Z",
            "html_url": "https://evil.example/x",
            "body": body,
        })
    }

    fn list(releases: &[serde_json::Value]) -> Vec<u8> {
        serde_json::to_vec(releases).expect("json")
    }

    const NOTES: &str = "## Downloads\n\n- AppImage\n\n## What's new in 0.4.5\n### Added\n- Pick a device.\n### Fixed\n- Echo.\n";

    #[test]
    fn version_order_includes_prereleases() {
        let v = |s| Version::parse(s).expect(s);
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("v1.0.0") > v("1.0.0-rc.1"));
        assert!(v("1.0.0-rc.1") > v("0.9.0"));
        assert_eq!(v("1.2.3+build"), v("1.2.3"));
        assert!(Version::parse("1.2").is_none());
        assert!(Version::parse("1.2.3.4").is_none());
        assert!(Version::parse("nightly").is_none());
        assert!(Version::parse("dev~a1b2c3d").is_none());
    }

    #[test]
    fn offers_newest_with_notes_of_every_version_between() {
        let body = list(&[
            release("v0.4.5", false, NOTES),
            release("v0.4.4", false, "no section"),
            release("v0.4.3", false, ""),
            release("v0.4.2", false, ""),
            release("v0.4.1", false, ""),
        ]);
        let offer = pick_offer(&body, "0.4.2", None).expect("offer");
        assert_eq!(offer.version, "0.4.5");
        let versions: Vec<_> = offer.notes.iter().map(|n| n.version.as_str()).collect();
        assert_eq!(versions, ["0.4.5", "0.4.4", "0.4.3"]);
        assert_eq!(
            offer.notes[0].changes,
            "### Added\n- Pick a device.\n### Fixed\n- Echo."
        );
        assert_eq!(offer.notes[0].date.as_deref(), Some("2026-11-12"));
        assert_eq!(offer.notes[1].changes, "");
    }

    #[test]
    fn no_downgrade_no_same_version_no_garbage() {
        assert!(pick_offer(&list(&[release("v0.4.2", false, "")]), "0.4.2", None).is_none());
        assert!(pick_offer(&list(&[release("v0.3.9", false, "")]), "0.4.2", None).is_none());
        assert!(pick_offer(b"not json", "0.4.2", None).is_none());
        assert!(pick_offer(&list(&[release("nightly", false, "")]), "0.4.2", None).is_none());
        let draft = serde_json::json!({"tag_name": "v9.0.0", "draft": true});
        assert!(pick_offer(&list(&[draft]), "0.4.2", None).is_none());
    }

    #[test]
    fn stable_channel_skips_prereleases() {
        let body = list(&[
            release("v0.5.0-beta.1", true, ""),
            release("v0.4.3", false, ""),
        ]);
        assert_eq!(
            pick_offer(&body, "0.4.2", None).expect("offer").version,
            "0.4.3"
        );
        assert_eq!(
            pick_offer(&body, "0.4.3-rc.1", None)
                .expect("offer")
                .version,
            "0.5.0-beta.1"
        );
    }

    #[test]
    fn skipped_version_is_not_offered_but_a_newer_one_is() {
        let one = list(&[release("v0.4.4", false, "")]);
        assert!(pick_offer(&one, "0.4.2", Some("0.4.4")).is_none());
        let two = list(&[release("v0.4.5", false, ""), release("v0.4.4", false, "")]);
        assert_eq!(
            pick_offer(&two, "0.4.2", Some("0.4.4"))
                .expect("offer")
                .version,
            "0.4.5"
        );
    }

    const BASE: &str = "https://github.com/Mumcord/mumcord/releases/download/v0.4.5/";

    fn manifest(version: &str, url: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "version": version,
            "notes": "",
            "platforms": {"linux-x86_64-appimage": {"url": url, "signature": "c2ln"}}
        }))
        .expect("json")
    }

    #[test]
    fn manifest_entry_must_stay_in_the_release_folder() {
        let url = format!("{BASE}Mumcord_0.4.5_amd64.AppImage");
        let (found, signature, _) = manifest_entry(
            &manifest("0.4.5", &url),
            "0.4.5",
            "linux-x86_64-appimage",
            BASE,
        )
        .expect("entry");
        assert_eq!(found, url);
        assert_eq!(signature, "c2ln");
        assert_eq!(file_name(&found), "Mumcord_0.4.5_amd64.AppImage");

        for bad in [
            "https://evil.example/Mumcord.AppImage".to_string(),
            "https://github.com/Mumcord/mumcord/releases/download/v0.4.4/old.AppImage".into(),
            format!("{BASE}../v0.4.4/old.AppImage"),
            format!("{BASE}x?y"),
        ] {
            assert_eq!(
                manifest_entry(
                    &manifest("0.4.5", &bad),
                    "0.4.5",
                    "linux-x86_64-appimage",
                    BASE
                )
                .map(|_| ()),
                Err(Rejected::Address),
                "{bad}"
            );
        }
    }

    #[test]
    fn manifest_must_announce_the_offered_version_and_platform() {
        let url = format!("{BASE}a.AppImage");
        let wrong_version = manifest("0.4.4", &url);
        assert_eq!(
            manifest_entry(&wrong_version, "0.4.5", "linux-x86_64-appimage", BASE).map(|_| ()),
            Err(Rejected::Manifest)
        );
        let body = manifest("0.4.5", &url);
        assert_eq!(
            manifest_entry(&body, "0.4.5", "windows-x86_64-nsis", BASE).map(|_| ()),
            Err(Rejected::Manifest)
        );
        assert_eq!(
            manifest_entry(b"{", "0.4.5", "linux-x86_64-appimage", BASE).map(|_| ()),
            Err(Rejected::Manifest)
        );
    }

    /// A throwaway key pair made for this test run; nothing is stored or committed.
    struct TestKey {
        pk: minisign::PublicKeyBox,
        sk: minisign::SecretKey,
    }

    impl TestKey {
        fn new() -> Self {
            let pair = minisign::KeyPair::generate_unencrypted_keypair().expect("key");
            Self {
                pk: pair.pk.to_box().expect("pk box"),
                sk: pair.sk,
            }
        }

        fn pubkey(&self) -> String {
            STANDARD.encode(self.pk.to_string())
        }

        fn sign(&self, data: &[u8], trusted: &str) -> String {
            let sig = minisign::sign(
                None,
                &self.sk,
                Cursor::new(data),
                Some(trusted),
                Some("signature from tauri secret key"),
            )
            .expect("sign");
            STANDARD.encode(sig.to_string())
        }
    }

    #[test]
    fn signature_good_bad_and_tampered() {
        let key = TestKey::new();
        let data = b"\x7fELF new Mumcord".to_vec();
        let good = key.sign(&data, "timestamp:1\tfile:a.AppImage\tversion:0.4.5");
        assert_eq!(
            verify_signature(&data, &good, &key.pubkey(), "0.4.5"),
            Ok(())
        );

        let mut tampered = data.clone();
        tampered[5] ^= 1;
        assert_eq!(
            verify_signature(&tampered, &good, &key.pubkey(), "0.4.5"),
            Err(Rejected::Signature)
        );

        let other = TestKey::new();
        assert_eq!(
            verify_signature(&data, &good, &other.pubkey(), "0.4.5"),
            Err(Rejected::Signature),
            "signed with another key"
        );
        assert_eq!(
            verify_signature(&data, "bm90IGEgc2lnbmF0dXJl", &key.pubkey(), "0.4.5"),
            Err(Rejected::Signature)
        );
        assert_eq!(
            verify_signature(&data, &good, "", "0.4.5"),
            Err(Rejected::Signature),
            "no public key in this build"
        );
    }

    #[test]
    fn signature_must_be_bound_to_the_offered_version() {
        let key = TestKey::new();
        let data = b"bundle".to_vec();
        let old = key.sign(&data, "timestamp:1\tfile:a\tversion:0.4.4");
        assert_eq!(
            verify_signature(&data, &old, &key.pubkey(), "0.4.5"),
            Err(Rejected::Signature)
        );
        let unbound = key.sign(&data, "timestamp:1\tfile:a");
        assert_eq!(
            verify_signature(&data, &unbound, &key.pubkey(), "0.4.5"),
            Err(Rejected::Signature)
        );
    }

    #[test]
    fn checksum_cross_check() {
        let data = b"bundle";
        let hash: String = Sha256::digest(data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let sums = format!("{hash}  Mumcord.AppImage\n0000  other.exe\n");
        assert!(checksum_matches(sums.as_bytes(), "Mumcord.AppImage", data));
        assert!(checksum_matches(
            format!("{hash} *Mumcord.AppImage").as_bytes(),
            "Mumcord.AppImage",
            data
        ));
        assert!(!checksum_matches(
            sums.as_bytes(),
            "Mumcord.AppImage",
            b"tampered"
        ));
        assert!(!checksum_matches(sums.as_bytes(), "missing.AppImage", data));
        assert!(!checksum_matches(b"", "Mumcord.AppImage", data));
    }
}
