import { announce } from "../../a11y";
import { getApi } from "../../api";
import { closeMedia, openMedia } from "../media/mediaView";
import { setWatchParty } from "./watchPartyStore";

/** Starts a watch party for a video in the user's channel (FR-2.39); failures are announced. */
export function startWatchParty(serverId: number, videoId: string, failedText: string): void {
  getApi()
    .watchPartyStart(serverId, videoId)
    .then((party) => {
      setWatchParty(serverId, party);
      openMedia("watch");
    })
    .catch((error: unknown) => {
      console.error("Starting the watch party failed", error);
      announce(failedText, "assertive");
    });
}

/** Joins the party in the user's channel and opens its cinema view (issue #163). */
export function joinWatchParty(serverId: number, failedText: string): void {
  getApi()
    .watchPartyJoin(serverId)
    .then((party) => {
      setWatchParty(serverId, party);
      openMedia("watch");
    })
    .catch((error: unknown) => {
      console.error("Joining the watch party failed", error);
      announce(failedText, "assertive");
    });
}

/** Leaves the party; its view and mini bar go with it. */
export function leaveWatchParty(serverId: number, failedText: string): void {
  getApi()
    .watchPartyLeave(serverId)
    .then(() => {
      closeMedia("watch");
    })
    .catch((error: unknown) => {
      console.error("Leaving the watch party failed", error);
      announce(failedText, "assertive");
    });
}
