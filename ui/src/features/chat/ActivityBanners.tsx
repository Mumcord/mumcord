/**
 * Slim banners at the top of the channel chat for a running screen share or watch party the user
 * is not part of (FR-2.73). They are the only invite: no chat message is posted for either.
 */
import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { useEmbedsAllowed } from "../../app/preferences";
import { useOpenPrivacySettings } from "../../app/view";
import { getApi, type ScreenShare } from "../../api";
import { announce } from "../../a11y";
import { useServerState, useShownServerId } from "../channels";
import { startWatching, useScreenShares, useWatchedShare } from "../screenshare";
import { useWatchParty } from "../watchparty";
import { setWatchParty } from "../watchparty/watchPartyStore";
import { watchUrl } from "../watchparty/youtube";
import { p2pBlockText } from "../p2p/relayReason";
import { loadPreview } from "./links";
import "./chat.css";

const MAX_BANNERS = 3;

/** The video title for the banner; loads nothing for `null`. */
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
  const { t: tParty } = useTranslation("watchparty");
  const { t: tViewer } = useTranslation("screenviewer");
  const { t: tP2p } = useTranslation("p2p");
  const reasonId = useId();
  const embeds = useEmbedsAllowed();
  const openPrivacy = useOpenPrivacySettings();
  const serverId = useShownServerId();
  const { users, selfSession } = useServerState();
  const shares = useScreenShares(serverId);
  const watched = useWatchedShare();
  const live = useWatchParty(serverId);
  const party = live?.party ?? null;
  const partyVideo = party !== null && party.channelId === channelId ? party.videoId : null;
  // The title comes from YouTube, so with embeds off it is not even asked for.
  const title = useVideoTitle(embeds ? partyVideo : null);

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
  const banners: {
    key: string;
    text: string;
    action: string;
    run: () => void;
    /** Why the action is unavailable (FR-2.46, FR-4.11); the button stays focusable. */
    blocked?: string;
  }[] = [];
  if (partyVideo !== null && party !== null && !party.joined) {
    banners.push(
      embeds
        ? {
            key: "party",
            text: t("activity.party", { title: title ?? t("activity.videoFallback") }),
            action: t("activity.join"),
            run: joinParty,
          }
        : {
            key: "party",
            text: tParty("embedsOffJoin"),
            action: tParty("openPrivacy"),
            run: () => {
              openPrivacy?.();
            },
          },
    );
  }
  for (const share of [...shares].reverse()) {
    if (share.session === selfSession) continue;
    if (watched?.serverId === serverId && watched.session === share.session) continue;
    const name = users.get(share.session)?.name ?? t("activity.someone");
    const blocked = watchBlockText(share, name);
    banners.push({
      key: `share-${String(share.session)}`,
      text: t("activity.share", { name }),
      action: t("activity.watch"),
      run: () => {
        if (blocked === undefined) startWatching(serverId, share.session);
      },
      ...(blocked === undefined ? {} : { blocked }),
    });
  }
  function watchBlockText(share: ScreenShare, name: string): string | undefined {
    if (share.blocked === null) return undefined;
    switch (share.blocked) {
      case "consentOff":
        return tViewer("consentOff");
      case "peerUnsupported":
        return tViewer("tooOld", { name });
      case "noDirectPath":
        return tViewer("unreachable", { name });
      default:
        return p2pBlockText(tP2p, share.blocked, share.relayReason) ?? undefined;
    }
  }

  if (banners.length === 0) return null;

  return (
    <div className="activity-banners">
      {banners.slice(0, MAX_BANNERS).map((banner) => (
        <div key={banner.key} className="activity-banner" role="status" aria-live="polite">
          <span className="activity-banner__text">{banner.text}</span>
          <button
            type="button"
            className="button button--primary"
            aria-disabled={banner.blocked === undefined ? undefined : true}
            aria-describedby={
              banner.blocked === undefined ? undefined : `${reasonId}-${banner.key}`
            }
            data-tooltip={banner.blocked}
            onClick={banner.run}
          >
            {banner.action}
          </button>
          {banner.blocked !== undefined && (
            <span id={`${reasonId}-${banner.key}`} className="visually-hidden">
              {banner.blocked}
            </span>
          )}
        </div>
      ))}
    </div>
  );
}
