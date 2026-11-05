import { useEffect, useSyncExternalStore } from "react";
import { getApi, type PluginPlatformSettings } from "../../api";

/**
 * The platform settings the engine keeps (FR-9.68, FR-9.81): "Don't allow high-risk plugins",
 * plugin sounds and the relay switch. Shared so a card in Browse reacts as soon as the switch in
 * Installed changes. `null` until the first load has finished.
 */
let state: PluginPlatformSettings | null = null;
let loading = false;
const listeners = new Set<() => void>();

function publish(next: PluginPlatformSettings): void {
  state = next;
  for (const listener of listeners) listener();
}

async function load(): Promise<void> {
  if (loading) return;
  loading = true;
  try {
    publish(await getApi().pluginPlatformSettings());
  } catch (error: unknown) {
    console.error("Loading the plugin platform settings failed", error);
  } finally {
    loading = false;
  }
}

/** Saves one change; the rest of the settings (e.g. the relay switch) stay as they are. */
export async function updatePlatformSettings(
  change: Partial<PluginPlatformSettings>,
): Promise<void> {
  const previous = state;
  if (previous === null) return;
  const next = { ...previous, ...change };
  publish(next);
  try {
    await getApi().pluginSetPlatformSettings(next);
  } catch (error: unknown) {
    console.error("Saving the plugin platform settings failed", error);
    publish(previous);
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function usePlatformSettings(): PluginPlatformSettings | null {
  const current = useSyncExternalStore(subscribe, () => state);
  useEffect(() => {
    if (state === null) void load();
  }, []);
  return current;
}

/** Test hook: forgets the cached settings. */
export function resetPlatformSettings(): void {
  state = null;
  loading = false;
}
