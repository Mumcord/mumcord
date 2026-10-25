//! Where Mumcord listens. Both ends compute the same per-user endpoint.

/// Linux: an abstract Unix socket (`@mumcord-overlay-<uid>`), not a file in `$XDG_RUNTIME_DIR`.
/// Games in the Steam Linux Runtime (pressure-vessel) or Flatpak get their own runtime directory
/// that only contains selected forwarded sockets, so a socket file there is invisible to them;
/// they do share the host's network namespace, which is where abstract sockets live. Abstract
/// sockets have no file permissions, so both ends check that the other side runs as the same
/// user ([`same_user`]). `MUMCORD_OVERLAY_SOCKET` replaces the name (isolated test runs).
#[cfg(target_os = "linux")]
pub fn name() -> String {
    std::env::var("MUMCORD_OVERLAY_SOCKET")
        .ok()
        .filter(|n| !n.is_empty())
        // Safety: getuid cannot fail and has no preconditions.
        .unwrap_or_else(|| format!("mumcord-overlay-{}", unsafe { libc::getuid() }))
}

#[cfg(target_os = "linux")]
fn address() -> std::io::Result<std::os::unix::net::SocketAddr> {
    use std::os::linux::net::SocketAddrExt;
    std::os::unix::net::SocketAddr::from_abstract_name(name())
}

/// Binds the endpoint. Fails with `AddrInUse` while another Mumcord (or anything else) holds it.
#[cfg(target_os = "linux")]
pub fn bind() -> std::io::Result<std::os::unix::net::UnixListener> {
    std::os::unix::net::UnixListener::bind_addr(&address()?)
}

/// Connects to Mumcord; refuses a listener that runs as another user (a squatter on the name).
#[cfg(target_os = "linux")]
pub fn connect() -> std::io::Result<std::os::unix::net::UnixStream> {
    let stream = std::os::unix::net::UnixStream::connect_addr(&address()?)?;
    if same_user(&stream) {
        Ok(stream)
    } else {
        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
    }
}

/// Whether the process on the other end runs as our user (`SO_PEERCRED`; the kernel maps the
/// uid across user namespaces, so a game in a container still reports the host uid).
#[cfg(target_os = "linux")]
pub fn same_user(stream: &std::os::unix::net::UnixStream) -> bool {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // Safety: `cred` and `len` are valid for writes and `len` holds the buffer size.
    let ok = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut cred).cast(),
            &raw mut len,
        )
    } == 0;
    // Safety: getuid cannot fail and has no preconditions.
    ok && cred.uid == unsafe { libc::getuid() }
}

/// Named pipe per Windows user name (`\\.\pipe\mumcord-overlay-<user>`). Mumcord creates it
/// with the default security descriptor, which only grants the creating user (and SYSTEM /
/// administrators) access.
#[cfg(windows)]
pub fn path() -> Option<std::path::PathBuf> {
    let user = std::env::var("USERNAME").ok()?;
    let safe: String = user
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    Some(std::path::PathBuf::from(format!(
        r"\\.\pipe\mumcord-overlay-{safe}"
    )))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn connects_to_the_same_user_without_a_file() {
        let unique = format!("mumcord-overlay-test-{}", std::process::id());
        // Safety: the only test in this crate that reads or writes the environment.
        unsafe { std::env::set_var("MUMCORD_OVERLAY_SOCKET", &unique) };
        let listener = bind().expect("bind");
        assert_eq!(
            bind().expect_err("second bind").kind(),
            std::io::ErrorKind::AddrInUse
        );
        let client = connect().expect("connect");
        let (server_side, _) = listener.accept().expect("accept");
        assert!(same_user(&client));
        assert!(same_user(&server_side));
        drop(listener);
        assert!(connect().is_err(), "the name is gone with the listener");
    }
}
