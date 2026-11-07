import { useSyncExternalStore } from "react";
import type { Settings } from "../../api";

/** The hotkey part of the settings record, kept here so the handler needs no settings screen. */
export type HotkeySettings = Pick<
  Settings,
  "globalHotkeys" | "hotkeys" | "pttKey" | "cameraHotkey"
>;

let current: HotkeySettings = {
  globalHotkeys: true,
  hotkeys: [],
  pttKey: null,
  cameraHotkey: null,
};
const listeners = new Set<() => void>();

/** Called with every loaded or saved settings record (see `PreferencesProvider`). */
export function setHotkeySettings(settings: HotkeySettings): void {
  const same =
    settings.globalHotkeys === current.globalHotkeys &&
    settings.hotkeys === current.hotkeys &&
    settings.pttKey === current.pttKey &&
    settings.cameraHotkey === current.cameraHotkey;
  if (same) return;
  current = {
    globalHotkeys: settings.globalHotkeys,
    hotkeys: settings.hotkeys,
    pttKey: settings.pttKey,
    cameraHotkey: settings.cameraHotkey,
  };
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
