//! The Linux tray as our own StatusNotifierItem over D-Bus (FR-2.52): Tauri's Linux tray
//! (libappindicator) ignores tooltips, so hovering it could not show the connection state.

use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{MenuItem, ToolTip, TrayMethods};
use tauri::AppHandle;
use tauri::image::Image;

use super::{
    Action, Entry, Labels, TrayState, base_icon, current_labels, current_state, menu_model,
    perform, tooltip, with_dot,
};
use crate::window;

/// What the StatusNotifierItem shows; refreshed from the app via [`KsniTray::sync`].
pub struct KsniTray {
    app: AppHandle,
    base: Option<Image<'static>>,
    state: TrayState,
    labels: Labels,
}

/// Registers the tray with the desktop's StatusNotifierWatcher. Fails when none is running
/// (e.g. GNOME without the AppIndicator extension).
pub async fn spawn(app: &AppHandle) -> Result<ksni::Handle<KsniTray>, ksni::Error> {
    KsniTray {
        app: app.clone(),
        base: base_icon(app),
        state: TrayState::default(),
        labels: current_labels(app),
    }
    .spawn()
    .await
}

impl KsniTray {
    /// Takes the current tray state and language from the app.
    pub fn sync(&mut self, app: &AppHandle) {
        if let Some(state) = current_state(app) {
            self.state = state;
        }
        self.labels = current_labels(app);
    }
}

impl ksni::Tray for KsniTray {
    fn id(&self) -> String {
        "mumcord".into()
    }

    fn title(&self) -> String {
        "Mumcord".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.base
            .as_ref()
            .map(|base| vec![argb(&with_dot(base, self.state.look()))])
            .unwrap_or_default()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: tooltip(self.labels.status(self.state.look())),
            ..ToolTip::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        window::show(&self.app);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        menu_model(self.state, &self.labels)
            .into_iter()
            .map(menu_item)
            .collect()
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        // Keep the service: the watcher comes back when the panel restarts.
        tracing::warn!(?reason, "the system tray went away");
        true
    }
}

fn menu_item(entry: Entry) -> MenuItem<KsniTray> {
    match entry {
        Entry::Status(label) => StandardItem {
            label,
            enabled: false,
            ..StandardItem::default()
        }
        .into(),
        Entry::Separator => MenuItem::Separator,
        Entry::Button(action, label) => StandardItem {
            label,
            activate: on_click(action),
            ..StandardItem::default()
        }
        .into(),
        Entry::Toggle {
            action,
            label,
            enabled,
            checked,
        } => CheckmarkItem {
            label,
            enabled,
            checked,
            activate: on_click(action),
            ..CheckmarkItem::default()
        }
        .into(),
    }
}

fn on_click(action: Action) -> Box<dyn Fn(&mut KsniTray) + Send> {
    Box::new(move |tray: &mut KsniTray| perform(&tray.app, action))
}

/// Converts RGBA pixels to the ARGB32 (network byte order) pixmap StatusNotifierItem expects.
fn argb(image: &Image<'_>) -> ksni::Icon {
    let data = image
        .rgba()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| [a, r, g, b])
        .collect();
    ksni::Icon {
        width: i32::try_from(image.width()).unwrap_or(i32::MAX),
        height: i32::try_from(image.height()).unwrap_or(i32::MAX),
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixmap_is_argb_in_network_order() {
        let image = Image::new_owned(vec![1, 2, 3, 4, 5, 6, 7, 8], 2, 1);
        let icon = argb(&image);
        assert_eq!((icon.width, icon.height), (2, 1));
        assert_eq!(icon.data, vec![4, 1, 2, 3, 8, 5, 6, 7]);
    }

    #[test]
    fn every_menu_entry_maps_to_a_ksni_item() {
        let labels = super::super::labels(Some("en"), None);
        let items: Vec<_> = menu_model(TrayState::default(), &labels)
            .into_iter()
            .map(menu_item)
            .collect();
        assert!(
            matches!(&items[0], MenuItem::Standard(s) if !s.enabled && s.label == labels.disconnected)
        );
        assert!(matches!(items[1], MenuItem::Separator));
        assert!(
            matches!(&items[3], MenuItem::Checkmark(c) if !c.enabled && c.label == labels.mute)
        );
        assert!(matches!(&items[5], MenuItem::Checkmark(c) if c.enabled && c.label == labels.dnd));
        assert!(matches!(&items[7], MenuItem::Standard(s) if s.enabled && s.label == labels.quit));
    }
}
