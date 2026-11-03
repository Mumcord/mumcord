//! Saving a received file (FR-7.25 – FR-7.27). The bytes go into a hidden partial file in the save
//! folder and only become a visible file after size and SHA-256 match the offer, so a broken or
//! tampered transfer never leaves anything a user could open.
//!
//! The API is synchronous file I/O; async callers run it on a blocking thread
//! (`tokio::task::spawn_blocking`).

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::sanitize::sanitize_file_name;

/// Why a download could not start or did not produce a file.
#[derive(Debug, thiserror::Error)]
pub enum ReceiveError {
    /// Free space in the save folder is below the announced size; nothing was created.
    #[error("Not enough disk space in {}", folder.display())]
    NoSpace { folder: PathBuf },
    /// More or fewer bytes than announced, or a different SHA-256; the partial file is deleted.
    #[error("The file changed in transit or doesn't match the offer – deleted")]
    Mismatch,
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// A download in progress. Dropping it without [`IncomingFile::finish`] (Cancel, connection
/// lost, mismatch) deletes the partial file.
pub struct IncomingFile {
    folder: PathBuf,
    name: String,
    partial: PathBuf,
    writer: Option<BufWriter<File>>,
    hasher: Sha256,
    expected_size: u64,
    expected_sha256: String,
    received: u64,
}

impl IncomingFile {
    /// Checks free space against `size`, then creates the hidden partial file for share `id` in
    /// `folder`. `name` is the offered file name (sanitized again here), `sha256` the offer's
    /// 64-hex checksum.
    pub fn create(
        folder: &Path,
        id: &str,
        name: &str,
        size: u64,
        sha256: &str,
    ) -> Result<Self, ReceiveError> {
        ensure_space(folder, available_space(folder)?, size)?;
        let partial = folder.join(format!(".mumcord-{id}.part"));
        let file = create_hidden(&partial)?;
        Ok(Self {
            folder: folder.to_path_buf(),
            name: sanitize_file_name(name),
            partial,
            writer: Some(BufWriter::new(file)),
            hasher: Sha256::new(),
            expected_size: size,
            expected_sha256: sha256.to_ascii_lowercase(),
            received: 0,
        })
    }

    /// Bytes written so far, for the progress display.
    pub fn received(&self) -> u64 {
        self.received
    }

    /// Appends `data`. More bytes than announced is a mismatch; the caller then drops `self`.
    pub fn write(&mut self, data: &[u8]) -> Result<(), ReceiveError> {
        let total = self.received + data.len() as u64;
        if total > self.expected_size {
            return Err(ReceiveError::Mismatch);
        }
        let writer = self.writer.as_mut().ok_or(ReceiveError::Mismatch)?;
        writer.write_all(data)?;
        // Hashing while the data streams in avoids reading up to 2 GiB a second time.
        self.hasher.update(data);
        self.received = total;
        Ok(())
    }

    /// After the last byte: checks size and SHA-256, moves the file to its final name in the
    /// save folder and marks it as downloaded from `server_host`. Returns the final path.
    pub fn finish(mut self, server_host: &str) -> Result<PathBuf, ReceiveError> {
        let writer = self.writer.take().ok_or(ReceiveError::Mismatch)?;
        let file = writer
            .into_inner()
            .map_err(io::IntoInnerError::into_error)?;
        file.sync_all()?;
        drop(file);
        let digest = hex(&std::mem::take(&mut self.hasher).finalize());
        if self.received != self.expected_size || digest != self.expected_sha256 {
            return Err(ReceiveError::Mismatch);
        }
        let target = self.move_to_final_name()?;
        unhide(&target);
        super::motw::mark(&target, server_host);
        Ok(target)
    }

    /// Reserves a free name (`report.pdf`, `report (1).pdf`, …) with an exclusive create, so an
    /// existing file is never overwritten, then replaces the reservation with the partial file.
    fn move_to_final_name(&mut self) -> Result<PathBuf, ReceiveError> {
        for n in 0u32.. {
            let candidate = self.folder.join(numbered_name(&self.name, n));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(_) => {
                    if let Err(error) = fs::rename(&self.partial, &candidate) {
                        // The empty reservation must not stay behind.
                        if let Err(cleanup) = fs::remove_file(&candidate) {
                            tracing::warn!(error = %cleanup, "could not remove a name reservation");
                        }
                        return Err(error.into());
                    }
                    // Renamed away: nothing left for `Drop` to delete.
                    self.partial = PathBuf::new();
                    return Ok(candidate);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        unreachable!("u32 names exhausted")
    }
}

impl Drop for IncomingFile {
    fn drop(&mut self) {
        // Close the handle first: Windows can't delete an open file.
        self.writer = None;
        if self.partial.as_os_str().is_empty() {
            return;
        }
        if let Err(error) = fs::remove_file(&self.partial)
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(%error, "could not delete a partial download");
        }
    }
}

fn ensure_space(folder: &Path, available: u64, size: u64) -> Result<(), ReceiveError> {
    if available < size {
        return Err(ReceiveError::NoSpace {
            folder: folder.to_path_buf(),
        });
    }
    Ok(())
}

/// `name` for `n == 0`, otherwise " (n)" inserted before the extension.
fn numbered_name(name: &str, n: u32) -> String {
    if n == 0 {
        return name.to_owned();
    }
    match name.rfind('.') {
        Some(dot) if dot > 0 => format!("{} ({n}){}", &name[..dot], &name[dot..]),
        _ => format!("{name} ({n})"),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(unix)]
fn available_space(folder: &Path) -> io::Result<u64> {
    let stat = rustix::fs::statvfs(folder)?;
    Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
}

#[cfg(windows)]
fn available_space(folder: &Path) -> io::Result<u64> {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::HSTRING;
    let mut available = 0u64;
    // SAFETY: the path is a valid null-terminated wide string and the out pointer lives for the
    // whole call.
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(folder), Some(&mut available), None, None) }
        .map_err(io::Error::other)?;
    Ok(available)
}

#[cfg(not(any(unix, windows)))]
fn available_space(_folder: &Path) -> io::Result<u64> {
    Ok(u64::MAX)
}

/// The leading dot hides the partial file on macOS and Linux; Windows needs the attribute.
fn create_hidden(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.attributes(windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN.0);
    }
    options.open(path)
}

/// The final file must be visible; on Windows the hidden attribute survives the rename.
fn unhide(path: &Path) {
    #[cfg(windows)]
    {
        use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, SetFileAttributesW};
        use windows::core::HSTRING;
        // SAFETY: the path is a valid null-terminated wide string for the duration of the call.
        if let Err(error) =
            unsafe { SetFileAttributesW(&HSTRING::from(path), FILE_ATTRIBUTE_NORMAL) }
        {
            tracing::warn!(%error, "could not unhide a received file");
        }
    }
    #[cfg(not(windows))]
    let _ = path;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef";

    fn sha(data: &[u8]) -> String {
        hex(&Sha256::digest(data))
    }

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn receive(
        dir: &Path,
        name: &str,
        data: &[u8],
        announced: &[u8],
    ) -> Result<PathBuf, ReceiveError> {
        let mut file =
            IncomingFile::create(dir, ID, name, announced.len() as u64, &sha(announced))?;
        file.write(data)?;
        file.finish("voice.example.org")
    }

    #[test]
    fn matching_file_gets_final_name_and_no_partial_remains() {
        let dir = tempfile::tempdir().unwrap();
        let path = receive(dir.path(), "notes.txt", b"hello", b"hello").unwrap();
        assert_eq!(path, dir.path().join("notes.txt"));
        assert_eq!(fs::read(&path).unwrap(), b"hello");
        assert_eq!(entries(dir.path()), ["notes.txt"]);
    }

    #[test]
    fn partial_file_is_hidden_while_downloading() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = IncomingFile::create(dir.path(), ID, "a.txt", 3, &sha(b"abc")).unwrap();
        file.write(b"ab").unwrap();
        assert_eq!(file.received(), 2);
        assert_eq!(entries(dir.path()), [format!(".mumcord-{ID}.part")]);
    }

    #[test]
    fn existing_names_get_a_number_before_the_extension() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("report.pdf"), b"old").unwrap();
        fs::write(dir.path().join("report (1).pdf"), b"old").unwrap();
        let path = receive(dir.path(), "report.pdf", b"new", b"new").unwrap();
        assert_eq!(path, dir.path().join("report (2).pdf"));
        assert_eq!(fs::read(dir.path().join("report.pdf")).unwrap(), b"old");
    }

    #[test]
    fn numbered_names() {
        let cases = [
            ("a.txt", 0, "a.txt"),
            ("a.txt", 1, "a (1).txt"),
            ("archive.tar.gz", 2, "archive.tar (2).gz"),
            ("README", 1, "README (1)"),
            (".profile", 1, ".profile (1)"),
        ];
        for (name, n, expected) in cases {
            assert_eq!(numbered_name(name, n), expected, "{name} {n}");
        }
    }

    #[test]
    fn mismatches_leave_nothing_behind() {
        let cases: [(&[u8], &[u8]); 3] = [
            (b"hellO", b"hello"),  // changed byte
            (b"hell", b"hello"),   // too few bytes
            (b"hello!", b"hello"), // too many bytes
        ];
        for (data, announced) in cases {
            let dir = tempfile::tempdir().unwrap();
            let result = receive(dir.path(), "a.txt", data, announced);
            assert!(matches!(result, Err(ReceiveError::Mismatch)), "{data:?}");
            assert!(entries(dir.path()).is_empty(), "{data:?}");
        }
    }

    #[test]
    fn dropping_a_download_deletes_the_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = IncomingFile::create(dir.path(), ID, "a.txt", 5, &sha(b"hello")).unwrap();
        file.write(b"he").unwrap();
        drop(file);
        assert!(entries(dir.path()).is_empty());
    }

    #[test]
    fn uppercase_offer_hash_matches() {
        let dir = tempfile::tempdir().unwrap();
        let mut file =
            IncomingFile::create(dir.path(), ID, "a.txt", 2, &sha(b"hi").to_uppercase()).unwrap();
        file.write(b"hi").unwrap();
        assert!(file.finish("h").is_ok());
    }

    #[test]
    fn hostile_name_is_sanitized_into_the_save_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = receive(dir.path(), "../../evil.txt", b"x", b"x").unwrap();
        assert_eq!(path.parent().unwrap(), dir.path());
    }

    #[test]
    fn not_enough_space_names_the_folder_and_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let error = ensure_space(dir.path(), 10, 11).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("Not enough disk space in {}", dir.path().display())
        );
        assert!(ensure_space(dir.path(), 11, 11).is_ok());
        let too_big = IncomingFile::create(dir.path(), ID, "a.txt", u64::MAX, &sha(b""));
        assert!(matches!(too_big, Err(ReceiveError::NoSpace { .. })));
        assert!(entries(dir.path()).is_empty());
    }

    #[test]
    fn free_space_is_reported_for_a_real_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert!(available_space(dir.path()).unwrap() > 0);
    }
}
