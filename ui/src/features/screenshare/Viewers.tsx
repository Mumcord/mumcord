/**
 * Who watches one share (FR-2.43): the count with the names, and the "share is full" line when all
 * eight places are taken. Used by the sharer's control bar and by the share row in a chat card.
 */
import { useTranslation } from "react-i18next";
import type { ReactNode } from "react";
import type { ScreenShare, ShareQuality, ViewerPath, ViewerQuality } from "../../api";
import { useServerStateOf } from "../channels";
import { Avatar } from "../members/Avatar";
import { BoltIcon, InfoCircleIcon, RelayIcon } from "./icons";
import { codecName, qualityText } from "./shareFormat";

/** Viewers per share (FR-2.43); the ninth gets a "share is full" answer. */
export const MAX_VIEWERS = 8;

/** Rows shown in the list before "and N more" (mockup 03). */
const LISTED = 6;

export interface ViewerListProps {
  own: ScreenShare;
  quality: ShareQuality | null;
  nameOf: (session: number) => string;
  srcOf: (session: number) => string | null;
}

/**
 * The sharer's viewer list (FR-7.52, mockup 03): the count, whether the share is sent once or as
 * one copy per viewer, and per viewer the quality received, codec, path and a "slow connection"
 * badge.
 */
export function ViewerList({ own, quality, nameOf, srcOf }: ViewerListProps): ReactNode {
  const { t } = useTranslation("screenshare");
  const count = own.viewers.length;
  const details = new Map((quality?.viewers ?? []).map((viewer) => [viewer.session, viewer]));
  const sentOnce = [...details.values()].some((viewer) => viewer.path !== "direct");
  const shown = own.viewers.slice(0, LISTED);
  const slow = quality?.viewers.find((viewer) => viewer.slow || viewer.struggling) ?? null;
  return (
    <>
      <p className="sharer-bar__popover-title">
        {t("viewerList.title")}{" "}
        <span className="sharer-bar__popover-count">
          {t("viewerList.count", { count, limit: MAX_VIEWERS })}
        </span>
      </p>
      {count > 0 && quality !== null && (
        <p className="viewer-list__path" data-tone={sentOnce ? undefined : "warn"}>
          <RelayIcon />
          {t(sentOnce ? "viewerList.relaySummary" : "viewerList.directSummary")}
        </p>
      )}
      {count === 0 ? (
        <p className="sharer-bar__empty">{t("viewersNone")}</p>
      ) : (
        <ul className="viewer-panel__list">
          {shown.map((session) => (
            <ViewerRow
              key={session}
              name={nameOf(session)}
              src={srcOf(session)}
              height={quality?.height ?? null}
              viewer={details.get(session) ?? null}
            />
          ))}
        </ul>
      )}
      {count > shown.length && (
        <p className="viewer-list__more">{t("viewerList.more", { count: count - shown.length })}</p>
      )}
      {count >= MAX_VIEWERS && <p className="sharer-bar__note">{t("fullShort")}</p>}
      {slow !== null && quality !== null && (
        <p className="sharer-bar__note">
          <InfoCircleIcon />
          {t("viewerList.layerNote", {
            name: nameOf(slow.session),
            fps: slow.fps,
            target: `${String(quality.fps)} fps`,
          })}
        </p>
      )}
    </>
  );
}

function ViewerRow({
  name,
  src,
  height,
  viewer,
}: {
  name: string;
  src: string | null;
  height: number | null;
  viewer: ViewerQuality | null;
}): ReactNode {
  const { t } = useTranslation("screenshare");
  const codec = viewer === null ? null : codecName(viewer.codec);
  const received =
    viewer === null || height === null
      ? null
      : [qualityText(height, viewer.fps), codec].filter((part) => part !== null).join(" · ");
  return (
    <li className="viewer-panel__person">
      <Avatar name={name} src={src} />
      <span className="viewer-panel__text">
        <span className="viewer-panel__name">{name}</span>
        {received !== null && <span className="viewer-panel__sub">{received}</span>}
      </span>
      {viewer !== null && (
        <span className="viewer-list__tags">
          {viewer.slow && <span className="viewer-list__slow">{t("viewerList.slow")}</span>}
          <span
            className={`share-path share-path--${viewer.path === "direct" ? "direct" : "relay"}`}
          >
            {viewer.path === "direct" ? <BoltIcon /> : <RelayIcon />}
            {t(pathKey(viewer.path))}
          </span>
        </span>
      )}
    </li>
  );
}

function pathKey(
  path: ViewerPath,
): "viewerList.pathDirect" | "viewerList.pathRelay" | "viewerList.pathBroadcast" {
  if (path === "direct") return "viewerList.pathDirect";
  return path === "relay" ? "viewerList.pathRelay" : "viewerList.pathBroadcast";
}

export function Viewers({ serverId, own }: { serverId: number; own: ScreenShare }) {
  const { t } = useTranslation("screenshare");
  const { users } = useServerStateOf(serverId);
  const count = own.viewers.length;
  if (count === 0) return <p className="sharer-bar__viewers">{t("viewersNone")}</p>;
  const names = own.viewers.map((session) => users.get(session)?.name ?? t("someone")).join(", ");
  const full = count >= MAX_VIEWERS;
  return (
    <p className="sharer-bar__viewers" data-full={full || undefined}>
      {full ? t("full", { max: MAX_VIEWERS, names }) : t("viewers", { count, names })}
    </p>
  );
}
