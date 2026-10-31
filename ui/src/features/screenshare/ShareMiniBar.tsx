/**
 * The mini bar of a watched screen share at the top of the chat (issue #163): who shares, that it is
 * live, "Open" back to the picture and Stop watching. It is what remains when the picture is left,
 * and the share keeps running here.
 */
import { type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useServerStateOf } from "../channels";
import { MiniBar } from "../media/MiniBar";
import { openMedia, useMediaView } from "../media/mediaView";
import { useScreenShares } from "./sharesStore";
import { useOwnShare } from "./useOwnShare";
import { stopWatching, useWatchedShare } from "./watching";

export function ShareMiniBar({ serverId }: { serverId: number }): ReactNode {
  const { t } = useTranslation("screenviewer");
  const { t: tc } = useTranslation("common");
  const view = useMediaView();
  const share = useWatchedShare();
  const { users, selfSession } = useServerStateOf(serverId);
  const own = useOwnShare();
  const live = useScreenShares(serverId).find((item) => item.session === share?.session);
  if (!view.mini.share || share === null || share.serverId !== serverId) return null;
  const name = users.get(share.session)?.name ?? t("someone");
  const watchingYourOwn = share.session === selfSession && own.own !== null;

  return (
    <MiniBar
      thumbnail={<span className="mini-bar__screen" aria-hidden="true" />}
      label={t("label", { name })}
      title={t("title", { name })}
      status={live?.paused ? t("paused", { name }) : t("live")}
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
