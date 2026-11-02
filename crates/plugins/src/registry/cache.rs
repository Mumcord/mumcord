//! Offline copy of a signed file (FR-6.57): body, signature and ETag in one directory. The copy
//! is re-verified on every load, so a tampered cache is simply ignored.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::SIGNATURE_SUFFIX;

pub(super) struct Cache {
    dir: PathBuf,
    name: &'static str,
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
        }
    }

    fn path(&self, suffix: &str) -> PathBuf {
        self.dir.join(format!("{}{suffix}", self.name))
    }

    pub fn load(&self) -> Option<Cached> {
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
