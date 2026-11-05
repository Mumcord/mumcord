import { useEffect, useState } from "react";

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
