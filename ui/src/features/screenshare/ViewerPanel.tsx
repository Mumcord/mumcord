/**
 * The right column panel of a watched share (issue #163, FR-3.126): the sharer with the quality they
 * send, who is watching with the frame rate each of them gets, and the people in the channel that are
 * not watching. Both sides show the same numbers because they come from the sharer's own list
 * (`screenShareQuality`).
 */
import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { getApi, type ShareQuality, type User } from "../../api";
import { useServerStateOf } from "../channels";
import { Avatar } from "../members/Avatar";
import { useAvatars } from "../members/useAvatars";
import { MediaPanel } from "../media/MediaPanel";
import { bitrate } from "../media/format";
import { setTab, useMediaView } from "../media/mediaView";
import { useScreenShares } from "./sharesStore";
import { useWatchedShare } from "./watching";

/** The sharer's quality list is read once a second (issue #163). */
const POLL_MS = 1000;

export function ViewerPanel(): ReactNode {
  const { t } = useTranslation("screenviewer");
  const { t: tc } = useTranslation("common");
  const view = useMediaView();
  const watched = useWatchedShare();
  const share = useScreenShares(watched === null ? null : watched.serverId).find(
    (item) => watched !== null && item.session === watched.session,
  );
  const serverId = watched === null ? null : watched.serverId;
  const { users, selfSession } = useServerStateOf(serverId);
  const [quality, setQuality] = useState<ShareQuality | null>(null);

  useEffect(() => {
    if (serverId === null) return;
    const api = getApi();
    let current = true;
    const read = () => {
      api
        .screenShareQuality(serverId)
        .then((q) => {
          if (current) setQuality(q);
        })
        .catch((error: unknown) => {
          console.warn("screen share quality failed", error);
        });
    };
    read();
    const timer = window.setInterval(read, POLL_MS);
    return () => {
      current = false;
      window.clearInterval(timer);
    };
  }, [serverId]);

  const people: User[] = serverId === null ? [] : [...users.values()];
  const avatarOf = useAvatars(people, serverId ?? 0);
  if (watched === null || serverId === null || share === undefined) return null;
  const nameOf = (session: number) => users.get(session)?.name ?? t("someone");
  const srcOf = (session: number) => {
    const user = users.get(session);
    return user === undefined ? null : avatarOf(user);
  };
  const watching = share.viewers;
  const notWatching = people.filter((user) => !watching.includes(user.session));
  const slow = quality?.viewers.find((viewer) => viewer.struggling) ?? null;

  return (
    <MediaPanel
      label={t("panel", { name: nameOf(watched.session) })}
      tabs={[
        {
          id: "viewers",
          label: t("viewersCount", { watching: watching.length, max: 8 }),
          count: watching.length,
        },
      ]}
      active={view.tab}
      onSelect={() => {
        setTab("viewers");
      }}
    >
      <h3 className="viewer-panel__heading">{t("sharing")}</h3>
      <p className="viewer-panel__person">
        <Avatar name={nameOf(watched.session)} src={srcOf(watched.session)} size="sm" live />
        <span className="viewer-panel__name">{nameOf(watched.session)}</span>
        <span className="viewer-panel__quality">
          {quality === null
            ? t("unknown")
            : `${String(quality.width)}×${String(quality.height)}, ${bitrate(quality.kbps)}`}
        </span>
      </p>
      <h3 className="viewer-panel__heading">
        {t("viewersCount", { watching: watching.length, max: 8 })}
      </h3>
      {watching.length === 0 && <p className="viewer-panel__empty">{t("viewersNone")}</p>}
      {watching.map((session) => {
        const viewer = quality?.viewers.find((v) => v.session === session);
        return (
          <p key={session} className="viewer-panel__person">
            <Avatar name={nameOf(session)} src={srcOf(session)} size="sm" />
            <span className="viewer-panel__name">
              {nameOf(session)}
              {session === selfSession && ` (${tc("media.you")})`}
            </span>
            <span className="viewer-panel__quality">
              {viewer === undefined
                ? t("unknown")
                : `${String(viewer.fps)} fps, ${t("pathDirect")}`}
            </span>
          </p>
        );
      })}
      {slow !== null && (
        <p className="viewer-panel__hint">
          {t("loweredFor", { name: nameOf(slow.session), fps: String(slow.fps) })}
        </p>
      )}
      <h3 className="viewer-panel__heading">{t("notWatching")}</h3>
      {notWatching.length === 0 && <p className="viewer-panel__empty">{tc("media.none")}</p>}
      {notWatching.map((user) => (
        <p key={user.session} className="viewer-panel__person">
          <Avatar name={user.name} src={avatarOf(user)} size="sm" />
          <span className="viewer-panel__name">
            {user.name}
            {user.session === selfSession && ` (${tc("media.you")})`}
          </span>
        </p>
      ))}
    </MediaPanel>
  );
}
