/**
 * The mini bar of a watched screen share at the top of the chat (issue #163): the sharer, LIVE,
 * what is shared and since when, "Open" back to the picture and Stop watching. It is what remains
 * when the picture is left, and the share keeps running here.
 */
import { type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useServerStateOf } from "../channels";
import { Avatar } from "../members/Avatar";
import { useAvatars } from "../members/useAvatars";
import { MiniBar } from "../media/MiniBar";
import { openMedia, useMediaView } from "../media/mediaView";
import { clockOf } from "./shareFormat";
import { shareStartedAt, useScreenShares } from "./sharesStore";
import { useOwnShare } from "./useOwnShare";
import { stopWatching, useWatchedShare } from "./watching";

export function ShareMiniBar({ serverId }: { serverId: number }): ReactNode {
  const { t, i18n } = useTranslation("screenviewer");
  const { t: tc } = useTranslation("common");
  const view = useMediaView();
  const share = useWatchedShare();
  const { users, selfSession } = useServerStateOf(serverId);
  const own = useOwnShare();
  const live = useScreenShares(serverId).find((item) => item.session === share?.session);
  const sharer = share === null ? undefined : users.get(share.session);
  const avatarOf = useAvatars(sharer === undefined ? [] : [sharer], serverId);
  if (!view.mini.share || share === null || share.serverId !== serverId) return null;
  const name = sharer?.name ?? t("someone");
  const watchingYourOwn = share.session === selfSession && own.own !== null;
  const what =
    live === undefined
      ? undefined
      : [
          t(
            live.kind === "window"
              ? "sharedWindow"
              : live.kind === "screen"
                ? "sharedScreen"
                : "sharedUnknown",
          ),
          t("since", { time: clockOf(shareStartedAt(live), i18n.language) }),
        ].join(" · ");

  return (
    <MiniBar
      thumbnail={
        <span className="share-mini-thumb">
          <Avatar name={name} src={sharer === undefined ? null : avatarOf(sharer)} size="sm" live />
        </span>
      }
      label={t("label", { name })}
      title={t("title", { name })}
      progress={what}
      status={
        <span className="live-badge" data-paused={live?.paused || undefined}>
          {t(live?.paused ? "pausedShort" : "live")}
        </span>
      }
      openLabel={tc("media.open")}
      closeLabel={t("stop")}
      onOpen={() => {
        openMedia("share");
      }}
      onClose={() => {
        // The sharer never watches their own picture, so here "Stop" only leaves the mini bar.
        if (!watchingYourOwn) stopWatching();
      }}
    />
  );
}
