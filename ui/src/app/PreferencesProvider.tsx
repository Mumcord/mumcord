import { useCallback, useEffect, useLayoutEffect, useMemo, useState, type ReactNode } from "react";
import type { ChatDisplay, Language, Settings, Theme } from "../api";
import { applyLanguage, osLanguages, resolveLanguage } from "../i18n";
import { applyThemeToDocument, osPrefersDark, resolveTheme, watchOsTheme } from "../theme/theme";
import { PreferencesContext, type Preferences } from "./preferences";

interface StoredPreferences {
  theme: Theme;
  language: Language | null;
  chatDisplay: ChatDisplay;
}

/** Keeps the document theme, UI language and chat layout in sync with the settings record and the OS. */
export function PreferencesProvider({ children }: { children: ReactNode }) {
  const [stored, setStored] = useState<StoredPreferences>({
    theme: "system",
    language: null,
    chatDisplay: "cozy",
  });
  const [prefersDark, setPrefersDark] = useState(osPrefersDark);

  useEffect(() => watchOsTheme(setPrefersDark), []);

  const resolvedTheme = resolveTheme(stored.theme, prefersDark);
  const language = resolveLanguage(stored.language, osLanguages());

  // Layout effect so the new colours are in place before the browser paints.
  useLayoutEffect(() => {
    applyThemeToDocument(resolvedTheme);
  }, [resolvedTheme]);

  useEffect(() => {
    applyLanguage(language).catch((error: unknown) => {
      console.error("Switching the UI language failed", error);
    });
  }, [language]);

  const applySettings = useCallback(
    (settings: Pick<Settings, "theme" | "language" | "chatDisplay">) => {
      setStored({
        theme: settings.theme,
        language: settings.language,
        chatDisplay: settings.chatDisplay,
      });
    },
    [],
  );

  const value = useMemo<Preferences>(
    () => ({
      theme: stored.theme,
      resolvedTheme,
      language,
      chatDisplay: stored.chatDisplay,
      applySettings,
    }),
    [stored.theme, resolvedTheme, language, stored.chatDisplay, applySettings],
  );

  return <PreferencesContext value={value}>{children}</PreferencesContext>;
}
