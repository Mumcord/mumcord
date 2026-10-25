import { useSyncExternalStore } from "react";

/**
 * Accessibility hint switches of the settings screen: visible keyboard hints, screen-reader
 * announcements and skip links. All are on by default.
 */
export interface HintPreferences {
  keyboardHints: boolean;
  announcements: boolean;
  skipLinks: boolean;
}

export type HintPreference = keyof HintPreferences;

export const DEFAULT_HINT_PREFERENCES: HintPreferences = {
  keyboardHints: true,
  announcements: true,
  skipLinks: true,
};

/*
 * Pure presentation preferences of this window, so they live in the webview's storage like the
 * column widths rather than in the engine's settings record. Storage can be unavailable (private
 * mode, quota); the app then simply uses the defaults.
 */
const STORAGE_KEY = "mumcord.accessibilityHints";

export function loadHintPreferences(storage?: Storage): HintPreferences {
  try {
    const raw = (storage ?? localStorage).getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_HINT_PREFERENCES;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return DEFAULT_HINT_PREFERENCES;
    const record = parsed as Partial<Record<HintPreference, unknown>>;
    const read = (key: HintPreference) => {
      const value = record[key];
      return typeof value === "boolean" ? value : DEFAULT_HINT_PREFERENCES[key];
    };
    return {
      keyboardHints: read("keyboardHints"),
      announcements: read("announcements"),
      skipLinks: read("skipLinks"),
    };
  } catch (error) {
    console.warn("Reading the accessibility hint settings failed; using the defaults", error);
    return DEFAULT_HINT_PREFERENCES;
  }
}

function saveHintPreferences(preferences: HintPreferences): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(preferences));
  } catch (error) {
    console.warn("Saving the accessibility hint settings failed", error);
  }
}

let current = loadHintPreferences();
const listeners = new Set<() => void>();

/** Current values; also readable outside React (the announcer checks it per message). */
export function hintPreferences(): HintPreferences {
  return current;
}

export function setHintPreference(key: HintPreference, value: boolean): void {
  if (current[key] === value) return;
  current = { ...current, [key]: value };
  saveHintPreferences(current);
  for (const listener of listeners) listener();
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
