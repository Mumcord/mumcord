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
  return (
    <span className="unread-badge">
      <span aria-hidden="true">{count > 99 ? "99+" : count}</span>
      <span className="visually-hidden">, {t("unread", { count })}</span>
    </span>
  );
}
