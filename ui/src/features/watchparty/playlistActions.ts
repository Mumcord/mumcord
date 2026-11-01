/**
 * Playlist edits of the watch party (FR-2.69). The engine applies them at once and reports the
 * new list as `watchPartyChanged`; it bundles them for the other clients. Failures are logged
 * and announced.
 */
import { announce } from "../../a11y";
import { getApi } from "../../api";

function report(message: string, error: unknown): void {
  console.error(message, error);
  announce(message, "assertive");
}

/** Adds a video at the end of the party's playlist. */
export function addToPlaylist(serverId: number, videoId: string, failedText: string): void {
  getApi()
    .watchPartyAdd(serverId, videoId)
    .catch((error: unknown) => {
      report(failedText, error);
    });
}

export function removeFromPlaylist(serverId: number, entry: number, failedText: string): void {
  getApi()
    .watchPartyRemove(serverId, entry)
    .catch((error: unknown) => {
      report(failedText, error);
    });
}

/** Moves an entry to `index` of the playlist (0 plays next). */
export function moveInPlaylist(
  serverId: number,
  entry: number,
  index: number,
  failedText: string,
): void {
  getApi()
    .watchPartyMove(serverId, entry, index)
    .catch((error: unknown) => {
      report(failedText, error);
    });
}

/**
 * Goes on from entry `from` to the next video for everyone: when it ended, or at once with
 * `skip`. The engine ignores it when the party already left `from`.
 */
export function playNext(serverId: number, from: number, skip: boolean, failedText: string): void {
  getApi()
    .watchPartyNext(serverId, from, skip)
    .catch((error: unknown) => {
      report(failedText, error);
    });
}

/**
 * Goes back to the video that played before the current one for everyone (issue #163). The engine
 * takes the last entry of the past it keeps; with nothing played yet it answers with an error.
 */
export function playPrevious(serverId: number, failedText: string): void {
  getApi()
    .watchPartyPrevious(serverId)
    .catch((error: unknown) => {
      report(failedText, error);
    });
}
