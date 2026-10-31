/**
 * Sends where this player is to the party (issue #163), so the watching list can show each person.
 * The hook that calls it throttles the reports; a failed report is logged and tried again with the
 * next one, nothing the user has to see.
 */
import { getApi } from "../../api";

export function reportPosition(
  serverId: number,
  positionMs: number,
  playing: boolean,
  buffering: boolean,
): void {
  getApi()
    .watchPartyReport(serverId, positionMs, playing, buffering)
    .catch((error: unknown) => {
      console.warn("watch party position report failed", error);
    });
}
