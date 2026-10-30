//! Reads the desktop's Do Not Disturb state over D-Bus and follows its changes.

use futures_util::StreamExt;
use tauri::AppHandle;
use zbus::OwnedVariant;

use super::set_system;
use crate::commands::dnd::SystemDnd;

const GNOME_NAMESPACE: &str = "org.gnome.desktop.notifications";
const GNOME_KEY: &str = "show-banners";

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    /// Set while the desktop holds notifications back (KDE Plasma's Do Not Disturb).
    #[zbus(property)]
    fn inhibited(&self) -> zbus::Result<bool>;
}

#[zbus::proxy(
    interface = "org.freedesktop.portal.Settings",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop"
)]
trait Settings {
    fn read_one(&self, namespace: &str, key: &str) -> zbus::Result<OwnedVariant>;

    #[zbus(signal)]
    fn setting_changed(
        &self,
        namespace: String,
        key: String,
        value: OwnedVariant,
    ) -> zbus::Result<()>;
}

/// The desktop's name for the settings ("KDE Plasma · Do Not Disturb is on"), from
/// `XDG_CURRENT_DESKTOP` (a colon-separated list such as "ubuntu:GNOME").
fn desktop_name(current_desktop: &str) -> String {
    let names: Vec<&str> = current_desktop
        .split(':')
        .filter(|n| !n.is_empty())
        .collect();
    if names.contains(&"KDE") {
        "KDE Plasma".into()
    } else if names.contains(&"GNOME") {
        "GNOME".into()
    } else {
        names
            .first()
            .map_or_else(|| "Linux".into(), |n| (*n).to_owned())
    }
}

fn is_gnome(current_desktop: &str) -> bool {
    current_desktop.split(':').any(|n| n == "GNOME")
}

/// Follows the system state for the app's lifetime. When it can't be read (or the service
/// goes away), the state becomes "unknown" and only the Mumcord switch counts.
pub async fn watch(app: AppHandle) {
    let current_desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if let Err(e) = follow(&app, &current_desktop).await {
        tracing::info!(error = %e, "the desktop's Do Not Disturb state can't be read");
    }
    set_system(&app, SystemDnd::default());
}

async fn follow(app: &AppHandle, current_desktop: &str) -> zbus::Result<()> {
    let connection = zbus::Connection::session().await?;
    let report = |on: bool| {
        set_system(
            app,
            SystemDnd {
                desktop: Some(desktop_name(current_desktop)),
                on: Some(on),
            },
        );
    };

    let notifications = NotificationsProxy::new(&connection).await?;
    if let Ok(on) = notifications.inhibited().await {
        report(on);
        let mut changes = notifications.receive_inhibited_changed().await;
        while let Some(change) = changes.next().await {
            match change.get().await {
                Ok(on) => report(on),
                Err(e) => tracing::debug!(error = %e, "reading the Inhibited change failed"),
            }
        }
        return Ok(());
    }

    // Other desktops' portals may expose GNOME's settings too, which they don't follow.
    if !is_gnome(current_desktop) {
        return Err(zbus::Error::Unsupported);
    }
    let portal = SettingsProxy::new(&connection).await?;
    let mut changes = portal.receive_setting_changed().await?;
    report(!bool::try_from(
        portal.read_one(GNOME_NAMESPACE, GNOME_KEY).await?,
    )?);
    while let Some(signal) = changes.next().await {
        let Ok(args) = signal.args() else { continue };
        if args.namespace == GNOME_NAMESPACE && args.key == GNOME_KEY {
            match bool::try_from(args.value) {
                Ok(show_banners) => report(!show_banners),
                Err(e) => tracing::debug!(error = %e, "unexpected show-banners value"),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_names_from_xdg_current_desktop() {
        assert_eq!(desktop_name("KDE"), "KDE Plasma");
        assert_eq!(desktop_name("ubuntu:GNOME"), "GNOME");
        assert_eq!(desktop_name("Hyprland"), "Hyprland");
        assert_eq!(desktop_name(""), "Linux");
        assert!(is_gnome("ubuntu:GNOME"));
        assert!(!is_gnome("KDE") && !is_gnome("GNOME-Flashback-ish"));
    }
}
