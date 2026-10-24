import { useTranslation } from "react-i18next";
import { useFollowState } from "./followStore";

/** Unread count of the own or a followed channel for the channel tree (`channelBadge`). */
export function ChannelUnreadBadge({ channelId }: { channelId: number }) {
  const { unread } = useFollowState();
  return <UnreadBadge count={unread.get(`channel:${String(channelId)}`) ?? 0} />;
}

export function UnreadBadge({ count }: { count: number }) {
  const { t } = useTranslation("follow");
  if (count <= 0) return null;
  // The digits are CSS-generated (::before) so the badge adds no DOM text: the visible count stays
  // part of the accessible name (WCAG 2.5.3 label-in-name). The spoken form is the hidden phrase.
  return (
    <span className="unread-badge" data-count={count > 99 ? "99+" : count}>
      <svg
        viewBox="0 0 24 24"
        width="12"
        height="12"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
        focusable="false"
      >
        <path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12" />
      </svg>
      <span className="visually-hidden">, {t("unread", { count })}</span>
    </span>
  );
}
