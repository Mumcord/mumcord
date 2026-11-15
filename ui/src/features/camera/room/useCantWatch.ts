/**
 * The room notice "You can't see cameras on this server: {reason}" of FR-8.5: a viewer for whom
 * camera sharing is unavailable (FR-8.4) hears it once per channel visit, while somebody in the
 * channel has a camera on. Reopening the room in the same channel stays quiet; leaving the channel
 * and coming back speaks again.
 */
import { useEffect, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { subscribeAllServerChanges } from "../../channels";
import { cameraBlockKey, useCameraUnavailable } from "../availability";

/** Servers whose current channel visit already showed the notice. */
let spokenInVisit = new Set<number>();
let unsubscribeVisits: (() => void) | null = null;
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

/** The next visit to a channel of `serverId` may show the notice again. */
function forgetVisit(serverId: number): void {
  if (!spokenInVisit.has(serverId)) return;
  spokenInVisit = new Set(spokenInVisit);
  spokenInVisit.delete(serverId);
  notify();
}

function speak(serverId: number): void {
  if (spokenInVisit.has(serverId)) return;
  spokenInVisit = new Set(spokenInVisit);
  spokenInVisit.add(serverId);
  notify();
}

/** A visit ends with the own user leaving the channel and with a fresh sync of the server. */
function trackVisits(): void {
  unsubscribeVisits ??= subscribeAllServerChanges((serverId, change, state) => {
    if (change.kind === "synced") {
      forgetVisit(serverId);
      return;
    }
    if (change.kind !== "userUpserted" || state.selfSession === null) return;
    if (change.user.session !== state.selfSession) return;
    if (change.previous?.channelId === change.user.channelId) return;
    forgetVisit(serverId);
  });
}

function subscribe(listener: () => void): () => void {
  trackVisits();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * The notice to show in the room of `serverId`, or `null`. `sharing` is how many cameras other
 * people in the channel have on: with nobody sharing the notice never appears.
 */
export function useCantWatch(serverId: number, sharing: number): string | null {
  const { t } = useTranslation("camera");
  const reason = useCameraUnavailable(serverId);
  const active = reason !== null && sharing > 0;
  const spoken = useSyncExternalStore(subscribe, () => spokenInVisit.has(serverId));
  // This room keeps its notice while the reason holds; a room mounted after it stays quiet,
  // because the visit is remembered outside the room (FR-8.5).
  const [spokeHere, setSpokeHere] = useState(false);
  // What the last render showed, kept the way React allows: set during render when it changed.
  if (active && !spoken && !spokeHere) setSpokeHere(true);
  useEffect(() => {
    if (active && !spoken) speak(serverId);
  }, [active, spoken, serverId]);
  if (reason === null || !active || (spoken && !spokeHere)) return null;
  return t("cantWatch", { reason: t(cameraBlockKey(reason)) });
}

/** Test helper: forgets the visits and stops listening to the server changes. */
export function resetCantWatch(): void {
  unsubscribeVisits?.();
  unsubscribeVisits = null;
  spokenInVisit = new Set();
  listeners.clear();
}
