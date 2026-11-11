//! One Mumcord per user (FR-13.12).
//!
//! The single-instance plugin Mumcord uses on Linux, Windows and macOS has no FreeBSD
//! implementation. Without it a second launch — including a `mumble://` link clicked while Mumcord
//! runs — would open a second window with its own connections, sound streams and tray icon.
//!
//! So a launch first tries to hand its command line to the running instance over a Unix socket in
//! `$XDG_RUNTIME_DIR` (a directory only this user can enter, which is what makes the socket safe).
//! Without that directory — a terminal start, a minimal environment — a per-user 0700 folder in
//! the temporary directory is used, but only if it really belongs to this user; another user could
//! have created it first to read or inject the handed-over links. If it does not, the socket goes
//! into a 0700 folder in the app's own data directory. When nobody is listening this process
//! becomes the single instance and serves the socket.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::AppHandle;

/// The arguments of one later launch, in order.
type Launch = Vec<String>;

/// Kept for the whole run so the socket keeps serving; `None` on platforms without it.
static OWNER: Mutex<Option<Owner>> = Mutex::new(None);

/// Socket path, for cleanup when the app exits.
static SOCKET: OnceLock<PathBuf> = OnceLock::new();

struct Owner {
    /// Later launches queued in the listener thread.
    launches: Receiver<Launch>,
}

/// Takes the single instance for this user, or hands this launch to the running one.
///
/// `true` means this process is the single instance; `false` means its arguments went to the
/// running one and the caller has to exit.
pub fn claim() -> bool {
    let Some(socket) = socket_path() else {
        // Logging is not initialised this early, so this one goes to stderr as well.
        tracing::warn!(
            "no private folder for the single-instance socket; a second launch may start twice"
        );
        eprintln!(
            "no private folder for the single-instance socket; a second launch may start twice"
        );
        return true;
    };
    let args: Launch = std::env::args().skip(1).collect();

    if hand_over(&socket, &args) {
        return false;
    }

    // A socket file nobody answers on is left over from a crash: replace it.
    let _ = std::fs::remove_file(&socket);
    let listener = match UnixListener::bind(&socket) {
        Ok(listener) => listener,
        Err(e) => {
            // Logging is not initialised this early, so this one goes to stderr as well.
            tracing::warn!(error = %e, "no single-instance socket; a second launch may start twice");
            eprintln!("single-instance socket {socket:?} not created: {e}");
            return true;
        }
    };
    let _ = SOCKET.set(socket);
    let (tx, launches) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("single-instance".into())
        .spawn(move || serve(listener, tx))
        .map(|_| ())
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "single-instance listener not started");
            eprintln!("single-instance listener not started: {e}");
        });
    *OWNER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Owner { launches });
    true
}

/// Hands this launch's arguments to a running instance; `true` when one took them.
fn hand_over(socket: &std::path::Path, args: &[String]) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return false;
    };
    let line = serde_json::to_string(args).unwrap_or_else(|_| "[]".to_owned());
    if let Err(e) = stream.write_all(format!("{line}\n").as_bytes()) {
        tracing::warn!(error = %e, "handing this launch to the running Mumcord failed");
        return false;
    }
    let _ = stream.flush();
    true
}

/// How long a connected launch may take to send its line; a stalled client must not block the
/// launches behind it for longer.
const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Longest accepted message. Real launches are a few links and flags, far below this.
const MAX_LINE: u64 = 64 * 1024;

/// Accepts launches and queues them, one line of JSON arguments each.
fn serve(listener: UnixListener, launches: Sender<Launch>) {
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(e) => {
                tracing::debug!(error = %e, "a single-instance connection failed");
                continue;
            }
        };
        if let Err(e) = stream.set_read_timeout(Some(READ_TIMEOUT)) {
            tracing::warn!(error = %e, "ignoring a single-instance connection without a read timeout");
            continue;
        }
        let Some(args) = read_launch(stream) else {
            continue;
        };
        if launches.send(args).is_err() {
            return;
        }
    }
}

/// Reads one launch: a single newline-terminated line of at most [`MAX_LINE`] bytes.
fn read_launch(stream: impl Read) -> Option<Launch> {
    let mut line = String::new();
    if let Err(e) = BufReader::new(stream.take(MAX_LINE)).read_line(&mut line) {
        tracing::debug!(error = %e, "ignoring an unreadable single-instance message");
        return None;
    }
    if !line.ends_with('\n') {
        tracing::debug!("ignoring an unterminated or oversized single-instance message");
        return None;
    }
    match serde_json::from_str::<Launch>(&line) {
        Ok(args) => Some(args),
        Err(e) => {
            tracing::debug!(error = %e, "ignoring a malformed single-instance message");
            None
        }
    }
}

/// Forwards the arguments of later launches to `handle`, in arrival order.
pub fn forward_launches(handle: AppHandle) {
    let Ok(mut owner) = OWNER.lock() else {
        return;
    };
    let Some(owner) = owner.take() else { return };
    let launches = owner.launches;
    std::thread::Builder::new()
        .name("single-instance-forward".into())
        .spawn(move || {
            while let Ok(args) = launches.recv() {
                crate::deeplink::open_arguments(&handle, &args);
            }
        })
        .map(|_| ())
        .unwrap_or_else(|e| tracing::warn!(error = %e, "forwarding a launch failed"));
}

/// Removes the socket file at exit; a leftover would be replaced by the next start anyway.
pub fn remove_socket() {
    if let Some(socket) = SOCKET.get() {
        let _ = std::fs::remove_file(socket);
    }
}

/// The app identifier, which also names the socket and the app's data directory.
const IDENTIFIER: &str = "app.mumcord.client";

/// Longest socket path that fits into `sun_path` (104 bytes on FreeBSD, NUL included).
const MAX_SOCKET_PATH: usize = 103;

/// `$XDG_RUNTIME_DIR/app.mumcord.client.sock`, else the socket in a folder only this user owns.
/// `None` when no such folder can be had.
fn socket_path() -> Option<PathBuf> {
    let name = format!("{IDENTIFIER}.sock");
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
        && runtime.is_dir()
    {
        return Some(runtime.join(name));
    }
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let shared = std::env::temp_dir().join(format!("mumcord-{uid}"));
    let own = crate::cache_dirs::before_start(IDENTIFIER).map(|dirs| dirs.local_data.join("ipc"));
    for dir in std::iter::once(shared).chain(own) {
        let socket = dir.join(&name);
        if socket.as_os_str().len() > MAX_SOCKET_PATH {
            eprintln!("single-instance socket {socket:?} is too long for a Unix socket");
            continue;
        }
        match private_dir(&dir, uid) {
            Ok(()) => return Some(socket),
            Err(e) => eprintln!("single-instance folder {dir:?} not used: {e}"),
        }
    }
    None
}

/// Creates `dir` with mode 0700, or accepts an existing one only if it is safe for the socket.
fn private_dir(dir: &Path, uid: u32) -> std::io::Result<()> {
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    // Not `metadata`: a symlink planted by another user must not be followed.
    let meta = std::fs::symlink_metadata(dir)?;
    check_private_dir(meta.file_type().is_dir(), meta.uid(), meta.mode(), uid)
        .map_err(|reason| std::io::Error::new(std::io::ErrorKind::PermissionDenied, reason))
}

/// A socket folder is safe when it is a real directory (not a symlink), owned by this user and
/// closed to group and others. Anything else may be in another user's hands.
fn check_private_dir(is_dir: bool, owner: u32, mode: u32, uid: u32) -> Result<(), String> {
    if !is_dir {
        return Err("not a directory (or a symlink)".to_owned());
    }
    if owner != uid {
        return Err(format!("owned by uid {owner}, not by this user"));
    }
    if mode & 0o077 != 0 {
        return Err(format!("mode {:o} lets other users in", mode & 0o777));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_round_trips_through_the_socket() {
        let dir = tempfile::tempdir().expect("temp dir");
        let socket = dir.path().join("single.sock");
        let listener = UnixListener::bind(&socket).expect("bind");
        let (tx, launches) = std::sync::mpsc::channel();
        std::thread::spawn(move || serve(listener, tx));

        let args = vec![
            "mumble://example.org/Lobby".to_owned(),
            "--autostart".to_owned(),
        ];
        assert!(
            hand_over(&socket, &args),
            "the running instance took the launch"
        );
        assert_eq!(launches.recv().expect("queued"), args);
    }

    /// A socket file without a listener is what a crashed run leaves behind: the launch stays
    /// local instead of failing.
    #[test]
    fn a_leftover_socket_file_keeps_the_launch_local() {
        let dir = tempfile::tempdir().expect("temp dir");
        let socket = dir.path().join("stale.sock");
        drop(
            std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&socket)
                .expect("create the file"),
        );
        assert!(!hand_over(&socket, &["--autostart".to_owned()]));
    }

    /// The socket has to fit into `sun_path` and live where only this user can reach it.
    #[test]
    fn the_socket_path_is_short_and_user_private() {
        let path = socket_path().expect("a private socket folder");
        assert!(
            path.as_os_str().len() <= MAX_SOCKET_PATH,
            "{path:?} is too long"
        );
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("app.mumcord.client.sock")
        );
    }

    fn uid() -> u32 {
        // SAFETY: getuid has no preconditions and cannot fail.
        unsafe { libc::getuid() }
    }

    #[test]
    fn a_new_folder_is_created_private() {
        let base = tempfile::tempdir().expect("temp dir");
        let dir = base.path().join("mumcord");
        private_dir(&dir, uid()).expect("created");
        let mode = std::fs::metadata(&dir).expect("metadata").mode();
        assert_eq!(mode & 0o077, 0, "mode {mode:o}");
    }

    #[test]
    fn an_own_0700_folder_is_accepted() {
        let base = tempfile::tempdir().expect("temp dir");
        let dir = base.path().join("mumcord");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .expect("create");
        private_dir(&dir, uid()).expect("accepted");
    }

    #[test]
    fn a_symlinked_folder_is_rejected() {
        let base = tempfile::tempdir().expect("temp dir");
        let target = base.path().join("target");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&target)
            .expect("create");
        let link = base.path().join("mumcord");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        assert!(private_dir(&link, uid()).is_err());
    }

    #[test]
    fn an_open_folder_is_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().expect("temp dir");
        let dir = base.path().join("mumcord");
        std::fs::create_dir(&dir).expect("create");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).expect("chmod");
        assert!(private_dir(&dir, uid()).is_err());
    }

    #[test]
    fn only_a_private_directory_of_this_user_passes() {
        assert!(check_private_dir(true, 1001, 0o40700, 1001).is_ok());
        assert!(
            check_private_dir(true, 1002, 0o40700, 1001).is_err(),
            "other owner"
        );
        assert!(
            check_private_dir(true, 1001, 0o40750, 1001).is_err(),
            "group bits"
        );
        assert!(
            check_private_dir(true, 1001, 0o40701, 1001).is_err(),
            "other bits"
        );
        assert!(
            check_private_dir(false, 1001, 0o120700, 1001).is_err(),
            "symlink"
        );
    }

    #[test]
    fn an_unterminated_or_oversized_message_is_dropped() {
        assert_eq!(read_launch(&b"[\"a\"]\n"[..]), Some(vec!["a".to_owned()]));
        assert_eq!(read_launch(&b"[\"a\"]"[..]), None, "no newline");
        let huge = format!("[\"{}\"]\n", "x".repeat(MAX_LINE as usize));
        assert_eq!(read_launch(huge.as_bytes()), None, "over the limit");
    }

    /// A client that connects and sends nothing must not hold the listener forever.
    #[test]
    fn a_stalled_client_does_not_block_later_launches() {
        let dir = tempfile::tempdir().expect("temp dir");
        let socket = dir.path().join("single.sock");
        let listener = UnixListener::bind(&socket).expect("bind");
        let (tx, launches) = std::sync::mpsc::channel();
        std::thread::spawn(move || serve(listener, tx));

        let _stalled = UnixStream::connect(&socket).expect("connect");
        let args = vec!["--autostart".to_owned()];
        assert!(hand_over(&socket, &args));
        assert_eq!(
            launches
                .recv_timeout(READ_TIMEOUT * 3)
                .expect("queued after the timeout"),
            args
        );
    }
}
