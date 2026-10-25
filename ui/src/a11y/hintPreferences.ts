import { useSyncExternalStore } from "react";

/**
 * Accessibility hint switches of the settings screen: visible keyboard hints, screen-reader
 * announcements and skip links. All are on by default and live in the settings record (FR-1.69).
 */
export interface HintPreferences {
  keyboardHints: boolean;
  announcements: boolean;
  skipLinks: boolean;
}

export type HintPreference = keyof HintPreferences;

export const HINT_KEYS: readonly HintPreference[] = ["keyboardHints", "announcements", "skipLinks"];

export const DEFAULT_HINT_PREFERENCES: HintPreferences = {
  keyboardHints: true,
  announcements: true,
  skipLinks: true,
};

let current: HintPreferences = DEFAULT_HINT_PREFERENCES;
const listeners = new Set<() => void>();

/** Current values; also readable outside React (the announcer checks it per message). */
export function hintPreferences(): HintPreferences {
  return current;
}

/** Switches take effect at once; saving them is up to the caller (settings record). */
export function setHintPreference(key: HintPreference, value: boolean): void {
  if (current[key] === value) return;
  current = { ...current, [key]: value };
  for (const listener of listeners) listener();
}

/** Takes the switches over from a settings record (on start, after a save or a rollback). */
export function applyHintPreferences(settings: HintPreferences): void {
  for (const key of HINT_KEYS) setHintPreference(key, settings[key]);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useHintPreferences(): HintPreferences {
  return useSyncExternalStore(subscribe, hintPreferences);
}
