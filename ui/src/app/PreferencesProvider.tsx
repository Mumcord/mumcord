import { useCallback, useEffect, useLayoutEffect, useMemo, useState, type ReactNode } from "react";
import { applyHintPreferences, type HintPreference } from "../a11y";
import type { ChatDisplay, Language, Settings, Theme } from "../api";
import { applyLanguage, osLanguages, resolveLanguage } from "../i18n";
import { applyShownTheme, resolveShownTheme } from "../theme/appTheme";
import { osPrefersDark, resolveTheme, watchOsTheme } from "../theme/theme";
import { setAppThemes, useThemeChoice } from "../theme/themeStore";
import { ducker } from "../features/watchparty/ducking";
import { PreferencesContext, type Preferences } from "./preferences";

interface StoredPreferences {
  theme: Theme;
  language: Language | null;
  chatDisplay: ChatDisplay;
  linkPreviews: boolean;
  hideEmptyChannels: boolean;
  talkingFirst: boolean;
  showWeakConnectionIcon: boolean;
  visualEffects: boolean;
}

/** Keeps the document theme (app, shown server or editor preview), UI language, chat layout and link previews in sync with the settings record and the OS. */
export function PreferencesProvider({ children }: { children: ReactNode }) {
  const [stored, setStored] = useState<StoredPreferences>({
    theme: "system",
    language: null,
    chatDisplay: "cozy",
    // Nothing is fetched before the stored choice is known.
    linkPreviews: false,
    hideEmptyChannels: false,
    talkingFirst: false,
    showWeakConnectionIcon: true,
    visualEffects: true,
  });
  const [prefersDark, setPrefersDark] = useState(osPrefersDark);

  useEffect(() => watchOsTheme(setPrefersDark), []);

  const resolvedTheme = resolveTheme(stored.theme, prefersDark);
  const themeChoice = useThemeChoice();
  const language = resolveLanguage(stored.language, osLanguages());

  // Layout effect so the new colours are in place before the browser paints.
  useLayoutEffect(() => {
    applyShownTheme(resolveShownTheme(themeChoice, resolvedTheme));
  }, [themeChoice, resolvedTheme]);

  // The effects tokens in tokens.css switch off on data-effects="off".
  useLayoutEffect(() => {
    document.documentElement.dataset.effects = stored.visualEffects ? "on" : "off";
  }, [stored.visualEffects]);

  useEffect(() => {
    applyLanguage(language).catch((error: unknown) => {
      console.error("Switching the UI language failed", error);
    });
  }, [language]);

  const applySettings = useCallback(
    (
      settings: Pick<
        Settings,
        | "theme"
        | "language"
        | "chatDisplay"
        | "linkPreviews"
        | "hideEmptyChannels"
        | "talkingFirst"
        | "showWeakConnectionIcon"
        | "visualEffects"
        | "appThemeId"
        | "customThemes"
        | "videoDucking"
        | "videoDuckingPercent"
        | HintPreference
      >,
    ) => {
      applyHintPreferences(settings);
      setAppThemes(settings.appThemeId, settings.customThemes);
      ducker.setSettings(settings);
      setStored({
        theme: settings.theme,
        language: settings.language,
        chatDisplay: settings.chatDisplay,
        linkPreviews: settings.linkPreviews,
        hideEmptyChannels: settings.hideEmptyChannels,
        talkingFirst: settings.talkingFirst,
        showWeakConnectionIcon: settings.showWeakConnectionIcon,
        visualEffects: settings.visualEffects,
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
      linkPreviews: stored.linkPreviews,
      hideEmptyChannels: stored.hideEmptyChannels,
      talkingFirst: stored.talkingFirst,
      showWeakConnectionIcon: stored.showWeakConnectionIcon,
      applySettings,
    }),
    [
      stored.theme,
      resolvedTheme,
      language,
      stored.chatDisplay,
      stored.linkPreviews,
      stored.hideEmptyChannels,
      stored.talkingFirst,
      stored.showWeakConnectionIcon,
      applySettings,
    ],
  );

  return <PreferencesContext value={value}>{children}</PreferencesContext>;
}
