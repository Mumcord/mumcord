import { useEffect, useState } from "react";
import { getApi } from "../../api";
import { shownServerId } from "../channels/serverStore";
import type { ConnectionStats } from "../../bindings/commands";

/** How often the open panel refreshes; matches the engine's bitrate controller tick. */
export const POLL_INTERVAL_MS = 1000;

export type StatsState =
  | { status: "loading" }
  | { status: "ready"; stats: ConnectionStats }
  | { status: "unavailable" }
  | { status: "error" };

/**
 * Polls `connection_stats` while `active` is true. Polling (instead of an engine event) means
 * nothing is measured or sent while nobody looks at the panel.
 */
export function useConnectionStats(active: boolean): StatsState {
  const [state, setState] = useState<StatsState>({ status: "loading" });
  const [lastActive, setLastActive] = useState(active);
  if (active !== lastActive) {
    setLastActive(active);
    if (active) setState({ status: "loading" });
  }

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    const load = () => {
      getApi()
        .connectionStats(shownServerId())
        .then(
          (stats) => {
            if (!cancelled)
              setState(stats ? { status: "ready", stats } : { status: "unavailable" });
          },
          (error: unknown) => {
            console.error("loading connection stats failed", error);
            if (!cancelled) setState({ status: "error" });
          },
        );
    };
    load();
    const timer = setInterval(load, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [active]);

  return state;
}
