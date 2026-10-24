import { createContext, useContext } from "react";
import type { Language, Settings, Theme } from "../api";
import type { ResolvedTheme } from "../theme/theme";

export interface Preferences {
  /** Theme setting as stored (`system` follows the OS). */
  theme: Theme;
  /** Theme actually shown. */
  resolvedTheme: ResolvedTheme;
  /** UI language actually shown. */
  language: Language;
  /**
   * Applies theme and language from a settings record immediately. The engine sends no event
   * when settings change, so whoever calls `settingsUpdate` passes the result here.
   */
  applySettings: (settings: Pick<Settings, "theme" | "language">) => void;
}

export const PreferencesContext = createContext<Preferences | null>(null);

export function usePreferences(): Preferences {
  const preferences = useContext(PreferencesContext);
  if (!preferences) throw new Error("usePreferences must be used inside <PreferencesProvider>");
  return preferences;
}
