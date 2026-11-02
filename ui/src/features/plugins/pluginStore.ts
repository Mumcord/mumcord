import { useEffect, useSyncExternalStore } from "react";
import { getApi, type PluginsOverview } from "../../api";

export interface PluginState {
  /** `null` until the first load has finished. */
  overview: PluginsOverview | null;
  loadFailed: boolean;
}

/**
 * The installed plugins as the engine reports them, shared by every plugin screen. The engine
 * sends `plugins://changed` after any change, so the store simply reloads the list then.
 */
let state: PluginState = { overview: null, loadFailed: false };
const listeners = new Set<() => void>();
let unsubscribeEvent: (() => void) | null = null;
let loadId = 0;

function publish(next: PluginState): void {
  state = next;
  for (const listener of listeners) listener();
}

/** Reloads the list; a slower, older answer never replaces a newer one. */
export async function refreshPlugins(): Promise<void> {
  const id = ++loadId;
  try {
    const overview = await getApi().pluginsList();
    if (id === loadId) publish({ overview, loadFailed: false });
  } catch (error: unknown) {
    console.error("Loading the plugin list failed", error);
    if (id === loadId) publish({ overview: state.overview, loadFailed: true });
  }
}

/** "Turn off all plugins" (FR-6.6) and turning them on again; the list follows the change event. */
export async function setAllPluginsOff(allOff: boolean): Promise<void> {
  try {
    await getApi().pluginsSetAllOff(allOff);
  } catch (error: unknown) {
    console.error("Changing 'turn off all plugins' failed", error);
  }
  await refreshPlugins();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  if (listeners.size === 1) {
    unsubscribeEvent = getApi().on("pluginsChanged", () => {
      void refreshPlugins();
    });
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) {
      unsubscribeEvent?.();
      unsubscribeEvent = null;
    }
  };
}

/** The plugin list, loaded on first use and kept current while a component uses it. */
export function usePlugins(): PluginState {
  const current = useSyncExternalStore(subscribe, () => state);
  useEffect(() => {
    void refreshPlugins();
  }, []);
  return current;
}

/** Test helper: forgets everything the store holds. */
export function resetPluginStore(): void {
  loadId++;
  state = { overview: null, loadFailed: false };
}
