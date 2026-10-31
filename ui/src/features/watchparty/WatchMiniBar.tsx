/**
 * The mini bar of the watch party at the top of the chat (issue #163): thumbnail, title, how far the
 * party is, the sync dot, play/pause, "Open" back to the cinema view and leave. It is what remains
 * when the cinema view is closed, and the party keeps running here.
 */
import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { announce } from "../../a11y";
import { safeImage } from "../chat/links";
import { clockTime } from "../media/format";
import { MiniBar } from "../media/MiniBar";
import { useMediaView, openMedia, closeMedia } from "../media/mediaView";
import { PlayPause } from "./PlayPause";
import { useVideoPreview } from "./useVideoPreview";
import { positionAt, useWatchParty } from "./watchPartyStore";
import { syncState } from "./syncState";

export function WatchMiniBar({ serverId }: { serverId: number }): ReactNode {
  const { t: tc } = useTranslation("common");
  const { t: tp } = useTranslation("watchparty");
  const view = useMediaView();
  const live = useWatchParty(serverId);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const preview = useVideoPreview(live === null ? "" : live.party.videoId);
  if (!view.mini.watch || live === null || !live.party.joined) return null;
  const title = preview?.title ?? tp("playlist.videoTitle");
  const state = live.party.positions.find((p) => p.session === live.party.host);
  const sync = state === undefined ? { kind: "unknown" as const } : syncState(state, positionAt(live, nowMs), live.atMs, nowMs);

  return (
    <MiniBar
      label={tp("title")}
      title={title}
      thumbnail={
        safeImage(preview?.image ?? null) !== null ? (
          <img src={safeImage(preview?.image ?? "") as string} alt="" />
        ) : (
          <span className="mini-bar__empty" aria-hidden="true" />
        )
      }
      progress={clockTime(positionAt(live, nowMs) / 1000)}
      status={<span className={`sync-dot sync-dot--${sync.kind}`} aria-hidden="true" />}
      actions={<PlayPause serverId={serverId} live={live} />}
      openLabel={tc("media.open")}
      onOpen={() => {
        openMedia("watch");
      }}
      closeLabel={tp("leave")}
      onClose={() => {
        getApi()
          .watchPartyLeave(serverId)
          .then(() => {
            closeMedia("watch");
          })
          .catch((error: unknown) => {
            console.error(tp("leaveFailed"), error);
            announce(tp("leaveFailed"), "assertive");
          });
        setNowMs(Date.now());
      }}
    />
  );
}
