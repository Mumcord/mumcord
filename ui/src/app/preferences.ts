import { createContext, useContext } from "react";
import type { ChatDisplay, Language, Settings, Theme } from "../api";
import type { ResolvedTheme } from "../theme/theme";

export interface Preferences {
  /** Theme setting as stored (`system` follows the OS). */
  theme: Theme;
  /** Theme actually shown. */
  resolvedTheme: ResolvedTheme;
  /** UI language actually shown. */
  language: Language;
  /** Chat message layout (FR-1.48a, QA.25). */
  chatDisplay: ChatDisplay;
  /**
   * Applies theme, language and chat layout from a settings record immediately. The engine sends no event
   * when settings change, so whoever calls `settingsUpdate` passes the result here.
   */
  applySettings: (settings: Pick<Settings, "theme" | "language" | "chatDisplay">) => void;
}

export const PreferencesContext = createContext<Preferences | null>(null);

export function usePreferences(): Preferences {
  const preferences = useContext(PreferencesContext);
  if (!preferences) throw new Error("usePreferences must be used inside <PreferencesProvider>");
  return preferences;
}
