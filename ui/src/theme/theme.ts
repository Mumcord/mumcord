import type { Theme } from "../api";

export type ResolvedTheme = "dark" | "light";

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
