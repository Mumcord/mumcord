import { useEffect, useState } from "react";
import { getApi, type PluginScored } from "../../../api";
import type { PluginScoreSource } from "../../../bindings/commands";

/** Listing scores never change while the index stays the same; keep one answer per plugin. */
const cache = new Map<string, PluginScored | null>();

/** A change of the index (a refresh) can change scores: forget everything. */
export function forgetListingScores(): void {
  cache.clear();
}

/**
 * The listing score of an unofficial plugin, as the engine worked it out (the UI never computes
 * one); with `from = "developer"` the score of a loaded developer plugin's folder. `null` while
 * loading, and when the engine has none.
 */
export function useListingScore(
  id: string,
  version: string | null,
  enabled: boolean,
  from: Extract<PluginScoreSource, "listing" | "developer"> = "listing",
): PluginScored | null {
  const key = `${from}:${id}@${version ?? ""}`;
  // Re-renders when an answer arrives; the answer itself is read from the cache.
  const [, setArrived] = useState(0);
  useEffect(() => {
    if (!enabled || cache.has(key)) return;
    let cancelled = false;
    getApi()
      .pluginScore(id, version, from)
      .then(
        (result) => {
          cache.set(key, result);
          if (!cancelled) setArrived((count) => count + 1);
        },
        (error: unknown) => {
          // Not listed (e.g. the repository was removed): no chip rather than a wrong one.
          console.warn("No listing score for a plugin", error);
          cache.set(key, null);
          if (!cancelled) setArrived((count) => count + 1);
        },
      );
    return () => {
      cancelled = true;
    };
  }, [id, version, key, enabled, from]);
  return enabled ? (cache.get(key) ?? null) : null;
}
