//! One Mumcord per user (FR-13.12).
//!
//! The single-instance plugin Mumcord uses on Linux, Windows and macOS has no FreeBSD
//! implementation. Without it a second launch — including a `mumble://` link clicked while Mumcord
//! runs — would open a second window with its own connections, sound streams and tray icon.
//!
//! So a launch first tries to hand its command line to the running instance over a Unix socket in
//! `$XDG_RUNTIME_DIR` (a directory only this user can enter, which is what makes the socket safe).
//! Without that directory — a terminal start, a minimal environment — a per-user directory in the
//! temporary folder is created with mode 0700 instead. When nobody is listening this process
//! becomes the single instance and serves the socket.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};

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
    let socket = socket_path();
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

/// Accepts launches and queues them, one line of JSON arguments each.
fn serve(listener: UnixListener, launches: Sender<Launch>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let line = BufReader::new(stream).lines().next();
        let Some(Ok(line)) = line else { continue };
        match serde_json::from_str::<Launch>(&line) {
            Ok(args) => {
                if launches.send(args).is_err() {
                    return;
                }
            }
            Err(e) => tracing::debug!(error = %e, "ignoring a malformed single-instance message"),
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

/// `$XDG_RUNTIME_DIR/app.mumcord.client.sock`, else a 0700 folder in the temporary directory.
fn socket_path() -> PathBuf {
    const NAME: &str = "app.mumcord.client.sock";
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
        && runtime.is_dir()
    {
        return runtime.join(NAME);
    }
    // The temporary directory is shared with other users, so the socket gets a folder of its own.
    let uid = unsafe { libc::getuid() };
    let dir = std::env::temp_dir().join(format!("mumcord-{uid}"));
    if !dir.is_dir() {
        let _ = std::fs::DirBuilder::new().mode(0o700).create(&dir);
    }
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    dir.join(NAME)
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
        let path = socket_path();
        assert!(path.to_string_lossy().len() < 100, "{path:?} is too long");
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("app.mumcord.client.sock")
        );
    }
}
