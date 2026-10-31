import { createContext, useContext } from "react";
import type { HintPreference } from "../a11y";
import type { ChatDisplay, Language, Settings, Theme } from "../api";
import type { ResolvedTheme } from "../theme/theme";

export interface Preferences {
  /** Theme setting as stored (`system` follows the OS). */
  theme: Theme;
  /** Dark or Light from the setting; shown unless a colour theme is chosen (FR-2.14). */
  resolvedTheme: ResolvedTheme;
  /** UI language actually shown. */
  language: Language;
  /** Chat message layout (FR-1.48a, QA.25). */
  chatDisplay: ChatDisplay;
  /** Show link previews and video player cards in chat (FR-1.49, QA.34). */
  linkPreviews: boolean;
  /** Channel tree hides channels without people (FR-2.23). */
  hideEmptyChannels: boolean;
  /** Talking people are listed first in the tree and the member list (FR-2.23). */
  talkingFirst: boolean;
  /** Signal icon next to people with a weak connection (FR-2.26). */
  showWeakConnectionIcon: boolean;
  /** Low-latency mode as set globally (#156); a channel audio profile can override it. */
  lowLatency: boolean;
  /**
   * Applies theme, language, chat layout and link previews and accessibility switches from a settings record immediately. The engine sends no event
   * when settings change, so whoever calls `settingsUpdate` passes the result here.
   */
  applySettings: (
    settings: Pick<
      Settings,
      | "theme"
      | "language"
      | "chatDisplay"
      | "linkPreviews"
      | "hideEmptyChannels"
      | "talkingFirst"
      | "showWeakConnectionIcon"
      | "lowLatency"
      | "visualEffects"
      | "appThemeId"
      | "customThemes"
      | "videoDucking"
      | "videoDuckingPercent"
      | "globalHotkeys"
      | "hotkeys"
      | HintPreference
    >,
  ) => void;
}

export const PreferencesContext = createContext<Preferences | null>(null);

export function usePreferences(): Preferences {
  const preferences = useContext(PreferencesContext);
  if (!preferences) throw new Error("usePreferences must be used inside <PreferencesProvider>");
  return preferences;
}

const LIST_DEFAULTS = { hideEmptyChannels: false, talkingFirst: false };

/**
 * The channel-list switches (FR-2.23). Unlike `usePreferences` it works without a provider and
 * then keeps both off, so the channel and member views also render on their own (tests).
 */
export function useListPreferences(): Pick<Preferences, "hideEmptyChannels" | "talkingFirst"> {
  return useContext(PreferencesContext) ?? LIST_DEFAULTS;
}

/**
 * "Show link previews and embeds" (Settings > Privacy): the one switch for everything that loads
 * third-party content, watch party included. Off without a provider, so nothing loads by accident.
 */
export function useEmbedsAllowed(): boolean {
  return (useContext(PreferencesContext) ?? { linkPreviews: false }).linkPreviews;
}

/** The weak connection switch (FR-2.26); on without a provider, like its default setting. */
export function useShowWeakConnectionIcon(): boolean {
  return (useContext(PreferencesContext) ?? { showWeakConnectionIcon: true })
    .showWeakConnectionIcon;
}

/** The global low-latency switch (#156); off without a provider, like its default setting. */
export function useLowLatencySetting(): boolean {
  return (useContext(PreferencesContext) ?? { lowLatency: false }).lowLatency;
}
