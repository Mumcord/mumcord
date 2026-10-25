import { createContext, useContext } from "react";
import type { HintPreference } from "../a11y";
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
  /** Show link previews and video player cards in chat (FR-1.49, QA.34). */
  linkPreviews: boolean;
  /**
   * Applies theme, language, chat layout and link previews and accessibility switches from a settings record immediately. The engine sends no event
   * when settings change, so whoever calls `settingsUpdate` passes the result here.
   */
  applySettings: (
    settings: Pick<
      Settings,
      "theme" | "language" | "chatDisplay" | "linkPreviews" | HintPreference
    >,
  ) => void;
}

export const PreferencesContext = createContext<Preferences | null>(null);

export function usePreferences(): Preferences {
  const preferences = useContext(PreferencesContext);
  if (!preferences) throw new Error("usePreferences must be used inside <PreferencesProvider>");
  return preferences;
}
