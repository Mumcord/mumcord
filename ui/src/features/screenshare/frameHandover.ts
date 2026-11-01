/**
 * The watch the main window keeps running while the screen share pop-out shows its frames
 * (issue #164). The engine watch stays open, so the sharer's viewers list does not change: the
 * pop-out attaches its own frame channel, and the main window attaches a new one when the view
 * docks again. The main window stops the kept watch once the user watches something else.
 */
import { getApi } from "../../api";
import type { WatchedShare } from "./watching";

let kept: WatchedShare | null = null;

/** The viewer left the main window for the pop-out; the engine watch stays open. */
export function keepWatch(share: WatchedShare): void {
  kept = share;
}

/** Takes the kept watch of this share back, so the viewer re-attaches instead of watching anew. */
export function takeKeptWatch(serverId: number, session: number): boolean {
  if (kept?.serverId !== serverId || kept.session !== session) return false;
  kept = null;
  return true;
}

/** Stops the kept watch unless it is `next`, the share the user watches from now on. */
export function releaseKeptWatch(next: WatchedShare | null): void {
  if (kept === null || (kept.serverId === next?.serverId && kept.session === next.session)) return;
  const { serverId, session } = kept;
  kept = null;
  getApi()
    .screenShareUnwatch(serverId, session)
    .catch((error: unknown) => {
      console.warn("stopping a screen share failed", error);
    });
}

/** Test helper. */
export function resetFrameHandover(): void {
  kept = null;
}
