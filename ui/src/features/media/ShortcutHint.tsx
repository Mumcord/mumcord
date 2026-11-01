/**
 * The small hint with the keys of the media views (issue #163): a chip on the picture, shown once per
 * run when a view first opens and only for a few seconds, and never when the keyboard hints are
 * turned off in Settings.
 */
import { useEffect, useState, type ReactNode } from "react";
import { useHintPreferences } from "../../a11y";

const seen = new Set<string>();
/** How long the chip stays on the picture. */
const HINT_MS = 8000;

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
  const [shown, setShown] = useState(true);
  useEffect(() => {
    const timer = window.setTimeout(() => {
      setShown(false);
    }, HINT_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, []);
  if (!hints.keyboardHints || !shown) return null;
  return <span className="media-frame__keys">{keys}</span>;
}
