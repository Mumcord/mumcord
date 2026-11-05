//! `app-menu` (FR-9.53 … FR-9.55): the plugin's Apps-menu items. The items are declared in the
//! manifest; the plugin can only enable or disable them and give a reason for a disabled one.
//!
//! [`AppMenus`] keeps the item state per plugin, so the app can list the items in a stable order
//! and the service can tell whether a chosen item may be activated. Reasons are plugin text and
//! therefore untrusted; the UI renders them as plain text.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::Serialize;

use crate::manifest::AppMenuItem;
use crate::runtime::PluginState;
use crate::runtime::wit::types::Error;

/// The permission every `app-menu` call needs.
pub const PERMISSION: &str = "ui.app-menu";
/// Items one plugin may declare (FR-9.53). The manifest validation enforces it at load; the
/// state ignores any further items as a second line of defence.
pub const MAX_ITEMS_PER_PLUGIN: usize = 2;
/// The reason of a disabled item, in characters (WIT `app-menu.set-state`).
pub const MAX_REASON_CHARS: usize = 64;

/// One Apps-menu item as the app shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppMenuEntry {
    pub plugin: String,
    pub plugin_name: String,
    pub item: String,
    pub label: String,
    /// The plugin's icon name from the manifest.
    pub icon: Option<String>,
    pub enabled: bool,
    /// Why a disabled item is disabled; shown as its sub-line.
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
struct ItemState {
    id: String,
    label: String,
    icon: Option<String>,
    enabled: bool,
    reason: Option<String>,
}

/// The item state of all loaded plugins. Plugins sort by id, items keep manifest order, so the
/// order only changes when plugins come or go (the UI shows the first 8, then "More plugins…").
#[derive(Debug, Default)]
pub struct AppMenus {
    plugins: Mutex<BTreeMap<String, Vec<ItemState>>>,
}

impl AppMenus {
    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, Vec<ItemState>>> {
        self.plugins.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Makes the declared items of the switched-on plugins the current ones. An item that stays
    /// keeps its state (a plugin that already disabled it is not reset by a later sync); a new
    /// one starts enabled. Plugins and items that are gone are dropped.
    pub fn sync(&self, declared: Vec<(String, Vec<AppMenuItem>)>) {
        let mut plugins = self.lock();
        let mut next = BTreeMap::new();
        for (plugin, items) in declared {
            let old = plugins.get(&plugin);
            let items: Vec<ItemState> = items
                .into_iter()
                .take(MAX_ITEMS_PER_PLUGIN)
                .map(|item| {
                    old.and_then(|old| old.iter().find(|o| o.id == item.id))
                        .map(|kept| ItemState {
                            label: item.label.clone(),
                            icon: item.icon.clone(),
                            ..kept.clone()
                        })
                        .unwrap_or(ItemState {
                            id: item.id,
                            label: item.label,
                            icon: item.icon,
                            enabled: true,
                            reason: None,
                        })
                })
                .collect();
            if !items.is_empty() {
                next.insert(plugin, items);
            }
        }
        *plugins = next;
    }

    pub fn forget(&self, plugin: &str) {
        self.lock().remove(plugin);
    }

    /// `set-state`. Returns whether anything changed. `invalid`: an unknown item, a disabled
    /// item without a reason, or a reason longer than [`MAX_REASON_CHARS`].
    pub fn set_state(
        &self,
        plugin: &str,
        item: &str,
        enabled: bool,
        reason: Option<String>,
    ) -> Result<bool, Error> {
        let reason = reason
            .map(|r| r.trim().to_owned())
            .filter(|r| !r.is_empty());
        if reason
            .as_ref()
            .is_some_and(|r| r.chars().count() > MAX_REASON_CHARS)
            || (!enabled && reason.is_none())
        {
            return Err(Error::Invalid);
        }
        // An enabled item shows no sub-line, so a reason given with it is dropped.
        let reason = if enabled { None } else { reason };
        let mut plugins = self.lock();
        let state = plugins
            .get_mut(plugin)
            .and_then(|items| items.iter_mut().find(|i| i.id == item))
            .ok_or(Error::Invalid)?;
        let changed = state.enabled != enabled || state.reason != reason;
        state.enabled = enabled;
        state.reason = reason;
        Ok(changed)
    }

    /// Whether `item` of `plugin` exists and is enabled.
    pub fn is_enabled(&self, plugin: &str, item: &str) -> bool {
        self.lock()
            .get(plugin)
            .and_then(|items| items.iter().find(|i| i.id == item))
            .is_some_and(|i| i.enabled)
    }

    /// The entries of the plugins in `shown` (id and display name), in menu order.
    pub fn entries(&self, shown: &BTreeMap<String, String>) -> Vec<AppMenuEntry> {
        self.lock()
            .iter()
            .filter_map(|(plugin, items)| shown.get(plugin).map(|name| (plugin, name, items)))
            .flat_map(|(plugin, name, items)| {
                items.iter().map(|i| AppMenuEntry {
                    plugin: plugin.clone(),
                    plugin_name: name.clone(),
                    item: i.id.clone(),
                    label: i.label.clone(),
                    icon: i.icon.clone(),
                    enabled: i.enabled,
                    reason: i.reason.clone(),
                })
            })
            .collect()
    }
}

/// `app-menu.set-state` for the plugin behind `state`.
pub(crate) fn set_state(
    state: &PluginState,
    item: &str,
    enabled: bool,
    reason: Option<String>,
) -> Result<(), Error> {
    state.ctx.grants.check(&state.id, PERMISSION)?;
    state
        .io()?
        .app_menu_set_state(&state.id, item, enabled, reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> AppMenuItem {
        AppMenuItem {
            id: id.into(),
            label: format!("Label {id}"),
            icon: None,
        }
    }

    fn menus() -> AppMenus {
        let menus = AppMenus::default();
        menus.sync(vec![(
            "dice".into(),
            vec![item("roll"), item("history"), item("third")],
        )]);
        menus
    }

    fn shown(ids: &[&str]) -> BTreeMap<String, String> {
        ids.iter()
            .map(|i| ((*i).into(), i.to_uppercase()))
            .collect()
    }

    #[test]
    fn at_most_two_items_per_plugin() {
        let entries = menus().entries(&shown(&["dice"]));
        let ids: Vec<_> = entries.iter().map(|e| e.item.as_str()).collect();
        assert_eq!(ids, ["roll", "history"]);
    }

    #[test]
    fn set_state_disables_with_a_reason_and_enables_again() {
        let menus = menus();
        assert_eq!(
            menus.set_state("dice", "roll", false, Some("Busy".into())),
            Ok(true)
        );
        assert!(!menus.is_enabled("dice", "roll"));
        let entry = &menus.entries(&shown(&["dice"]))[0];
        assert_eq!(
            (entry.enabled, entry.reason.as_deref()),
            (false, Some("Busy"))
        );
        assert_eq!(
            menus.set_state("dice", "roll", false, Some("Busy".into())),
            Ok(false)
        );
        assert_eq!(
            menus.set_state("dice", "roll", true, Some("ignored".into())),
            Ok(true)
        );
        let entry = &menus.entries(&shown(&["dice"]))[0];
        assert_eq!((entry.enabled, entry.reason.clone()), (true, None));
    }

    #[test]
    fn set_state_rejects_bad_input() {
        let menus = menus();
        for (item, enabled, reason) in [
            ("nope", true, None),
            ("third", true, None),
            ("roll", false, None),
            ("roll", false, Some("  ".into())),
            ("roll", false, Some("x".repeat(MAX_REASON_CHARS + 1))),
        ] {
            assert_eq!(
                menus.set_state("dice", item, enabled, reason),
                Err(Error::Invalid)
            );
        }
        assert_eq!(
            menus.set_state("other", "roll", true, None),
            Err(Error::Invalid)
        );
        assert_eq!(
            menus.set_state("dice", "roll", false, Some("x".repeat(MAX_REASON_CHARS))),
            Ok(true)
        );
    }

    #[test]
    fn entries_are_ordered_by_plugin_and_only_for_shown_plugins() {
        let menus = menus();
        menus.sync(vec![
            ("dice".into(), vec![item("roll"), item("history")]),
            ("abc".into(), vec![item("one")]),
            ("zzz".into(), vec![item("two")]),
        ]);
        let entries = menus.entries(&shown(&["zzz", "dice", "abc"]));
        let order: Vec<_> = entries
            .iter()
            .map(|e| (e.plugin.as_str(), e.item.as_str()))
            .collect();
        assert_eq!(
            order,
            [
                ("abc", "one"),
                ("dice", "roll"),
                ("dice", "history"),
                ("zzz", "two")
            ]
        );
        assert_eq!(entries[0].plugin_name, "ABC");
        assert_eq!(menus.entries(&shown(&["abc"])).len(), 1);
        menus.forget("abc");
        assert!(menus.entries(&shown(&["abc"])).is_empty());
        menus.sync(vec![("dice".into(), vec![])]);
        assert!(!menus.is_enabled("dice", "roll"));
    }

    #[test]
    fn a_sync_keeps_the_state_of_items_that_stay() {
        let menus = menus();
        menus
            .set_state("dice", "roll", false, Some("Busy".into()))
            .expect("set");
        menus.sync(vec![("dice".into(), vec![item("roll"), item("new")])]);
        assert!(!menus.is_enabled("dice", "roll"));
        assert!(menus.is_enabled("dice", "new"));
        assert!(!menus.is_enabled("dice", "history"));
    }
}
