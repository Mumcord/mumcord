/**
 * The small hint with the keys of the media views (issue #163): shown once per run when a view first
 * opens, and never when the keyboard hints are turned off in Settings.
 */
import { useEffect, useState, type ReactNode } from "react";
import { useHintPreferences } from "../../a11y";

const seen = new Set<string>();

/** Whether `key` has not been shown yet in this run. */
export function useFirstUse(key: string): boolean {
  const [first] = useState(!seen.has(key));
  useEffect(() => {
    seen.add(key);
  }, [key]);
  return first;
}

/** Test helper: the hints may appear again. */
export function resetSeenHints(): void {
  seen.clear();
}

export function ShortcutHint({ keys }: { keys: string }): ReactNode {
  const hints = useHintPreferences();
  if (!hints.keyboardHints) return null;
  return <span className="media-frame__keys">{keys}</span>;
}
