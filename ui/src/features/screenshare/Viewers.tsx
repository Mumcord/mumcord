/**
 * Who watches one share (FR-2.43): the count with the names, and the "share is full" line when all
 * eight places are taken. Used by the sharer's control bar and by the share row in a chat card.
 */
import { useTranslation } from "react-i18next";
import type { ScreenShare } from "../../api";
import { useServerStateOf } from "../channels";

/** Viewers per share (FR-2.43); the ninth gets a "share is full" answer. */
export const MAX_VIEWERS = 8;

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
