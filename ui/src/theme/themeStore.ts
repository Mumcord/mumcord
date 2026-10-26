import { useSyncExternalStore } from "react";
import type { ThemeRecord as StoredTheme } from "../api";
import { fromStoredTheme, type ThemeChoice, type ThemePreview } from "./appTheme";
import { MUMCORD_PRESET_ID } from "./themeModel";

/*
 * Which theme the window shows comes from three places: the settings record (app theme and
 * custom themes), the shown server (its own theme) and the theme editor (live preview). They meet
 * here so neither the servers feature nor the settings screen needs the other.
 */
let state: ThemeChoice = {
  appThemeId: MUMCORD_PRESET_ID,
  customThemes: [],
  serverThemeId: null,
  preview: null,
};
const listeners = new Set<() => void>();

function setState(patch: Partial<ThemeChoice>): void {
  state = { ...state, ...patch };
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): ThemeChoice {
  return state;
}

/** The current choice outside React. */
export function getThemeChoice(): ThemeChoice {
  return state;
}

export function useThemeChoice(): ThemeChoice {
  return useSyncExternalStore(subscribe, getSnapshot);
}

/** True while the theme editor previews its draft on the whole app (FR-2.63). */
export function useThemePreviewing(): boolean {
  return useThemeChoice().preview?.kind === "draft";
}

/** Called with every settings record the engine confirmed. */
export function setAppThemes(appThemeId: string, customThemes: readonly StoredTheme[]): void {
  setState({ appThemeId, customThemes: customThemes.map(fromStoredTheme) });
}

export function setServerThemeId(serverThemeId: string | null): void {
  if (serverThemeId !== state.serverThemeId) setState({ serverThemeId });
}

export function setThemePreview(preview: ThemePreview | null): void {
  setState({ preview });
}

/** Back to the start state; for tests. */
export function resetThemeChoice(): void {
  setState({ appThemeId: MUMCORD_PRESET_ID, customThemes: [], serverThemeId: null, preview: null });
}
