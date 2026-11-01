//! The Linux tray as our own StatusNotifierItem over D-Bus (FR-2.52): Tauri's Linux tray
//! (libappindicator) ignores tooltips, so hovering it could not show the connection state.

use ksni::menu::{CheckmarkItem, StandardItem, SubMenu};
use ksni::{MenuItem, ToolTip, TrayMethods};
use tauri::AppHandle;
use tauri::image::Image;

use super::menu::{Action, Dot, Entry};
use super::{DOT_SIZE, View, icon, perform};
use crate::window;

/// What the StatusNotifierItem shows; replaced from the app via [`KsniTray::show`].
pub struct KsniTray {
    app: AppHandle,
    icon: Option<Image<'static>>,
    tooltip: String,
    menu: Vec<Entry>,
}

/// Registers the tray with the desktop's StatusNotifierWatcher. Fails when none is running
/// (e.g. GNOME without the AppIndicator extension).
pub async fn spawn(app: &AppHandle) -> Result<ksni::Handle<KsniTray>, ksni::Error> {
    KsniTray {
        app: app.clone(),
        icon: None,
        tooltip: String::new(),
        menu: Vec::new(),
    }
    .spawn()
    .await
}

impl KsniTray {
    /// Takes a new view; the menu stays when the view carries none.
    pub fn show(&mut self, view: View) {
        self.icon = Some(view.icon);
        self.tooltip = view.tooltip;
        if let Some(menu) = view.menu {
            self.menu = menu;
        }
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
        self.icon
            .as_ref()
            .map(|i| vec![argb(i)])
            .unwrap_or_default()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: self.tooltip.clone(),
            ..ToolTip::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        window::show(&self.app);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        self.menu.iter().map(menu_item).collect()
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        // Keep the service: the watcher comes back when the panel restarts.
        tracing::warn!(?reason, "the system tray went away");
        true
    }
}

/// The dot as PNG, the form StatusNotifierItem menus take icons in.
fn dot_png(dot: Option<Dot>) -> Vec<u8> {
    let Some(dot) = dot else {
        return Vec::new();
    };
    let image = icon::dot(dot.colour(), DOT_SIZE);
    let mut png = Vec::new();
    let encoded = image::RgbaImage::from_raw(image.width(), image.height(), image.rgba().to_vec())
        .map(|buffer| {
            buffer.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        });
    match encoded {
        Some(Ok(())) => png,
        Some(Err(e)) => {
            tracing::warn!(error = %e, "encoding a tray menu dot failed");
            Vec::new()
        }
        None => Vec::new(),
    }
}

fn menu_item(entry: &Entry) -> MenuItem<KsniTray> {
    match entry {
        Entry::Info { label, dot } => StandardItem {
            label: label.clone(),
            enabled: false,
            icon_data: dot_png(*dot),
            ..StandardItem::default()
        }
        .into(),
        Entry::Separator => MenuItem::Separator,
        Entry::Button { action, label, dot } => StandardItem {
            label: label.clone(),
            icon_data: dot_png(*dot),
            activate: on_click(action.clone()),
            ..StandardItem::default()
        }
        .into(),
        Entry::Toggle {
            action,
            label,
            enabled,
            checked,
        } => CheckmarkItem {
            label: label.clone(),
            enabled: *enabled,
            checked: *checked,
            activate: on_click(action.clone()),
            ..CheckmarkItem::default()
        }
        .into(),
        Entry::Submenu { label, entries } => SubMenu {
            label: label.clone(),
            enabled: !entries.is_empty(),
            submenu: entries.iter().map(menu_item).collect(),
            ..SubMenu::default()
        }
        .into(),
    }
}

fn on_click(action: Action) -> Box<dyn Fn(&mut KsniTray) + Send> {
    Box::new(move |tray: &mut KsniTray| perform(&tray.app, action.clone()))
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
        let entries = super::super::menu_model(&super::super::TrayState::default(), &labels);
        let items: Vec<_> = entries.iter().map(menu_item).collect();
        assert!(matches!(
            &items[0],
            MenuItem::Standard(s) if !s.enabled && s.label == labels.disconnected && !s.icon_data.is_empty()
        ));
        assert!(matches!(items[1], MenuItem::Separator));
        assert!(
            matches!(&items[2], MenuItem::Checkmark(c) if !c.enabled && c.label == labels.mute)
        );
        assert!(matches!(&items[6], MenuItem::SubMenu(m) if !m.enabled && m.submenu.is_empty()));
        assert!(
            matches!(items.last(), Some(MenuItem::Standard(s)) if s.enabled && s.label == labels.quit)
        );
    }

    #[test]
    fn menu_dots_are_png() {
        assert!(dot_png(Some(Dot::Connected)).starts_with(b"\x89PNG"));
        assert!(dot_png(None).is_empty());
    }
}
