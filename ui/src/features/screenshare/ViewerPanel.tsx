/**
 * The right column panel of a watched share (issue #163, FR-3.126): the sharer, who is watching,
 * and the people in the channel that are not watching, with the keys at the bottom. A viewer only
 * measures their own stream, so the quality and the path show on the user's own row; the sharer
 * sees everyone's in the viewers popover of their bar.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { User } from "../../api";
import { useServerStateOf } from "../channels";
import { Avatar } from "../members/Avatar";
import { useAvatars } from "../members/useAvatars";
import { MediaPanel } from "../media/MediaPanel";
import { setTab } from "../media/mediaView";
import { useTick } from "../media/useTick";
import { useP2pLink } from "../p2p/useP2pLink";
import { KeyboardIcon } from "./icons";
import { PathTag } from "./parts";
import { qualityText } from "./shareFormat";
import { useScreenShares } from "./sharesStore";
import { MAX_VIEWERS } from "./Viewers";
import { useWatchedShare, watchMeter } from "./watching";

/** The own row's figures are read once a second, like the chips (issue #163). */
const TICK_MS = 1000;

export function ViewerPanel(): ReactNode {
  const { t } = useTranslation("screenviewer");
  const { t: tc } = useTranslation("common");
  const watched = useWatchedShare();
  const share = useScreenShares(watched === null ? null : watched.serverId).find(
    (item) => watched !== null && item.session === watched.session,
  );
  const serverId = watched === null ? null : watched.serverId;
  const { users, selfSession } = useServerStateOf(serverId);
  const link = useP2pLink(serverId, watched?.session ?? 0);
  useTick(TICK_MS);

  const people: User[] = serverId === null ? [] : [...users.values()];
  const avatarOf = useAvatars(people, serverId ?? 0);
  if (watched === null || serverId === null || share === undefined) return null;
  const nameOf = (session: number) => users.get(session)?.name ?? t("someone");
  const srcOf = (session: number) => {
    const user = users.get(session);
    return user === undefined ? null : avatarOf(user);
  };
  const watching = share.viewers;
  const channelId = users.get(watched.session)?.channelId;
  const notWatching = people.filter(
    (user) =>
      user.channelId === channelId &&
      user.session !== watched.session &&
      !watching.includes(user.session),
  );
  const own = watchMeter().snapshot();
  const sharerSub = [
    t(
      share.kind === "window"
        ? "sharedWindow"
        : share.kind === "screen"
          ? "sharedScreen"
          : "sharedUnknown",
    ),
    t(share.sound ? "soundOnShort" : "soundOffShort"),
  ]
    // The narrow panel may wrap the line, but between the parts, not inside "sound on".
    .map((part) => part.replace(/ /g, "\u00a0"))
    .join(" · ");
  const label = t("viewersCount", { watching: watching.length, max: MAX_VIEWERS });

  return (
    <MediaPanel
      label={t("panel", { name: nameOf(watched.session) })}
      tabs={[{ id: "viewers", label }]}
      active="viewers"
      onSelect={() => {
        setTab("viewers");
      }}
    >
      <div className="viewer-panel">
        <h3 className="viewer-panel__heading">{t("sharing")}</h3>
        <ul className="viewer-panel__list">
          <li className="viewer-panel__person">
            <Avatar name={nameOf(watched.session)} src={srcOf(watched.session)} live />
            <span className="viewer-panel__text">
              <span className="viewer-panel__name">{nameOf(watched.session)}</span>
              <span className="viewer-panel__sub">{sharerSub}</span>
            </span>
            <span className="live-badge" data-paused={share.paused || undefined}>
              {t(share.paused ? "pausedShort" : "live")}
            </span>
          </li>
        </ul>
        <h3 className="viewer-panel__heading">{t("watching")}</h3>
        {watching.length === 0 ? (
          <p className="viewer-panel__empty">{t("viewersNone")}</p>
        ) : (
          <ul className="viewer-panel__list">
            {watching.map((session) => {
              const self = session === selfSession;
              return (
                <li key={session} className="viewer-panel__person">
                  <Avatar name={nameOf(session)} src={srcOf(session)} />
                  <span className="viewer-panel__text">
                    <span className="viewer-panel__name">
                      {self ? `${nameOf(session)} (${tc("media.you")})` : nameOf(session)}
                    </span>
                    {self && own.height > 0 && (
                      <span className="viewer-panel__sub">{qualityText(own.height, own.fps)}</span>
                    )}
                  </span>
                  {self && link !== null && <PathTag path={link.path} />}
                </li>
              );
            })}
          </ul>
        )}
        <h3 className="viewer-panel__heading">{t("notWatching")}</h3>
        {notWatching.length === 0 ? (
          <p className="viewer-panel__empty">{tc("media.none")}</p>
        ) : (
          <ul className="viewer-panel__list">
            {notWatching.map((user) => (
              <li key={user.session} className="viewer-panel__person">
                <Avatar name={user.name} src={avatarOf(user)} />
                <span className="viewer-panel__text">
                  <span className="viewer-panel__name">
                    {user.session === selfSession ? `${user.name} (${tc("media.you")})` : user.name}
                  </span>
                </span>
              </li>
            ))}
          </ul>
        )}
        <p className="share-keys">
          <KeyboardIcon />
          <span className="visually-hidden">{t("keys")}</span>
          <span>
            <kbd>F</kbd> {t("keyFullscreen")}
          </span>
          <span>
            <kbd>Z</kbd> {t("keyFocus")}
          </span>
          <span>
            <kbd>1</kbd> {t("oneToOne")}
          </span>
          <span>
            <kbd>Esc</kbd> {t("keyExit")}
          </span>
        </p>
      </div>
    </MediaPanel>
  );
}
