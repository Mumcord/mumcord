//! Offline copy of a signed file (FR-6.57): body, signature and ETag in one directory. The copy
//! is re-verified on every load, so a tampered cache is simply ignored.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::SIGNATURE_SUFFIX;

pub(super) struct Cache {
    dir: PathBuf,
    name: &'static str,
    /// Off (spec 12 FR-12.11): never reads or writes the offline copy, always fetch fresh.
    enabled: bool,
}

pub(super) struct Cached {
    pub bytes: Vec<u8>,
    pub signature: String,
    pub etag: Option<String>,
}

impl Cache {
    pub fn new(dir: &Path, name: &'static str) -> Self {
        Self {
            dir: dir.to_owned(),
            name,
            enabled: true,
        }
    }

    /// A cache that is never used: `load` finds nothing and `store` writes nothing.
    pub fn disabled(dir: &Path, name: &'static str) -> Self {
        Self {
            dir: dir.to_owned(),
            name,
            enabled: false,
        }
    }

    fn path(&self, suffix: &str) -> PathBuf {
        self.dir.join(format!("{}{suffix}", self.name))
    }

    pub fn load(&self) -> Option<Cached> {
        if !self.enabled {
            return None;
        }
        let bytes = fs::read(self.path("")).ok()?;
        let signature = fs::read_to_string(self.path(SIGNATURE_SUFFIX)).ok()?;
        let etag = fs::read_to_string(self.path(".etag")).ok();
        Some(Cached {
            bytes,
            signature,
            etag,
        })
    }

    pub fn store(&self, cached: &Cached) -> io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        fs::create_dir_all(&self.dir)?;
        // The ETag goes first and is removed when absent: a stale ETag next to a newer body
        // could otherwise answer "not modified" for the wrong copy.
        let etag = self.path(".etag");
        match &cached.etag {
            Some(value) => write_atomic(&etag, value.as_bytes())?,
            None => match fs::remove_file(&etag) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            },
        }
        write_atomic(&self.path(""), &cached.bytes)?;
        write_atomic(&self.path(SIGNATURE_SUFFIX), cached.signature.as_bytes())
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn disabled_cache_neither_stores_nor_loads() {
        // FR-12.11: with the offline copy off, nothing is written and nothing is read back.
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::disabled(dir.path(), "index");
        cache
            .store(&Cached {
                bytes: vec![1],
                signature: "s".into(),
                etag: Some("e".into()),
            })
            .unwrap();
        assert!(!dir.path().join("index").exists());
        assert!(cache.load().is_none());
    }
}
