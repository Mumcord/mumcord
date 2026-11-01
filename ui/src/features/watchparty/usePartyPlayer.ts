/**
 * The party's player wired to the engine (issue #163): the user's commands, the next playlist entry
 * when a video ends, and where this player is. The cinema view and the mini bar use it, never both
 * at once.
 */
import { useCallback, useEffect, useRef, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { announce } from "../../a11y";
import { getApi, type WatchCommand } from "../../api";
import { setKnownDuration } from "./knownDuration";
import { playNext } from "./playlistActions";
import { reportPosition } from "./reportPosition";
import { useWatchPlayer, type WatchPlayer } from "./useWatchPlayer";
import type { LiveParty } from "./watchPartyStore";

export interface PartyPlayer {
  player: WatchPlayer;
  frameRef: RefObject<HTMLIFrameElement | null>;
}

/** The party's player, wired to the engine: commands, the next entry and position reports. */
export function usePartyPlayer(serverId: number, live: LiveParty): PartyPlayer {
  const { t } = useTranslation("watchparty");
  const frameRef = useRef<HTMLIFrameElement | null>(null);
  const onCommand = useCallback(
    (command: WatchCommand) => {
      getApi()
        .watchPartyCommand(serverId, command)
        .catch((error: unknown) => {
          console.error(t("commandFailed"), error);
          announce(t("commandFailed"), "assertive");
        });
    },
    [serverId, t],
  );
  const onEnded = useCallback(
    (entry: number) => {
      playNext(serverId, entry, false, t("playlist.nextFailed"));
    },
    [serverId, t],
  );
  const onReport = useCallback(
    (positionMs: number, playing: boolean, buffering: boolean) => {
      reportPosition(serverId, positionMs, playing, buffering);
    },
    [serverId],
  );
  const player = useWatchPlayer(live, { onCommand, onEnded, onReport }, frameRef);
  const videoId = live.party.videoId;
  useEffect(() => {
    setKnownDuration(videoId, player.durationMs);
  }, [videoId, player.durationMs]);
  return { player, frameRef };
}
