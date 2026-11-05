//! Screen-lock events on Linux from systemd-logind over the system D-Bus (FR-8.21): the
//! session's `Lock` signal (`loginctl lock-session`, idle locking) and its `LockedHint` property,
//! which GNOME and KDE set when their lock screen comes up.

use futures_util::StreamExt;
use tokio::runtime::Handle;
use zbus::OwnedObjectPath;

#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait Manager {
    fn get_session(&self, session_id: &str) -> zbus::Result<OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
trait Session {
    #[zbus(property)]
    fn id(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn locked_hint(&self) -> zbus::Result<bool>;

    #[zbus(signal)]
    fn lock(&self) -> zbus::Result<()>;
}

/// Follows the user's logind session on `rt` and calls `on_lock` for each lock. Without logind
/// (or outside a session) the camera simply isn't turned off on lock; that is logged once.
pub(super) fn watch(rt: &Handle, on_lock: fn()) {
    rt.spawn(async move {
        if let Err(e) = follow(on_lock).await {
            tracing::info!(error = %e, "screen lock can't be followed (logind)");
        }
    });
}

async fn follow(on_lock: fn()) -> zbus::Result<()> {
    let connection = zbus::Connection::system().await?;
    // `auto` is the caller's session, or the user's graphical one; signals come from the
    // session's real path, so it is looked up by id.
    let auto = SessionProxy::builder(&connection)
        .path("/org/freedesktop/login1/session/auto")
        .build()
        .await?;
    let id = auto.id().await?;
    let path = ManagerProxy::new(&connection)
        .await?
        .get_session(&id)
        .await?;
    let session = SessionProxy::builder(&connection)
        .path(path)
        .build()
        .await?;
    let mut locks = session.receive_lock().await?;
    let mut hints = session.receive_locked_hint_changed().await;
    loop {
        tokio::select! {
            lock = locks.next() => {
                if lock.is_none() {
                    return Ok(());
                }
                on_lock();
            }
            hint = hints.next() => {
                let Some(hint) = hint else {
                    return Ok(());
                };
                if hint.get().await? {
                    on_lock();
                }
            }
        }
    }
}
