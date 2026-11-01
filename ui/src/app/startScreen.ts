import type { Theme } from "../api";

/**
 * The start screen lives in index.html (issue #323) so it shows before the app bundle loads.
 * This module hands over from it to the app and caches the theme it should use next time.
 */

/** Read by the inline script in index.html; keep both in sync. */
export const START_THEME_KEY = "mumcord.startTheme";

const START_SCREEN_ID = "start-screen";
/** Matches the CSS transition in index.html, plus slack in case `transitionend` never fires. */
const FADE_FALLBACK_MS = 300;

function prefersReducedMotion(): boolean {
  return (
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

/**
 * Fades the start screen out and removes it from the page; with reduced motion it disappears at
 * once. Safe to call more than once and in windows without a start screen.
 */
export function hideStartScreen(doc: Document = document): void {
  const screen = doc.getElementById(START_SCREEN_ID);
  if (!screen || screen.hasAttribute("data-leaving")) return;
  screen.setAttribute("aria-busy", "false");
  if (prefersReducedMotion()) {
    screen.remove();
    return;
  }
  screen.setAttribute("data-leaving", "");
  const remove = () => {
    screen.remove();
  };
  screen.addEventListener("transitionend", remove, { once: true });
  window.setTimeout(remove, FADE_FALLBACK_MS);
}

/**
 * Remembers the theme setting for the next start screen. A fixed choice is stored; "system" is
 * not, so the start screen keeps following the OS.
 */
export function rememberStartTheme(theme: Theme, storage?: Storage): void {
  try {
    const store = storage ?? localStorage;
    if (theme === "system") store.removeItem(START_THEME_KEY);
    else store.setItem(START_THEME_KEY, theme);
  } catch (error) {
    console.warn("Caching the start theme failed", error);
  }
}
