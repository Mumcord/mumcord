/**
 * The mini bar of the watch party at the top of the chat (issue #163): thumbnail, title, how far the
 * party is, the sync dot, play/pause, "Open" back to the cinema view and leave. It is what remains
 * when the cinema view is closed, and the party keeps running here.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { safeImage } from "../chat/links";
import { clockTime } from "../media/format";
import { MiniBar } from "../media/MiniBar";
import { useMediaView, openMedia } from "../media/mediaView";
import { useTick } from "../media/useTick";
import { useKnownDuration } from "./knownDuration";
import { PlayPause } from "./PlayPause";
import { leaveWatchParty } from "./startParty";
import { syncState } from "./syncState";
import { useVideoPreview } from "./useVideoPreview";
import { positionAt, useWatchParty } from "./watchPartyStore";

export function WatchMiniBar({ serverId }: { serverId: number }): ReactNode {
  const { t: tc } = useTranslation("common");
  const { t: tp } = useTranslation("watchparty");
  const view = useMediaView();
  const live = useWatchParty(serverId);
  const nowMs = useTick(1000);
  const videoId = live === null ? "" : live.party.videoId;
  const preview = useVideoPreview(videoId);
  const durationMs = useKnownDuration(videoId);
  if (!view.mini.watch || live === null || !live.party.joined) return null;
  const title = preview?.title ?? tp("playlist.videoTitle");
  const positionMs = positionAt(live, nowMs);
  const state = live.party.positions.find((p) => p.session === live.party.host);
  const sync =
    state === undefined
      ? { kind: "unknown" as const }
      : syncState(state, positionMs, live.atMs, nowMs);
  const thumbnail = safeImage(preview?.image ?? null);

  return (
    <MiniBar
      label={tp("title")}
      title={title}
      thumbnail={
        thumbnail !== null ? (
          <img src={thumbnail} alt="" />
        ) : (
          <span className="mini-bar__empty" aria-hidden="true" />
        )
      }
      progress={
        durationMs === null
          ? clockTime(positionMs / 1000)
          : `${clockTime(positionMs / 1000)} / ${clockTime(durationMs / 1000)}`
      }
      {...(durationMs === null ? {} : { fraction: positionMs / durationMs })}
      status={<span className={`sync-dot sync-dot--${sync.kind}`} aria-hidden="true" />}
      actions={<PlayPause serverId={serverId} live={live} className="button mini-bar__icon" />}
      openLabel={tc("media.open")}
      onOpen={() => {
        openMedia("watch");
      }}
      closeLabel={tp("leave")}
      onClose={() => {
        leaveWatchParty(serverId, tp("leaveFailed"));
      }}
    />
  );
}
