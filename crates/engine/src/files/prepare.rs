//! Preparing a file for sharing (FR-7.12 – FR-7.14): size check, zipping folders into the cache
//! and the streaming SHA-256. Everything here does blocking file I/O, so the async wrappers run it
//! on tokio's blocking pool; the blocking functions are public for callers already off the runtime.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use ring::rand::{SecureRandom, SystemRandom};
use sha2::{Digest, Sha256};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use super::MAX_FILE_SIZE;

/// Read buffer for hashing and zipping; big enough to keep SHA-256 above 300 MB/s.
const CHUNK: usize = 1024 * 1024;

/// Entries at or above this size need zip64 headers.
const ZIP64_THRESHOLD: u64 = u32::MAX as u64;

/// Why preparing a file failed. `Display` is the text the chip shows.
#[derive(Debug)]
pub enum PrepareError {
    /// Bigger than 2 GB (FR-7.12); holds the file or folder name.
    TooLarge(String),
    /// An entry couldn't be read (FR-7.13); holds its path.
    Unreadable(PathBuf),
    /// The user removed the chip.
    Cancelled,
    /// Writing the zip into the cache failed.
    Io(io::Error),
}

impl fmt::Display for PrepareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge(name) => write!(f, "{name} is larger than 2 GB and can't be shared"),
            Self::Unreadable(path) => write!(f, "Couldn't read {}", path.display()),
            Self::Cancelled => f.write_str("Cancelled"),
            Self::Io(err) => write!(f, "Couldn't prepare the file: {err}"),
        }
    }
}

impl std::error::Error for PrepareError {}

/// Size and modification time of a shared file, compared before each transfer (FR-7.15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    pub size: u64,
    pub modified: Option<SystemTime>,
}

impl FileStamp {
    pub fn of(path: &Path) -> io::Result<Self> {
        let meta = fs::metadata(path)?;
        Ok(Self {
            size: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// Cooperative cancel flag shared between the chip and the blocking task.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Progress callback: bytes done, bytes total.
pub type Progress = Box<dyn FnMut(u64, u64) + Send>;

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Refuses files over 2 GB (FR-7.12) and returns the file's stamp. Empty files are fine.
pub fn check_file(path: &Path) -> Result<FileStamp, PrepareError> {
    let stamp = FileStamp::of(path).map_err(|_| PrepareError::Unreadable(path.to_path_buf()))?;
    if stamp.size > MAX_FILE_SIZE {
        return Err(PrepareError::TooLarge(display_name(path)));
    }
    Ok(stamp)
}

/// Streaming SHA-256 of `path` as 64 lowercase hex characters.
pub fn hash_file(
    path: &Path,
    cancel: &Cancel,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<String, PrepareError> {
    let unreadable = |_| PrepareError::Unreadable(path.to_path_buf());
    let mut file = File::open(path).map_err(unreadable)?;
    let total = file.metadata().map_err(unreadable)?.len();
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    progress(0, total);
    loop {
        if cancel.is_cancelled() {
            return Err(PrepareError::Cancelled);
        }
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(unreadable(err)),
        };
        hasher.update(&buf[..n]);
        done += n as u64;
        progress(done, total);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    use fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Zips the folder `src` into `<cache_dir>/files/<random>/<folder name>.zip` (FR-7.13).
/// Symbolic links are skipped, any unreadable entry stops the zip, and a zip growing past 2 GB is
/// refused. On any failure the partial zip and its folder are removed. Delete the returned zip's
/// parent folder with [`remove_zip`] when the share ends.
pub fn zip_folder(
    src: &Path,
    cache_dir: &Path,
    cancel: &Cancel,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, PrepareError> {
    zip_folder_capped(src, cache_dir, cancel, progress, MAX_FILE_SIZE)
}

fn zip_folder_capped(
    src: &Path,
    cache_dir: &Path,
    cancel: &Cancel,
    progress: &mut dyn FnMut(u64, u64),
    cap: u64,
) -> Result<PathBuf, PrepareError> {
    let name = display_name(src);
    let entries = collect_entries(src)?;
    let total = entries.iter().map(|e| e.size).sum();

    let dir = cache_dir.join("files").join(random_hex(8));
    fs::create_dir_all(&dir).map_err(PrepareError::Io)?;
    let zip_path = dir.join(format!("{name}.zip"));
    let result =
        write_zip(&zip_path, &entries, total, cancel, progress, cap).map_err(|err| match err {
            PrepareError::TooLarge(_) => PrepareError::TooLarge(name.clone()),
            other => other,
        });
    match result {
        Ok(()) => Ok(zip_path),
        Err(err) => {
            remove_zip(&zip_path);
            Err(err)
        }
    }
}

/// Removes a zip made by [`zip_folder`] together with its private cache folder.
pub fn remove_zip(zip_path: &Path) {
    let Some(dir) = zip_path.parent() else {
        return;
    };
    if let Err(err) = fs::remove_dir_all(dir)
        && err.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!("couldn't remove a shared folder's zip: {err}");
    }
}

struct Entry {
    path: PathBuf,
    /// Path inside the zip, `/`-separated, starting with the folder name.
    name: String,
    dir: bool,
    size: u64,
}

fn collect_entries(src: &Path) -> Result<Vec<Entry>, PrepareError> {
    let root = display_name(src);
    let mut entries = vec![Entry {
        path: src.to_path_buf(),
        name: format!("{root}/"),
        dir: true,
        size: 0,
    }];
    let mut i = 0;
    while i < entries.len() {
        if entries[i].dir {
            let dir = entries[i].path.clone();
            let prefix = entries[i].name.clone();
            let unreadable = |_| PrepareError::Unreadable(dir.clone());
            let mut children = Vec::new();
            for item in fs::read_dir(&dir).map_err(unreadable)? {
                let item = item.map_err(unreadable)?;
                let path = item.path();
                let meta = fs::symlink_metadata(&path)
                    .map_err(|_| PrepareError::Unreadable(path.clone()))?;
                if meta.file_type().is_symlink() {
                    continue;
                }
                let file_name = item.file_name().to_string_lossy().into_owned();
                children.push(Entry {
                    name: if meta.is_dir() {
                        format!("{prefix}{file_name}/")
                    } else {
                        format!("{prefix}{file_name}")
                    },
                    dir: meta.is_dir(),
                    size: if meta.is_dir() { 0 } else { meta.len() },
                    path,
                });
            }
            children.sort_by(|a, b| a.name.cmp(&b.name));
            entries.extend(children);
        }
        i += 1;
    }
    Ok(entries)
}

fn write_zip(
    zip_path: &Path,
    entries: &[Entry],
    total: u64,
    cancel: &Cancel,
    progress: &mut dyn FnMut(u64, u64),
    cap: u64,
) -> Result<(), PrepareError> {
    let file = File::create(zip_path).map_err(PrepareError::Io)?;
    let mut zip = ZipWriter::new(CappedWriter {
        inner: file,
        pos: 0,
        cap,
    });
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    progress(0, total);
    for entry in entries {
        if cancel.is_cancelled() {
            return Err(PrepareError::Cancelled);
        }
        if entry.dir {
            zip.add_directory(entry.name.as_str(), options)
                .map_err(zip_error)?;
            continue;
        }
        let unreadable = |_| PrepareError::Unreadable(entry.path.clone());
        let mut source = File::open(&entry.path).map_err(unreadable)?;
        zip.start_file(
            entry.name.as_str(),
            options.large_file(entry.size >= ZIP64_THRESHOLD),
        )
        .map_err(zip_error)?;
        loop {
            if cancel.is_cancelled() {
                return Err(PrepareError::Cancelled);
            }
            let n = match source.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(unreadable(err)),
            };
            zip.write_all(&buf[..n]).map_err(io_error)?;
            done += n as u64;
            progress(done, total);
        }
    }
    zip.finish()
        .map_err(zip_error)?
        .inner
        .sync_all()
        .map_err(PrepareError::Io)
}

fn io_error(err: io::Error) -> PrepareError {
    if err.kind() == io::ErrorKind::FileTooLarge {
        PrepareError::TooLarge(String::new())
    } else {
        PrepareError::Io(err)
    }
}

fn zip_error(err: zip::result::ZipError) -> PrepareError {
    match err {
        zip::result::ZipError::Io(err) => io_error(err),
        other => PrepareError::Io(io::Error::other(other)),
    }
}

/// Fails with `FileTooLarge` as soon as the zip would grow past `cap`, so an oversized folder is
/// refused while zipping instead of after filling the disk.
struct CappedWriter {
    inner: File,
    pos: u64,
    cap: u64,
}

impl Write for CappedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.pos + buf.len() as u64 > self.cap {
            return Err(io::Error::from(io::ErrorKind::FileTooLarge));
        }
        let n = self.inner.write(buf)?;
        self.pos += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl Seek for CappedWriter {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.pos = self.inner.seek(pos)?;
        Ok(self.pos)
    }
}

/// `bytes` random bytes from the system CSPRNG as lowercase hex.
pub(crate) fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    // The system RNG only fails when the OS has no entropy source at all; nothing can work then.
    SystemRandom::new()
        .fill(&mut buf)
        .expect("the system random number generator failed");
    hex(&buf)
}

/// [`hash_file`] on the blocking pool.
pub async fn hash_file_async(
    path: PathBuf,
    cancel: Cancel,
    mut progress: Progress,
) -> Result<String, PrepareError> {
    tokio::task::spawn_blocking(move || hash_file(&path, &cancel, &mut *progress))
        .await
        .map_err(|err| PrepareError::Io(io::Error::other(err)))?
}

/// [`zip_folder`] on the blocking pool.
pub async fn zip_folder_async(
    src: PathBuf,
    cache_dir: PathBuf,
    cancel: Cancel,
    mut progress: Progress,
) -> Result<PathBuf, PrepareError> {
    tokio::task::spawn_blocking(move || zip_folder(&src, &cache_dir, &cancel, &mut *progress))
        .await
        .map_err(|err| PrepareError::Io(io::Error::other(err)))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_progress() -> impl FnMut(u64, u64) {
        |_, _| {}
    }

    #[test]
    fn hashes_known_vectors() {
        let dir = tempfile::tempdir().expect("test fixture");
        let empty = dir.path().join("empty");
        fs::write(&empty, b"").expect("test fixture");
        let abc = dir.path().join("abc");
        fs::write(&abc, b"abc").expect("test fixture");
        let cancel = Cancel::default();
        assert_eq!(
            hash_file(&empty, &cancel, &mut no_progress()).expect("test fixture"),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hash_file(&abc, &cancel, &mut no_progress()).expect("test fixture"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hash_reports_progress_and_cancels() {
        let dir = tempfile::tempdir().expect("test fixture");
        let path = dir.path().join("big");
        fs::write(&path, vec![7u8; 3 * CHUNK + 5]).expect("test fixture");
        let mut last = (0, 0);
        hash_file(&path, &Cancel::default(), &mut |d, t| last = (d, t)).expect("test fixture");
        assert_eq!(last, (3 * CHUNK as u64 + 5, 3 * CHUNK as u64 + 5));

        let cancel = Cancel::default();
        cancel.cancel();
        assert!(matches!(
            hash_file(&path, &cancel, &mut no_progress()),
            Err(PrepareError::Cancelled)
        ));
    }

    #[test]
    fn check_file_allows_empty_and_reports_missing() {
        let dir = tempfile::tempdir().expect("test fixture");
        let path = dir.path().join("empty");
        fs::write(&path, b"").expect("test fixture");
        assert_eq!(check_file(&path).expect("test fixture").size, 0);
        let missing = dir.path().join("missing");
        let err = check_file(&missing).expect_err("should fail");
        assert_eq!(
            err.to_string(),
            format!("Couldn't read {}", missing.display())
        );
    }

    #[test]
    fn too_large_text() {
        assert_eq!(
            PrepareError::TooLarge("movie.mkv".into()).to_string(),
            "movie.mkv is larger than 2 GB and can't be shared"
        );
    }

    #[test]
    fn stamp_changes_with_size() {
        let dir = tempfile::tempdir().expect("test fixture");
        let path = dir.path().join("f");
        fs::write(&path, b"one").expect("test fixture");
        let before = FileStamp::of(&path).expect("test fixture");
        fs::write(&path, b"three").expect("test fixture");
        assert_ne!(FileStamp::of(&path).expect("test fixture"), before);
    }

    fn make_folder(root: &Path) -> PathBuf {
        let src = root.join("Photos");
        fs::create_dir_all(src.join("sub/empty")).expect("test fixture");
        fs::write(src.join("a.txt"), b"hello").expect("test fixture");
        fs::write(src.join("sub/b.bin"), vec![1u8; 2 * CHUNK + 3]).expect("test fixture");
        src
    }

    fn zip_names(path: &Path) -> Vec<String> {
        let mut archive =
            zip::ZipArchive::new(File::open(path).expect("test fixture")).expect("test fixture");
        let mut names: Vec<String> = archive
            .file_names()
            .map(|n| n.expect("zip entry name").into_owned())
            .collect();
        names.sort();
        let mut a = archive.by_name("Photos/a.txt").expect("test fixture");
        let mut content = String::new();
        a.read_to_string(&mut content).expect("test fixture");
        assert_eq!(content, "hello");
        assert_eq!(a.compression(), CompressionMethod::Deflated);
        names
    }

    #[test]
    fn zips_folder_into_cache() {
        let dir = tempfile::tempdir().expect("test fixture");
        let src = make_folder(dir.path());
        let cache = dir.path().join("cache");
        let mut last = (0, 0);
        let zip = zip_folder(&src, &cache, &Cancel::default(), &mut |d, t| last = (d, t))
            .expect("test fixture");
        assert_eq!(zip.file_name().expect("test fixture"), "Photos.zip");
        assert!(zip.starts_with(cache.join("files")));
        assert_eq!(last.0, last.1);
        assert_eq!(last.1, 5 + 2 * CHUNK as u64 + 3);
        assert_eq!(
            zip_names(&zip),
            [
                "Photos/",
                "Photos/a.txt",
                "Photos/sub/",
                "Photos/sub/b.bin",
                "Photos/sub/empty/"
            ]
        );
        remove_zip(&zip);
        assert!(!zip.parent().expect("test fixture").exists());
        assert!(cache.join("files").exists());
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinks() {
        let dir = tempfile::tempdir().expect("test fixture");
        let src = make_folder(dir.path());
        let outside = dir.path().join("secret.txt");
        fs::write(&outside, b"secret").expect("test fixture");
        std::os::unix::fs::symlink(&outside, src.join("link.txt")).expect("test fixture");
        std::os::unix::fs::symlink(dir.path(), src.join("loop")).expect("test fixture");
        let zip = zip_folder(
            &src,
            &dir.path().join("cache"),
            &Cancel::default(),
            &mut no_progress(),
        )
        .expect("test fixture");
        let names = zip_names(&zip);
        assert!(
            !names
                .iter()
                .any(|n| n.contains("link") || n.contains("loop"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_entry_stops_and_cleans_up() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("test fixture");
        let src = make_folder(dir.path());
        let locked = src.join("sub/b.bin");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("test fixture");
        if File::open(&locked).is_ok() {
            // Running as root: permissions don't apply, nothing to test.
            return;
        }
        let cache = dir.path().join("cache");
        let err = zip_folder(&src, &cache, &Cancel::default(), &mut no_progress())
            .expect_err("should fail");
        assert_eq!(
            err.to_string(),
            format!("Couldn't read {}", locked.display())
        );
        assert_eq!(
            fs::read_dir(cache.join("files"))
                .expect("test fixture")
                .count(),
            0
        );
    }

    #[test]
    fn cancel_removes_partial_zip() {
        let dir = tempfile::tempdir().expect("test fixture");
        let src = make_folder(dir.path());
        let cache = dir.path().join("cache");
        let cancel = Cancel::default();
        let flag = cancel.clone();
        let err = zip_folder(&src, &cache, &cancel, &mut |done, _| {
            if done > 0 {
                flag.cancel();
            }
        })
        .expect_err("should fail");
        assert!(matches!(err, PrepareError::Cancelled));
        assert_eq!(
            fs::read_dir(cache.join("files"))
                .expect("test fixture")
                .count(),
            0
        );
    }

    #[test]
    fn oversized_zip_is_refused_and_removed() {
        let dir = tempfile::tempdir().expect("test fixture");
        let src = make_folder(dir.path());
        // Incompressible data so the zip really grows past the cap.
        let mut noise = vec![0u8; CHUNK];
        SystemRandom::new().fill(&mut noise).expect("test fixture");
        fs::write(src.join("noise.bin"), noise).expect("test fixture");
        let cache = dir.path().join("cache");
        let err = zip_folder_capped(
            &src,
            &cache,
            &Cancel::default(),
            &mut no_progress(),
            64 * 1024,
        )
        .expect_err("should fail");
        assert_eq!(
            err.to_string(),
            "Photos is larger than 2 GB and can't be shared"
        );
        assert_eq!(
            fs::read_dir(cache.join("files"))
                .expect("test fixture")
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn async_wrappers_run_on_blocking_pool() {
        let dir = tempfile::tempdir().expect("test fixture");
        let src = make_folder(dir.path());
        let zip = zip_folder_async(
            src,
            dir.path().join("cache"),
            Cancel::default(),
            Box::new(|_, _| {}),
        )
        .await
        .expect("test fixture");
        let hash = hash_file_async(zip, Cancel::default(), Box::new(|_, _| {}))
            .await
            .expect("test fixture");
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn random_hex_is_fresh() {
        let a = random_hex(16);
        assert_eq!(a.len(), 32);
        assert_ne!(a, random_hex(16));
    }

    /// FR-7.14 speed target; run with `cargo test --release -- --ignored hash_throughput`.
    #[test]
    #[ignore = "throughput check, release builds only"]
    fn hash_throughput() {
        let dir = tempfile::tempdir().expect("test fixture");
        let path = dir.path().join("big");
        fs::write(&path, vec![0x5au8; 512 * 1024 * 1024]).expect("test fixture");
        let start = std::time::Instant::now();
        hash_file(&path, &Cancel::default(), &mut no_progress()).expect("test fixture");
        let mb_per_s = 512.0 / start.elapsed().as_secs_f64();
        assert!(mb_per_s >= 300.0, "{mb_per_s:.0} MB/s");
    }
}
