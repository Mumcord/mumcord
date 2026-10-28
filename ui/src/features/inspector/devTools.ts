/**
 * The hidden developer tools (issue #60): unlocked by clicking the version in Settings → About
 * seven times, hidden again with a switch. The choice is kept on this device only; while the
 * tools are unlocked the engine captures packets for the packet inspector.
 */
import { useEffect, useRef, useSyncExternalStore } from "react";
import { getApi } from "../../api";

const STORAGE_KEY = "mumcord.devTools";
/** Clicks on the version that unlock the tools. */
export const UNLOCK_CLICKS = 7;
/** Clicks further apart than this start counting again. */
const CLICK_WINDOW_MS = 3000;

const listeners = new Set<() => void>();

function read(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "1";
  } catch {
    // Storage can be unavailable (private mode, tests); the tools are locked then.
    return false;
  }
}

let unlocked = read();

export function setDevToolsUnlocked(value: boolean): void {
  unlocked = value;
  try {
    if (value) localStorage.setItem(STORAGE_KEY, "1");
    else localStorage.removeItem(STORAGE_KEY);
  } catch (error: unknown) {
    console.error("Saving the developer tools switch failed", error);
  }
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useDevToolsUnlocked(): boolean {
  return useSyncExternalStore(subscribe, () => unlocked);
}

/** Re-reads the stored switch; for tests that change `localStorage` directly. */
export function reloadDevToolsUnlocked(): void {
  unlocked = read();
  for (const listener of listeners) listener();
}

/** Keeps the engine's packet capture on exactly while the developer tools are unlocked. */
export function useDevToolsCapture(): void {
  const on = useDevToolsUnlocked();
  useEffect(() => {
    getApi()
      .packetCaptureSet(on)
      .catch((error: unknown) => {
        console.error("Switching the packet capture failed", error);
      });
  }, [on]);
}

/**
 * Click handler for the version in About: the seventh click within a few seconds of each
 * other unlocks the developer tools and calls `onUnlock`.
 */
export function useUnlockClicks(onUnlock: () => void): () => void {
  const clicks = useRef<number[]>([]);
  return () => {
    if (unlocked) return;
    const now = Date.now();
    const recent = clicks.current.filter((at) => now - at < CLICK_WINDOW_MS);
    recent.push(now);
    clicks.current = recent;
    if (recent.length >= UNLOCK_CLICKS) {
      clicks.current = [];
      setDevToolsUnlocked(true);
      onUnlock();
    }
  };
}
