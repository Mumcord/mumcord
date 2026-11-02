//! The kill switch (FR-6.70): entries of the official `revoked.json` turn matching plugins off
//! (`disable`) or put a warning on their row (`warn`). Official revocations apply to plugins
//! from every repository; developer folders are unsigned and never match. Revocation never
//! touches a plugin's data, and a plugin turned off this way cannot be turned on again while
//! the entry still matches. A newer version that is not revoked can replace it (updates).

use std::path::Path;

use mumcord_store::PluginRecord;

use super::{Origin, PluginService, ServiceError};
use crate::registry::{REVOKED_FILE, Revocation, RevocationList, SIGNATURE_SUFFIX, Severity};
use crate::verify::RegistryKeys;

/// Shown before the reason of a `disable` revocation on the plugin row (AC-6.20).
pub const REVOKED_PREFIX_TEXT: &str = "Turned off by Mumcord: ";

/// `plugin.disabled_reason` of a plugin a revocation turned off.
const REVOKED_REASON: &str = "revoked";

/// A revocation matching an installed plugin, for its row in Settings → Plugins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevocationNotice {
    pub severity: Severity,
    /// In the UI language, falling back to English. Comes from the signed official list.
    pub reason: String,
}

impl RevocationNotice {
    /// The row text: "Turned off by Mumcord: <reason>" for `disable`, the reason for `warn`.
    pub fn text(&self) -> String {
        match self.severity {
            Severity::Disable => format!("{REVOKED_PREFIX_TEXT}{}", self.reason),
            Severity::Warn => self.reason.clone(),
        }
    }
}

/// The list that applies before today's check has run: the copy bundled with this release
/// merged with the last verified download in `cache_dir` (the official registry cache).
pub fn cached_revocations(cache_dir: &Path) -> RevocationList {
    let mut list = RevocationList::bundled();
    if let Some(cached) = read_cached(cache_dir) {
        list.merge(cached);
    }
    list
}

fn read_cached(cache_dir: &Path) -> Option<RevocationList> {
    let bytes = std::fs::read(cache_dir.join(REVOKED_FILE)).ok()?;
    let signature =
        std::fs::read_to_string(cache_dir.join(format!("{REVOKED_FILE}{SIGNATURE_SUFFIX}")))
            .ok()?;
    if let Err(error) = RegistryKeys::official().verify(&bytes, &signature) {
        tracing::warn!(%error, "ignoring the cached revocation list");
        return None;
    }
    RevocationList::parse(&bytes)
        .inspect_err(|error| tracing::warn!(%error, "ignoring the cached revocation list"))
        .ok()
}

impl PluginService {
    /// Applies a revocation list (bundled, cached or freshly fetched): every installed plugin
    /// matching a `disable` entry is stopped and turned off with reason `revoked`; `warn`
    /// entries only show on the row. With `--no-plugins` nothing is persisted.
    pub fn apply_revocations(&mut self, list: RevocationList) -> Result<(), ServiceError> {
        self.revoked = list;
        if !self.no_plugins {
            let records = self.store().plugins()?;
            for record in records.iter().filter(|r| r.removed_at.is_none()) {
                if self.disabling_revocation(record).is_none()
                    || (!record.enabled
                        && record.disabled_reason.as_deref() == Some(REVOKED_REASON))
                {
                    continue;
                }
                self.update_record(&record.id, |r| {
                    r.enabled = false;
                    r.disabled_reason = Some(REVOKED_REASON.into());
                })?;
                self.stop(&record.id);
                tracing::info!(plugin = record.id.as_str(), "turned off by a revocation");
            }
        }
        self.changed();
        Ok(())
    }

    /// The strongest entry matching the installed package of `record`.
    fn matching_revocation(&self, record: &PluginRecord) -> Option<&Revocation> {
        match Origin::parse(&record.source) {
            Some(Origin::Official | Origin::Repo(_)) => {}
            Some(Origin::Dev(_)) | None => return None,
        }
        let sha256 = record.sha256.as_deref()?;
        self.revoked.find(
            &record.id,
            &record.version,
            sha256,
            record.author_key.as_deref(),
        )
    }

    pub(super) fn revocation_notice(&self, record: &PluginRecord) -> Option<RevocationNotice> {
        self.matching_revocation(record).map(|r| RevocationNotice {
            severity: r.severity,
            reason: r.reason(&self.language).to_owned(),
        })
    }

    /// The reason of a `disable` entry matching `record`; such a plugin stays off.
    pub(super) fn disabling_revocation(&self, record: &PluginRecord) -> Option<String> {
        self.matching_revocation(record)
            .filter(|r| r.severity == Severity::Disable)
            .map(|r| r.reason(&self.language).to_owned())
    }

    /// Whether a package that is not installed yet matches any entry; such versions are never
    /// offered as an update.
    pub(super) fn is_revoked(
        &self,
        id: &str,
        version: &str,
        sha256: &str,
        author_key: Option<&str>,
    ) -> bool {
        self.revoked.find(id, version, sha256, author_key).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_texts() {
        let disable = RevocationNotice {
            severity: Severity::Disable,
            reason: "Leaks data".into(),
        };
        assert_eq!(disable.text(), "Turned off by Mumcord: Leaks data");
        let warn = RevocationNotice {
            severity: Severity::Warn,
            reason: "Old".into(),
        };
        assert_eq!(warn.text(), "Old");
    }

    #[test]
    fn without_a_cached_copy_the_bundled_list_applies() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(cached_revocations(dir.path()), RevocationList::bundled());
        std::fs::write(dir.path().join(REVOKED_FILE), b"{}").expect("write");
        std::fs::write(dir.path().join("revoked.json.minisig"), "not a signature").expect("write");
        assert_eq!(cached_revocations(dir.path()), RevocationList::bundled());
    }
}
