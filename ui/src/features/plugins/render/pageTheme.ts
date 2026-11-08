/**
 * Theme tokens for the web pages of API 1.2 plugins (FR-13.9 … FR-13.16). One table maps every
 * `--mc-` token to the app variable it carries; the same table gives the values Rust embeds into
 * a page when it is served (FR-13.13) and the values sent to open pages on every change
 * (FR-13.15). Pages of 1.0 and 1.1 plugins get none of this.
 */
import type { PluginPageTheme } from "../../../api";
import { TOKEN_VARS, type ThemeToken } from "./tokens";

/** The nine FR-6.23 colour names, unchanged after `--mc-color-`. */
const COLOR_TOKENS = Object.fromEntries(
  (Object.keys(TOKEN_VARS) as ThemeToken[]).map((token) => [
    `--mc-color-${token}`,
    TOKEN_VARS[token],
  ]),
);

/** `--mc-` token → Mumcord's CSS variable (FR-13.9). Removing one breaks plugin pages. */
export const PAGE_TOKEN_VARS: Readonly<Record<string, string>> = {
  ...COLOR_TOKENS,
  "--mc-color-text-secondary": "--color-text-secondary",
  "--mc-color-accent-text": "--color-accent-text",
  "--mc-color-on-accent": "--color-on-accent",
  "--mc-color-primary": "--color-primary-bg",
  "--mc-color-on-primary": "--color-primary-text",
  "--mc-color-hover": "--color-bg-hover",
  "--mc-color-selected": "--color-bg-selected",
  "--mc-color-border-strong": "--color-border-strong",
  "--mc-color-focus": "--color-focus",
  "--mc-color-backdrop": "--color-backdrop",
  "--mc-font-family": "--font-family",
  "--mc-font-family-display": "--font-family-display",
  "--mc-font-family-mono": "--font-family-mono",
  "--mc-font-size-xs": "--font-size-xs",
  "--mc-font-size-sm": "--font-size-sm",
  "--mc-font-size-md": "--font-size-md",
  "--mc-font-size-lg": "--font-size-lg",
  "--mc-font-size-xl": "--font-size-xl",
  "--mc-font-weight-normal": "--font-weight-normal",
  "--mc-font-weight-bold": "--font-weight-bold",
  "--mc-line-height": "--line-height",
  "--mc-space-1": "--space-1",
  "--mc-space-2": "--space-2",
  "--mc-space-3": "--space-3",
  "--mc-space-4": "--space-4",
  "--mc-space-5": "--space-5",
  "--mc-space-6": "--space-6",
  "--mc-radius-sm": "--radius-sm",
  "--mc-radius-md": "--radius-md",
  "--mc-radius-lg": "--radius-lg",
  "--mc-radius-full": "--radius-full",
  "--mc-focus-ring-width": "--focus-ring-width",
  "--mc-shadow-raised": "--shadow-raised",
  // Mixed from the accent, so it is read resolved per theme like the colours.
  "--mc-shadow-primary-glow": "--shadow-primary-glow",
  "--mc-duration-fast": "--duration-fast",
  "--mc-duration-normal": "--duration-normal",
  "--mc-ease-out": "--ease-out",
};

/** Durations that are `0ms` while reduced motion is on (FR-13.12). */
const DURATIONS = new Set(["--mc-duration-fast", "--mc-duration-normal"]);

const REDUCED_MOTION = "(prefers-reduced-motion: reduce)";
const FORCED_COLORS = "(forced-colors: active)";

function matches(query: string): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia(query).matches;
}

/**
 * Reduced motion as plugins see it (FR-6.23): the system asks for less motion or the user turned
 * Mumcord's visual effects off (FR-2.19).
 */
export function reducedMotion(): boolean {
  return matches(REDUCED_MOTION) || document.documentElement.dataset.effects === "off";
}

/** The page theme for the theme this window shows right now. */
export function readPageTheme(): PluginPageTheme {
  const root = document.documentElement;
  const style = getComputedStyle(root);
  const scheme = root.dataset.theme === "light" ? "light" : "dark";
  const reduced = reducedMotion();
  const tokens: Record<string, string> = {};
  for (const [token, appVar] of Object.entries(PAGE_TOKEN_VARS)) {
    // Values written over several lines in tokens.css can keep their line breaks, which the
    // Rust token check refuses; any run of whitespace becomes one space.
    const value =
      reduced && DURATIONS.has(token)
        ? "0ms"
        : style.getPropertyValue(appVar).replace(/\s+/g, " ").trim();
    if (value !== "") tokens[token] = value;
  }
  return {
    mode: matches(FORCED_COLORS) ? "high-contrast" : scheme,
    scheme,
    reducedMotion: reduced,
    tokens,
  };
}

type Listener = (theme: PluginPageTheme) => void;

const listeners = new Set<Listener>();
let last = "";
let stopWatching: (() => void) | null = null;

function check(): void {
  const theme = readPageTheme();
  const key = JSON.stringify(theme);
  if (key === last) return;
  last = key;
  for (const listener of listeners) listener(theme);
}

/**
 * The theme is applied to `<html>` (`data-theme`, `data-effects`, a custom theme's inline
 * colours); the system scheme reaches it through `data-theme`, high contrast and reduced motion
 * only through media queries.
 */
function watch(): () => void {
  const observer = new MutationObserver(check);
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["style", "data-theme", "data-effects"],
  });
  const queries =
    typeof window.matchMedia === "function"
      ? [REDUCED_MOTION, FORCED_COLORS].map((query) => window.matchMedia(query))
      : [];
  for (const query of queries) query.addEventListener("change", check);
  return () => {
    observer.disconnect();
    for (const query of queries) query.removeEventListener("change", check);
  };
}

/**
 * Calls `listener` with the new page theme whenever it changes in this window, never for the
 * theme at the time of subscribing (a page already got that one when it was served, FR-13.16).
 */
export function onPageThemeChange(listener: Listener): () => void {
  if (listeners.size === 0) {
    last = JSON.stringify(readPageTheme());
    stopWatching = watch();
  }
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) {
      stopWatching?.();
      stopWatching = null;
    }
  };
}
