import { announce } from "../../a11y";
import { getApi } from "../../api";
import { setWatchParty } from "./watchPartyStore";

/** Starts a watch party for a video in the user's channel (FR-2.39); failures are announced. */
export function startWatchParty(serverId: number, videoId: string, failedText: string): void {
  getApi()
    .watchPartyStart(serverId, videoId)
    .then((party) => {
      setWatchParty(serverId, party);
    })
    .catch((error: unknown) => {
      console.error("Starting the watch party failed", error);
      announce(failedText, "assertive");
    });
}
