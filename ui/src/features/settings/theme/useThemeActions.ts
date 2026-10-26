import { useCallback } from "react";
import { getApi, type Settings } from "../../../api";
import { usePreferences } from "../../../app/preferences";
import {
  findImported,
  fromStoredTheme,
  newCustomThemeId,
  toStoredTheme,
} from "../../../theme/appTheme";
import type { SharedTheme } from "../../../theme/shareCodec";
import { useThemeChoice } from "../../../theme/themeStore";

export interface ThemeActions {
  /** Adds the theme to the list once (FR-2.16) and returns its id. */
  importTheme: (theme: SharedTheme) => Promise<string>;
  /** Imports the theme and makes it the app theme. */
  applyAppTheme: (theme: SharedTheme) => Promise<string>;
}

/**
 * Theme import outside the settings screen, e.g. from a shared theme in chat (FR-2.16). To set
 * it as a server's theme, import it and pass the id to the servers feature's `saveServerTheme`.
 */
export function useThemeActions(): ThemeActions {
  const { applySettings } = usePreferences();

  const save = useCallback(
    async (theme: SharedTheme, patch: (id: string) => Partial<Settings>): Promise<string> => {
      const api = getApi();
      const current = await api.settingsGet();
      const existing = findImported(theme, current.customThemes.map(fromStoredTheme));
      const id = existing?.id ?? newCustomThemeId();
      const customThemes = existing
        ? current.customThemes
        : [...current.customThemes, toStoredTheme({ id, ...theme })];
      applySettings(await api.settingsUpdate({ ...current, customThemes, ...patch(id) }));
      return id;
    },
    [applySettings],
  );

  const importTheme = useCallback((theme: SharedTheme) => save(theme, () => ({})), [save]);
  // The Dark / Light / System setting stays: it picks which version of the theme shows.
  const applyAppTheme = useCallback(
    (theme: SharedTheme) => save(theme, (id) => ({ appThemeId: id })),
    [save],
  );
  return { importTheme, applyAppTheme };
}

/** True when the same theme (name and colours) is already in the list ("Imported ✓"). */
export function useIsImported(theme: SharedTheme): boolean {
  return findImported(theme, useThemeChoice().customThemes) !== null;
}
