/**
 * Figures behind the audio rating (issue #152). Pure functions plus the small storage wrapper for
 * the open state of the details; the components own the state.
 */
import type { AudioHealth } from "../../bindings/commands";

/** Capture processing must finish within one 10 ms frame to stay in time. */
export const FRAME_BUDGET_MS = 10;

/** The recent window in whole minutes, at least one, for "the last 2 minutes". */
export function windowMinutes(windowSecs: number): number {
  return Math.max(1, Math.round(windowSecs / 60));
}

/** A count over the recent window as a rate per minute. */
export function perMinute(count: number, windowSecs: number): number {
  return windowSecs > 0 ? (count * 60) / windowSecs : 0;
}

/** Share of the voice frames from others that had to be concealed, in percent. */
export function concealedShare(audio: AudioHealth): number {
  const total = audio.recentReceivedFrames + audio.recentConcealedFrames;
  return total > 0 ? (audio.recentConcealedFrames * 100) / total : 0;
}

/*
 * A pure view preference of this window, so it lives in the webview's storage rather than the
 * engine's settings record. Storage can be unavailable (private mode, quota); the details then
 * simply start collapsed.
 */
const STORAGE_KEY = "mumcord.audioDetailsOpen";

export function loadDetailsOpen(storage?: Storage): boolean {
  try {
    return (storage ?? localStorage).getItem(STORAGE_KEY) === "true";
  } catch (error) {
    console.warn("Reading the audio details state failed; starting collapsed", error);
    return false;
  }
}

export function saveDetailsOpen(open: boolean, storage?: Storage): void {
  try {
    (storage ?? localStorage).setItem(STORAGE_KEY, String(open));
  } catch (error) {
    console.warn("Saving the audio details state failed", error);
  }
}
