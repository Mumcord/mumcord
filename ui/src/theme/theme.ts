import type { Theme } from "../api";
import { deriveTokens, THEME_TOKENS, themeBase } from "./derive";
import { MUMCORD_PRESET_ID, type ThemeRecord, type ThemeVariant } from "./themeModel";

export type ResolvedTheme = ThemeVariant;

const DARK_QUERY = "(prefers-color-scheme: dark)";

/** `system` follows the OS; without OS information the designed default (dark) applies. */
export function resolveTheme(setting: Theme, osPrefersDark: boolean | null): ResolvedTheme {
  if (setting !== "system") return setting;
  return osPrefersDark === false ? "light" : "dark";
}

/** OS colour-scheme preference, or `null` where `matchMedia` is unavailable (e.g. jsdom). */
export function osPrefersDark(): boolean | null {
  if (typeof window.matchMedia !== "function") return null;
  return window.matchMedia(DARK_QUERY).matches;
}

/** Calls `onChange` whenever the OS colour scheme changes; returns the unsubscribe function. */
export function watchOsTheme(onChange: (prefersDark: boolean) => void): () => void {
  if (typeof window.matchMedia !== "function") return () => undefined;
  const query = window.matchMedia(DARK_QUERY);
  const listener = (event: MediaQueryListEvent) => {
    onChange(event.matches);
  };
  query.addEventListener("change", listener);
  return () => {
    query.removeEventListener("change", listener);
  };
}

/** The token sets in tokens.css are selected by `data-theme` on <html>. */
export function applyThemeToDocument(theme: ResolvedTheme): void {
  document.documentElement.dataset.theme = theme;
}

/**
 * Shows `variant` of `theme` (FR-2.14). Mumcord uses the hand-tuned token sets; every other theme
 * sets its derived colours as inline variables over the base set matching those colours.
 */
export function applyThemeRecord(theme: ThemeRecord, variant: ThemeVariant): void {
  clearThemeColors();
  if (theme.id === MUMCORD_PRESET_ID) {
    applyThemeToDocument(variant);
    return;
  }
  const colors = theme[variant];
  applyThemeToDocument(themeBase(colors));
  const tokens = deriveTokens(colors);
  const { style } = document.documentElement;
  for (const token of THEME_TOKENS) style.setProperty(`--color-${token}`, tokens[token]);
}

/** Removes a custom theme's inline colours so the base token set applies again. */
export function clearThemeColors(): void {
  const { style } = document.documentElement;
  for (const token of THEME_TOKENS) style.removeProperty(`--color-${token}`);
}
