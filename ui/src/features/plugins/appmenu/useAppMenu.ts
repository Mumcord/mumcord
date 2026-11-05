import { useEffect, useState } from "react";
import { getApi, type AppMenuItem } from "../../../api";
import { usePlugins } from "../pluginStore";

/** FR-9.53: at most two items per plugin, at most eight in the Apps menu itself. */
export const ITEMS_PER_PLUGIN = 2;
export const MENU_LIMIT = 8;

export interface AppMenuEntry extends AppMenuItem {
  /** Plugins from a repository are labelled "Unofficial" (mockup 06). */
  official: boolean;
}

/**
 * The plugins' Apps-menu items. The engine sends `plugins://app-menu-changed` when a plugin
 * enables or disables an item and `plugins://changed` when plugins start or stop, so both
 * trigger a fresh fetch; a slower, older answer never replaces a newer one.
 */
export function useAppMenu(): AppMenuEntry[] {
  const { overview } = usePlugins();
  const [items, setItems] = useState<AppMenuItem[]>([]);

  useEffect(() => {
    let latest = 0;
    let alive = true;
    const load = () => {
      const id = ++latest;
      getApi()
        .appMenu()
        .then((next) => {
          if (alive && id === latest) setItems(next);
        })
        .catch((error: unknown) => {
          console.error("Loading the Apps menu items failed", error);
        });
    };
    load();
    const offMenu = getApi().on("pluginsAppMenuChanged", load);
    const offPlugins = getApi().on("pluginsChanged", load);
    return () => {
      alive = false;
      offMenu();
      offPlugins();
    };
  }, []);

  const perPlugin = new Map<string, number>();
  return items
    .filter((item) => {
      const count = perPlugin.get(item.plugin) ?? 0;
      perPlugin.set(item.plugin, count + 1);
      return count < ITEMS_PER_PLUGIN;
    })
    .map((item) => ({
      ...item,
      official: overview?.plugins.find((p) => p.id === item.plugin)?.source.kind === "official",
    }));
}

/** Activating opens the gesture window on the host side (FR-9.55). */
export function activateAppMenuItem(entry: AppMenuItem): void {
  getApi()
    .appMenuActivate(entry.plugin, entry.item)
    .catch((error: unknown) => {
      console.error("Starting an Apps menu item failed", error);
    });
}
