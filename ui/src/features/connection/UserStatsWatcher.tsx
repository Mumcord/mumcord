import { useEffect } from "react";
import { getApi } from "../../api";
import { useServerSelector, useShownServerId } from "../channels/serverStore";
import type { ServerState } from "../channels/serverStore";

/** Wait for joins to settle before the visible list is sent again. */
export const WATCH_DEBOUNCE_MS = 500;

const sessionsOf = (state: ServerState): string =>
  [...state.users.keys()].sort((a, b) => a - b).join(",");

/**
 * Tells the engine which people of the shown server are visible (FR-2.27). The engine requests
 * their UserStats at a paced rate; this component only reports the list and stops it when
 * another server is shown or the app closes the view.
 */
export function UserStatsWatcher() {
  const serverId = useShownServerId();
  const sessions = useServerSelector(sessionsOf);

  useEffect(() => {
    if (serverId === null || sessions === "") return;
    const list = sessions.split(",").map(Number);
    const timer = setTimeout(() => {
      getApi()
        .userStatsWatch(serverId, list)
        .catch((error: unknown) => {
          console.error("Watching connection quality failed", error);
        });
    }, WATCH_DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [serverId, sessions]);

  useEffect(() => {
    if (serverId === null) return;
    return () => {
      getApi()
        .userStatsWatch(serverId, [])
        .catch(() => {
          // The server is already gone; nothing is left to stop.
        });
    };
  }, [serverId]);

  return null;
}
