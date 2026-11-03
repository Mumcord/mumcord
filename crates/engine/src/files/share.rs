//! The sender's share registry (FR-7.14, FR-7.15): which files this client offers right now.
//! It lives in memory only; nothing about shares survives a restart.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use mumcord_store::ServerId;

use super::prepare::{FileStamp, random_hex, remove_zip};

/// How long a share stays active after its offer was sent (FR-7.15).
const LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

/// Share lifetime, shortened by the debug-only test hook.
pub fn lifetime() -> Duration {
    crate::test_hooks::file_expiry().unwrap_or(LIFETIME)
}

/// A fresh 128-bit share id as 32 lowercase hex characters (FR-7.14).
pub fn new_share_id() -> String {
    random_hex(16)
}

/// Why a share ended; the sender's card shows it after "Sharing ended".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    Expired,
    /// The sender pressed Stop sharing.
    Stopped,
    /// The sender left the server the offer was posted on.
    LeftServer,
    /// Mumcord is quitting.
    Quit,
    /// The file changed or disappeared on disk.
    FileChanged,
}

impl EndReason {
    /// Note shown on the sender's card, where the spec defines one.
    pub fn note(self) -> Option<&'static str> {
        match self {
            Self::FileChanged => Some("The file changed on disk, so sharing stopped"),
            _ => None,
        }
    }
}

/// One active share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    pub id: String,
    /// Server the offer was posted on.
    pub server_id: ServerId,
    /// File that is sent: the user's file, or the zip in the cache for a folder.
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub mime: String,
    pub sha256: String,
    /// Size and modification time when the share was created.
    pub stamp: FileStamp,
    /// Unix seconds.
    pub expires: u64,
    /// Whether `path` is a zip Mumcord made and must delete when the share ends.
    pub owns_zip: bool,
    /// Names of users who finished a download, in order.
    pub downloaded_by: Vec<String>,
}

/// A share that just ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    pub share: Share,
    pub reason: EndReason,
}

/// All active shares of this client, across all servers.
#[derive(Debug, Default)]
pub struct ShareRegistry {
    shares: HashMap<String, Share>,
}

impl ShareRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a share once its offer was posted. Replaces nothing: ids are random.
    pub fn insert(&mut self, share: Share) {
        self.shares.insert(share.id.clone(), share);
    }

    pub fn get(&self, id: &str) -> Option<&Share> {
        self.shares.get(id)
    }

    pub fn is_active(&self, id: &str) -> bool {
        self.shares.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.shares.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shares.is_empty()
    }

    /// Records a finished download; returns the updated list of names.
    pub fn record_download(&mut self, id: &str, name: &str) -> Option<&[String]> {
        let share = self.shares.get_mut(id)?;
        share.downloaded_by.push(name.to_owned());
        Some(&share.downloaded_by)
    }

    /// Stop sharing.
    pub fn stop(&mut self, id: &str) -> Option<Ended> {
        self.end(id, EndReason::Stopped)
    }

    /// The sender left `server_id`: ends every share posted there.
    pub fn end_server(&mut self, server_id: ServerId) -> Vec<Ended> {
        self.end_where(|s| s.server_id == server_id, EndReason::LeftServer)
    }

    /// Mumcord quits: ends everything (and deletes the zips).
    pub fn end_all(&mut self) -> Vec<Ended> {
        self.end_where(|_| true, EndReason::Quit)
    }

    /// Ends shares whose expiry (unix seconds) is at or before `now`.
    pub fn sweep_expired(&mut self, now: u64) -> Vec<Ended> {
        self.end_where(|s| s.expires <= now, EndReason::Expired)
    }

    /// Checks the file before a transfer starts (FR-7.15). An unchanged share is returned; a
    /// changed or missing file ends the share. `None` means there is no such share.
    pub fn verify_unchanged(&mut self, id: &str) -> Option<Result<&Share, Ended>> {
        let share = self.shares.get(id)?;
        if FileStamp::of(&share.path).is_ok_and(|now| now == share.stamp) {
            return self.shares.get(id).map(Ok);
        }
        self.end(id, EndReason::FileChanged).map(Err)
    }

    fn end(&mut self, id: &str, reason: EndReason) -> Option<Ended> {
        let share = self.shares.remove(id)?;
        if share.owns_zip {
            remove_zip(&share.path);
        }
        Some(Ended { share, reason })
    }

    fn end_where(&mut self, pred: impl Fn(&Share) -> bool, reason: EndReason) -> Vec<Ended> {
        let ids: Vec<String> = self
            .shares
            .values()
            .filter(|s| pred(s))
            .map(|s| s.id.clone())
            .collect();
        ids.iter().filter_map(|id| self.end(id, reason)).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::files::is_share_id;
    use crate::files::prepare::{Cancel, zip_folder};

    fn share(id: &str, server_id: ServerId, path: &Path, expires: u64) -> Share {
        Share {
            id: id.into(),
            server_id,
            path: path.to_path_buf(),
            name: "a.txt".into(),
            size: 3,
            mime: "text/plain".into(),
            sha256: "0".repeat(64),
            stamp: FileStamp::of(path).expect("test fixture"),
            expires,
            owns_zip: false,
            downloaded_by: Vec::new(),
        }
    }

    fn ids(ended: &[Ended]) -> Vec<&str> {
        let mut ids: Vec<&str> = ended.iter().map(|e| e.share.id.as_str()).collect();
        ids.sort();
        ids
    }

    #[test]
    fn share_ids_are_random_128_bit() {
        let a = new_share_id();
        assert!(is_share_id(&a));
        assert_ne!(a, new_share_id());
    }

    #[test]
    fn ends_by_stop_server_expiry_and_quit() {
        let dir = tempfile::tempdir().expect("test fixture");
        let path = dir.path().join("a.txt");
        fs::write(&path, b"abc").expect("test fixture");
        let mut reg = ShareRegistry::new();
        reg.insert(share("a", 1, &path, 100));
        reg.insert(share("b", 1, &path, 200));
        reg.insert(share("c", 2, &path, 300));
        reg.insert(share("d", 2, &path, 400));

        assert_eq!(
            reg.stop("a").expect("test fixture").reason,
            EndReason::Stopped
        );
        assert!(reg.stop("a").is_none());

        let ended = reg.sweep_expired(300);
        assert_eq!(ids(&ended), ["b", "c"]);
        assert!(ended.iter().all(|e| e.reason == EndReason::Expired));

        reg.insert(share("e", 1, &path, 500));
        let ended = reg.end_server(1);
        assert_eq!(ids(&ended), ["e"]);
        assert_eq!(ended[0].reason, EndReason::LeftServer);

        let ended = reg.end_all();
        assert_eq!(ids(&ended), ["d"]);
        assert_eq!(ended[0].reason, EndReason::Quit);
        assert!(reg.is_empty());
    }

    #[test]
    fn changed_or_missing_file_ends_share() {
        let dir = tempfile::tempdir().expect("test fixture");
        let path = dir.path().join("a.txt");
        fs::write(&path, b"abc").expect("test fixture");
        let mut reg = ShareRegistry::new();
        reg.insert(share("a", 1, &path, u64::MAX));
        reg.insert(share("b", 1, &path, u64::MAX));

        assert!(matches!(reg.verify_unchanged("a"), Some(Ok(s)) if s.id == "a"));

        fs::write(&path, b"abcd").expect("test fixture");
        let ended = reg
            .verify_unchanged("a")
            .expect("test fixture")
            .expect_err("should fail");
        assert_eq!(ended.reason, EndReason::FileChanged);
        assert_eq!(
            ended.reason.note(),
            Some("The file changed on disk, so sharing stopped")
        );
        assert!(!reg.is_active("a"));

        fs::remove_file(&path).expect("test fixture");
        assert!(reg.verify_unchanged("b").expect("test fixture").is_err());
        assert!(reg.verify_unchanged("b").is_none());
    }

    #[test]
    fn ending_a_folder_share_deletes_its_zip() {
        let dir = tempfile::tempdir().expect("test fixture");
        let src = dir.path().join("Folder");
        fs::create_dir(&src).expect("test fixture");
        fs::write(src.join("x"), b"x").expect("test fixture");
        let zip = zip_folder(
            &src,
            &dir.path().join("cache"),
            &Cancel::default(),
            &mut |_, _| {},
        )
        .expect("test fixture");
        let mut reg = ShareRegistry::new();
        let mut s = share("z", 1, &zip, u64::MAX);
        s.owns_zip = true;
        reg.insert(s);
        reg.stop("z").expect("test fixture");
        assert!(!zip.exists());
        assert!(!zip.parent().expect("test fixture").exists());
    }

    #[test]
    fn records_downloads() {
        let dir = tempfile::tempdir().expect("test fixture");
        let path = dir.path().join("a.txt");
        fs::write(&path, b"abc").expect("test fixture");
        let mut reg = ShareRegistry::new();
        reg.insert(share("a", 1, &path, u64::MAX));
        reg.record_download("a", "Bob");
        assert_eq!(
            reg.record_download("a", "Carol").expect("test fixture"),
            ["Bob", "Carol"]
        );
        assert!(reg.record_download("x", "Bob").is_none());
    }

    #[test]
    fn default_lifetime_is_a_day() {
        if crate::test_hooks::file_expiry().is_none() {
            assert_eq!(lifetime(), Duration::from_secs(86_400));
        }
    }
}
