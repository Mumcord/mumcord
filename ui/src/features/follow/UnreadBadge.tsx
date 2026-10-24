import { useTranslation } from "react-i18next";
import { useFollowState } from "./followStore";

/** Unread count of a followed channel for the channel tree (`channelBadge`). */
export function ChannelUnreadBadge({ channelId }: { channelId: number }) {
  const { followed, unread } = useFollowState();
  return (
    <UnreadBadge
      count={unread.get(`channel:${String(channelId)}`) ?? 0}
      following={followed.includes(channelId)}
    />
  );
}

export function UnreadBadge({ count, following = true }: { count: number; following?: boolean }) {
  const { t } = useTranslation("follow");
  if (!following || count <= 0) return null;
  // The digits are CSS-generated (::before) so the badge adds no DOM text: the visible count stays
  // part of the accessible name (WCAG 2.5.3 label-in-name). The spoken form is the hidden phrase.
  return (
    <span className="unread-badge" data-count={count > 99 ? "99+" : count}>
      <span className="visually-hidden">, {t("unread", { count })}</span>
    </span>
  );
}
