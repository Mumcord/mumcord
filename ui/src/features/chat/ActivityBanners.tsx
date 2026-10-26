/**
 * Slim banners at the top of the channel chat for a running screen share or watch party the user
 * is not part of (FR-2.73). They are the only invite: no chat message is posted for either.
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getApi } from "../../api";
import { announce } from "../../a11y";
import { useServerState, useShownServerId } from "../channels";
import { startWatching, useScreenShares, useWatchedShare } from "../screenshare";
import { useWatchParty } from "../watchparty";
import { setWatchParty } from "../watchparty/watchPartyStore";
import { watchUrl } from "../watchparty/youtube";
import { loadPreview } from "./links";
import "./chat.css";

const MAX_BANNERS = 3;

function useVideoTitle(videoId: string | null): string | null {
  const [loaded, setLoaded] = useState<{ videoId: string; title: string | null }>();
  useEffect(() => {
    if (videoId === null) return;
    let current = true;
    void loadPreview(watchUrl(videoId)).then((preview) => {
      if (current) setLoaded({ videoId, title: preview?.title ?? null });
    });
    return () => {
      current = false;
    };
  }, [videoId]);
  return loaded?.videoId === videoId ? loaded.title : null;
}

export function ActivityBanners({ channelId }: { channelId: number }) {
  const { t } = useTranslation("chat");
  const serverId = useShownServerId();
  const { users, selfSession } = useServerState();
  const shares = useScreenShares(serverId);
  const watched = useWatchedShare();
  const live = useWatchParty(serverId);
  const party = live?.party ?? null;
  const partyVideo = party !== null && party.channelId === channelId ? party.videoId : null;
  const title = useVideoTitle(partyVideo);

  const inChannel = selfSession !== null && users.get(selfSession)?.channelId === channelId;
  if (serverId === null || !inChannel) return null;

  const joinParty = () => {
    getApi()
      .watchPartyJoin(serverId)
      .then((joined) => {
        setWatchParty(serverId, joined);
      })
      .catch((error: unknown) => {
        console.error("Joining the watch party failed", error);
        announce(t("activity.joinFailed"), "assertive");
      });
  };

  // The engine lists shares oldest first, so reversing puts the newest on top.
  const banners: { key: string; text: string; action: string; run: () => void }[] = [];
  if (partyVideo !== null && party !== null && !party.joined) {
    banners.push({
      key: "party",
      text: t("activity.party", { title: title ?? t("activity.videoFallback") }),
      action: t("activity.join"),
      run: joinParty,
    });
  }
  for (const share of [...shares].reverse()) {
    if (share.session === selfSession) continue;
    if (watched?.serverId === serverId && watched.session === share.session) continue;
    const name = users.get(share.session)?.name ?? t("activity.someone");
    banners.push({
      key: `share-${String(share.session)}`,
      text: t("activity.share", { name }),
      action: t("activity.watch"),
      run: () => {
        startWatching(serverId, share.session);
      },
    });
  }
  if (banners.length === 0) return null;

  return (
    <div className="activity-banners">
      {banners.slice(0, MAX_BANNERS).map((banner) => (
        <div key={banner.key} className="activity-banner" role="status" aria-live="polite">
          <span className="activity-banner__text">{banner.text}</span>
          <button type="button" className="button button--primary" onClick={banner.run}>
            {banner.action}
          </button>
        </div>
      ))}
    </div>
  );
}
