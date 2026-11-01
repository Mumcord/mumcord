import { useSyncExternalStore } from "react";
import type { Settings } from "../../api";

/** The hotkey part of the settings record, kept here so the handler needs no settings screen. */
export type HotkeySettings = Pick<Settings, "globalHotkeys" | "hotkeys">;

let current: HotkeySettings = { globalHotkeys: true, hotkeys: [] };
const listeners = new Set<() => void>();

/** Called with every loaded or saved settings record (see `PreferencesProvider`). */
export function setHotkeySettings(settings: HotkeySettings): void {
  if (settings.globalHotkeys === current.globalHotkeys && settings.hotkeys === current.hotkeys) {
    return;
  }
  current = { globalHotkeys: settings.globalHotkeys, hotkeys: settings.hotkeys };
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useHotkeySettings(): HotkeySettings {
  return useSyncExternalStore(subscribe, () => current);
}
